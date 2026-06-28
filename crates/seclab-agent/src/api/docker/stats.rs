//! Docker 统计 API：实时与缓存的资源数据输出。

use crate::config;
use crate::models::docker;
use crate::state::AppState;
use crate::types::{ApiResponse, ApiResult};
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
    cpu_percent: f64,
    memory_usage_bytes: i64,
    memory_limit_bytes: i64,
    memory_percent: f64,
    network_rx_bytes: i64,
    network_tx_bytes: i64,
    container_count: i64,
}

#[derive(Debug, FromRow)]
struct ContainerRow {
    created_at: i64,
    cpu_percent: f64,
    memory_usage_bytes: i64,
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
            cpu_percent,
            memory_usage_bytes,
            memory_limit_bytes,
            memory_percent,
            network_rx_bytes,
            network_tx_bytes,
            container_count
        FROM docker_metrics_summary
        WHERE created_at >= ?
        ORDER BY created_at ASC
        "#,
    )
    .bind(cutoff)
    .fetch_all(&state.metadata_db)
    .await?;

    let points = rows.into_iter().map(row_to_summary_point).collect();
    let response = docker::ResourceUsageHistory { points };
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
            cpu_percent,
            memory_usage_bytes,
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

    let summary = row
        .map(row_to_container_summary)
        .unwrap_or_else(empty_container_summary);
    Ok(
        ApiResponse::success_with_raw("Container resource summary loaded", Some(summary))
            .into_response(),
    )
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
            cpu_percent,
            memory_usage_bytes,
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

    let points = rows.into_iter().map(row_to_container_point).collect();
    let response = docker::ResourceUsageHistory { points };
    Ok(
        ApiResponse::success_with_raw("Container resource history loaded", Some(response))
            .into_response(),
    )
}

/// 批量返回多个容器的历史趋势，并补齐容器名称。
pub async fn container_histories(
    State(state): State<Arc<AppState>>,
    payload: Option<Json<StatsHistoryQuery>>,
) -> ApiResult<Response> {
    info!("Requesting batch container resource usage history (cache)");
    let hours = clamp_hours(payload.as_ref().and_then(|value| value.hours));
    let cutoff = Utc::now().timestamp() - hours * 3600;

    let rows = sqlx::query_as::<_, ContainerHistoryRow>(
        r#"
        SELECT
            container_id,
            created_at,
            cpu_percent,
            memory_usage_bytes,
            memory_limit_bytes,
            memory_percent,
            network_rx_bytes,
            network_tx_bytes
        FROM docker_metrics_container
        WHERE created_at >= ?
        ORDER BY container_id ASC, created_at ASC
        "#,
    )
    .bind(cutoff)
    .fetch_all(&state.metadata_db)
    .await?;

    let mut points_map: HashMap<String, Vec<docker::ResourceUsagePoint>> = HashMap::new();
    for row in rows {
        let points = points_map.entry(row.container_id.clone()).or_default();
        points.push(row_to_container_point(ContainerRow {
            created_at: row.created_at,
            cpu_percent: row.cpu_percent,
            memory_usage_bytes: row.memory_usage_bytes,
            memory_limit_bytes: row.memory_limit_bytes,
            memory_percent: row.memory_percent,
            network_rx_bytes: row.network_rx_bytes,
            network_tx_bytes: row.network_tx_bytes,
        }));
    }

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

    let mut containers: Vec<docker::ContainerStatsHistoryAllItem> = points_map
        .into_iter()
        .map(|(id, points)| docker::ContainerStatsHistoryAllItem {
            name: name_map.get(&id).cloned().unwrap_or_else(|| id.clone()),
            id,
            points,
        })
        .collect();
    containers.sort_by(|a, b| a.name.cmp(&b.name));

    let response = docker::ContainerStatsHistoryAllResponse { containers };
    Ok(
        ApiResponse::success_with_raw("Container resource history loaded", Some(response))
            .into_response(),
    )
}

#[derive(Debug, FromRow)]
struct ContainerHistoryRow {
    container_id: String,
    created_at: i64,
    cpu_percent: f64,
    memory_usage_bytes: i64,
    memory_limit_bytes: i64,
    memory_percent: f64,
    network_rx_bytes: i64,
    network_tx_bytes: i64,
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
    let mut summaries: HashMap<String, docker::ResourceUsageSummary> = HashMap::new();

    for id in payload.ids {
        let row = sqlx::query_as::<_, ContainerRow>(
            r#"
            SELECT
                created_at,
                cpu_percent,
                memory_usage_bytes,
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

fn empty_container_summary() -> docker::ResourceUsageSummary {
    docker::ResourceUsageSummary {
        cpu_percent: 0.0,
        memory_usage_bytes: 0,
        memory_limit_bytes: 0,
        memory_percent: 0.0,
        network_rx_bytes: 0,
        network_tx_bytes: 0,
        container_count: 0,
    }
}

fn row_to_container_summary(row: ContainerRow) -> docker::ResourceUsageSummary {
    docker::ResourceUsageSummary {
        cpu_percent: row.cpu_percent,
        memory_usage_bytes: row.memory_usage_bytes as u64,
        memory_limit_bytes: row.memory_limit_bytes as u64,
        memory_percent: row.memory_percent,
        network_rx_bytes: row.network_rx_bytes as u64,
        network_tx_bytes: row.network_tx_bytes as u64,
        container_count: 1,
    }
}

fn row_to_summary_point(row: SummaryRow) -> docker::ResourceUsagePoint {
    docker::ResourceUsagePoint {
        timestamp: row.created_at,
        cpu_percent: row.cpu_percent,
        memory_usage_bytes: row.memory_usage_bytes as u64,
        memory_limit_bytes: row.memory_limit_bytes as u64,
        memory_percent: row.memory_percent,
        network_rx_bytes: row.network_rx_bytes as u64,
        network_tx_bytes: row.network_tx_bytes as u64,
        container_count: Some(row.container_count as usize),
    }
}

fn row_to_container_point(row: ContainerRow) -> docker::ResourceUsagePoint {
    docker::ResourceUsagePoint {
        timestamp: row.created_at,
        cpu_percent: row.cpu_percent,
        memory_usage_bytes: row.memory_usage_bytes as u64,
        memory_limit_bytes: row.memory_limit_bytes as u64,
        memory_percent: row.memory_percent,
        network_rx_bytes: row.network_rx_bytes as u64,
        network_tx_bytes: row.network_tx_bytes as u64,
        container_count: None,
    }
}
