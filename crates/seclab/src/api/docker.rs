//! Docker 镜像后台分发管理 API。

use crate::models::NodeRuntimeClient;
use crate::state::AppState;
use crate::types::{ApiResponse, ApiResult};
use axum::{
    Json, Router,
    extract::{Multipart, Query, State},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use bollard::Docker;
use bollard::query_parameters::{CreateImageOptions, ListImagesOptions};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::SystemTime;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio_util::codec::{BytesCodec, FramedRead};
use tracing::{error, info};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistributeNodeStatus {
    pub progress_percent: u32,
    pub status: String, // "waiting", "exporting", "uploading", "loading", "success", "failed"
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistributeSession {
    pub task_id: String,
    pub created_at: u64,
    pub node_statuses: HashMap<String, DistributeNodeStatus>,
}

static DISTRIBUTE_SESSIONS: LazyLock<Mutex<HashMap<String, DistributeSession>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const AGENT_DOCKER_IMAGE_LOAD_PATH: &str = "/api/v1/agent/docker/images/load";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusQuery {
    pub task_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub image_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalDistributeRequest {
    pub image_name: String,
    pub node_ids: Vec<String>,
}

pub fn docker_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/images/distribute", post(distribute_image))
        .route("/images/distribute/local", post(distribute_local_image))
        .route("/images/distribute/status", get(get_distribute_status))
        .route("/local-images", get(list_local_images))
        .route("/local-images/pull", post(pull_local_image))
}

fn get_local_docker() -> Result<Docker, seclab_api::error::ApiError> {
    Docker::connect_with_local_defaults().map_err(|e| {
        seclab_api::error::ApiError::bad_gateway(
            seclab_contracts::api::ErrorCode::DockerUnavailable,
            format!("Local docker daemon is not available: {}", e),
        )
    })
}

pub async fn list_local_images() -> ApiResult<Response> {
    let docker = get_local_docker()?;
    let images = docker
        .list_images(Some(ListImagesOptions::default()))
        .await
        .map_err(|e| {
            seclab_api::error::ApiError::internal(format!("failed to list local images: {}", e))
        })?;
    Ok(ApiResponse::success_with_raw("Local images loaded", Some(images)).into_response())
}

pub async fn pull_local_image(Json(payload): Json<PullRequest>) -> ApiResult<Response> {
    let docker = get_local_docker()?;

    let full_name = payload.image_name;
    let (from_image, tag) = if let Some((img, tg)) = full_name.split_once(':') {
        (img.to_string(), tg.to_string())
    } else {
        (full_name, "latest".to_string())
    };

    let options = CreateImageOptions {
        from_image: Some(from_image),
        tag: Some(tag),
        ..Default::default()
    };

    let mut stream = docker.create_image(Some(options), None, None);
    let mut logs = Vec::new();

    while let Some(msg) = stream.next().await {
        let info = msg.map_err(|e| {
            seclab_api::error::ApiError::internal(format!("docker pull stream error: {}", e))
        })?;
        if let Some(status) = info.status {
            logs.push(status);
        }
    }

    Ok(
        ApiResponse::success_with_raw("Image pulled successfully", Some(logs.join("\n")))
            .into_response(),
    )
}

pub async fn get_distribute_status(Query(query): Query<StatusQuery>) -> ApiResult<Response> {
    let sessions = DISTRIBUTE_SESSIONS.lock().unwrap();
    if let Some(session) = sessions.get(&query.task_id) {
        Ok(
            ApiResponse::success_with_raw("Distribute task status loaded", Some(session.clone()))
                .into_response(),
        )
    } else {
        Ok(ApiResponse::error("Task not found", (), 404).into_response())
    }
}

pub async fn distribute_image(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> ApiResult<Response> {
    info!("Starting docker image distribution");

    let mut temp_file_path = std::env::temp_dir();
    let task_id = Uuid::new_v4().to_string();
    let file_name = format!("dist-image-{}.tar", task_id);
    temp_file_path.push(&file_name);

    let mut file = File::create(&temp_file_path).await.map_err(|e| {
        seclab_api::error::ApiError::internal(format!("failed to create temp file: {}", e))
    })?;

    let mut node_ids: Vec<String> = Vec::new();
    let mut has_file = false;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        seclab_api::error::ApiError::bad_request(
            seclab_contracts::api::ErrorCode::BadRequest,
            format!("multipart parse error: {}", e),
        )
    })? {
        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
            has_file = true;
            let mut stream = field;
            while let Some(chunk) = stream.next().await {
                let bytes = chunk.map_err(|e| {
                    seclab_api::error::ApiError::bad_request(
                        seclab_contracts::api::ErrorCode::BadRequest,
                        format!("chunk read error: {}", e),
                    )
                })?;
                file.write_all(&bytes).await.map_err(|e| {
                    seclab_api::error::ApiError::internal(format!("file write error: {}", e))
                })?;
            }
        } else if name == "nodeIds" {
            let text = field.text().await.map_err(|e| {
                seclab_api::error::ApiError::bad_request(
                    seclab_contracts::api::ErrorCode::BadRequest,
                    format!("read nodeIds error: {}", e),
                )
            })?;
            if text.starts_with('[') {
                if let Ok(parsed) = serde_json::from_str::<Vec<String>>(&text) {
                    node_ids = parsed;
                }
            } else {
                node_ids = text
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
    }

    if !has_file {
        return Err(seclab_api::error::ApiError::bad_request(
            seclab_contracts::api::ErrorCode::BadRequest,
            "missing file".to_string(),
        ));
    }
    if node_ids.is_empty() {
        return Err(seclab_api::error::ApiError::bad_request(
            seclab_contracts::api::ErrorCode::BadRequest,
            "node_ids must not be empty".to_string(),
        ));
    }

    file.flush()
        .await
        .map_err(|e| seclab_api::error::ApiError::internal(format!("file flush error: {}", e)))?;
    drop(file);

    let mut node_statuses = HashMap::new();
    for node_id in &node_ids {
        node_statuses.insert(
            node_id.clone(),
            DistributeNodeStatus {
                progress_percent: 0,
                status: "waiting".to_string(),
                error: None,
            },
        );
    }

    let created_at = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    {
        let mut sessions = DISTRIBUTE_SESSIONS.lock().unwrap();
        sessions.insert(
            task_id.clone(),
            DistributeSession {
                task_id: task_id.clone(),
                created_at,
                node_statuses,
            },
        );
    }

    let task_id_clone = task_id.clone();
    let state_clone = state.clone();
    let temp_file_path_clone = temp_file_path.clone();

    tokio::spawn(async move {
        for node_id in node_ids {
            info!(
                "Distributing image to node {} for task {}",
                node_id, task_id_clone
            );

            update_node_status(&task_id_clone, &node_id, "uploading", 10, None);

            let result = distribute_to_single_node(
                &state_clone,
                &task_id_clone,
                &node_id,
                &temp_file_path_clone,
            )
            .await;

            match result {
                Ok(_) => {
                    info!(
                        "Successfully distributed image to node {} for task {}",
                        node_id, task_id_clone
                    );
                    update_node_status(&task_id_clone, &node_id, "success", 100, None);
                }
                Err(err) => {
                    error!(
                        "Failed to distribute image to node {} for task {}: {}",
                        node_id, task_id_clone, err
                    );
                    update_node_status(
                        &task_id_clone,
                        &node_id,
                        "failed",
                        100,
                        Some(err.to_string()),
                    );
                }
            }
        }

        if let Err(e) = tokio::fs::remove_file(&temp_file_path_clone).await {
            error!(
                "Failed to remove temporary distributed tar pack {:?}: {}",
                temp_file_path_clone, e
            );
        }
    });

    Ok(ApiResponse::success_with_raw("Distribute task started", Some(task_id)).into_response())
}

pub async fn distribute_local_image(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LocalDistributeRequest>,
) -> ApiResult<Response> {
    info!(
        "Starting docker local image distribution: {}",
        payload.image_name
    );

    let task_id = Uuid::new_v4().to_string();
    let mut temp_file_path = std::env::temp_dir();
    let file_name = format!("dist-local-{}.tar", task_id);
    temp_file_path.push(&file_name);

    if payload.node_ids.is_empty() {
        return Err(seclab_api::error::ApiError::bad_request(
            seclab_contracts::api::ErrorCode::BadRequest,
            "node_ids must not be empty".to_string(),
        ));
    }

    let mut node_statuses = HashMap::new();
    for node_id in &payload.node_ids {
        node_statuses.insert(
            node_id.clone(),
            DistributeNodeStatus {
                progress_percent: 0,
                status: "waiting".to_string(),
                error: None,
            },
        );
    }

    let created_at = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    {
        let mut sessions = DISTRIBUTE_SESSIONS.lock().unwrap();
        sessions.insert(
            task_id.clone(),
            DistributeSession {
                task_id: task_id.clone(),
                created_at,
                node_statuses,
            },
        );
    }

    let task_id_clone = task_id.clone();
    let state_clone = state.clone();
    let temp_file_path_clone = temp_file_path.clone();
    let image_name_clone = payload.image_name.clone();
    let node_ids = payload.node_ids;

    tokio::spawn(async move {
        for node_id in &node_ids {
            update_node_status(&task_id_clone, node_id, "exporting", 5, None);
        }

        let export_result: Result<(), anyhow::Error> = async {
            let docker = get_local_docker()?;
            let mut stream = docker.export_image(&image_name_clone);
            let mut file = File::create(&temp_file_path_clone).await?;
            while let Some(chunk) = stream.next().await {
                let bytes = chunk?;
                file.write_all(&bytes).await?;
            }
            file.flush().await?;
            Ok(())
        }
        .await;

        if let Err(e) = export_result {
            error!("Failed to export local image {}: {}", image_name_clone, e);
            for node_id in &node_ids {
                update_node_status(
                    &task_id_clone,
                    node_id,
                    "failed",
                    100,
                    Some(format!("Export failed: {}", e)),
                );
            }
            return;
        }

        for node_id in node_ids {
            info!(
                "Distributing local image to node {} for task {}",
                node_id, task_id_clone
            );

            update_node_status(&task_id_clone, &node_id, "uploading", 10, None);

            let result = distribute_to_single_node(
                &state_clone,
                &task_id_clone,
                &node_id,
                &temp_file_path_clone,
            )
            .await;

            match result {
                Ok(_) => {
                    info!(
                        "Successfully distributed local image to node {} for task {}",
                        node_id, task_id_clone
                    );
                    update_node_status(&task_id_clone, &node_id, "success", 100, None);
                }
                Err(err) => {
                    error!(
                        "Failed to distribute local image to node {} for task {}: {}",
                        node_id, task_id_clone, err
                    );
                    update_node_status(
                        &task_id_clone,
                        &node_id,
                        "failed",
                        100,
                        Some(err.to_string()),
                    );
                }
            }
        }

        if let Err(e) = tokio::fs::remove_file(&temp_file_path_clone).await {
            error!(
                "Failed to remove temporary distributed local tar pack {:?}: {}",
                temp_file_path_clone, e
            );
        }
    });

    Ok(
        ApiResponse::success_with_raw("Local distribute task started", Some(task_id))
            .into_response(),
    )
}

fn update_node_status(
    task_id: &str,
    node_id: &str,
    status: &str,
    progress: u32,
    error: Option<String>,
) {
    let mut sessions = DISTRIBUTE_SESSIONS.lock().unwrap();
    if let Some(session) = sessions.get_mut(task_id)
        && let Some(node_status) = session.node_statuses.get_mut(node_id)
    {
        node_status.status = status.to_string();
        node_status.progress_percent = progress;
        node_status.error = error;
    }
}

async fn distribute_to_single_node(
    state: &Arc<AppState>,
    task_id: &str,
    node_id: &str,
    file_path: &std::path::Path,
) -> anyhow::Result<()> {
    let client = NodeRuntimeClient::from_node_route(&state.metadata_db, Some(node_id)).await?;

    let file = File::open(file_path).await?;
    let file_size = file.metadata().await?.len();
    let progress_task_id = task_id.to_string();
    let progress_node_id = node_id.to_string();
    let mut uploaded_bytes = 0_u64;
    let mut last_progress = 10_u32;
    let file_stream = FramedRead::new(file, BytesCodec::new()).map(move |chunk| {
        if let Ok(bytes) = &chunk {
            uploaded_bytes = uploaded_bytes.saturating_add(bytes.len() as u64);
            let next_progress = upload_progress_percent(uploaded_bytes, file_size);
            if next_progress > last_progress {
                update_node_status(
                    &progress_task_id,
                    &progress_node_id,
                    "uploading",
                    next_progress,
                    None,
                );
                last_progress = next_progress;
            }
            if uploaded_bytes >= file_size && last_progress < 95 {
                update_node_status(&progress_task_id, &progress_node_id, "loading", 95, None);
                last_progress = 95;
            }
        }
        chunk
    });
    let file_body = reqwest::Body::wrap_stream(file_stream);

    let part = reqwest::multipart::Part::stream(file_body)
        .file_name("file.tar")
        .mime_str("application/x-tar")?;

    let form = reqwest::multipart::Form::new().part("file", part);

    let url = client.build_uri(AGENT_DOCKER_IMAGE_LOAD_PATH);

    info!("Sending POST request to node {} URL: {}", node_id, url);
    let response = client
        .client
        .post(&url)
        .multipart(form)
        .timeout(std::time::Duration::from_secs(600))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Agent image load request failed: url={}; {}",
            url,
            agent_error_response_message(response).await
        ));
    }

    Ok(())
}

