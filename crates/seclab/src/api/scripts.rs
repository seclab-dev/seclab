//! 脚本库公共 API：脚本资产、乐观修订、异步运行、历史和有界输出。

use crate::{
    api::auth::AuthenticatedAdmin,
    models::{
        logging::{LogModule, LogStatus, PlatformLogLevel},
        node_runtime_client::NodeRuntimeClient,
        scripts::{self, ScriptActor, ScriptListFilter, ScriptRunListFilter},
    },
    services::{
        logging::{self, OperationEventBuilder},
        node_read_model,
    },
    state::AppState,
    types::{ApiError, ApiResponse, ApiResult},
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header::HeaderName},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use seclab_contracts::{
    api::ErrorCode,
    scripts::{CreateScriptRequest, CreateScriptRunRequest, ScriptRunStatus, UpdateScriptRequest},
};
use serde::Deserialize;
use serde_json::json;
use std::{net::IpAddr, sync::Arc};

static IDEMPOTENCY_KEY: HeaderName = HeaderName::from_static("idempotency-key");

/// 构建脚本资产路由。
pub fn scripts_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{script_id}", get(detail).patch(update).delete(remove))
        .route("/{script_id}/runs", post(start_run))
}

/// 构建全局脚本运行路由。
pub fn script_runs_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_runs))
        .route("/{run_id}", get(run_detail))
        .route("/{run_id}/output", get(run_output))
        .route("/{run_id}/cancel", post(cancel_run))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListQuery {
    keyword: Option<String>,
    #[serde(default = "default_page")]
    page: u32,
    #[serde(default = "default_page_size")]
    page_size: u32,
    #[serde(default = "default_sort_by")]
    sort_by: String,
    #[serde(default = "default_sort_order")]
    sort_order: String,
}

async fn list(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListQuery>,
) -> ApiResult<Response> {
    validate_page(query.page, query.page_size)?;
    if !["name", "updatedAt"].contains(&query.sort_by.as_str())
        || !["asc", "desc"].contains(&query.sort_order.as_str())
    {
        return Err(ApiError::bad_request(
            ErrorCode::BadRequest,
            "invalid script sort query",
        ));
    }
    let data = scripts::list(
        &state.metadata_db,
        &ScriptListFilter {
            keyword: query.keyword.as_deref().map(str::trim),
            page: query.page,
            page_size: query.page_size,
            sort_by: &query.sort_by,
            sort_order: &query.sort_order,
        },
    )
    .await?;
    Ok(ApiResponse::success_with_raw("Scripts loaded", Some(data)).into_response())
}

async fn detail(
    State(state): State<Arc<AppState>>,
    Path(script_id): Path<String>,
) -> ApiResult<Response> {
    let data = scripts::detail(&state.metadata_db, &script_id).await?;
    Ok(ApiResponse::success_with_raw("Script loaded", Some(data)).into_response())
}

async fn create(
    State(state): State<Arc<AppState>>,
    admin: AuthenticatedAdmin,
    headers: HeaderMap,
    Json(request): Json<CreateScriptRequest>,
) -> ApiResult<Response> {
    let actor = actor(&admin, &headers)?;
    let result = scripts::create(&state.metadata_db, &request, &actor).await;
    let target_id = result
        .as_ref()
        .ok()
        .map(|script| script.summary.script_id.as_str());
    let target_name = result
        .as_ref()
        .ok()
        .map(|script| script.summary.name.as_str());
    record_change(
        &state,
        &admin,
        &actor,
        "script_created",
        "POST",
        target_id,
        target_name,
        None,
        false,
        &result,
    );
    let data = result?;
    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::success_with_raw("Script created", data)),
    )
        .into_response())
}

