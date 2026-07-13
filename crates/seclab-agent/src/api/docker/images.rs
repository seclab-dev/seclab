//! Docker 镜像 API：镜像列表与管理操作。

use crate::models::docker;
use crate::state::AppState;
use crate::types::{AgentError, ApiResponse, ApiResult};

use axum::{
    Json,
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
};

use bollard::models::ImageSummary;
use bollard::query_parameters;
use bollard::query_parameters::{CreateImageOptions, ImportImageOptions};
use futures_util::{StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::services::logging::{AgentLogModule, LoggerEntry};
use std::collections::HashMap;
use std::default::Default;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{
    Arc, LazyLock, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio_util::codec::{BytesCodec, FramedRead};
use tracing::info;
use uuid::Uuid;

static IMAGE_PULL_TASKS: LazyLock<Mutex<HashMap<String, ImagePullTask>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

const DOCKER_HUB_TAGS_URL: &str = "https://hub.docker.com/v2/repositories";
const DOCKER_HUB_SEARCH_URL: &str = "https://hub.docker.com/v2/search/repositories/";
const DOCKER_HUB_TAG_PAGE_SIZE: u32 = 100;
const DEFAULT_PAGE_SIZE: u32 = 10;
const MAX_PAGE_SIZE: u32 = 100;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub image_name: String,
    pub tag: Option<String>,
}

/// 批量检查镜像是否存在的请求。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageAvailabilityRequest {
    pub images: Vec<String>,
}

