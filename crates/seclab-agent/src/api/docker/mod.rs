//! Docker API 聚合：容器、镜像、网络与统计子路由。

use crate::models::docker;
use crate::services::docker_stats;
use crate::state::AppState;
use crate::types::{ApiResponse, ApiResult};

use crate::services::logging::{self, AgentLogModule, LogPayload};
use axum::{
    Router,
    extract::{Json, State},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use bollard::models::ContainerSummaryStateEnum;
use bollard::query_parameters;
use seclab_contracts::types::{DockerServiceStatus, DockerStatusSummary};
use std::sync::Arc;
use tracing::info;

pub mod compose;
pub mod containers;
pub mod daemon_settings;
pub mod images;
pub mod install;
pub mod networks;
pub mod stats;
pub mod suites;
pub mod system;
pub mod volumes;

/// 返回 Docker 引擎的系统信息。
pub async fn info(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    info!("Requesting docker info");
    let docker = state.docker_client().await?;
    let info: bollard::secret::SystemInfo = docker.info().await?;
    Ok(ApiResponse::success_with_raw("Docker info loaded", Some(info)).into_response())
}

/// 汇总容器与镜像概览以及实时资源统计。
pub async fn overview_realtime(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    info!("Requesting docker overview realtime");
    let docker = state.docker_client().await?;

    let options = query_parameters::ListContainersOptionsBuilder::new()
        .all(true)
        .build();
    let containers: Vec<bollard::secret::ContainerSummary> =
        docker.list_containers(Some(options)).await?;

    let images: Vec<bollard::secret::ImageSummary> = docker
        .list_images(Some(query_parameters::ListImagesOptions::default()))
        .await?;

    let project_total_count = containers
        .iter()
        .filter(|container| {
            container
                .labels
                .as_ref()
                .is_some_and(|labels| labels.contains_key("com.docker.compose.project"))
        })
        .count();
    let project_running_count = containers
        .iter()
        .filter(|container| {
            container.labels.as_ref().is_some_and(|labels| {
                labels.contains_key("com.docker.compose.project")
                    && container.state == Some(ContainerSummaryStateEnum::RUNNING)
            })
        })
        .count();

    let overview = docker::OverviewStatus {
        status: true,
        total_container_count: containers.len(),
        running_container_count: containers
            .iter()
            .filter(|c| c.state == Some(ContainerSummaryStateEnum::RUNNING))
            .count(),
        // TODO: 需要处理不同的非运行状态
        // `stopped_container_count` 可由前端 `total - running` 计算，因此在后端此字段已移除。
        total_image_count: images.len(),
        project_total_count,
        project_running_count,
    };

    let overview_containers = containers
        .iter()
        .filter(|container| container.state == Some(ContainerSummaryStateEnum::RUNNING))
        .filter_map(|container| {
            let id = container.id.as_ref()?.to_string();
            let name = container
                .names
                .as_ref()
                .and_then(|names| names.first())
                .map(|value| value.trim_start_matches('/').to_string())
                .unwrap_or_else(|| id.clone());
            let created_at = container.created.unwrap_or_default();
            Some(docker::OverviewContainerItem {
                id,
                name,
                created_at,
            })
        })
        .collect::<Vec<_>>();

    let resource_usage = docker_stats::collect_realtime_summary(&state)
        .await
        .unwrap_or(docker::ResourceUsageSummary {
            cpu_percent: 0.0,
            memory_usage_bytes: 0,
            memory_limit_bytes: 0,
            memory_percent: 0.0,
            network_rx_bytes: 0,
            network_tx_bytes: 0,
            container_count: 0,
        });

    let response = docker::OverviewRealtimeResponse {
        overview,
        resource_usage,
        overview_containers,
    };

    Ok(ApiResponse::success_with_raw("Docker overview loaded", Some(response)).into_response())
}

/// 获取所有网络的详细信息。
///
/// 底层行为等同于对每个已存在的网络执行 `docker network inspect <name>`。
pub async fn inspect_networks(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    info!("Requesting docker network");
    let docker = state.docker_client().await?;

    let options = query_parameters::InspectNetworkOptions::default();

    let mut networks = vec![];
    for network in docker
        .list_networks(Some(query_parameters::ListNetworksOptions::default()))
        .await?
        .iter()
    {
        networks.push(
            docker
                .inspect_network(network.name.as_ref().unwrap(), Some(options.clone()))
                .await?,
        );
    }
    Ok(ApiResponse::success_with_raw("Docker networks loaded", Some(networks)).into_response())
}

/// 返回当前宿主机的 Docker 卷列表。
pub async fn volumes(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    info!("Requesting docker node");
    let docker = state.docker_client().await?;

    let volume = docker
        .list_volumes(Some(query_parameters::ListVolumesOptions::default()))
        .await?;
    Ok(ApiResponse::success_with_raw("Docker volumes loaded", Some(volume)).into_response())
}

/// 返回 Docker 服务是否可用及具体状态。
pub async fn status(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    let docker_status = state.docker_status().await;
    let docker_available = docker_status == DockerServiceStatus::Available;
    let summary = DockerStatusSummary {
        docker_available,
        docker_status,
    };
    Ok(ApiResponse::success_with_raw("Docker status loaded", Some(summary)).into_response())
}

/// 获取 Docker 模块的日志
pub async fn logs(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LogPayload>,
) -> ApiResult<Response> {
    info!(
        "Requesting Docker module platform logs: page={}, page_size={}",
        payload.page, payload.page_size
    );

    let dev_ops_query = LogPayload {
        modules: Some(vec![AgentLogModule::Docker]),
        ..payload
    };

    let logs = logging::fetch_agent_logs(&state.metadata_db, dev_ops_query).await?;

    Ok(ApiResponse::success_with_raw("Platform logs loaded", Some(logs)).into_response())
}

/// 构建 Docker 子模块的路由集合。
pub fn docker_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/status", get(status))
        .route("/install", post(install::install))
        .route(
            "/daemon/settings",
            get(daemon_settings::get_settings).put(daemon_settings::update_settings),
        )
        .route("/info", get(info))
        .route("/overview/realtime", post(overview_realtime))
        .route("/action", post(containers::handle_action))
        .route(
            "/containers",
            get(containers::list_containers).post(containers::create_container),
        )
        .route(
            "/compose/containers",
            get(containers::list_project_containers),
        )
        .route(
            "/compose/projects",
            get(compose::list_projects).post(compose::create_project),
        )
        .route("/compose/root", get(compose::compose_root))
        .route(
            "/compose/projects/{name}/start",
            post(compose::start_project),
        )
        .route("/compose/projects/{name}/stop", post(compose::stop_project))
        .route(
            "/compose/projects/{name}/restart",
            post(compose::restart_project),
        )
        .route("/compose/projects/{name}/logs", get(compose::project_logs))
        .route(
            "/compose/projects/{name}/update",
            post(compose::update_project),
        )
        .route("/compose/projects/{name}", delete(compose::delete_project))
        .route(
            "/containers/{id}",
            get(containers::inspect_container).delete(containers::remove_container),
        )
        .route(
            "/containers/{id}/rename",
            post(containers::rename_container),
        )
        .route("/containers/{id}/pause", post(containers::pause_container))
        .route(
            "/containers/{id}/unpause",
            post(containers::unpause_container),
        )
        .route("/containers/{id}/kill", post(containers::kill_container))
        .route("/containers/{id}/exec", post(containers::exec_container))
        .route("/containers/{id}/top", get(containers::top_container))
        .route(
            "/containers/{id}/stats/summary",
            get(stats::container_summary),
        )
        .route(
            "/containers/{id}/stats/history",
            post(stats::container_history),
        )
        .route(
            "/containers/stats/summary",
            post(stats::container_summaries),
        )
        .route(
            "/containers/stats/history",
            post(stats::container_histories),
        )
        .route(
            "/containers/{id}/logs",
            get(containers::latest_container_logs),
        )
        .route("/images", get(images::list_images))
        .route("/images/load", post(images::load_image))
        .route("/image/remove", delete(images::remove_image))
        .route(
            "/networks",
            get(inspect_networks).post(networks::create_network),
        )
        .route(
            "/networks/{id}",
            get(networks::inspect_network).delete(networks::remove_network),
        )
        .route("/networks/{id}/connect", post(networks::connect_network))
        .route(
            "/networks/{id}/disconnect",
            post(networks::disconnect_network),
        )
        .route("/stats/history", post(stats::history))
        .route("/volumes", get(volumes).post(volumes::create_volume))
        .route(
            "/volumes/{name}",
            get(volumes::inspect_volume).delete(volumes::remove_volume),
        )
        .route(
            "/compose/projects/{name}/scale",
            post(compose::scale_project),
        )
        .route("/compose/validate", post(compose::validate_compose))
        .route("/suites/install", post(suites::install_suite))
        .route("/suite/{project}/enable", post(suites::enable_suite))
        .route("/suite/{project}/disable", post(suites::disable_suite))
        .route("/suite/{project}/uninstall", post(suites::uninstall_suite))
        .route(
            "/suite/{project}/proxy/{entry_id}/{*path}",
            axum::routing::any(suites::proxy_suite_entry),
        )
        .route(
            "/suite/{project}/proxy/{entry_id}/",
            axum::routing::any(suites::proxy_suite_entry),
        )
        .route(
            "/suite/{project}/proxy/{entry_id}",
            axum::routing::any(suites::proxy_suite_entry),
        )
        .route("/system/df", get(system::system_df))
        .route("/system/prune", post(system::system_prune))
        .route("/logs", post(logs))
}