async fn update(
    State(state): State<Arc<AppState>>,
    admin: AuthenticatedAdmin,
    headers: HeaderMap,
    Path(script_id): Path<String>,
    Json(request): Json<UpdateScriptRequest>,
) -> ApiResult<Response> {
    let actor = actor(&admin, &headers)?;
    let result = scripts::update(&state.metadata_db, &script_id, &request, &actor).await;
    record_change(
        &state,
        &admin,
        &actor,
        "script_updated",
        "PATCH",
        Some(&script_id),
        Some(request.name.as_str()),
        None,
        false,
        &result,
    );
    let data = result?;
    Ok(ApiResponse::success_with_raw("Script updated", Some(data)).into_response())
}

async fn remove(
    State(state): State<Arc<AppState>>,
    admin: AuthenticatedAdmin,
    headers: HeaderMap,
    Path(script_id): Path<String>,
) -> ApiResult<Response> {
    let actor = actor(&admin, &headers)?;
    let result = scripts::remove(&state.metadata_db, &script_id).await;
    record_change(
        &state,
        &admin,
        &actor,
        "script_removed",
        "DELETE",
        Some(&script_id),
        None,
        None,
        true,
        &result,
    );
    result?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn start_run(
    State(state): State<Arc<AppState>>,
    admin: AuthenticatedAdmin,
    headers: HeaderMap,
    Path(script_id): Path<String>,
    Json(request): Json<CreateScriptRunRequest>,
) -> ApiResult<Response> {
    let actor = actor(&admin, &headers)?;
    let key = idempotency_key(&headers)?;
    let node = node_read_model::get_node_summary(&state.metadata_db, &request.node_id)
        .await?
        .ok_or_else(|| ApiError::not_found(ErrorCode::NodeNotFound, "execution node not found"))?;
    if node.status != "online" {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::ScriptNodeUnavailable,
            "script execution node is unavailable",
        ));
    }
    NodeRuntimeClient::from_node_route(&state.metadata_db, Some(&request.node_id))
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorCode::ScriptNodeUnavailable,
                "script execution node is unavailable",
            )
        })?;
    let result = scripts::create_run(
        &state.metadata_db,
        &script_id,
        &request.node_id,
        &node.name,
        request.timeout_seconds,
        &key,
        &actor,
    )
    .await;
    let target_name = result
        .as_ref()
        .ok()
        .map(|(run, _)| run.script_name.as_str());
    let task_id = result.as_ref().ok().map(|(run, _)| run.run_id.as_str());
    record_change(
        &state,
        &admin,
        &actor,
        "script_run_submitted",
        "POST",
        Some(&script_id),
        target_name,
        task_id,
        true,
        &result,
    );
    let (row, _) = result?;
    let run = scripts::run_dto(row)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(ApiResponse::success_with_raw("Script run accepted", run)),
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunListQuery {
    script_id: Option<String>,
    node_id: Option<String>,
    status: Option<ScriptRunStatus>,
    #[serde(default = "default_page")]
    page: u32,
    #[serde(default = "default_page_size")]
    page_size: u32,
}

async fn list_runs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<RunListQuery>,
) -> ApiResult<Response> {
    validate_page(query.page, query.page_size)?;
    let data = scripts::list_runs(
        &state.metadata_db,
        &ScriptRunListFilter {
            script_id: query.script_id.as_deref(),
            node_id: query.node_id.as_deref(),
            status: query.status,
            page: query.page,
            page_size: query.page_size,
        },
    )
    .await?;
    Ok(ApiResponse::success_with_raw("Script runs loaded", Some(data)).into_response())
}

