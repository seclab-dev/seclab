//! 套件中心 API：导入、安装、生命周期管理与代理入口。

use crate::models::NodeRuntimeClient;
use crate::models::desktop_apps::{delete_desktop_apps, hide_suite_desktop_apps};
use crate::models::suites::{
    SuiteAppEntryManifest, SuiteAppEntryRecord, SuiteInstanceSummary, SuiteManifest,
    SuitePackageFile, SuitePackageSnapshot, delete_catalog_item, delete_instance,
    delete_instance_app_entries, fetch_catalog_payload, fetch_instance, fetch_instance_by_suite_id,
    insert_instance, list_suites, replace_instance_app_entries, update_instance_status,
    upsert_catalog_item,
};
use crate::state::AppState;
use crate::types::{ApiError, ApiResponse, ApiResult, new_uuid_v7};
use axum::body::Body;
use axum::extract::{Multipart, OriginalUri, Path, State};
use axum::http::{HeaderMap, Method, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use flate2::read::GzDecoder;
use ring::digest;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read};
use std::sync::Arc;

const MAX_SUITE_PACKAGE_BYTES: usize = 50 * 1024 * 1024;
const MAX_SUITE_UNPACKED_BYTES: usize = 50 * 1024 * 1024;
const SUITE_PACKAGE_EXTENSION: &str = ".slsp";
const MIN_SUITE_ICON_SIZE: u32 = 128;

