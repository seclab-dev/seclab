//! Master 专用脚本运行 API：幂等启动、查询和取消。

use crate::{
    models::script_runs,
    services::script_runs as script_run_service,
    state::AppState,
    types::{ApiResponse, ApiResult},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use seclab_contracts::scripts::AgentStartScriptRunRequest;
use std::sync::Arc;

/// 构建只允许 Master 调用的脚本运行路由。
pub fn script_run_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", post(start))
        .route("/{run_id}/cancel", post(cancel))
}

async fn start(
    State(state): State<Arc<AppState>>,
    Json(request): Json<AgentStartScriptRunRequest>,
) -> ApiResult<Response> {
    let run_id = script_run_service::submit(state, request).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(ApiResponse::success_with_raw(
            "Script run accepted",
            Some(serde_json::json!({ "runId": run_id })),
        )),
    )
        .into_response())
}

async fn cancel(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<String>,
) -> ApiResult<Response> {
    let run = script_runs::request_cancel(&state.metadata_db, &run_id).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(ApiResponse::success_with_raw(
            "Script run cancellation accepted",
            Some(serde_json::json!({ "runId": run.run_id, "status": run.status })),
        )),
    )
        .into_response())
}
