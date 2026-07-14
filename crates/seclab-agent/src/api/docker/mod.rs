//! Docker API 聚合：容器、镜像、网络与统计子路由。

use crate::models::docker;
use crate::services::{docker_activity, docker_stats};
use crate::state::AppState;
use crate::types::{ApiResponse, ApiResult};

use axum::{
    Router,
    extract::{Json, State},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use bollard::models::{ContainerSummary, ContainerSummaryStateEnum, ImageSummary, SystemInfo};
use bollard::query_parameters;
use chrono::Utc;
use seclab_contracts::types::{DockerServiceStatus, DockerStatusSummary};
use std::sync::Arc;
use tracing::info;

pub mod compose;
pub mod containers;
pub mod context;
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
    let info: SystemInfo = docker.info().await?;
    Ok(ApiResponse::success_with_raw("Docker info loaded", Some(info)).into_response())
}

/// 汇总容器与镜像概览以及实时资源统计。
pub async fn overview_realtime(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    info!("Requesting docker overview realtime");
    let docker = state.docker_client().await?;

    let options = query_parameters::ListContainersOptionsBuilder::new()
        .all(true)
        .build();
    let containers: Vec<ContainerSummary> = docker.list_containers(Some(options)).await?;

    let images: Vec<ImageSummary> = docker
        .list_images(Some(query_parameters::ListImagesOptions::default()))
        .await?;

    let container_states = summarize_container_states(&containers);
    let projects = compose::load_project_summaries(&state).await?;
    let project_states = summarize_project_states(&projects);
    let image_counts = docker::ImageCounts {
        total: images.len(),
        dangling: images
            .iter()
            .filter(|image| {
                image.repo_tags.is_empty()
                    || image.repo_tags.iter().all(|tag| tag == "<none>:<none>")
            })
            .count(),
    };

    let trend_containers = containers
        .iter()
        .filter_map(|container| {
            let id = container.id.as_ref()?.to_string();
            let name = container
                .names
                .as_ref()
                .and_then(|names| names.first())
                .map(|value| value.trim_start_matches('/').to_string())
                .unwrap_or_else(|| id.clone());
            let created_at = container.created.unwrap_or_default();
            Some(docker::TrendContainerItem {
                id,
                name,
                created_at,
                state: container
                    .state
                    .map(|state| state.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
            })
        })
        .collect::<Vec<_>>();

    let resource_usage = docker_stats::load_latest_summary(&state).await.unwrap_or(
        docker::HostResourceUsageSummary {
            status: docker::ResourceSampleStatus::Unavailable,
            collected_at: None,
            running_container_count: container_states.running,
            sampled_container_count: 0,
            cpu_host_percent: 0.0,
            cpu_core_percent: 0.0,
            memory_working_set_bytes: 0,
            memory_limit_bytes: 0,
            memory_percent: 0.0,
        },
    );

    let response = docker::OverviewRealtimeResponse {
        collected_at: Utc::now().timestamp(),
        container_states,
        project_states,
        images: image_counts,
        resource_usage,
        trend_containers,
    };

    Ok(ApiResponse::success_with_raw("Docker overview loaded", Some(response)).into_response())
}

/// 汇总 Docker 容器状态分布。
fn summarize_container_states(containers: &[ContainerSummary]) -> docker::ContainerStateCounts {
    let mut counts = docker::ContainerStateCounts {
        total: containers.len(),
        running: 0,
        paused: 0,
        restarting: 0,
        exited: 0,
        other: 0,
    };
    for container in containers {
        match container.state {
            Some(ContainerSummaryStateEnum::RUNNING) => counts.running += 1,
            Some(ContainerSummaryStateEnum::PAUSED) => counts.paused += 1,
            Some(ContainerSummaryStateEnum::RESTARTING) => counts.restarting += 1,
            Some(ContainerSummaryStateEnum::EXITED) => counts.exited += 1,
            _ => counts.other += 1,
        }
    }
    counts
}

/// 汇总登记 Compose 项目的健康状态分布。
fn summarize_project_states(
    projects: &[docker::ComposeProjectSummary],
) -> docker::ProjectStateCounts {
    let mut counts = docker::ProjectStateCounts {
        total: projects.len(),
        healthy: 0,
        partial: 0,
        stopped: 0,
        unknown: 0,
    };
    for project in projects {
        if project.total_containers > 0 && project.running_containers == project.total_containers {
            counts.healthy += 1;
        } else if project.running_containers > 0
            || project.paused_containers > 0
            || project.restarting_containers > 0
        {
            counts.partial += 1;
        } else if project.total_containers == 0
            || project.exited_containers == project.total_containers
        {
            counts.stopped += 1;
        } else {
            counts.unknown += 1;
        }
    }
    counts
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

/// 查询 Docker 操作日志。
pub async fn activity_logs(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<docker::DockerActivityLogQuery>,
) -> ApiResult<Response> {
    let page = docker_activity::query(&state.metadata_db, payload).await?;
    Ok(ApiResponse::success_with_raw("Docker activity logs loaded", Some(page)).into_response())
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
        .route("/images/export", get(images::export_image))
        .route("/images/load", post(images::load_image))
        .route("/images/resolve", post(images::resolve_image))
        .route("/images/search", post(images::search_images))
        .route("/images/tags", post(images::image_tags))
        .route("/images/availability", post(images::image_availability))
        .route("/image-pull-tasks", post(images::pull_image))
        .route(
            "/image-pull-tasks/{task_id}/progress",
            get(images::pull_image_progress),
        )
        .route(
            "/image-pull-tasks/{task_id}",
            delete(images::cancel_pull_image),
        )
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
        .route("/suites/install-progress", get(suites::install_progress))
        .route(
            "/suites/install-progress/{instance_id}/cancel",
            post(suites::cancel_install),
        )
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
        .route("/activity-logs/query", post(activity_logs))
        .layer(axum::middleware::from_fn(context::operation_context_layer))
}

#[cfg(test)]
mod tests {
    use super::summarize_project_states;
    use crate::models::docker::ComposeProjectSummary;

    fn project(
        total: usize,
        running: usize,
        paused: usize,
        restarting: usize,
        exited: usize,
    ) -> ComposeProjectSummary {
        ComposeProjectSummary {
            name: "project".to_string(),
            status: "unknown".to_string(),
            total_containers: total,
            running_containers: running,
            exited_containers: exited,
            paused_containers: paused,
            restarting_containers: restarting,
            has_compose_file: true,
            compose_dir: None,
            project_type: None,
        }
    }

    #[test]
    fn project_counts_are_distinct_and_stateful() {
        let projects = vec![
            project(3, 3, 0, 0, 0),
            project(3, 1, 0, 0, 2),
            project(1, 0, 1, 0, 0),
            project(2, 0, 0, 0, 2),
            project(0, 0, 0, 0, 0),
        ];
        let counts = summarize_project_states(&projects);
        assert_eq!(counts.total, 5);
        assert_eq!(counts.healthy, 1);
        assert_eq!(counts.partial, 2);
        assert_eq!(counts.stopped, 2);
        assert_eq!(counts.unknown, 0);
    }
}