/// 构建套件中心路由集合。
pub fn suites_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/list", get(list))
        .route("/import", post(import_suite))
        .route("/{suite_id}/install", post(install_suite))
        .route("/{suite_id}/delete", post(delete_suite))
        .route("/{suite_id}/assets/{*asset_path}", get(read_catalog_asset))
        .route("/instance/{instance_id}/enable", post(enable_instance))
        .route("/instance/{instance_id}/disable", post(disable_instance))
        .route(
            "/instance/{instance_id}/uninstall",
            post(uninstall_instance),
        )
        .route(
            "/instance/{instance_id}/assets/{*asset_path}",
            get(read_suite_asset),
        )
        .route(
            "/instance/{instance_id}/proxy/{entry_id}/",
            any(proxy_suite_entry),
        )
        .route(
            "/instance/{instance_id}/proxy/{entry_id}/{*path}",
            any(proxy_suite_entry),
        )
        .route(
            "/instance/{instance_id}/proxy/{entry_id}",
            any(proxy_suite_entry),
        )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallSuiteRequest {
    pub node_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallSuiteRequest {
    #[serde(default)]
    pub remove_data: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentSuiteInstallRequest {
    instance_id: String,
    suite_id: String,
    version: String,
    compose_project_name: String,
    compose_file: String,
    files: Vec<SuitePackageFile>,
    app_entries: Vec<SuiteAppEntryManifest>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentSuiteActionRequest {
    compose_project_name: String,
    #[serde(default)]
    remove_data: bool,
}

#[derive(Debug, Deserialize)]
pub struct SuiteCatalogPath {
    suite_id: String,
}

#[derive(Debug, Deserialize)]
pub struct SuiteProxyPath {
    instance_id: String,
    entry_id: String,
}

#[derive(Debug, Deserialize)]
pub struct SuiteAssetPath {
    instance_id: String,
    asset_path: String,
}

#[derive(Debug, Deserialize)]
pub struct SuiteCatalogAssetPath {
    suite_id: String,
    asset_path: String,
}

/// 返回套件目录和实例列表。
pub async fn list(State(state): State<Arc<AppState>>, headers: HeaderMap) -> ApiResult<Response> {
    // 套件清单本地化只依赖请求语言，不把语言状态写入数据库。
    let locale = resolve_request_locale(&headers);
    let data = list_suites(&state.metadata_db, locale.as_deref()).await?;
    Ok(ApiResponse::success_with_raw("Suites loaded", data).into_response())
}

/// 上传并导入套件包。
pub async fn import_suite(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> ApiResult<Response> {
    let bytes = read_suite_upload(&mut multipart).await?;
    let checksum = hex::encode(digest::digest(&digest::SHA256, &bytes).as_ref());
    let (manifest, package) = parse_suite_package(&bytes)?;
    upsert_catalog_item(&state.metadata_db, &manifest, &package, &checksum).await?;
    Ok(ApiResponse::success_with_raw("Suite imported", manifest).into_response())
}

/// 从套件中心删除未安装的套件目录项。
pub async fn delete_suite(
    State(state): State<Arc<AppState>>,
    Path(path): Path<SuiteCatalogPath>,
) -> ApiResult<Response> {
    if fetch_instance_by_suite_id(&state.metadata_db, &path.suite_id)
        .await?
        .is_some()
    {
        return Err(ApiError::BadRequest(
            "suite is installed; uninstall it before deleting".to_string(),
        ));
    }
    fetch_catalog_payload(&state.metadata_db, &path.suite_id)
        .await?
        .ok_or_else(|| ApiError::BadRequest(format!("suite not found: {}", path.suite_id)))?;
    delete_catalog_item(&state.metadata_db, &path.suite_id).await?;
    Ok(ApiResponse::ok("Suite deleted").into_response())
}

/// 安装套件到目标节点，但不自动启用。
pub async fn install_suite(
    State(state): State<Arc<AppState>>,
    Path(suite_id): Path<String>,
    Json(payload): Json<InstallSuiteRequest>,
) -> ApiResult<Response> {
    let node_id = payload.node_id.unwrap_or_else(|| "local".to_string());
    let (manifest, package) = fetch_catalog_payload(&state.metadata_db, &suite_id)
        .await?
        .ok_or_else(|| ApiError::BadRequest(format!("suite not found: {suite_id}")))?;
    if fetch_instance_by_suite_id(&state.metadata_db, &manifest.metadata.suite_id)
        .await?
        .is_some()
    {
        return Err(ApiError::BadRequest(format!(
            "suite is already installed: {}",
            manifest.metadata.suite_id
        )));
    }
    let instance_id = new_uuid_v7();
    let compose_project_name = build_compose_project_name(&manifest.metadata.slug);

    let instance = SuiteInstanceSummary {
        instance_id: instance_id.clone(),
        suite_id: manifest.metadata.suite_id.clone(),
        version: manifest.metadata.version.clone(),
        node_id: node_id.clone(),
        compose_project_name: compose_project_name.clone(),
        status: "installing".to_string(),
        last_error: None,
        created_at: String::new(),
        updated_at: String::new(),
    };
    insert_instance(&state.metadata_db, &instance).await?;

    let client = NodeRuntimeClient::from_node_route(&state.metadata_db, Some(&node_id)).await?;
    let request = AgentSuiteInstallRequest {
        instance_id: instance_id.clone(),
        suite_id: manifest.metadata.suite_id.clone(),
        version: manifest.metadata.version.clone(),
        compose_project_name,
        compose_file: manifest.runtime.compose_file.clone(),
        files: package.files,
        app_entries: manifest.app_entries,
    };

    let agent_response = match client
        .post_json::<serde_json::Value, _>("/api/v1/agent/docker/suites/install", &request)
        .await
    {
        Ok(value) => value,
        Err(err) => {
            let detail = err.to_string();
            compensate_failed_install(&client, &request, &instance_id).await;
            delete_instance(&state.metadata_db, &instance_id).await?;
            return Err(ApiError::Internal(detail));
        }
    };
    if let Err(err) = ensure_agent_success(&agent_response) {
        delete_instance(&state.metadata_db, &instance_id).await?;
        return Err(ApiError::BadRequest(err));
    }

    update_instance_status(&state.metadata_db, &instance_id, "installed", None).await?;
    let installed = fetch_instance(&state.metadata_db, &instance_id).await?;
    Ok(ApiResponse::success_with_raw("Suite installed", installed).into_response())
}

/// Agent 安装请求发生传输异常时，尽力清理可能已经写入的远端安装内容。
async fn compensate_failed_install(
    client: &NodeRuntimeClient,
    request: &AgentSuiteInstallRequest,
    instance_id: &str,
) {
    let action = AgentSuiteActionRequest {
        compose_project_name: request.compose_project_name.clone(),
        remove_data: false,
    };
    let path = format!(
        "/api/v1/agent/docker/suite/{}/uninstall",
        request.compose_project_name
    );
    if let Err(err) = client
        .post_json::<serde_json::Value, _>(&path, &action)
        .await
    {
        tracing::warn!(
            instance_id,
            error = %err,
            "failed to compensate interrupted suite installation"
        );
    }
}

/// 启用套件实例并注册应用入口。
pub async fn enable_instance(
    State(state): State<Arc<AppState>>,
    Path(instance_id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let instance = fetch_instance(&state.metadata_db, &instance_id)
        .await?
        .ok_or_else(|| ApiError::BadRequest(format!("suite instance not found: {instance_id}")))?;
    update_instance_status(&state.metadata_db, &instance_id, "enabling", None).await?;

    let client =
        NodeRuntimeClient::from_node_route(&state.metadata_db, Some(&instance.node_id)).await?;
    let request = AgentSuiteActionRequest {
        compose_project_name: instance.compose_project_name.clone(),
        remove_data: false,
    };
    let agent_response = match client
        .post_json::<serde_json::Value, _>(
            &format!(
                "/api/v1/agent/docker/suite/{}/enable",
                instance.compose_project_name
            ),
            &request,
        )
        .await
    {
        Ok(value) => value,
        Err(err) => {
            let detail = err.to_string();
            update_instance_status(&state.metadata_db, &instance_id, "error", Some(&detail))
                .await?;
            return Err(ApiError::Internal(detail));
        }
    };
    if let Err(err) = ensure_agent_success(&agent_response) {
        update_instance_status(&state.metadata_db, &instance_id, "error", Some(&err)).await?;
        return Err(ApiError::Internal(err));
    }

    let (manifest, _) = fetch_catalog_payload(&state.metadata_db, &instance.suite_id)
        .await?
        .ok_or_else(|| ApiError::BadRequest(format!("suite not found: {}", instance.suite_id)))?;
    let locale = resolve_request_locale(&headers);
    let entries = build_app_entry_records(
        &instance,
        &manifest.metadata.icon,
        &manifest.app_entries,
        &manifest,
        locale.as_deref(),
    );
    replace_instance_app_entries(&state.metadata_db, &instance_id, &entries).await?;
    update_instance_status(&state.metadata_db, &instance_id, "enabled", None).await?;
    let enabled = fetch_instance(&state.metadata_db, &instance_id).await?;
    Ok(ApiResponse::success_with_raw("Suite enabled", enabled).into_response())
}

/// 停用套件实例并隐藏应用入口。
pub async fn disable_instance(
    State(state): State<Arc<AppState>>,
    Path(instance_id): Path<String>,
) -> ApiResult<Response> {
    let instance = fetch_instance(&state.metadata_db, &instance_id)
        .await?
        .ok_or_else(|| ApiError::BadRequest(format!("suite instance not found: {instance_id}")))?;
    update_instance_status(&state.metadata_db, &instance_id, "disabling", None).await?;

    let client =
        NodeRuntimeClient::from_node_route(&state.metadata_db, Some(&instance.node_id)).await?;
    let request = AgentSuiteActionRequest {
        compose_project_name: instance.compose_project_name.clone(),
        remove_data: false,
    };
    let agent_response = match client
        .post_json::<serde_json::Value, _>(
            &format!(
                "/api/v1/agent/docker/suite/{}/disable",
                instance.compose_project_name
            ),
            &request,
        )
        .await
    {
        Ok(value) => value,
        Err(err) => {
            let detail = err.to_string();
            update_instance_status(&state.metadata_db, &instance_id, "error", Some(&detail))
                .await?;
            return Err(ApiError::Internal(detail));
        }
    };
    if let Err(err) = ensure_agent_success(&agent_response) {
        update_instance_status(&state.metadata_db, &instance_id, "error", Some(&err)).await?;
        return Err(ApiError::Internal(err));
    }

    let (manifest, _) = fetch_catalog_payload(&state.metadata_db, &instance.suite_id)
        .await?
        .ok_or_else(|| ApiError::BadRequest(format!("suite not found: {}", instance.suite_id)))?;
    let app_ids = build_suite_app_ids(&instance_id, &manifest.app_entries);
    hide_suite_desktop_apps(&state.metadata_db, &app_ids).await?;
    delete_instance_app_entries(&state.metadata_db, &instance_id).await?;
    update_instance_status(&state.metadata_db, &instance_id, "disabled", None).await?;
    let disabled = fetch_instance(&state.metadata_db, &instance_id).await?;
    Ok(ApiResponse::success_with_raw("Suite disabled", disabled).into_response())
}

/// 卸载套件实例。
pub async fn uninstall_instance(
    State(state): State<Arc<AppState>>,
    Path(instance_id): Path<String>,
    Json(payload): Json<UninstallSuiteRequest>,
) -> ApiResult<Response> {
    let instance = fetch_instance(&state.metadata_db, &instance_id)
        .await?
        .ok_or_else(|| ApiError::BadRequest(format!("suite instance not found: {instance_id}")))?;
    let client =
        NodeRuntimeClient::from_node_route(&state.metadata_db, Some(&instance.node_id)).await?;
    let request = AgentSuiteActionRequest {
        compose_project_name: instance.compose_project_name.clone(),
        remove_data: payload.remove_data,
    };
    client
        .post_json::<serde_json::Value, _>(
            &format!(
                "/api/v1/agent/docker/suite/{}/disable",
                instance.compose_project_name
            ),
            &request,
        )
        .await
        .ok();
    let agent_response = match client
        .post_json::<serde_json::Value, _>(
            &format!(
                "/api/v1/agent/docker/suite/{}/uninstall",
                instance.compose_project_name
            ),
            &request,
        )
        .await
    {
        Ok(value) => value,
        Err(err) => {
            let detail = err.to_string();
            update_instance_status(&state.metadata_db, &instance_id, "error", Some(&detail))
                .await?;
            return Err(ApiError::Internal(detail));
        }
    };
    if let Err(err) = ensure_agent_success(&agent_response) {
        update_instance_status(&state.metadata_db, &instance_id, "error", Some(&err)).await?;
        return Err(ApiError::Internal(err));
    }
    if let Some((manifest, _)) =
        fetch_catalog_payload(&state.metadata_db, &instance.suite_id).await?
    {
        let app_ids = build_suite_app_ids(&instance_id, &manifest.app_entries);
        delete_desktop_apps(&state.metadata_db, &app_ids).await?;
    }
    delete_instance_app_entries(&state.metadata_db, &instance_id).await?;
    delete_instance(&state.metadata_db, &instance_id).await?;
    Ok(ApiResponse::ok("Suite uninstalled").into_response())
}

/// 读取套件目录包内的静态资源。
pub async fn read_catalog_asset(
    State(state): State<Arc<AppState>>,
    Path(path): Path<SuiteCatalogAssetPath>,
) -> ApiResult<Response> {
    let (_, package) = fetch_catalog_payload(&state.metadata_db, &path.suite_id)
        .await?
        .ok_or_else(|| ApiError::BadRequest(format!("suite not found: {}", path.suite_id)))?;
    let asset_path = normalize_asset_path(&format!("assets/{}", path.asset_path))?;
    respond_suite_asset(&package, &asset_path)
}

/// 读取套件交付包内的静态资源。
pub async fn read_suite_asset(
    State(state): State<Arc<AppState>>,
    Path(path): Path<SuiteAssetPath>,
) -> ApiResult<Response> {
    let instance = fetch_instance(&state.metadata_db, &path.instance_id)
        .await?
        .ok_or_else(|| {
            ApiError::BadRequest(format!("suite instance not found: {}", path.instance_id))
        })?;
    let (_, package) = fetch_catalog_payload(&state.metadata_db, &instance.suite_id)
        .await?
        .ok_or_else(|| ApiError::BadRequest(format!("suite not found: {}", instance.suite_id)))?;
    let asset_path = normalize_asset_path(&format!("assets/{}", path.asset_path))?;
    respond_suite_asset(&package, &asset_path)
}

fn respond_suite_asset(package: &SuitePackageSnapshot, asset_path: &str) -> ApiResult<Response> {
    let file = package
        .files
        .iter()
        .find(|file| file.path == asset_path)
        .ok_or_else(|| ApiError::BadRequest(format!("suite asset not found: {asset_path}")))?;
    let content_type = content_type_for_asset(asset_path);
    let content = decode_package_file(file)?;
    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(content))
        .map_err(|err| ApiError::Internal(format!("failed to build suite asset response: {err}")))
}

/// 将套件入口请求代理到目标 Agent。
pub async fn proxy_suite_entry(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    Path(path): Path<SuiteProxyPath>,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> ApiResult<Response> {
    let instance = fetch_instance(&state.metadata_db, &path.instance_id)
        .await?
        .ok_or_else(|| {
            ApiError::BadRequest(format!("suite instance not found: {}", path.instance_id))
        })?;
    if path.entry_id.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "suite app entry id is required".to_string(),
        ));
    }
    if instance.status != "enabled" {
        return Err(ApiError::BadRequest(
            "suite instance is not enabled".to_string(),
        ));
    }
    let client =
        NodeRuntimeClient::from_node_route(&state.metadata_db, Some(&instance.node_id)).await?;
    let raw_path = uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or(uri.path());
    let public_prefix = format!("/api/v1/suites/instance/{}", path.instance_id);
    let agent_prefix = format!(
        "/api/v1/agent/docker/suite/{}",
        instance.compose_project_name
    );
    let agent_path = raw_path.replacen(&public_prefix, &agent_prefix, 1);
    client
        .forward_streaming(method, &agent_path, headers, body)
        .await
}

async fn read_suite_upload(multipart: &mut Multipart) -> ApiResult<Vec<u8>> {
    while let Some(field) = multipart.next_field().await? {
        if field.name() != Some("file") {
            continue;
        }
        let file_name = field.file_name().unwrap_or_default().to_string();
        if !file_name.ends_with(SUITE_PACKAGE_EXTENSION) {
            return Err(ApiError::BadRequest(
                "suite package must be .slsp".to_string(),
            ));
        }
        let bytes = field.bytes().await?;
        if bytes.len() > MAX_SUITE_PACKAGE_BYTES {
            return Err(ApiError::BadRequest(
                "suite package is too large".to_string(),
            ));
        }
        return Ok(bytes.to_vec());
    }
    Err(ApiError::BadRequest(
        "missing suite package file".to_string(),
    ))
}

fn parse_suite_package(bytes: &[u8]) -> ApiResult<(SuiteManifest, SuitePackageSnapshot)> {
    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let mut files = Vec::new();
    let mut unpacked_bytes = 0usize;
    for entry in archive
        .entries()
        .map_err(|err| ApiError::BadRequest(format!("invalid suite archive: {err}")))?
    {
        let mut entry =
            entry.map_err(|err| ApiError::BadRequest(format!("invalid suite entry: {err}")))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry
            .path()
            .map_err(|err| ApiError::BadRequest(format!("invalid suite path: {err}")))?;
        let path = normalize_package_path(path.as_ref())?;
        let declared_size = usize::try_from(entry.header().size().map_err(|err| {
            ApiError::BadRequest(format!("invalid suite entry size: {path}: {err}"))
        })?)
        .map_err(|_| ApiError::BadRequest(format!("suite entry is too large: {path}")))?;
        unpacked_bytes = unpacked_bytes
            .checked_add(declared_size)
            .ok_or_else(|| ApiError::BadRequest("suite unpacked size exceeds limit".to_string()))?;
        if unpacked_bytes > MAX_SUITE_UNPACKED_BYTES {
            return Err(ApiError::BadRequest(
                "suite unpacked size exceeds limit".to_string(),
            ));
        }
        let mut content = Vec::new();
        entry.read_to_end(&mut content)?;
        files.push(SuitePackageFile {
            path,
            content_base64: STANDARD.encode(content),
        });
    }

    strip_common_package_root(&mut files);

    let manifest_text = files
        .iter()
        .find(|file| file.path == "suite.yaml")
        .ok_or_else(|| ApiError::BadRequest("suite.yaml is missing".to_string()))
        .and_then(decode_package_text)?;
    let manifest = serde_yaml::from_str::<SuiteManifest>(&manifest_text)
        .map_err(|err| ApiError::BadRequest(format!("invalid suite.yaml: {err}")))?;
    let compose_file = manifest.runtime.compose_file.trim();
    let compose = files
        .iter()
        .find(|file| file.path == compose_file)
        .ok_or_else(|| ApiError::BadRequest(format!("compose file is missing: {compose_file}")))?;
    decode_package_text(compose)?;
    validate_manifest(&manifest, &files)?;
    Ok((manifest, SuitePackageSnapshot { files }))
}

fn normalize_package_path(path: &std::path::Path) -> ApiResult<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(value) => {
                parts.push(value.to_string_lossy().to_string());
            }
            std::path::Component::CurDir => {}
            _ => {
                return Err(ApiError::BadRequest(
                    "suite package contains unsafe path".to_string(),
                ));
            }
        }
    }
    if parts.is_empty() {
        return Err(ApiError::BadRequest(
            "suite package path is empty".to_string(),
        ));
    }
    Ok(parts.join("/"))
}

