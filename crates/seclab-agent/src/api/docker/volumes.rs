//! Docker 卷 API：创建、删除与详情。

#![allow(deprecated)]

use crate::models::docker;
use crate::state::AppState;
use crate::types::{ApiResponse, ApiResult};

use axum::extract::{Json, Path, State};
use axum::response::{IntoResponse, Response};
use bollard::models::VolumeCreateRequest;
use std::sync::Arc;
use tracing::info;

/// 创建一个新的 Docker 数据卷。
pub async fn create_volume(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<docker::VolumeCreateRequest>,
) -> ApiResult<Response> {
    info!("Requesting docker volume create: {}", payload.name);
    let docker = state.docker_client().await?;
    let options = VolumeCreateRequest {
        name: Some(payload.name),
        driver: Some(payload.driver.unwrap_or_else(|| "local".to_string())),
        driver_opts: payload.driver_opts,
        labels: payload.labels,
        ..Default::default()
    };
    let volume = docker.create_volume(options).await?;
    Ok(ApiResponse::success_with_raw("Docker volume created", Some(volume)).into_response())
}

/// 删除指定的 Docker 数据卷。
pub async fn remove_volume(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<Response> {
    info!("Requesting docker volume remove: {}", name);
    let docker = state.docker_client().await?;
    docker
        .remove_volume(
            &name,
            None::<bollard::query_parameters::RemoveVolumeOptions>,
        )
        .await?;
    Ok(ApiResponse::ok("Docker volume removed").into_response())
}

/// 查询指定 Docker 数据卷的详细信息。
pub async fn inspect_volume(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<Response> {
    info!("Requesting docker volume inspect: {}", name);
    let docker = state.docker_client().await?;
    let volume = docker.inspect_volume(&name).await?;
    Ok(ApiResponse::success_with_raw("Docker volume inspected", Some(volume)).into_response())
}