async fn run_detail(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<String>,
) -> ApiResult<Response> {
    let data = scripts::run(&state.metadata_db, &run_id).await?;
    Ok(ApiResponse::success_with_raw("Script run loaded", Some(data)).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OutputQuery {
    #[serde(default)]
    cursor: u64,
    #[serde(default = "default_output_limit")]
    limit: u32,
}

async fn run_output(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<String>,
    Query(query): Query<OutputQuery>,
) -> ApiResult<Response> {
    if query.limit == 0 || query.limit > 100 {
        return Err(ApiError::bad_request(
            ErrorCode::BadRequest,
            "invalid output page size",
        ));
    }
    let data = scripts::output(&state.metadata_db, &run_id, query.cursor, query.limit).await?;
    Ok(ApiResponse::success_with_raw("Script run output loaded", Some(data)).into_response())
}

async fn cancel_run(
    State(state): State<Arc<AppState>>,
    admin: AuthenticatedAdmin,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> ApiResult<Response> {
    let actor = actor(&admin, &headers)?;
    let result = scripts::request_cancel(&state.metadata_db, &run_id).await;
    let target_id = result.as_ref().ok().map(|run| run.script_id.as_str());
    let target_name = result.as_ref().ok().map(|run| run.script_name.as_str());
    record_change(
        &state,
        &admin,
        &actor,
        "script_run_cancel_requested",
        "POST",
        target_id,
        target_name,
        Some(&run_id),
        true,
        &result,
    );
    let data = scripts::run_dto(result?)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(ApiResponse::success_with_raw(
            "Script run cancellation accepted",
            data,
        )),
    )
        .into_response())
}

fn actor(admin: &AuthenticatedAdmin, headers: &HeaderMap) -> ApiResult<ScriptActor> {
    let client_ip = admin.session.client_ip.clone().ok_or_else(|| {
        ApiError::forbidden(
            ErrorCode::AuthForbidden,
            "authenticated session is missing a trusted client IP",
        )
    })?;
    client_ip.parse::<IpAddr>().map_err(|_| {
        ApiError::forbidden(
            ErrorCode::AuthForbidden,
            "authenticated session has an invalid trusted client IP",
        )
    })?;
    Ok(ScriptActor {
        user_id: admin.id,
        name: admin.username.clone(),
        client_ip,
        trace_id: logging::resolve_trace_id(headers),
    })
}

fn idempotency_key(headers: &HeaderMap) -> ApiResult<String> {
    let value = headers
        .get(&IDEMPOTENCY_KEY)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .unwrap_or_default();
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(ApiError::bad_request(
            ErrorCode::BadRequest,
            "a valid Idempotency-Key header is required",
        ));
    }
    Ok(value.to_string())
}

#[allow(clippy::too_many_arguments)]
fn record_change<T>(
    state: &AppState,
    admin: &AuthenticatedAdmin,
    actor: &ScriptActor,
    event: &str,
    method: &str,
    target_id: Option<&str>,
    target_name: Option<&str>,
    task_id: Option<&str>,
    high_impact: bool,
    result: &ApiResult<T>,
) {
    let Ok(ip) = actor.client_ip.parse::<IpAddr>() else {
        return;
    };
    let failed = result.is_err();
    let mut operation = OperationEventBuilder::new(&admin.username, event, ip)
        .user_id(admin.id).module(LogModule::System).trace_id(&actor.trace_id)
        .request(method, "/api/v1/scripts")
        .status(if failed { LogStatus::Failed } else { LogStatus::Success })
        .level(if failed { PlatformLogLevel::Error } else if high_impact { PlatformLogLevel::Warning } else { PlatformLogLevel::Info })
        .metadata(json!({"result": if failed {"failed"} else {"submitted"}, "errorCode": result.as_ref().err().map(|error| error.code.as_str())}));
    if let Some(target_id) = target_id {
        operation = operation.target_type("script").target_id(target_id);
    }
    if let Some(target_name) = target_name {
        operation = operation.target_display_name(target_name);
    }
    if let Some(task_id) = task_id {
        operation = operation.task_id(task_id);
    }
    operation.finish(&state.metadata_db);
}

fn validate_page(page: u32, page_size: u32) -> ApiResult<()> {
    if page == 0 || page_size == 0 || page_size > 100 {
        Err(ApiError::bad_request(
            ErrorCode::BadRequest,
            "invalid pagination",
        ))
    } else {
        Ok(())
    }
}
const fn default_page() -> u32 {
    1
}
const fn default_page_size() -> u32 {
    50
}
fn default_sort_by() -> String {
    "updatedAt".into()
}
fn default_sort_order() -> String {
    "desc".into()
}
const fn default_output_limit() -> u32 {
    50
}