fn strip_common_package_root(files: &mut [SuitePackageFile]) {
    if files.iter().any(|file| file.path == "suite.yaml") {
        return;
    }
    let Some(first_root) = files
        .first()
        .and_then(|file| file.path.split('/').next())
        .map(str::to_string)
    else {
        return;
    };
    if first_root.is_empty()
        || !files
            .iter()
            .all(|file| file.path.split('/').next() == Some(first_root.as_str()))
    {
        return;
    }
    for file in files {
        if let Some(stripped) = file.path.strip_prefix(&format!("{first_root}/")) {
            file.path = stripped.to_string();
        }
    }
}

fn validate_manifest(manifest: &SuiteManifest, files: &[SuitePackageFile]) -> ApiResult<()> {
    if manifest.api_version != "seclab.io/v1alpha1" {
        return Err(ApiError::BadRequest(
            "suite apiVersion must be seclab.io/v1alpha1".to_string(),
        ));
    }
    if manifest.kind != "ComposeSuite" {
        return Err(ApiError::BadRequest(
            "suite kind must be ComposeSuite".to_string(),
        ));
    }
    validate_id("suite_id", &manifest.metadata.suite_id)?;
    validate_slug(&manifest.metadata.slug)?;
    if manifest.metadata.version.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "suite version is required".to_string(),
        ));
    }
    if manifest.metadata.name.trim().is_empty() {
        return Err(ApiError::BadRequest("suite name is required".to_string()));
    }
    validate_suite_icon(&manifest.metadata.icon, files, true)?;
    if manifest.runtime.runtime_type != "compose" {
        return Err(ApiError::BadRequest(
            "suite runtime type must be compose".to_string(),
        ));
    }
    if manifest.runtime.compose_file != "compose.yaml" {
        return Err(ApiError::BadRequest(
            "suite compose file must be compose.yaml".to_string(),
        ));
    }
    if manifest.app_entries.is_empty() {
        return Err(ApiError::BadRequest(
            "suite must declare at least one app entry".to_string(),
        ));
    }
    for entry in &manifest.app_entries {
        validate_id("app entry id", &entry.id)?;
        if !matches!(entry.entry_type.as_str(), "proxied_web" | "compose_detail") {
            return Err(ApiError::BadRequest(format!(
                "unsupported suite app entry type: {}",
                entry.entry_type
            )));
        }
        if entry.entry_type == "proxied_web" && (entry.service.is_none() || entry.port.is_none()) {
            return Err(ApiError::BadRequest(
                "proxied_web entry must declare service and port".to_string(),
            ));
        }
        if !entry.icon.trim().is_empty() {
            validate_suite_icon(&entry.icon, files, true)?;
        }
    }
    validate_i18n(manifest)?;
    Ok(())
}

