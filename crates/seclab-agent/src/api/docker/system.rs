//! Docker 系统清理与磁盘使用统计 API。

use crate::models::docker::{DockerDiskUsageCategory, DockerDiskUsageSummary};
use crate::state::AppState;
use crate::types::{ApiError, ApiResponse, ApiResult};

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
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
    let summary = summarize_disk_usage(df, Utc::now().timestamp());
    Ok(ApiResponse::success_with_raw("Docker system df loaded", Some(summary)).into_response())
}

/// 将 Docker Engine 的磁盘统计归一化为稳定领域模型。
fn summarize_disk_usage(
    df: bollard::models::SystemDataUsageResponse,
    collected_at: i64,
) -> DockerDiskUsageSummary {
    let image_size_bytes = positive_bytes(df.layers_size.unwrap_or_default());
    let images = df.images.unwrap_or_default();
    let containers = df.containers.unwrap_or_default();
    let volumes = df.volumes.unwrap_or_default();
    let build_cache = df.build_cache.unwrap_or_default();
    DockerDiskUsageSummary {
        collected_at,
        images: DockerDiskUsageCategory {
            total_count: images.len(),
            active_count: images.iter().filter(|image| image.containers > 0).count(),
            size_bytes: image_size_bytes,
            reclaimable_bytes: images
                .iter()
                .filter(|image| image.containers == 0)
                .map(|image| positive_bytes(image.size.saturating_sub(image.shared_size.max(0))))
                .sum(),
        },
        containers: DockerDiskUsageCategory {
            total_count: containers.len(),
            active_count: containers
                .iter()
                .filter(|container| is_active_container_state(&container.state))
                .count(),
            size_bytes: containers
                .iter()
                .map(|container| positive_bytes(container.size_rw.unwrap_or(0)))
                .sum(),
            reclaimable_bytes: containers
                .iter()
                .filter(|container| !is_active_container_state(&container.state))
                .map(|container| positive_bytes(container.size_rw.unwrap_or(0)))
                .sum(),
        },
        volumes: DockerDiskUsageCategory {
            total_count: volumes.len(),
            active_count: volumes
                .iter()
                .filter(|volume| {
                    volume
                        .usage_data
                        .as_ref()
                        .is_some_and(|usage| usage.ref_count > 0)
                })
                .count(),
            size_bytes: volumes
                .iter()
                .filter_map(|volume| volume.usage_data.as_ref())
                .map(|usage| positive_bytes(usage.size))
                .sum(),
            reclaimable_bytes: volumes
                .iter()
                .filter_map(|volume| volume.usage_data.as_ref())
                .filter(|usage| usage.ref_count == 0)
                .map(|usage| positive_bytes(usage.size))
                .sum(),
        },
        build_cache: DockerDiskUsageCategory {
            total_count: build_cache.len(),
            active_count: build_cache
                .iter()
                .filter(|cache| cache.in_use.unwrap_or(false))
                .count(),
            size_bytes: build_cache
                .iter()
                .map(|cache| positive_bytes(cache.size.unwrap_or(0)))
                .sum(),
            reclaimable_bytes: build_cache
                .iter()
                .filter(|cache| !cache.in_use.unwrap_or(false) && !cache.shared.unwrap_or(false))
                .map(|cache| positive_bytes(cache.size.unwrap_or(0)))
                .sum(),
        },
    }
}

/// 判断容器状态是否应计入 Docker 活跃容器。
fn is_active_container_state(state: &Option<bollard::models::ContainerSummaryStateEnum>) -> bool {
    matches!(
        state,
        Some(
            bollard::models::ContainerSummaryStateEnum::RUNNING
                | bollard::models::ContainerSummaryStateEnum::PAUSED
                | bollard::models::ContainerSummaryStateEnum::RESTARTING
        )
    )
}

/// 将 Docker 可能返回的负数或未知容量归一化为零。
fn positive_bytes(value: i64) -> u64 {
    value.max(0) as u64
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

#[cfg(test)]
mod tests {
    use super::summarize_disk_usage;
    use bollard::models::{
        BuildCache, ContainerSummary, ContainerSummaryStateEnum, ImageSummary,
        SystemDataUsageResponse,
    };

    #[test]
    fn disk_usage_matches_docker_shared_resource_semantics() {
        let summary = summarize_disk_usage(
            SystemDataUsageResponse {
                layers_size: Some(300),
                images: Some(vec![
                    ImageSummary {
                        containers: 0,
                        size: 250,
                        shared_size: 100,
                        ..Default::default()
                    },
                    ImageSummary {
                        containers: 1,
                        size: 200,
                        shared_size: 100,
                        ..Default::default()
                    },
                ]),
                containers: Some(vec![
                    container(ContainerSummaryStateEnum::RUNNING, 10),
                    container(ContainerSummaryStateEnum::PAUSED, 20),
                    container(ContainerSummaryStateEnum::RESTARTING, 30),
                    container(ContainerSummaryStateEnum::EXITED, 40),
                ]),
                build_cache: Some(vec![
                    build_cache(false, false, 100),
                    build_cache(false, true, 200),
                    build_cache(true, false, 300),
                ]),
                ..Default::default()
            },
            123,
        );

        assert_eq!(summary.collected_at, 123);
        assert_eq!(summary.images.size_bytes, 300);
        assert_eq!(summary.images.reclaimable_bytes, 150);
        assert_eq!(summary.containers.active_count, 3);
        assert_eq!(summary.containers.size_bytes, 100);
        assert_eq!(summary.containers.reclaimable_bytes, 40);
        assert_eq!(summary.build_cache.size_bytes, 600);
        assert_eq!(summary.build_cache.reclaimable_bytes, 100);
    }

    fn container(state: ContainerSummaryStateEnum, size_rw: i64) -> ContainerSummary {
        ContainerSummary {
            state: Some(state),
            size_rw: Some(size_rw),
            ..Default::default()
        }
    }

    fn build_cache(in_use: bool, shared: bool, size: i64) -> BuildCache {
        BuildCache {
            in_use: Some(in_use),
            shared: Some(shared),
            size: Some(size),
            ..Default::default()
        }
    }
}
