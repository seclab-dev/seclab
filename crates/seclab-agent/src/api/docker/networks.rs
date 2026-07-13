//! Docker 网络 API：网络列表与管理操作。

use crate::models::docker;
use crate::state::AppState;
use crate::types::{ApiResponse, ApiResult};

use axum::extract::{Json, Path, State};
use axum::response::{IntoResponse, Response};
use bollard::models::{
    Ipam, IpamConfig, NetworkConnectRequest, NetworkCreateRequest, NetworkDisconnectRequest,
};
use bollard::query_parameters;
use std::sync::Arc;
use tracing::info;

/// 创建一个 Docker 网络，并支持可选的子网配置。
pub async fn create_network(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<docker::NetworkCreateRequest>,
) -> ApiResult<Response> {
    info!("Requesting network create: {}", payload.name);
    let docker = state.docker_client().await?;

    let ipam = if payload.subnet.is_some() || payload.gateway.is_some() {
        Some(Ipam {
            config: Some(vec![IpamConfig {
                subnet: payload.subnet,
                gateway: payload.gateway,
                ..Default::default()
            }]),
            ..Default::default()
        })
    } else {
        None
    };

    let request = NetworkCreateRequest {
        name: payload.name,
        driver: payload.driver,
        ipam,
        labels: payload.labels,
        ..Default::default()
    };

    let result = docker.create_network(request).await?;
    Ok(ApiResponse::success_with_raw("Network created", Some(result)).into_response())
}

/// 获取指定网络的详细信息。
pub async fn inspect_network(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    info!("Requesting network inspect: {}", id);
    let docker = state.docker_client().await?;
    let options = query_parameters::InspectNetworkOptions::default();
    let detail = docker.inspect_network(&id, Some(options)).await?;
    Ok(ApiResponse::success_with_raw("Network detail loaded", Some(detail)).into_response())
}

/// 删除指定的 Docker 网络。
pub async fn remove_network(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    info!("Requesting network remove: {}", id);
    let docker = state.docker_client().await?;
    docker.remove_network(&id).await?;
    Ok(ApiResponse::ok("Network removed").into_response())
}

/// 将容器加入指定网络。
pub async fn connect_network(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(payload): Json<docker::NetworkConnectRequest>,
) -> ApiResult<Response> {
    info!(
        "Requesting network connect: network={}, container={}",
        id, payload.container
    );
    let docker = state.docker_client().await?;
    let request = NetworkConnectRequest {
        container: payload.container,
        endpoint_config: None,
    };
    docker.connect_network(&id, request).await?;
    Ok(ApiResponse::ok("Network connected").into_response())
}

/// 将容器从指定网络断开。
pub async fn disconnect_network(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(payload): Json<docker::NetworkDisconnectRequest>,
) -> ApiResult<Response> {
    info!(
        "Requesting network disconnect: network={}, container={}",
        id, payload.container
    );
    let docker = state.docker_client().await?;
    let request = NetworkDisconnectRequest {
        container: payload.container,
        force: Some(payload.force.unwrap_or(false)),
    };
    docker.disconnect_network(&id, request).await?;
    Ok(ApiResponse::ok("Network disconnected").into_response())
}
