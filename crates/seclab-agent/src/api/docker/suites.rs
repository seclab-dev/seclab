//! Docker Compose 套件运行接口：安装、启停、卸载与入口代理。

use crate::config;
use crate::state::AppState;
use crate::types::{ApiError, ApiResponse, ApiResult};
use axum::body::Body;
use axum::extract::{Json, OriginalUri, Path, Query, State};
use axum::http::{HeaderMap, HeaderName, Method, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bollard::models::NetworkCreateRequest;
use bollard::query_parameters::{self, CreateImageOptions};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path as FsPath, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use tokio::process::Command;

const SUITE_NETWORK_NAME: &str = "seclab-suite-network";
const COMPOSE_FILE_NAME: &str = "compose.yaml";
const SUITE_METADATA_FILE: &str = "suite-agent.json";
static SUITE_INSTALL_PROGRESS: LazyLock<Mutex<HashMap<String, SuiteInstallProgress>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 套件包内二进制安全文件。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SuitePackageFile {
    pub path: String,
    pub content_base64: String,
}

/// 套件应用入口。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SuiteAppEntry {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub icon: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
}

/// 安装套件的请求。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SuiteInstallRequest {
    pub instance_id: String,
    pub suite_id: String,
    pub version: String,
    pub compose_project_name: String,
    pub compose_file: String,
    pub files: Vec<SuitePackageFile>,
    pub app_entries: Vec<SuiteAppEntry>,
}

/// 套件生命周期动作请求。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuiteActionRequest {
    pub compose_project_name: String,
    #[serde(default)]
    pub remove_data: bool,
}

/// 查询套件安装进度的请求参数。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuiteInstallProgressQuery {
    pub instance_id: String,
}

/// 当前节点上的套件安装进度状态。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuiteInstallProgress {
    pub instance_id: String,
    pub progress_percent: u32,
    pub status: String,
    pub current_step: String,
    pub current_image: Option<String>,
    pub is_finished: bool,
    pub error: Option<String>,
    pub cancel_requested: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SuiteAgentMetadata {
    instance_id: String,
    suite_id: String,
    version: String,
    compose_project_name: String,
    app_entries: Vec<SuiteAppEntry>,
}

#[derive(Debug, Deserialize)]
pub struct SuiteProxyPath {
    project: String,
    entry_id: String,
}

/// 取消套件安装任务的路径参数。
#[derive(Debug, Deserialize)]
pub struct SuiteInstallCancelPath {
    instance_id: String,
}

/// 安装套件文件、准备镜像并登记 Compose 项目。
pub async fn install_suite(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SuiteInstallRequest>,
) -> ApiResult<Response> {
    validate_id("instance_id", &payload.instance_id)?;
    validate_project_name(&payload.compose_project_name)?;
    upsert_install_progress(SuiteInstallProgress {
        instance_id: payload.instance_id.clone(),
        progress_percent: 1,
        status: "running".to_string(),
        current_step: "prepare".to_string(),
        current_image: None,
        is_finished: false,
        error: None,
        cancel_requested: false,
    });
    ensure_suite_network(&state).await?;

    let dir = suite_project_dir(&payload.compose_project_name);
    ensure_suite_project_available(&state, &payload.compose_project_name, &dir).await?;
    let result = install_suite_inner(&state, &payload, &dir).await;
    if let Err(err) = result {
        let canceled = is_install_cancel_requested(&payload.instance_id);
        update_install_progress(
            &payload.instance_id,
            100,
            if canceled { "canceled" } else { "failed" },
            if canceled { "canceled" } else { "failed" },
            None,
            true,
            if canceled {
                None
            } else {
                Some(err.to_string())
            },
        );
        rollback_suite_install(&payload.compose_project_name, &dir).await;
        return Err(err);
    }

    update_install_progress(
        &payload.instance_id,
        100,
        "success",
        "completed",
        None,
        true,
        None,
    );
    Ok(ApiResponse::ok("Suite installed").into_response())
}