fn validate_suite_icon(
    icon_path: &str,
    files: &[SuitePackageFile],
    require_png_dimensions: bool,
) -> ApiResult<()> {
    let icon_path = normalize_asset_path(icon_path.trim())?;
    if !icon_path.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '/' | '.' | '-' | '_')
    }) {
        return Err(ApiError::BadRequest(format!(
            "suite icon path contains unsupported characters: {icon_path}"
        )));
    }
    let extension = std::path::Path::new(&icon_path)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| ApiError::BadRequest("suite icon extension is required".to_string()))?;
    if !matches!(extension.as_str(), "png" | "webp" | "svg") {
        return Err(ApiError::BadRequest(format!(
            "unsupported suite icon format: {icon_path}"
        )));
    }
    let file = files
        .iter()
        .find(|file| file.path == icon_path)
        .ok_or_else(|| ApiError::BadRequest(format!("suite icon is missing: {icon_path}")))?;
    let content = decode_package_file(file)?;
    if extension == "png" && require_png_dimensions {
        validate_png_icon(&content, &icon_path)?;
    }
    Ok(())
}

fn validate_png_icon(content: &[u8], path: &str) -> ApiResult<()> {
    let decoder = png::Decoder::new(Cursor::new(content));
    let mut reader = decoder.read_info().map_err(|err| {
        ApiError::BadRequest(format!("suite icon is not a valid PNG: {path}: {err}"))
    })?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).map_err(|err| {
        ApiError::BadRequest(format!("suite icon is not a valid PNG: {path}: {err}"))
    })?;
    let width = info.width;
    let height = info.height;
    if width != height {
        return Err(ApiError::BadRequest(format!(
            "suite PNG icon must be square: {path}"
        )));
    }
    if width < MIN_SUITE_ICON_SIZE {
        return Err(ApiError::BadRequest(format!(
            "suite PNG icon must be at least {MIN_SUITE_ICON_SIZE}x{MIN_SUITE_ICON_SIZE}: {path}"
        )));
    }
    Ok(())
}