fn upload_progress_percent(uploaded_bytes: u64, total_bytes: u64) -> u32 {
    if total_bytes == 0 {
        return 90;
    }

    let uploaded = u128::from(uploaded_bytes.min(total_bytes));
    let total = u128::from(total_bytes);
    let scaled = 10 + ((uploaded * 80) / total) as u32;
    scaled.clamp(10, 90)
}

async fn agent_error_response_message(response: reqwest::Response) -> String {
    let status = response.status().as_u16();
    match response.text().await {
        Ok(body) => format_agent_error_body(status, &body),
        Err(err) => format!(
            "Agent returned error: status={}; failed to read response body: {}",
            status, err
        ),
    }
}

fn format_agent_error_body(status: u16, body: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        return format!("Agent returned empty error response: status={status}");
    }

    if let Ok(value) = serde_json::from_str::<Value>(body)
        && let Some(message) = value.get("message").and_then(Value::as_str)
    {
        let detail = response_json_detail(&value, message);
        if detail.is_empty() {
            return format!("Agent returned error: status={status}; message={message}");
        }
        return format!(
            "Agent returned error: status={status}; message={message}; detail={detail}"
        );
    }

    format!(
        "Agent returned error: status={}; body={}",
        status,
        response_body_excerpt(body)
    )
}