/// 查询套件安装在当前节点执行面的实时进度。
pub async fn install_progress(
    Query(query): Query<SuiteInstallProgressQuery>,
) -> ApiResult<Response> {
    let sessions = SUITE_INSTALL_PROGRESS.lock().unwrap();
    let progress = sessions
        .get(&query.instance_id)
        .cloned()
        .ok_or(ApiError::NotFound)?;
    Ok(ApiResponse::success_with_raw("Suite install progress fetched", progress).into_response())
}

/// 请求取消当前节点上的套件安装任务。
pub async fn cancel_install(Path(path): Path<SuiteInstallCancelPath>) -> ApiResult<Response> {
    let mut sessions = SUITE_INSTALL_PROGRESS.lock().unwrap();
    let progress = sessions
        .get_mut(&path.instance_id)
        .ok_or(ApiError::NotFound)?;
    progress.cancel_requested = true;
    if !progress.is_finished {
        progress.status = "canceling".to_string();
        progress.current_step = "canceling".to_string();
    }
    Ok(
        ApiResponse::success_with_raw("Suite install cancellation requested", progress.clone())
            .into_response(),
    )
}

/// 新建或覆盖当前节点内存中的套件安装进度。
fn upsert_install_progress(progress: SuiteInstallProgress) {
    let mut sessions = SUITE_INSTALL_PROGRESS.lock().unwrap();
    sessions.insert(progress.instance_id.clone(), progress);
}

/// 更新当前节点内存中的安装进度，并保持百分比单调递增。
fn update_install_progress(
    instance_id: &str,
    progress_percent: u32,
    status: &str,
    current_step: &str,
    current_image: Option<String>,
    is_finished: bool,
    error: Option<String>,
) {
    let mut sessions = SUITE_INSTALL_PROGRESS.lock().unwrap();
    if let Some(progress) = sessions.get_mut(instance_id) {
        progress.progress_percent = progress.progress_percent.max(progress_percent.min(100));
        progress.status = status.to_string();
        progress.current_step = current_step.to_string();
        progress.current_image = current_image;
        progress.is_finished = is_finished;
        progress.error = error;
        if matches!(status, "canceled" | "failed" | "success") {
            progress.cancel_requested = false;
        }
    }
}

/// 判断指定实例的安装任务是否已经收到取消请求。
fn is_install_cancel_requested(instance_id: &str) -> bool {
    let sessions = SUITE_INSTALL_PROGRESS.lock().unwrap();
    sessions
        .get(instance_id)
        .is_some_and(|progress| progress.cancel_requested)
}

/// 执行套件安装事务，只有镜像全部可用后才持久化 Agent 元数据。
async fn install_suite_inner(
    state: &Arc<AppState>,
    payload: &SuiteInstallRequest,
    dir: &FsPath,
) -> ApiResult<()> {
    ensure_install_not_canceled(&payload.instance_id)?;
    tokio::fs::create_dir_all(&dir).await?;

    for file in &payload.files {
        let relative = normalize_relative_path(&file.path)?;
        let target = dir.join(relative);
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let content = STANDARD.decode(&file.content_base64).map_err(|err| {
            ApiError::BadRequest(format!(
                "suite file has invalid base64 content: {}: {err}",
                file.path
            ))
        })?;
        tokio::fs::write(target, content).await?;
    }

    let compose_source = dir.join(normalize_relative_path(&payload.compose_file)?);
    if tokio::fs::metadata(&compose_source).await.is_err() {
        return Err(ApiError::BadRequest(format!(
            "compose file does not exist: {}",
            payload.compose_file
        )));
    }
    let compose_target = dir.join(COMPOSE_FILE_NAME);
    if compose_source != compose_target {
        let compose_text = tokio::fs::read_to_string(&compose_source).await?;
        tokio::fs::write(&compose_target, compose_text).await?;
    }
    prepare_compose_images(state, payload, &compose_target).await?;
    ensure_install_not_canceled(&payload.instance_id)?;

    let metadata = SuiteAgentMetadata {
        instance_id: payload.instance_id.clone(),
        suite_id: payload.suite_id.clone(),
        version: payload.version.clone(),
        compose_project_name: payload.compose_project_name.clone(),
        app_entries: payload.app_entries.clone(),
    };
    let metadata_text = serde_json::to_string_pretty(&metadata)
        .map_err(|err| ApiError::Internal(err.to_string()))?;
    tokio::fs::write(dir.join(SUITE_METADATA_FILE), metadata_text).await?;

    let dir_str = dir.to_string_lossy().to_string();
    sqlx::query(
        "INSERT INTO docker_compose_projects (name, compose_dir, project_type) VALUES (?1, ?2, 'suite')",
    )
    .bind(&payload.compose_project_name)
    .bind(&dir_str)
    .execute(&state.metadata_db)
    .await?;

    Ok(())
}