fn decode_package_file(file: &SuitePackageFile) -> ApiResult<Vec<u8>> {
    STANDARD.decode(&file.content_base64).map_err(|err| {
        ApiError::BadRequest(format!(
            "suite file has invalid base64 content: {}: {err}",
            file.path
        ))
    })
}

fn decode_package_text(file: &SuitePackageFile) -> ApiResult<String> {
    let bytes = STANDARD.decode(&file.content_base64).map_err(|err| {
        ApiError::BadRequest(format!(
            "suite file has invalid base64 content: {}: {err}",
            file.path
        ))
    })?;
    String::from_utf8(bytes)
        .map_err(|_| ApiError::BadRequest(format!("suite file must be UTF-8 text: {}", file.path)))
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

fn validate_slug(slug: &str) -> ApiResult<()> {
    if slug.trim().is_empty()
        || slug.starts_with('-')
        || slug.ends_with('-')
        || !slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(ApiError::BadRequest(
            "suite slug may only contain lowercase letters, digits, and hyphen".to_string(),
        ));
    }
    Ok(())
}

fn validate_i18n(manifest: &SuiteManifest) -> ApiResult<()> {
    let Some(i18n) = &manifest.i18n else {
        return Ok(());
    };
    validate_locale_tag("defaultLocale", &i18n.default_locale)?;
    // i18n 只能覆盖已声明应用入口，避免打包后出现无法生效的翻译配置。
    let entry_ids = manifest
        .app_entries
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    for (locale, override_value) in &i18n.locales {
        validate_locale_tag("locale", locale)?;
        for entry_id in override_value.app_entries.keys() {
            if !entry_ids.contains(entry_id.as_str()) {
                return Err(ApiError::BadRequest(format!(
                    "suite i18n references unknown app entry: {entry_id}"
                )));
            }
        }
    }
    Ok(())
}

