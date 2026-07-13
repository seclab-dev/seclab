//! Docker 统计 API：实时与缓存的资源数据输出。

use crate::config;
use crate::models::docker;
use crate::state::AppState;
use crate::types::{ApiError, ApiResponse, ApiResult};
use axum::Json;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use bollard::query_parameters;
use chrono::Utc;
use serde::Deserialize;
use sqlx::FromRow;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

/// 查询统计历史的时间窗口配置。
#[derive(Debug, Deserialize)]
pub struct StatsHistoryQuery {
    pub hours: Option<i64>,
}

#[derive(Debug, FromRow)]
struct SummaryRow {
    created_at: i64,
    cpu_core_percent: f64,
    cpu_host_percent: f64,
    memory_working_set_bytes: i64,
    memory_limit_bytes: i64,
    memory_percent: f64,
    running_container_count: i64,
    sampled_container_count: i64,
}

#[derive(Debug, FromRow)]
struct ContainerRow {
    created_at: i64,
    cpu_core_percent: f64,
    memory_working_set_bytes: i64,
    memory_limit_bytes: i64,
    memory_percent: f64,
    network_rx_bytes: i64,
    network_tx_bytes: i64,
}

/// 返回全局资源统计的历史趋势数据。
pub async fn history(
    State(state): State<Arc<AppState>>,
    payload: Option<Json<StatsHistoryQuery>>,
) -> ApiResult<Response> {
    let hours = clamp_hours(payload.as_ref().and_then(|value| value.hours));
    let cutoff = Utc::now().timestamp() - hours * 3600;

    let rows = sqlx::query_as::<_, SummaryRow>(
        r#"
        SELECT
            created_at,
            cpu_core_percent,
            cpu_host_percent,
            memory_working_set_bytes,
            memory_limit_bytes,
            memory_percent,
            running_container_count,
            sampled_container_count
        FROM docker_metrics_summary
        WHERE created_at >= ?
        ORDER BY created_at ASC
        "#,
    )
    .bind(cutoff)
    .fetch_all(&state.metadata_db)
    .await?;

    let points = rows.into_iter().map(row_to_summary_point).collect();
    let response = docker::HostResourceUsageHistory { points };
    Ok(
        ApiResponse::success_with_raw("Resource usage history loaded", Some(response))
            .into_response(),
    )
}