/// 在安装关键步骤前检查取消状态，已取消时中止安装事务。
fn ensure_install_not_canceled(instance_id: &str) -> ApiResult<()> {
    if is_install_cancel_requested(instance_id) {
        return Err(ApiError::BadRequest("suite install canceled".to_string()));
    }
    Ok(())
}

/// 安装失败时尽力清理尚未登记的套件目录。
async fn rollback_suite_install(project: &str, dir: &FsPath) {
    if tokio::fs::metadata(dir).await.is_ok()
        && let Err(err) = tokio::fs::remove_dir_all(dir).await
    {
        tracing::error!(
            project,
            path = %dir.display(),
            error = %err,
            "failed to rollback suite directory"
        );
    }
}

/// 确保稳定的套件项目名和目录没有被其他 Compose 项目占用。
async fn ensure_suite_project_available(
    state: &Arc<AppState>,
    project: &str,
    dir: &FsPath,
) -> ApiResult<()> {
    let registered = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM docker_compose_projects WHERE name = ?1",
    )
    .bind(project)
    .fetch_one(&state.metadata_db)
    .await?;
    if registered > 0 {
        return Err(ApiError::BadRequest(format!(
            "compose project name is already in use: {project}"
        )));
    }
    if tokio::fs::metadata(dir).await.is_ok() {
        return Err(ApiError::BadRequest(format!(
            "suite compose directory already exists: {}",
            dir.display()
        )));
    }
    Ok(())
}

/// 启用套件实例。
pub async fn enable_suite(
    State(state): State<Arc<AppState>>,
    Path(project): Path<String>,
    Json(payload): Json<SuiteActionRequest>,
) -> ApiResult<Response> {
    validate_suite_project(&project, &payload.compose_project_name)?;
    ensure_suite_network(&state).await?;
    let compose_file = suite_project_dir(&project).join(COMPOSE_FILE_NAME);
    run_compose_command(&payload.compose_project_name, &compose_file, &["up", "-d"]).await?;
    Ok(ApiResponse::ok("Suite enabled").into_response())
}

/// 停用套件实例。
pub async fn disable_suite(
    Path(project): Path<String>,
    Json(payload): Json<SuiteActionRequest>,
) -> ApiResult<Response> {
    validate_suite_project(&project, &payload.compose_project_name)?;
    let compose_file = suite_project_dir(&project).join(COMPOSE_FILE_NAME);
    run_compose_command(&payload.compose_project_name, &compose_file, &["stop"]).await?;
    Ok(ApiResponse::ok("Suite disabled").into_response())
}