fn validate_locale_tag(label: &str, value: &str) -> ApiResult<()> {
    if value.trim().is_empty() || !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(ApiError::BadRequest(format!(
            "suite i18n {label} may only contain letters, digits, and hyphen"
        )));
    }
    Ok(())
}

fn build_compose_project_name(slug: &str) -> String {
    format!("seclab-{slug}")
}

fn build_app_entry_records(
    instance: &SuiteInstanceSummary,
    suite_icon: &str,
    entries: &[SuiteAppEntryManifest],
    manifest: &SuiteManifest,
    locale: Option<&str>,
) -> Vec<SuiteAppEntryRecord> {
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let width = entry
                .window
                .as_ref()
                .and_then(|window| window.width)
                .unwrap_or(entry.default_width);
            let height = entry
                .window
                .as_ref()
                .and_then(|window| window.height)
                .unwrap_or(entry.default_height);
            let min_width = entry
                .window
                .as_ref()
                .and_then(|window| window.min_width)
                .unwrap_or(entry.min_width);
            let min_height = entry
                .window
                .as_ref()
                .and_then(|window| window.min_height)
                .unwrap_or(entry.min_height);
            let app_id = format!("suite:{}:{}", instance.instance_id, entry.id);
            let entry_target = if entry.entry_type == "compose_detail" {
                instance.compose_project_name.clone()
            } else {
                build_suite_proxy_target(&instance.instance_id, &entry.id, entry.path.as_deref())
            };
            SuiteAppEntryRecord {
                app_id,
                suite_instance_id: instance.instance_id.clone(),
                app_entry_id: entry.id.clone(),
                title: manifest.localized_app_entry_title(entry, locale),
                icon: resolve_suite_icon(instance, suite_icon, entry),
                entry_type: entry.entry_type.clone(),
                entry_target,
                default_width: width,
                default_height: height,
                min_width,
                min_height,
                sort_order: 10_000 + index as i64,
                enabled: 1,
            }
        })
        .collect()
}

