//! 计划任务执行 API：执行来自 SecLab 的节点任务。
//!
//! 该接口提供高危 shell 执行能力，仅供脚本库与计划任务使用。
//! 明确业务动作应使用专用 API，避免把任意 `command` 暴露为功能参数。

use crate::types::{ApiError, ApiResponse, ApiResult};
use axum::{
    Json, Router,
    extract::State,
    response::{IntoResponse, Response},
    routing::post,
};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskPayload {
    pub command: String,
    pub timeout_secs: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTaskResult {
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub started_at: i64,
    pub finished_at: i64,
}

pub fn task_router() -> Router<std::sync::Arc<crate::state::AppState>> {
    Router::new().route("/execute", post(execute))
}

pub async fn execute(
    State(_state): State<std::sync::Arc<crate::state::AppState>>,
    Json(payload): Json<ExecuteTaskPayload>,
) -> ApiResult<Response> {
    if payload.command.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "command must not be empty".to_string(),
        ));
    }

    let timeout_secs = payload.timeout_secs.clamp(1, 86_400) as u64;
    let started_at = chrono::Utc::now().timestamp();

    let output = Command::new("/usr/bin/timeout")
        .arg(format!("{}s", timeout_secs))
        .arg("/bin/bash")
        .arg("-lc")
        .arg(&payload.command)
        .output()
        .await
        .map_err(|err| ApiError::Internal(format!("failed to execute command: {err}")))?;

    let exit_code = output.status.code().unwrap_or(-1) as i64;
    let timed_out = exit_code == 124;
    let result = ExecuteTaskResult {
        exit_code,
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        timed_out,
        started_at,
        finished_at: chrono::Utc::now().timestamp(),
    };

    Ok(ApiResponse::success_with_raw("Task executed", Some(result)).into_response())
}