/// 卸载套件实例，默认不删除 named volume；用户明确选择删除数据时执行 `down -v`。
pub async fn uninstall_suite(
    State(state): State<Arc<AppState>>,
    Path(project): Path<String>,
    Json(payload): Json<SuiteActionRequest>,
) -> ApiResult<Response> {
    validate_suite_project(&project, &payload.compose_project_name)?;
    let dir = suite_project_dir(&project);
    let compose_file = dir.join(COMPOSE_FILE_NAME);
    if tokio::fs::metadata(&compose_file).await.is_ok() {
        let args = if payload.remove_data {
            vec!["down", "-v"]
        } else {
            vec!["down"]
        };
        run_compose_command(&payload.compose_project_name, &compose_file, &args).await?;
    }
    sqlx::query("DELETE FROM docker_compose_projects WHERE name = ?1")
        .bind(&payload.compose_project_name)
        .execute(&state.metadata_db)
        .await?;
    if tokio::fs::metadata(&dir).await.is_ok() {
        tokio::fs::remove_dir_all(dir).await?;
    }
    Ok(ApiResponse::ok("Suite uninstalled").into_response())
}

/// 代理套件 Web 入口。
pub async fn proxy_suite_entry(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    Path(path): Path<SuiteProxyPath>,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> ApiResult<Response> {
    validate_project_name(&path.project)?;
    validate_id("entry_id", &path.entry_id)?;
    let metadata = read_suite_metadata(&path.project).await?;
    let entry = metadata
        .app_entries
        .iter()
        .find(|item| item.id == path.entry_id)
        .ok_or_else(|| {
            ApiError::BadRequest(format!("suite app entry not found: {}", path.entry_id))
        })?;
    if entry.entry_type != "proxied_web" {
        return Err(ApiError::BadRequest(
            "suite entry is not a proxied web entry".to_string(),
        ));
    }
    let service = entry
        .service
        .as_deref()
        .ok_or_else(|| ApiError::BadRequest("suite entry service is missing".to_string()))?;
    let port = entry
        .port
        .ok_or_else(|| ApiError::BadRequest("suite entry port is missing".to_string()))?;
    let container_ip = resolve_service_ip(&state, &metadata.compose_project_name, service).await?;
    let suffix = proxy_suffix(
        uri.path_and_query()
            .map(|value| value.as_str())
            .unwrap_or(uri.path()),
        &path.project,
        &path.entry_id,
    );
    let target_url = format!("http://{container_ip}:{port}{suffix}");

    let client = reqwest::Client::new();
    let mut request = client.request(method, target_url);
    for (name, value) in headers.iter() {
        if name.as_str().eq_ignore_ascii_case("host") {
            continue;
        }
        request = request.header(name, value);
    }
    let body_stream = body
        .into_data_stream()
        .map(|chunk| chunk.map_err(std::io::Error::other));
    let response = request
        .body(reqwest::Body::wrap_stream(body_stream))
        .send()
        .await?;

    let status = response.status();
    let mut headers = response.headers().clone();
    strip_hop_by_hop_headers(&mut headers);
    let stream = response
        .bytes_stream()
        .map(|chunk| chunk.map_err(std::io::Error::other));
    let body = Body::from_stream(stream);
    let mut proxied = Response::builder().status(status).body(body)?;
    *proxied.headers_mut() = headers;
    Ok(proxied)
}

fn strip_hop_by_hop_headers(headers: &mut HeaderMap) {
    headers.remove(header::CONNECTION);
    headers.remove(header::TRANSFER_ENCODING);
    headers.remove(header::CONTENT_LENGTH);
    headers.remove(header::UPGRADE);
    headers.remove(header::PROXY_AUTHENTICATE);
    headers.remove(header::PROXY_AUTHORIZATION);
    headers.remove(header::TE);
    headers.remove(header::TRAILER);
    headers.remove(HeaderName::from_static("keep-alive"));
}

async fn ensure_suite_network(state: &Arc<AppState>) -> ApiResult<()> {
    let docker = state.docker_client().await?;
    let labels = suite_network_labels();
    if let Ok(network) = docker
        .inspect_network(
            SUITE_NETWORK_NAME,
            Some(query_parameters::InspectNetworkOptions::default()),
        )
        .await
    {
        let current_labels = network.labels.as_ref();
        let has_required_labels = labels
            .iter()
            .all(|(key, value)| current_labels.and_then(|items| items.get(key)) == Some(value));
        if has_required_labels {
            return Ok(());
        }

        let has_attached_containers = network
            .containers
            .as_ref()
            .is_some_and(|items| !items.is_empty());
        if has_attached_containers {
            tracing::warn!(
                network = SUITE_NETWORK_NAME,
                "suite network exists without standard labels and is currently in use"
            );
            return Ok(());
        }
        docker.remove_network(SUITE_NETWORK_NAME).await?;
    }

    let request = NetworkCreateRequest {
        name: SUITE_NETWORK_NAME.to_string(),
        driver: Some("bridge".to_string()),
        labels: Some(labels),
        ..Default::default()
    };
    docker.create_network(request).await?;
    Ok(())
}

fn suite_network_labels() -> HashMap<String, String> {
    HashMap::from([("seclab.owner".to_string(), "suite".to_string())])
}

async fn resolve_service_ip(
    state: &Arc<AppState>,
    project: &str,
    service: &str,
) -> ApiResult<String> {
    let docker = state.docker_client().await?;
    let mut filters = HashMap::new();
    filters.insert(
        "label".to_string(),
        vec![
            format!("com.docker.compose.project={project}"),
            format!("com.docker.compose.service={service}"),
        ],
    );
    filters.insert("status".to_string(), vec!["running".to_string()]);
    let options = query_parameters::ListContainersOptionsBuilder::new()
        .all(true)
        .filters(&filters)
        .build();
    let containers = docker.list_containers(Some(options)).await?;
    let container_id = containers
        .first()
        .and_then(|container| container.id.as_deref())
        .ok_or_else(|| ApiError::BadRequest(format!("suite service is not running: {service}")))?;
    let detail = docker
        .inspect_container(
            container_id,
            None::<query_parameters::InspectContainerOptions>,
        )
        .await?;
    let ip = detail
        .network_settings
        .and_then(|settings| settings.networks)
        .and_then(|mut networks| networks.remove(SUITE_NETWORK_NAME))
        .and_then(|endpoint| endpoint.ip_address)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApiError::BadRequest(format!(
                "suite service is not connected to {SUITE_NETWORK_NAME}: {service}"
            ))
        })?;
    Ok(ip)
}

