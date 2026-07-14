//! Docker 卷 API：创建、删除与详情。

#![allow(deprecated)]

use crate::api::docker::context::DockerOperationContext;
use crate::models::docker;
use crate::state::AppState;
use crate::types::{ApiResponse, ApiResult};

use axum::extract::{Json, Path, State};
use axum::response::{IntoResponse, Response};
use bollard::models::VolumeCreateRequest;
use serde_json::json;
use std::sync::Arc;
use tracing::info;

/// 创建一个新的 Docker 数据卷。
pub async fn create_volume(
    State(state): State<Arc<AppState>>,
    context: DockerOperationContext,
    Json(payload): Json<docker::VolumeCreateRequest>,
) -> ApiResult<Response> {
    info!("Requesting docker volume create: {}", payload.name);
    let volume_name = payload.name.clone();
    let result: ApiResult<Response> = async {
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
    .await;
    context
        .finish(
            &state.metadata_db,
            "volume.create",
            Some(("volume", &volume_name)),
            json!({ "name": volume_name }),
            false,
            result,
        )
        .await
}

/// 删除指定的 Docker 数据卷。
pub async fn remove_volume(
    State(state): State<Arc<AppState>>,
    context: DockerOperationContext,
    Path(name): Path<String>,
) -> ApiResult<Response> {
    info!("Requesting docker volume remove: {}", name);
    let result: ApiResult<Response> = async {
        let docker = state.docker_client().await?;
        docker
            .remove_volume(
                &name,
                None::<bollard::query_parameters::RemoveVolumeOptions>,
            )
            .await?;
        Ok(ApiResponse::ok("Docker volume removed").into_response())
    }
    .await;
    context
        .finish(
            &state.metadata_db,
            "volume.remove",
            Some(("volume", &name)),
            json!({ "name": name }),
            true,
            result,
        )
        .await
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
