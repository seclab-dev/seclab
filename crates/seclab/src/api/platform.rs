//! 平台日志 API：查询与展示平台业务事件记录。

use crate::models::{node_runtime_client::NodeRuntimeClient, nodes::get_node_by_id};
use crate::services::{
    logging::{self, LogPayload},
    runtime_logs,
};
use crate::state::AppState;
use crate::types::{ApiError, ApiResponse, ApiResult};

use axum::{
    Json, Router,
    extract::{Query, State},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use seclab_api::response::ApiResponse as RuntimeApiResponse;
use seclab_contracts::runtime_logs::{RuntimeLogFile, RuntimeLogQuery, RuntimeLogQueryResult};
use serde::Deserialize;
use std::sync::Arc;
use tracing::info;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLogFilesQuery {
    pub service: Option<String>,
    pub node_id: Option<String>,
}

/// 查询平台日志，支持模块、动作、状态、时间范围和关键词筛选。
pub async fn logs(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LogPayload>,
) -> ApiResult<Response> {
    info!(
        "Requesting platform logs: page={}, page_size={}, modules={:?}",
        payload.page, payload.page_size, payload.modules
    );

    let logs = logging::fetch_platform_logs(&state.metadata_db, payload).await?;

    Ok(ApiResponse::success_with_raw("Platform logs loaded", Some(logs)).into_response())
}

/// 平台日志 API 路由。
pub fn platform_log_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/logs", post(logs))
        .route("/runtime-logs/files", get(runtime_log_files))
        .route("/runtime-logs/query", post(query_runtime_logs))
}

/// 查询可读取的运行日志文件。
pub async fn runtime_log_files(
    State(state): State<Arc<AppState>>,
    Query(query): Query<RuntimeLogFilesQuery>,
) -> ApiResult<Response> {
    let service = query.service.as_deref();
    let files = if service == Some("agent") && is_remote_node(query.node_id.as_deref()) {
        let node_id = query.node_id.as_deref().unwrap_or_default();
        let node_name = resolve_node_name(&state, node_id).await?;
        let client = NodeRuntimeClient::from_node_route(&state.metadata_db, Some(node_id)).await?;
        let mut files = unwrap_runtime_response::<Vec<RuntimeLogFile>>(
            client
                .get_json("/api/v1/agent/runtime-logs/files")
                .await
                .map_err(|err| ApiError::Internal(err.to_string()))?,
        )?;
        enrich_agent_files(&mut files, node_id, node_name.as_deref());
        files
    } else {
        runtime_logs::list_runtime_log_files(service).await?
    };
    Ok(ApiResponse::success_with_raw("Runtime log files loaded", Some(files)).into_response())
}

/// 查询指定运行日志文件片段。
pub async fn query_runtime_logs(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RuntimeLogQuery>,
) -> ApiResult<Response> {
    let result = if payload.service == "agent" && is_remote_node(payload.node_id.as_deref()) {
        let node_id = payload.node_id.as_deref().unwrap_or_default();
        let client = NodeRuntimeClient::from_node_route(&state.metadata_db, Some(node_id)).await?;
        unwrap_runtime_response::<RuntimeLogQueryResult>(
            client
                .post_json("/api/v1/agent/runtime-logs/query", &payload)
                .await
                .map_err(|err| ApiError::Internal(err.to_string()))?,
        )?
    } else {
        runtime_logs::query_runtime_logs(payload).await?
    };
    Ok(ApiResponse::success_with_raw("Runtime logs loaded", Some(result)).into_response())
}

fn is_remote_node(node_id: Option<&str>) -> bool {
    matches!(node_id, Some(value) if !value.trim().is_empty() && value != "local")
}

async fn resolve_node_name(state: &Arc<AppState>, node_id: &str) -> ApiResult<Option<String>> {
    Ok(get_node_by_id(&state.metadata_db, node_id)
        .await?
        .map(|node| node.name)
        .filter(|name| !name.trim().is_empty()))
}

fn enrich_agent_files(files: &mut [RuntimeLogFile], node_id: &str, node_name: Option<&str>) {
    for file in files {
        file.service = "agent".to_string();
        file.node_id = Some(node_id.to_string());
        file.node_name = node_name.map(ToString::to_string);
    }
}

fn unwrap_runtime_response<T>(response: RuntimeApiResponse<T>) -> ApiResult<T> {
    if !response.success {
        return Err(ApiError::BadRequest(response.message));
    }
    response
        .data
        .ok_or_else(|| ApiError::Internal("node runtime log response is empty".to_string()))
}