fn response_json_detail(value: &Value, message: &str) -> String {
    ["data", "errorCode"]
        .iter()
        .filter_map(|key| {
            let raw_value = value.get(*key)?;
            let detail = match raw_value {
                Value::Null => return None,
                Value::String(text) => text.trim().to_string(),
                other => other.to_string(),
            };
            if detail.is_empty() || detail == message {
                None
            } else {
                Some(detail)
            }
        })
        .fold(Vec::new(), |mut details, detail| {
            if !details.contains(&detail) {
                details.push(detail);
            }
            details
        })
        .join("; ")
}

fn response_body_excerpt(body: &str) -> String {
    const MAX_CHARS: usize = 2048;

    let mut chars = body.chars();
    let excerpt: String = chars.by_ref().take(MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{}... [truncated]", excerpt)
    } else {
        excerpt
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AGENT_DOCKER_IMAGE_LOAD_PATH, format_agent_error_body, response_body_excerpt,
        upload_progress_percent,
    };

    #[test]
    fn uses_agent_prefixed_image_load_path() {
        assert_eq!(
            AGENT_DOCKER_IMAGE_LOAD_PATH,
            "/api/v1/agent/docker/images/load"
        );
    }

    #[test]
    fn formats_agent_api_error_body() {
        let body = r#"{"success":false,"code":502,"message":"Docker operation failed","errorCode":"DockerOperationFailed","data":"DockerOperationFailed"}"#;

        assert_eq!(
            format_agent_error_body(502, body),
            "Agent returned error: status=502; message=Docker operation failed; detail=DockerOperationFailed"
        );
    }

    #[test]
    fn maps_upload_progress_to_transfer_range() {
        assert_eq!(upload_progress_percent(0, 100), 10);
        assert_eq!(upload_progress_percent(50, 100), 50);
        assert_eq!(upload_progress_percent(100, 100), 90);
        assert_eq!(upload_progress_percent(120, 100), 90);
    }

    #[test]
    fn formats_empty_agent_error_body() {
        assert_eq!(
            format_agent_error_body(413, ""),
            "Agent returned empty error response: status=413"
        );
    }

    #[test]
    fn formats_plain_text_agent_error_body() {
        assert_eq!(
            format_agent_error_body(413, "request body too large"),
            "Agent returned error: status=413; body=request body too large"
        );
    }

    #[test]
    fn truncates_large_plain_text_agent_error_body() {
        let body = "a".repeat(2050);

        assert_eq!(
            response_body_excerpt(&body),
            format!("{}... [truncated]", "a".repeat(2048))
        );
    }
}