/// 返回单个容器的最新资源统计快照。
pub async fn container_summary(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    info!(
        "Requesting container resource usage summary (cache): {}",
        id
    );
    let row = sqlx::query_as::<_, ContainerRow>(
        r#"
        SELECT
            created_at,
            cpu_core_percent,
            memory_working_set_bytes,
            memory_limit_bytes,
            memory_percent,
            network_rx_bytes,
            network_tx_bytes
        FROM docker_metrics_container
        WHERE container_id = ?
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(&id)
    .fetch_optional(&state.metadata_db)
    .await?;

    match row {
        Some(row) => Ok(ApiResponse::success_with_raw(
            "Container resource summary loaded",
            Some(row_to_container_summary(row)),
        )
        .into_response()),
        None => Err(ApiError::NotFound),
    }
}

/// 返回单个容器在时间窗口内的资源趋势。
pub async fn container_history(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    payload: Option<Json<StatsHistoryQuery>>,
) -> ApiResult<Response> {
    let hours = clamp_hours(payload.as_ref().and_then(|value| value.hours));
    let cutoff = Utc::now().timestamp() - hours * 3600;

    let rows = sqlx::query_as::<_, ContainerRow>(
        r#"
        SELECT
            created_at,
            cpu_core_percent,
            memory_working_set_bytes,
            memory_limit_bytes,
            memory_percent,
            network_rx_bytes,
            network_tx_bytes
        FROM docker_metrics_container
        WHERE container_id = ?
          AND created_at >= ?
        ORDER BY created_at ASC
        "#,
    )
    .bind(&id)
    .bind(cutoff)
    .fetch_all(&state.metadata_db)
    .await?;

    let points = rows_to_container_points(rows);
    let response = docker::ContainerStatsHistoryAllResponse {
        containers: vec![docker::ContainerStatsHistoryAllItem {
            id: id.clone(),
            name: id,
            points,
        }],
    };
    Ok(
        ApiResponse::success_with_raw("Container resource history loaded", Some(response))
            .into_response(),
    )
}

/// 批量返回多个容器的历史趋势，并补齐容器名称。
pub async fn container_histories(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<docker::ContainerStatsHistoryQuery>,
) -> ApiResult<Response> {
    info!("Requesting batch container resource usage history (cache)");
    let mut ids = payload.ids;
    ids.sort();
    ids.dedup();
    if ids.len() > 5 {
        return Err(ApiError::BadRequest(
            "At most 5 containers can be queried at once".to_string(),
        ));
    }
    let hours = clamp_hours(payload.hours);
    let cutoff = Utc::now().timestamp() - hours * 3600;

    let docker = state.docker_client().await?;
    let options = query_parameters::ListContainersOptionsBuilder::new()
        .all(true)
        .build();
    let containers = docker.list_containers(Some(options)).await?;
    let mut name_map: HashMap<String, String> = HashMap::new();
    for container in containers {
        if let Some(id) = container.id {
            let name = container
                .names
                .as_ref()
                .and_then(|names| names.first())
                .map(|value| value.trim_start_matches('/').to_string())
                .unwrap_or_else(|| id.clone());
            name_map.insert(id, name);
        }
    }

    let mut containers = Vec::with_capacity(ids.len());
    for id in ids {
        let rows = sqlx::query_as::<_, ContainerRow>(
            r#"
            SELECT
                created_at,
                cpu_core_percent,
                memory_working_set_bytes,
                memory_limit_bytes,
                memory_percent,
                network_rx_bytes,
                network_tx_bytes
            FROM docker_metrics_container
            WHERE container_id = ? AND created_at >= ?
            ORDER BY created_at ASC
            "#,
        )
        .bind(&id)
        .bind(cutoff)
        .fetch_all(&state.metadata_db)
        .await?;
        containers.push(docker::ContainerStatsHistoryAllItem {
            name: name_map.get(&id).cloned().unwrap_or_else(|| id.clone()),
            id,
            points: rows_to_container_points(rows),
        });
    }
    containers.sort_by(|a, b| a.name.cmp(&b.name));

    let response = docker::ContainerStatsHistoryAllResponse { containers };
    Ok(
        ApiResponse::success_with_raw("Container resource history loaded", Some(response))
            .into_response(),
    )
}

/// 批量返回多个容器的最新资源统计快照。
pub async fn container_summaries(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<docker::ContainerStatsBatchRequest>,
) -> ApiResult<Response> {
    info!(
        "Requesting batch container resource usage summary (cache): {} ids",
        payload.ids.len()
    );
    let mut summaries: HashMap<String, docker::ContainerResourceUsageSummary> = HashMap::new();

    for id in payload.ids {
        let row = sqlx::query_as::<_, ContainerRow>(
            r#"
            SELECT
                created_at,
                cpu_core_percent,
                memory_working_set_bytes,
                memory_limit_bytes,
                memory_percent,
                network_rx_bytes,
                network_tx_bytes
            FROM docker_metrics_container
            WHERE container_id = ?
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(&id)
        .fetch_optional(&state.metadata_db)
        .await?;

        if let Some(row) = row {
            summaries.insert(id, row_to_container_summary(row));
        }
    }

    let response = docker::ContainerStatsBatchResponse { summaries };
    Ok(
        ApiResponse::success_with_raw("Container resource summary loaded", Some(response))
            .into_response(),
    )
}

fn clamp_hours(value: Option<i64>) -> i64 {
    let max_hours = config::stats_retention_hours() as i64;
    let hours = value.unwrap_or(max_hours).max(1);
    hours.min(max_hours)
}

fn row_to_container_summary(row: ContainerRow) -> docker::ContainerResourceUsageSummary {
    docker::ContainerResourceUsageSummary {
        cpu_core_percent: row.cpu_core_percent,
        memory_working_set_bytes: row.memory_working_set_bytes as u64,
        memory_limit_bytes: row.memory_limit_bytes as u64,
        memory_percent: row.memory_percent,
        network_rx_bytes: row.network_rx_bytes as u64,
        network_tx_bytes: row.network_tx_bytes as u64,
    }
}

fn row_to_summary_point(row: SummaryRow) -> docker::HostResourceUsagePoint {
    docker::HostResourceUsagePoint {
        timestamp: row.created_at,
        cpu_host_percent: row.cpu_host_percent,
        cpu_core_percent: row.cpu_core_percent,
        memory_working_set_bytes: row.memory_working_set_bytes as u64,
        memory_limit_bytes: row.memory_limit_bytes as u64,
        memory_percent: row.memory_percent,
        running_container_count: row.running_container_count as usize,
        sampled_container_count: row.sampled_container_count as usize,
    }
}

fn rows_to_container_points(rows: Vec<ContainerRow>) -> Vec<docker::ContainerResourceUsagePoint> {
    let max_gap_seconds = (config::stats_sample_interval().as_secs() as i64 * 5) / 2;
    let mut previous: Option<&ContainerRow> = None;
    let mut points = Vec::with_capacity(rows.len());
    for row in &rows {
        let (network_rx_bytes_per_second, network_tx_bytes_per_second) = previous
            .filter(|previous| {
                let gap = row.created_at - previous.created_at;
                gap > 0
                    && gap <= max_gap_seconds
                    && row.network_rx_bytes >= previous.network_rx_bytes
                    && row.network_tx_bytes >= previous.network_tx_bytes
            })
            .map(|previous| {
                let elapsed = (row.created_at - previous.created_at) as f64;
                (
                    Some((row.network_rx_bytes - previous.network_rx_bytes) as f64 / elapsed),
                    Some((row.network_tx_bytes - previous.network_tx_bytes) as f64 / elapsed),
                )
            })
            .unwrap_or((None, None));
        points.push(docker::ContainerResourceUsagePoint {
            timestamp: row.created_at,
            cpu_core_percent: row.cpu_core_percent,
            memory_working_set_bytes: row.memory_working_set_bytes as u64,
            memory_limit_bytes: row.memory_limit_bytes as u64,
            memory_percent: row.memory_percent,
            network_rx_bytes_per_second,
            network_tx_bytes_per_second,
        });
        previous = Some(row);
    }
    points
}

#[cfg(test)]
mod tests {
    use super::{ContainerRow, rows_to_container_points};

    fn row(created_at: i64, rx: i64, tx: i64) -> ContainerRow {
        ContainerRow {
            created_at,
            cpu_core_percent: 120.0,
            memory_working_set_bytes: 1024,
            memory_limit_bytes: 4096,
            memory_percent: 25.0,
            network_rx_bytes: rx,
            network_tx_bytes: tx,
        }
    }

    #[test]
    fn network_rate_handles_normal_counter_growth() {
        let points = rows_to_container_points(vec![row(100, 100, 200), row(160, 700, 500)]);
        assert_eq!(points[0].network_rx_bytes_per_second, None);
        assert_eq!(points[1].network_rx_bytes_per_second, Some(10.0));
        assert_eq!(points[1].network_tx_bytes_per_second, Some(5.0));
    }

    #[test]
    fn network_rate_ignores_counter_reset_and_long_gap() {
        let points = rows_to_container_points(vec![
            row(100, 1000, 1000),
            row(160, 100, 100),
            row(400, 500, 500),
        ]);
        assert_eq!(points[1].network_rx_bytes_per_second, None);
        assert_eq!(points[2].network_rx_bytes_per_second, None);
    }
}
