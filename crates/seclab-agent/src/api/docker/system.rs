//! Docker 系统清理与磁盘使用统计 API。

use crate::state::AppState;
use crate::types::{ApiError, ApiResponse, ApiResult};

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use std::sync::Arc;
use tokio::process::Command;
use tracing::info;

/// 获取 Docker 系统磁盘使用统计（镜像、容器、卷、缓存）。
pub async fn system_df(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    info!("Requesting docker system df");
    let docker = state.docker_client().await?;
    let df = docker
        .df(None::<bollard::query_parameters::DataUsageOptions>)
        .await?;
    Ok(ApiResponse::success_with_raw("Docker system df loaded", Some(df)).into_response())
}

/// 执行一键清理（清理停止的容器、未使用的网络和挂起镜像）。
pub async fn system_prune() -> ApiResult<Response> {
    info!("Requesting docker system prune");
    let output = Command::new("docker")
        .args(["system", "prune", "-f"])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ApiError::Internal(format!(
            "Docker system prune failed: {}",
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(
        ApiResponse::success_with_raw("Docker system prune completed", Some(stdout))
            .into_response(),
    )
}