fn resolve_request_locale(headers: &HeaderMap) -> Option<String> {
    let raw = headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|value| value.to_str().ok())?;
    // 当前只使用首选语言；质量权重不影响 SecLab 目前的 zh-CN/en-US 双语选择。
    raw.split(',')
        .filter_map(|part| part.trim().split(';').next())
        .map(str::trim)
        .find(|part| !part.is_empty())
        .map(|part| part.replace('_', "-"))
}

fn build_suite_app_ids(instance_id: &str, entries: &[SuiteAppEntryManifest]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| format!("suite:{instance_id}:{}", entry.id))
        .collect()
}

fn resolve_suite_icon(
    instance: &SuiteInstanceSummary,
    suite_icon: &str,
    entry: &SuiteAppEntryManifest,
) -> String {
    let icon = if entry.icon.trim().is_empty() {
        suite_icon
    } else {
        entry.icon.trim()
    };
    build_suite_asset_url(&instance.instance_id, icon)
}

fn build_suite_asset_url(instance_id: &str, asset_path: &str) -> String {
    format!("/api/v1/suites/instance/{instance_id}/{asset_path}")
}

fn normalize_asset_path(asset_path: &str) -> ApiResult<String> {
    let path = normalize_package_path(std::path::Path::new(asset_path))?;
    if !path.starts_with("assets/") {
        return Err(ApiError::BadRequest(
            "suite asset path must be under assets/".to_string(),
        ));
    }
    Ok(path)
}