async fn read_suite_metadata(project: &str) -> ApiResult<SuiteAgentMetadata> {
    let text =
        tokio::fs::read_to_string(suite_project_dir(project).join(SUITE_METADATA_FILE)).await?;
    serde_json::from_str::<SuiteAgentMetadata>(&text)
        .map_err(|err| ApiError::BadRequest(format!("invalid suite metadata: {err}")))
}

fn proxy_suffix(path_with_query: &str, project: &str, entry_id: &str) -> String {
    let prefix = format!("/api/v1/agent/docker/suite/{project}/proxy/{entry_id}");
    let suffix = path_with_query.strip_prefix(&prefix).unwrap_or("/");
    if suffix.is_empty() {
        "/".to_string()
    } else {
        suffix.to_string()
    }
}

fn suite_project_dir(project: &str) -> PathBuf {
    config::compose_root_dir().join("suite").join(project)
}

fn normalize_relative_path(path: &str) -> ApiResult<PathBuf> {
    let p = FsPath::new(path);
    let mut normalized = PathBuf::new();
    for component in p.components() {
        match component {
            std::path::Component::Normal(value) => normalized.push(value),
            std::path::Component::CurDir => {}
            _ => {
                return Err(ApiError::BadRequest(
                    "suite file path must be relative and safe".to_string(),
                ));
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(ApiError::BadRequest("suite file path is empty".to_string()));
    }
    Ok(normalized)
}

fn validate_id(label: &str, value: &str) -> ApiResult<()> {
    if value.trim().is_empty()
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(ApiError::BadRequest(format!(
            "{label} may only contain letters, digits, hyphen, underscore, and dot"
        )));
    }
    Ok(())
}

fn validate_project_name(value: &str) -> ApiResult<()> {
    validate_id("compose_project_name", value)
}

fn validate_suite_project(path_project: &str, payload_project: &str) -> ApiResult<()> {
    validate_project_name(path_project)?;
    validate_project_name(payload_project)?;
    if path_project != payload_project {
        return Err(ApiError::BadRequest(
            "suite compose project does not match request path".to_string(),
        ));
    }
    Ok(())
}

/// 解析 Compose 中的镜像，并确保每个镜像在目标节点本地可用。
async fn prepare_compose_images(
    state: &Arc<AppState>,
    payload: &SuiteInstallRequest,
    compose_file: &FsPath,
) -> ApiResult<()> {
    update_install_progress(
        &payload.instance_id,
        10,
        "running",
        "resolve_images",
        None,
        false,
        None,
    );
    let output = Command::new("docker")
        .args(["compose", "-f"])
        .arg(compose_file)
        .args(["-p", &payload.compose_project_name, "config", "--images"])
        .output()
        .await?;
    if !output.status.success() {
        let detail = command_error_detail(&output);
        tracing::error!(
            project = %payload.compose_project_name,
            compose_file = %compose_file.display(),
            status = ?output.status.code(),
            stderr = %String::from_utf8_lossy(&output.stderr).trim(),
            "failed to resolve suite compose images"
        );
        return Err(ApiError::BadRequest(format!(
            "invalid suite compose configuration: {detail}"
        )));
    }

    let images = parse_compose_images(&output.stdout);
    if images.is_empty() {
        return Err(ApiError::BadRequest(
            "suite compose configuration must declare at least one image".to_string(),
        ));
    }

    let images: Vec<String> = images.into_iter().collect();
    let image_count = images.len().max(1);
    for (index, image) in images.iter().enumerate() {
        ensure_install_not_canceled(&payload.instance_id)?;
        ensure_image_available(state, &payload.instance_id, image, index, image_count).await?;
    }
    Ok(())
}

/// 解析并去重 `docker compose config --images` 的输出。
fn parse_compose_images(output: &[u8]) -> BTreeSet<String> {
    String::from_utf8_lossy(output)
        .lines()
        .map(str::trim)
        .filter(|image| !image.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// 本地镜像存在时直接复用，否则从镜像仓库拉取固定版本镜像。
async fn ensure_image_available(
    state: &Arc<AppState>,
    instance_id: &str,
    image: &str,
    image_index: usize,
    image_count: usize,
) -> ApiResult<()> {
    ensure_install_not_canceled(instance_id)?;
    let base_progress = image_progress(image_index, image_count, 0);
    update_install_progress(
        instance_id,
        base_progress,
        "running",
        "check_image",
        Some(image.to_string()),
        false,
        None,
    );
    let inspect = Command::new("docker")
        .args(["image", "inspect", image])
        .output()
        .await?;
    if inspect.status.success() {
        update_install_progress(
            instance_id,
            image_progress(image_index, image_count, 100),
            "running",
            "image_ready",
            Some(image.to_string()),
            false,
            None,
        );
        return Ok(());
    }

    ensure_install_not_canceled(instance_id)?;
    pull_image_with_progress(state, instance_id, image, image_index, image_count).await
}

/// 通过 Docker API 拉取镜像，并把 pull stream 转换为套件安装进度。
async fn pull_image_with_progress(
    state: &Arc<AppState>,
    instance_id: &str,
    image: &str,
    image_index: usize,
    image_count: usize,
) -> ApiResult<()> {
    let docker = state.docker_client().await?;
    let (from_image, tag) = split_image_name(image);
    let options = CreateImageOptions {
        from_image: Some(from_image),
        tag: Some(tag),
        ..Default::default()
    };
    let mut stream = docker.create_image(Some(options), None, None);

    update_install_progress(
        instance_id,
        image_progress(image_index, image_count, 5),
        "running",
        "pull_image",
        Some(image.to_string()),
        false,
        None,
    );

    while let Some(message) = stream.next().await {
        ensure_install_not_canceled(instance_id)?;
        let info = message.map_err(|err| {
            ApiError::BadRequest(format!("suite image `{image}` pull failed: {err}"))
        })?;
        if let Some(error) = info.error {
            return Err(ApiError::BadRequest(format!(
                "suite image `{image}` pull failed: {error}"
            )));
        }
        if let Some(detail) = info.progress_detail
            && let (Some(current), Some(total)) = (detail.current, detail.total)
            && total > 0
        {
            let layer_percent = ((current as f64 / total as f64) * 100.0).round() as u32;
            update_install_progress(
                instance_id,
                image_progress(image_index, image_count, layer_percent),
                "running",
                "pull_image",
                Some(image.to_string()),
                false,
                None,
            );
        }
    }

    update_install_progress(
        instance_id,
        image_progress(image_index, image_count, 100),
        "running",
        "image_ready",
        Some(image.to_string()),
        false,
        None,
    );
    Ok(())
}

/// 将单个镜像的拉取百分比映射到整个安装任务的进度区间。
fn image_progress(image_index: usize, image_count: usize, image_percent: u32) -> u32 {
    let span = 75.0 / image_count as f64;
    let value = 15.0 + span * image_index as f64 + span * (image_percent.min(100) as f64 / 100.0);
    value.round().clamp(15.0, 90.0) as u32
}

/// 拆分 Docker 镜像名和标签，兼容包含 registry 端口的镜像地址。
fn split_image_name(image: &str) -> (String, String) {
    let Some((head, tail)) = image.rsplit_once('/') else {
        return split_image_tag(image);
    };
    let (name, tag) = split_image_tag(tail);
    (format!("{head}/{name}"), tag)
}

/// 拆分不含路径前缀的镜像名和标签，未声明标签时默认 latest。
fn split_image_tag(image: &str) -> (String, String) {
    if let Some((name, tag)) = image.rsplit_once(':') {
        return (name.to_string(), tag.to_string());
    }
    (image.to_string(), "latest".to_string())
}

/// 提取 Docker 命令的有效错误文本。
fn command_error_detail(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim();
    if !detail.is_empty() {
        return detail.to_string();
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = stdout.trim();
    if !detail.is_empty() {
        return detail.to_string();
    }
    match output.status.code() {
        Some(code) => format!("docker command exited with code {code}"),
        None => "docker command terminated without an exit code".to_string(),
    }
}

async fn run_compose_command(project: &str, compose_file: &FsPath, args: &[&str]) -> ApiResult<()> {
    if tokio::fs::metadata(compose_file).await.is_err() {
        return Err(ApiError::BadRequest(format!(
            "compose file does not exist: {}",
            compose_file.display()
        )));
    }
    let output = Command::new("docker")
        .args(["compose", "-f"])
        .arg(compose_file)
        .args(["-p", project])
        .args(args)
        .output()
        .await?;

    if !output.status.success() {
        let detail = command_error_detail(&output);
        tracing::error!(
            project,
            compose_file = %compose_file.display(),
            args = ?args,
            status = ?output.status.code(),
            stdout = %String::from_utf8_lossy(&output.stdout).trim(),
            stderr = %String::from_utf8_lossy(&output.stderr).trim(),
            "suite compose command failed"
        );
        return Err(ApiError::BadRequest(format!(
            "suite compose command failed: {detail}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_compose_images;

    #[test]
    fn parse_compose_images_removes_blank_lines_and_duplicates() {
        let images = parse_compose_images(
            b"docker.io/library/nginx:1.27-alpine\n\n custom/app:0.1.0-alpha.1 \ncustom/app:0.1.0-alpha.1\n",
        );

        assert_eq!(
            images.into_iter().collect::<Vec<_>>(),
            vec![
                "custom/app:0.1.0-alpha.1".to_string(),
                "docker.io/library/nginx:1.27-alpine".to_string(),
            ]
        );
    }
}