/// 单个镜像在当前节点的可用状态。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageAvailability {
    pub image_ref: String,
    pub available: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveRequest {
    pub image_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageSearchRequest {
    pub keyword: String,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageTagsRequest {
    pub repository: String,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageSearchResponse {
    pub page: u32,
    pub page_size: u32,
    pub has_more: bool,
    pub results: Vec<ImageSearchResult>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageSearchResult {
    pub repository: String,
    pub display_name: String,
    pub description: Option<String>,
    pub star_count: Option<i64>,
    pub pull_count: Option<i64>,
    pub is_official: bool,
    pub is_automated: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageTagsResponse {
    pub repository: String,
    pub page: u32,
    pub page_size: u32,
    pub has_more: bool,
    pub tags: Vec<ImageTagInfo>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageResolveResponse {
    pub repository: String,
    pub display_name: String,
    pub requested_tag: Option<String>,
    pub default_tag: Option<String>,
    pub tags: Vec<ImageTagInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImageTagInfo {
    pub name: String,
    pub full_size: Option<i64>,
    pub last_updated: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagePullStartResponse {
    pub task_id: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImagePullProgressResponse {
    pub task_id: String,
    pub image_name: String,
    pub tag: String,
    pub status: ImagePullStatus,
    pub progress_percent: u8,
    pub status_text: Option<String>,
    pub error: Option<String>,
    pub layers: Vec<ImagePullLayerProgress>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ImagePullStatus {
    Pending,
    Pulling,
    Success,
    Failed,
    Cancelled,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImagePullLayerProgress {
    pub id: String,
    pub status: Option<String>,
    pub progress_percent: Option<u8>,
}

#[derive(Debug, Clone)]
struct ImagePullTask {
    image_name: String,
    tag: String,
    status: ImagePullStatus,
    progress_percent: u8,
    status_text: Option<String>,
    error: Option<String>,
    layers: HashMap<String, ImagePullLayerProgress>,
    cancel: Arc<AtomicBool>,
}

/// 精确检查请求中的镜像引用是否存在于当前 Docker 守护进程。
pub async fn image_availability(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ImageAvailabilityRequest>,
) -> ApiResult<Response> {
    if payload.images.is_empty() {
        return Err(AgentError::BadRequest("images must not be empty".to_string()).into());
    }
    let docker = state.docker_client().await?;
    let mut result = Vec::with_capacity(payload.images.len());
    for image in payload.images {
        let image_ref = image.trim().to_string();
        if image_ref.is_empty() {
            return Err(
                AgentError::BadRequest("image reference must not be empty".to_string()).into(),
            );
        }
        result.push(ImageAvailability {
            available: docker.inspect_image(&image_ref).await.is_ok(),
            image_ref,
        });
    }
    Ok(ApiResponse::success_with_raw("Image availability loaded", Some(result)).into_response())
}

#[derive(Debug, Deserialize)]
struct DockerHubTagsResponse {
    count: Option<i64>,
    next: Option<String>,
    results: Vec<DockerHubTag>,
}

#[derive(Debug, Deserialize)]
struct DockerHubTag {
    name: String,
    full_size: Option<i64>,
    last_updated: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DockerHubSearchResponse {
    count: Option<i64>,
    next: Option<String>,
    results: Vec<DockerHubSearchItem>,
}

#[derive(Debug, Deserialize)]
struct DockerHubSearchItem {
    repo_name: String,
    short_description: Option<String>,
    star_count: Option<i64>,
    pull_count: Option<i64>,
    is_official: Option<bool>,
    is_automated: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportQuery {
    pub image_name: String,
}

/// 获取本地所有 Docker 镜像的摘要信息列表。
///
/// 此函数通过调用 `bollard` 的 `list_images` 方法与 Docker 守护进程通信，
/// 并将获取到的镜像列表作为 `ApiResponse` 返回。
pub async fn list_images(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    info!("Requesting docker images");
    let docker = state.docker_client().await?;
    let images: Vec<ImageSummary> = docker
        .list_images(Some(query_parameters::ListImagesOptions::default()))
        .await?;
    Ok(ApiResponse::success_with_raw("Image list loaded", Some(images)).into_response())
}

/// 根据镜像 ID 或名称删除一个本地 Docker 镜像。
///
/// # 参数
/// - `state`: 共享的应用状态。
/// - `query`: 包含要删除镜像的 ID 或名称 (`docker::ImageRef`) 的查询参数。
///
/// # 处理流程
/// 1. 调用 `bollard` 的 `remove_image` 方法执行删除操作。
/// 2. 如果删除成功，记录一条平台日志 (`LoggerEntry`)。
/// 3. 将 Docker API 的返回结果封装在 `ApiResponse` 中返回给客户端。
pub async fn remove_image(
    State(state): State<Arc<AppState>>,
    Query(query): Query<docker::ImageRef>,
) -> ApiResult<Response> {
    info!("Requesting Remove {} image", query.name);

    let docker = state.docker_client().await?;
    let images = docker
        .remove_image(
            &query.id,
            Some(query_parameters::RemoveImageOptions::default()),
            None,
        )
        .await?;
    LoggerEntry::new(
        "docker",
        "docker_image_removed",
        IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
    )
    .set_success()
    .module(AgentLogModule::Docker)
    .metadata(json!({
        "message_key": "platformLog.docker.image.removed",
        "image_name": query.name
    }))
    .finish(&state.metadata_db);
    info!("{} image removed", query.name);

    Ok(ApiResponse::success_with_raw("Image removed", Some(images)).into_response())
}

/// 搜索 Docker Hub 镜像仓库。
pub async fn search_images(Json(payload): Json<ImageSearchRequest>) -> ApiResult<Response> {
    let keyword = payload.keyword.trim();
    if keyword.is_empty() {
        return Err(AgentError::BadRequest("keyword must not be empty".to_string()).into());
    }

    let page = payload.page.unwrap_or(1).max(1);
    let page_size = clamp_page_size(payload.page_size);
    let response = reqwest::Client::new()
        .get(DOCKER_HUB_SEARCH_URL)
        .query(&[
            ("query", keyword.to_string()),
            ("page", page.to_string()),
            ("page_size", page_size.to_string()),
        ])
        .send()
        .await
        .map_err(|err| {
            AgentError::DockerOperation(format!("failed to search Docker Hub images: {err}"))
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AgentError::DockerOperation(format!(
            "Docker Hub image search failed with status {status}: {body}"
        ))
        .into());
    }

    let result = response
        .json::<DockerHubSearchResponse>()
        .await
        .map_err(|err| {
            AgentError::DockerOperation(format!("failed to parse image search response: {err}"))
        })?;
    let results = result
        .results
        .into_iter()
        .map(|item| {
            let is_official = item.is_official.unwrap_or(false);
            let repository = normalize_search_repository(&item.repo_name, is_official);
            ImageSearchResult {
                repository,
                display_name: item.repo_name,
                description: item.short_description,
                star_count: item.star_count,
                pull_count: item.pull_count,
                is_official,
                is_automated: item.is_automated.unwrap_or(false),
            }
        })
        .collect::<Vec<_>>();

    Ok(ApiResponse::success_with_raw(
        "Image search loaded",
        Some(ImageSearchResponse {
            page,
            page_size,
            has_more: has_more_page(result.next, result.count, page, page_size),
            results,
        }),
    )
    .into_response())
}

/// 分页加载 Docker Hub 镜像 tag。
pub async fn image_tags(Json(payload): Json<ImageTagsRequest>) -> ApiResult<Response> {
    let image_ref = parse_docker_hub_image_ref(&payload.repository)?;
    let page = payload.page.unwrap_or(1).max(1);
    let page_size = clamp_page_size(payload.page_size);
    let response = fetch_docker_hub_tags(&image_ref, page, page_size).await?;
    let has_more = has_more_page(response.next.clone(), response.count, page, page_size);
    let tags = response
        .results
        .into_iter()
        .map(|tag| ImageTagInfo {
            name: tag.name,
            full_size: tag.full_size,
            last_updated: tag.last_updated,
        })
        .collect::<Vec<_>>();

    Ok(ApiResponse::success_with_raw(
        "Image tags loaded",
        Some(ImageTagsResponse {
            repository: image_ref.repository_ref(),
            page,
            page_size,
            has_more,
            tags,
        }),
    )
    .into_response())
}

/// 解析 Docker Hub 镜像引用并加载可选 tag。
pub async fn resolve_image(Json(payload): Json<ResolveRequest>) -> ApiResult<Response> {
    let image_ref = parse_docker_hub_image_ref(&payload.image_name)?;
    let tags = fetch_docker_hub_tags(&image_ref, 1, DOCKER_HUB_TAG_PAGE_SIZE)
        .await?
        .results
        .into_iter()
        .map(|tag| ImageTagInfo {
            name: tag.name,
            full_size: tag.full_size,
            last_updated: tag.last_updated,
        })
        .collect::<Vec<_>>();

    let default_tag = image_ref
        .requested_tag
        .as_ref()
        .filter(|requested| tags.iter().any(|tag| tag.name == **requested))
        .cloned();

    Ok(ApiResponse::success_with_raw(
        "Image resolved",
        Some(ImageResolveResponse {
            repository: image_ref.repository_ref(),
            display_name: image_ref.display_name(),
            requested_tag: image_ref.requested_tag,
            default_tag,
            tags,
        }),
    )
    .into_response())
}

async fn fetch_docker_hub_tags(
    image_ref: &DockerHubImageRef,
    page: u32,
    page_size: u32,
) -> Result<DockerHubTagsResponse, AgentError> {
    let url = format!(
        "{}/{}/{}/tags",
        DOCKER_HUB_TAGS_URL, image_ref.namespace, image_ref.repository
    );
    let response = reqwest::Client::new()
        .get(url)
        .query(&[
            ("page", page.to_string()),
            ("page_size", page_size.to_string()),
        ])
        .send()
        .await
        .map_err(|err| {
            AgentError::DockerOperation(format!("failed to query Docker Hub tags: {err}"))
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AgentError::DockerOperation(format!(
            "Docker Hub tag query failed with status {status}: {body}"
        )));
    }

    response
        .json::<DockerHubTagsResponse>()
        .await
        .map_err(|err| AgentError::DockerOperation(format!("failed to parse tag response: {err}")))
}

fn clamp_page_size(page_size: Option<u32>) -> u32 {
    page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE)
}

fn has_more_page(next: Option<String>, count: Option<i64>, page: u32, page_size: u32) -> bool {
    if next.as_deref().is_some_and(|value| !value.is_empty()) {
        return true;
    }
    count.is_some_and(|count| i64::from(page * page_size) < count)
}

fn normalize_search_repository(repo_name: &str, is_official: bool) -> String {
    if repo_name.contains('/') {
        repo_name.to_string()
    } else if is_official {
        format!("library/{repo_name}")
    } else {
        repo_name.to_string()
    }
}

/// 启动镜像拉取任务，并返回可轮询的任务 ID。
pub async fn pull_image(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<PullRequest>,
) -> ApiResult<Response> {
    let image_ref = parse_docker_hub_image_ref(&payload.image_name)?;
    let tag = if let Some(tag) = payload.tag.filter(|tag| !tag.trim().is_empty()) {
        tag.trim().to_string()
    } else if let Some(tag) = image_ref.requested_tag.clone() {
        tag
    } else {
        "latest".to_string()
    };
    let image_name = image_ref.repository_ref();
    let task_id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));

    {
        let mut tasks = IMAGE_PULL_TASKS
            .lock()
            .expect("image pull task lock poisoned");
        tasks.insert(
            task_id.clone(),
            ImagePullTask {
                image_name: image_name.clone(),
                tag: tag.clone(),
                status: ImagePullStatus::Pending,
                progress_percent: 0,
                status_text: Some("Waiting to pull image".to_string()),
                error: None,
                layers: HashMap::new(),
                cancel: Arc::clone(&cancel),
            },
        );
    }

    let task_id_for_worker = task_id.clone();
    tokio::spawn(async move {
        run_image_pull_task(state, task_id_for_worker, image_name, tag, cancel).await;
    });

    Ok(ApiResponse::success_with_raw(
        "Image pull task started",
        Some(ImagePullStartResponse { task_id }),
    )
    .into_response())
}

/// 获取镜像拉取任务进度。
pub async fn pull_image_progress(Path(task_id): Path<String>) -> ApiResult<Response> {
    let progress = {
        let tasks = IMAGE_PULL_TASKS
            .lock()
            .expect("image pull task lock poisoned");
        let Some(task) = tasks.get(&task_id) else {
            return Err(
                AgentError::BadRequest(format!("image pull task `{task_id}` not found")).into(),
            );
        };
        task.to_response(task_id.clone())
    };

    Ok(ApiResponse::success_with_raw("Image pull progress loaded", Some(progress)).into_response())
}

/// 取消镜像拉取任务。
pub async fn cancel_pull_image(Path(task_id): Path<String>) -> ApiResult<Response> {
    let progress = {
        let mut tasks = IMAGE_PULL_TASKS
            .lock()
            .expect("image pull task lock poisoned");
        let Some(task) = tasks.get_mut(&task_id) else {
            return Err(
                AgentError::BadRequest(format!("image pull task `{task_id}` not found")).into(),
            );
        };
        match task.status {
            ImagePullStatus::Pending | ImagePullStatus::Pulling => {
                task.cancel.store(true, Ordering::Relaxed);
                task.status_text = Some("Canceling image pull".to_string());
            }
            ImagePullStatus::Success | ImagePullStatus::Failed | ImagePullStatus::Cancelled => {}
        }
        task.to_response(task_id.clone())
    };

    Ok(
        ApiResponse::success_with_raw("Image pull cancellation requested", Some(progress))
            .into_response(),
    )
}

async fn run_image_pull_task(
    state: Arc<AppState>,
    task_id: String,
    image_name: String,
    tag: String,
    cancel: Arc<AtomicBool>,
) {
    update_pull_task(&task_id, |task| {
        task.status = ImagePullStatus::Pulling;
        task.progress_percent = 5;
        task.status_text = Some(format!("Pulling {image_name}:{tag}"));
    });

    let result = async {
        pull_registry_image(
            &state,
            &format!("{image_name}:{tag}"),
            || cancel.load(Ordering::Relaxed),
            |info| update_pull_task_from_stream(&task_id, info),
        )
        .await
    }
    .await;

    update_pull_task(&task_id, |task| match result {
        Ok(()) => {
            task.status = ImagePullStatus::Success;
            task.progress_percent = 100;
            task.status_text = Some("Image pull completed".to_string());
        }
        Err(_) if cancel.load(Ordering::Relaxed) => {
            task.status = ImagePullStatus::Cancelled;
            task.progress_percent = task.progress_percent.min(99);
            task.status_text = Some("Image pull cancelled".to_string());
            task.error = None;
        }
        Err(err) => {
            task.status = ImagePullStatus::Failed;
            task.progress_percent = task.progress_percent.min(99);
            task.status_text = Some("Image pull failed".to_string());
            task.error = Some(err.to_string());
        }
    });
}

/// 通过 Docker Registry 拉取镜像，并把原始进度事件交给调用方。
pub async fn pull_registry_image(
    state: &Arc<AppState>,
    image_ref: &str,
    is_cancelled: impl Fn() -> bool,
    mut on_progress: impl FnMut(bollard::models::CreateImageInfo),
) -> Result<(), AgentError> {
    let docker = state
        .docker_client()
        .await
        .map_err(|err| AgentError::DockerOperation(err.to_string()))?;
    let (from_image, tag) = split_registry_image_ref(image_ref);
    let options = CreateImageOptions {
        from_image: Some(from_image),
        tag: Some(tag),
        ..Default::default()
    };
    let mut stream = docker.create_image(Some(options), None, None);
    while let Some(message) = stream.next().await {
        if is_cancelled() {
            return Err(AgentError::DockerOperation(
                "image pull cancelled".to_string(),
            ));
        }
        let info = message.map_err(|err| AgentError::DockerOperation(err.to_string()))?;
        if let Some(error) = info
            .error_detail
            .as_ref()
            .and_then(|detail| detail.message.clone())
        {
            return Err(AgentError::DockerOperation(error));
        }
        on_progress(info);
    }
    Ok(())
}

fn split_registry_image_ref(image_ref: &str) -> (String, String) {
    let slash = image_ref.rfind('/').unwrap_or(0);
    if let Some(colon) = image_ref.rfind(':').filter(|colon| *colon > slash) {
        (
            image_ref[..colon].to_string(),
            image_ref[colon + 1..].to_string(),
        )
    } else {
        (image_ref.to_string(), "latest".to_string())
    }
}

fn update_pull_task_from_stream(task_id: &str, info: bollard::models::CreateImageInfo) {
    update_pull_task(task_id, |task| {
        if let Some(status) = info.status.clone() {
            task.status_text = Some(match &info.id {
                Some(id) => format!("{id}: {status}"),
                None => status.clone(),
            });
        }

        let layer_id = info.id.unwrap_or_else(|| "image".to_string());
        if let Some(status) = info.status {
            let progress_percent = info.progress_detail.and_then(|detail| {
                let (Some(current), Some(total)) = (detail.current, detail.total) else {
                    return None;
                };
                if total <= 0 {
                    return None;
                }
                Some(
                    ((current as f64 / total as f64) * 100.0)
                        .round()
                        .clamp(0.0, 100.0) as u8,
                )
            });

            task.layers.insert(
                layer_id.clone(),
                ImagePullLayerProgress {
                    id: layer_id,
                    status: Some(status),
                    progress_percent,
                },
            );
        }

        let layer_progress = task
            .layers
            .values()
            .filter_map(|layer| layer.progress_percent.map(u32::from))
            .collect::<Vec<_>>();
        if layer_progress.is_empty() {
            task.progress_percent = task.progress_percent.clamp(10, 95);
        } else {
            let average = layer_progress.iter().sum::<u32>()
                / u32::try_from(layer_progress.len()).unwrap_or(1);
            task.progress_percent = (average as u8).clamp(5, 95);
        }
    });
}

fn update_pull_task(task_id: &str, update: impl FnOnce(&mut ImagePullTask)) {
    let mut tasks = IMAGE_PULL_TASKS
        .lock()
        .expect("image pull task lock poisoned");
    if let Some(task) = tasks.get_mut(task_id) {
        update(task);
    }
}

impl ImagePullTask {
    fn to_response(&self, task_id: String) -> ImagePullProgressResponse {
        let mut layers = self.layers.values().cloned().collect::<Vec<_>>();
        layers.sort_by(|left, right| left.id.cmp(&right.id));
        ImagePullProgressResponse {
            task_id,
            image_name: self.image_name.clone(),
            tag: self.tag.clone(),
            status: self.status.clone(),
            progress_percent: self.progress_percent,
            status_text: self.status_text.clone(),
            error: self.error.clone(),
            layers,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DockerHubImageRef {
    namespace: String,
    repository: String,
    requested_tag: Option<String>,
}

impl DockerHubImageRef {
    fn repository_ref(&self) -> String {
        format!("{}/{}", self.namespace, self.repository)
    }

    fn display_name(&self) -> String {
        if self.namespace == "library" {
            self.repository.clone()
        } else {
            self.repository_ref()
        }
    }
}

fn parse_docker_hub_image_ref(raw: &str) -> Result<DockerHubImageRef, AgentError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AgentError::BadRequest(
            "imageName must not be empty".to_string(),
        ));
    }
    if trimmed.contains('@') {
        return Err(AgentError::BadRequest(
            "image digest references are not supported by image registry parsing".to_string(),
        ));
    }

    let (without_tag, requested_tag) = split_reference_tag(trimmed)?;
    let path = without_tag
        .strip_prefix("docker.io/")
        .unwrap_or(without_tag);
    let parts = path.split('/').collect::<Vec<_>>();
    if parts.iter().any(|part| part.trim().is_empty()) {
        return Err(AgentError::BadRequest(
            "imageName contains an empty path segment".to_string(),
        ));
    }

    let (namespace, repository) = match parts.as_slice() {
        [repository] => ("library".to_string(), (*repository).to_string()),
        [namespace, repository] => ((*namespace).to_string(), (*repository).to_string()),
        [registry, ..]
            if registry.contains('.') || registry.contains(':') || *registry == "localhost" =>
        {
            return Err(AgentError::BadRequest(
                "only Docker Hub image references are supported in image registry".to_string(),
            ));
        }
        _ => {
            return Err(AgentError::BadRequest(
                "Docker Hub image references must be image or namespace/image".to_string(),
            ));
        }
    };

    Ok(DockerHubImageRef {
        namespace,
        repository,
        requested_tag,
    })
}

fn split_reference_tag(reference: &str) -> Result<(&str, Option<String>), AgentError> {
    let slash_index = reference.rfind('/').map_or(0, |index| index + 1);
    let Some(relative_colon_index) = reference[slash_index..].rfind(':') else {
        return Ok((reference, None));
    };
    let colon_index = slash_index + relative_colon_index;
    let name = &reference[..colon_index];
    let tag = reference[colon_index + 1..].trim();
    if tag.is_empty() {
        return Err(AgentError::BadRequest(
            "image tag must not be empty".to_string(),
        ));
    }
    Ok((name, Some(tag.to_string())))
}

/// 将当前节点 Docker 镜像导出为 tar 流。
pub async fn export_image(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ExportQuery>,
) -> ApiResult<Response> {
    let image_name = query.image_name.trim();
    if image_name.is_empty() {
        return Err(AgentError::BadRequest("imageName must not be empty".to_string()).into());
    }

    let docker = state.docker_client().await?;
    let stream = docker
        .export_image(image_name)
        .map(|chunk| chunk.map_err(std::io::Error::other));
    let body = Body::from_stream(stream);

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/x-tar")
        .header(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"image.tar\"",
        )
        .body(body)?)
}

/// 接收流式上传的镜像 tar 包，并导入到本地 Docker 中。
pub async fn load_image(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> ApiResult<Response> {
    info!("Starting docker image load");
    let mut temp_file_path = std::env::temp_dir();
    let file_name = format!("docker-image-{}.tar", Uuid::new_v4());
    temp_file_path.push(&file_name);

    let mut file = File::create(&temp_file_path)
        .await
        .map_err(AgentError::FileOperation)?;

    let mut has_file = false;
    while let Some(field) = multipart.next_field().await.map_err(AgentError::from)? {
        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
            has_file = true;
            let mut stream = field;
            while let Some(chunk) = stream.next().await {
                let bytes = chunk.map_err(|e| AgentError::FileUploadInvalid(e.to_string()))?;
                file.write_all(&bytes)
                    .await
                    .map_err(AgentError::FileOperation)?;
            }
        }
    }

    if !has_file {
        return Err(AgentError::BadRequest("missing upload file field".to_string()).into());
    }

    // 确保写入完成并关闭文件
    file.flush().await.map_err(AgentError::FileOperation)?;
    drop(file);

    // 重新以只读方式打开文件，用于 load_image
    let read_file = File::open(&temp_file_path)
        .await
        .map_err(AgentError::FileOperation)?;
    let byte_stream = FramedRead::new(read_file, BytesCodec::new()).map_ok(|bytes| bytes.freeze());

    let docker = state.docker_client().await?;
    let options = ImportImageOptions {
        ..Default::default()
    };

    let mut stream = docker.import_image_stream(options, byte_stream, None);
    let mut logs = Vec::new();

    // 收集 stream
    let load_result: Result<(), AgentError> = async {
        while let Some(msg) = stream
            .try_next()
            .await
            .map_err(|e| AgentError::DockerOperation(e.to_string()))?
        {
            if let Some(stream_msg) = msg.stream {
                logs.push(stream_msg);
            }
            if let Some(error_msg) = msg.error_detail.and_then(|detail| detail.message) {
                return Err(AgentError::DockerOperation(error_msg));
            }
        }
        Ok(())
    }
    .await;

    // 清理临时文件
    if let Err(e) = tokio::fs::remove_file(&temp_file_path).await {
        tracing::error!("Failed to remove temp file {:?}: {}", temp_file_path, e);
    }

    load_result?;

    let log_output = logs.join("");

    LoggerEntry::new(
        "docker",
        "docker_image_loaded",
        IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
    )
    .set_success()
    .module(AgentLogModule::Docker)
    .metadata(json!({
        "message_key": "platformLog.docker.image.loaded",
    }))
    .finish(&state.metadata_db);

    Ok(
        ApiResponse::success_with_raw("Docker image loaded successfully", Some(log_output))
            .into_response(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_registry_port_and_tag() {
        assert_eq!(
            split_registry_image_ref("registry.local:5000/team/app:1.2.3"),
            (
                "registry.local:5000/team/app".to_string(),
                "1.2.3".to_string()
            )
        );
        assert_eq!(
            split_registry_image_ref("registry.local:5000/team/app"),
            (
                "registry.local:5000/team/app".to_string(),
                "latest".to_string()
            )
        );
    }

    #[test]
    fn parses_official_docker_hub_image() {
        let image_ref = parse_docker_hub_image_ref("nginx").expect("image ref should parse");

        assert_eq!(image_ref.repository_ref(), "library/nginx");
        assert_eq!(image_ref.display_name(), "nginx");
        assert_eq!(image_ref.requested_tag, None);
    }

    #[test]
    fn parses_namespace_image_with_tag() {
        let image_ref =
            parse_docker_hub_image_ref("aaa/bbb:0.1.0").expect("image ref should parse");

        assert_eq!(image_ref.repository_ref(), "aaa/bbb");
        assert_eq!(image_ref.display_name(), "aaa/bbb");
        assert_eq!(image_ref.requested_tag.as_deref(), Some("0.1.0"));
    }

    #[test]
    fn strips_docker_io_registry_prefix() {
        let image_ref =
            parse_docker_hub_image_ref("docker.io/aaa/bbb:latest").expect("image ref should parse");

        assert_eq!(image_ref.repository_ref(), "aaa/bbb");
        assert_eq!(image_ref.requested_tag.as_deref(), Some("latest"));
    }

    #[test]
    fn rejects_non_docker_hub_registry() {
        let result = parse_docker_hub_image_ref("registry.example.com/aaa/bbb:latest");

        assert!(result.is_err());
    }
}