fn content_type_for_asset(asset_path: &str) -> &'static str {
    match std::path::Path::new(asset_path)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn build_suite_proxy_target(instance_id: &str, entry_id: &str, entry_path: Option<&str>) -> String {
    let base = format!("/api/v1/suites/instance/{instance_id}/proxy/{entry_id}");
    let Some(path) = entry_path.map(str::trim).filter(|value| !value.is_empty()) else {
        return format!("{base}/");
    };
    if path == "/" {
        return format!("{base}/");
    }
    if path.starts_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    }
}

fn ensure_agent_success(value: &serde_json::Value) -> Result<(), String> {
    if value
        .get("success")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(());
    }
    let message = value
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("agent operation failed");
    let detail = match value.get("data") {
        Some(serde_json::Value::String(detail)) => detail.clone(),
        Some(detail) if !detail.is_null() => detail.to_string(),
        _ => String::new(),
    };
    if detail.is_empty() || detail == message {
        Err(message.to_string())
    } else {
        Err(format!("{message}: {detail}"))
    }
}

#[cfg(test)]
mod suite_asset_tests {
    use super::*;
    use png::{BitDepth, ColorType, Encoder};

    fn png_image(width: u32, height: u32) -> Vec<u8> {
        let mut content = Vec::new();
        {
            let mut encoder = Encoder::new(&mut content, width, height);
            encoder.set_color(ColorType::Rgba);
            encoder.set_depth(BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            let pixels = vec![0; width as usize * height as usize * 4];
            writer.write_image_data(&pixels).unwrap();
        }
        content
    }

    #[test]
    fn png_icon_requires_square_minimum_dimensions() {
        assert!(validate_png_icon(&png_image(256, 256), "assets/suite-icon.png").is_ok());
        assert!(validate_png_icon(&png_image(256, 128), "assets/suite-icon.png").is_err());
        assert!(validate_png_icon(&png_image(64, 64), "assets/suite-icon.png").is_err());
        assert!(validate_png_icon(b"not-png", "assets/suite-icon.png").is_err());
    }

    #[test]
    fn suite_asset_content_types_cover_supported_images() {
        assert_eq!(content_type_for_asset("assets/suite-icon.png"), "image/png");
        assert_eq!(
            content_type_for_asset("assets/suite-icon.webp"),
            "image/webp"
        );
        assert_eq!(
            content_type_for_asset("assets/suite-icon.svg"),
            "image/svg+xml; charset=utf-8"
        );
        assert_eq!(
            content_type_for_asset("assets/readme.txt"),
            "application/octet-stream"
        );
    }

    #[test]
    fn package_file_base64_round_trips_binary_content() {
        let expected = vec![0, 159, 146, 150, 255];
        let file = SuitePackageFile {
            path: "assets/suite-icon.png".to_string(),
            content_base64: STANDARD.encode(&expected),
        };
        assert_eq!(decode_package_file(&file).unwrap(), expected);
    }

    #[test]
    fn app_entry_inherits_suite_icon() {
        let instance = SuiteInstanceSummary {
            instance_id: "instance-1".to_string(),
            suite_id: "seclab.example".to_string(),
            version: "0.1.0-alpha.1".to_string(),
            node_id: "local".to_string(),
            compose_project_name: "seclab-example".to_string(),
            status: "enabled".to_string(),
            last_error: None,
            created_at: String::new(),
            updated_at: String::new(),
        };
        let entry = SuiteAppEntryManifest {
            id: "main".to_string(),
            title: "Example".to_string(),
            icon: String::new(),
            entry_type: "proxied_web".to_string(),
            service: Some("web".to_string()),
            port: Some(8080),
            path: Some("/".to_string()),
            target: None,
            default_width: 1024,
            default_height: 720,
            min_width: 860,
            min_height: 560,
            window: None,
        };
        assert_eq!(
            resolve_suite_icon(&instance, "assets/suite-icon.png", &entry),
            "/api/v1/suites/instance/instance-1/assets/suite-icon.png"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{build_compose_project_name, build_suite_proxy_target};

    #[test]
    fn compose_project_name_uses_slug_and_does_not_include_instance_id() {
        assert_eq!(
            build_compose_project_name("host-scanner"),
            "seclab-host-scanner"
        );
    }

    #[test]
    fn suite_proxy_target_keeps_web_entry_as_directory() {
        assert_eq!(
            build_suite_proxy_target("instance-1", "main", Some("/")),
            "/api/v1/suites/instance/instance-1/proxy/main/"
        );
        assert_eq!(
            build_suite_proxy_target("instance-1", "main", None),
            "/api/v1/suites/instance/instance-1/proxy/main/"
        );
        assert_eq!(
            build_suite_proxy_target("instance-1", "main", Some("/console")),
            "/api/v1/suites/instance/instance-1/proxy/main/console"
        );
    }
}
