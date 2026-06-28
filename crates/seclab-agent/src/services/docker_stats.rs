//! Docker 统计采样服务：定时采样并写入缓存。

use crate::config;
use crate::models::docker::ResourceUsageSummary;
use crate::state::AppState;
use bollard::models::ContainerSummaryStateEnum;
use bollard::query_parameters;
use chrono::Utc;
use futures_util::stream::{self, StreamExt};
use std::sync::Arc;
use tokio::time::{Duration, timeout};
use tracing::{debug, warn};

/// 启动后台采样任务，定期收集并清理 Docker 统计数据。
pub fn spawn_stats_collector(state: Arc<AppState>) {
    let interval = config::stats_sample_interval();
    let interval_secs = interval.as_secs().max(1);
    let cleanup_every = (300 / interval_secs).max(1);

    tokio::spawn(async move {
        debug!(
            "Docker resource usage sampling interval set to {:?} seconds",
            interval
        );
        let mut ticker = tokio::time::interval(interval);
        let mut cleanup_ticks = 0_u64;

        loop {
            ticker.tick().await;

            if let Err(err) = collect_and_store_stats(&state).await {
                warn!("Docker stats sampling failed: {}", err);
            }

            cleanup_ticks += 1;
            if cleanup_ticks >= cleanup_every {
                cleanup_ticks = 0;
                if let Err(err) = cleanup_old_stats(&state).await {
                    warn!("Docker stats cleanup failed: {}", err);
                }
            }
        }
    });
}

pub async fn collect_realtime_summary(state: &AppState) -> anyhow::Result<ResourceUsageSummary> {
    let docker = match state.docker_client().await {
        Ok(client) => client,
        Err(_) => {
            return Ok(ResourceUsageSummary {
                cpu_percent: 0.0,
                memory_usage_bytes: 0,
                memory_limit_bytes: 0,
                memory_percent: 0.0,
                network_rx_bytes: 0,
                network_tx_bytes: 0,
                container_count: 0,
            });
        }
    };

    let (summary, _) = collect_samples_and_summary(&docker, false).await?;
    Ok(summary)
}

async fn collect_and_store_stats(state: &AppState) -> anyhow::Result<()> {
    let docker = match state.docker_client().await {
        Ok(client) => client,
        Err(_) => return Ok(()),
    };

    let (summary, samples) = collect_samples_and_summary(&docker, true).await?;
    let timestamp = Utc::now().timestamp();
    let mut tx = state.metadata_db.begin().await?;

    sqlx::query(
        r#"
        INSERT INTO docker_metrics_summary (
            created_at,
            cpu_percent,
            memory_usage_bytes,
            memory_limit_bytes,
            memory_percent,
            network_rx_bytes,
            network_tx_bytes,
            container_count
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(timestamp)
    .bind(summary.cpu_percent)
    .bind(summary.memory_usage_bytes as i64)
    .bind(summary.memory_limit_bytes as i64)
    .bind(summary.memory_percent)
    .bind(summary.network_rx_bytes as i64)
    .bind(summary.network_tx_bytes as i64)
    .bind(summary.container_count as i64)
    .execute(&mut *tx)
    .await?;

    if !samples.is_empty() {
        for (id, sample) in samples {
            sqlx::query(
                r#"
                INSERT INTO docker_metrics_container (
                    container_id,
                    created_at,
                    cpu_percent,
                    memory_usage_bytes,
                    memory_limit_bytes,
                    memory_percent,
                    network_rx_bytes,
                    network_tx_bytes
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(id)
            .bind(timestamp)
            .bind(sample.cpu_percent)
            .bind(sample.memory_usage_bytes as i64)
            .bind(sample.memory_limit_bytes as i64)
            .bind(sample.memory_percent)
            .bind(sample.network_rx_bytes as i64)
            .bind(sample.network_tx_bytes as i64)
            .execute(&mut *tx)
            .await?;
        }
    }

    tx.commit().await?;
    debug!(
        "Docker stats sampled: containers={}, timestamp={}",
        summary.container_count, timestamp
    );
    Ok(())
}

async fn collect_samples_and_summary(
    docker: &bollard::Docker,
    include_network: bool,
) -> anyhow::Result<(ResourceUsageSummary, Vec<(String, ResourceUsageSummary)>)> {
    let options = query_parameters::ListContainersOptionsBuilder::new()
        .all(true)
        .build();
    let containers = docker.list_containers(Some(options)).await?;
    let running_ids: Vec<String> = containers
        .into_iter()
        .filter(|container| container.state == Some(ContainerSummaryStateEnum::RUNNING))
        .filter_map(|container| container.id)
        .collect();

    let samples: Vec<(String, ResourceUsageSummary)> = stream::iter(running_ids)
        .map(|id| async {
            let summary = fetch_container_stats_snapshot(docker, &id, include_network).await;
            summary.map(|value| (id, value))
        })
        .buffer_unordered(6)
        .filter_map(|value| async move { value })
        .collect()
        .await;

    let mut cpu_percent_total = 0.0_f64;
    let mut memory_usage_bytes = 0_u64;
    let mut memory_limit_bytes = 0_u64;
    let mut network_rx_bytes = 0_u64;
    let mut network_tx_bytes = 0_u64;
    let mut running_count = 0_usize;

    for (_, sample) in &samples {
        running_count += 1;
        cpu_percent_total += sample.cpu_percent;
        memory_usage_bytes += sample.memory_usage_bytes;
        memory_limit_bytes = memory_limit_bytes.max(sample.memory_limit_bytes);
        network_rx_bytes += sample.network_rx_bytes;
        network_tx_bytes += sample.network_tx_bytes;
    }

    if let Ok(info) = docker.info().await
        && let Some(mem_total) = info.mem_total
        && mem_total > 0
    {
        memory_limit_bytes = mem_total as u64;
    }

    if memory_limit_bytes > 0 {
        memory_usage_bytes = memory_usage_bytes.min(memory_limit_bytes);
    }

    let memory_percent = if memory_limit_bytes > 0 {
        (memory_usage_bytes as f64 / memory_limit_bytes as f64) * 100.0
    } else {
        0.0
    };

    let summary = ResourceUsageSummary {
        cpu_percent: cpu_percent_total,
        memory_usage_bytes,
        memory_limit_bytes,
        memory_percent,
        network_rx_bytes,
        network_tx_bytes,
        container_count: running_count,
    };

    Ok((summary, samples))
}

async fn cleanup_old_stats(state: &AppState) -> anyhow::Result<()> {
    let retention_hours = config::stats_retention_hours();
    let cutoff = Utc::now().timestamp() - (retention_hours as i64 * 3600);

    let mut tx = state.metadata_db.begin().await?;
    sqlx::query("DELETE FROM docker_metrics_summary WHERE created_at < ?")
        .bind(cutoff)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM docker_metrics_container WHERE created_at < ?")
        .bind(cutoff)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn fetch_container_stats_snapshot(
    docker: &bollard::Docker,
    id: &str,
    include_network: bool,
) -> Option<ResourceUsageSummary> {
    let stats_options = query_parameters::StatsOptionsBuilder::new()
        .stream(false)
        .one_shot(true)
        .build();
    let mut stream = docker.stats(id, Some(stats_options));
    let sample = timeout(Duration::from_millis(1200), stream.next())
        .await
        .ok()??
        .ok()?;

    let cpu_percent = calculate_cpu_percent(&sample);

    let (mut memory_usage_bytes, mut memory_limit_bytes) = (0_u64, 0_u64);
    if let Some(memory_stats) = &sample.memory_stats {
        if let Some(usage) = memory_stats.usage {
            memory_usage_bytes = usage;
        }
        if let Some(limit) = memory_stats.limit {
            memory_limit_bytes = limit;
        }
    }

    let mut network_rx_bytes = 0_u64;
    let mut network_tx_bytes = 0_u64;
    if include_network && let Some(networks) = sample.networks {
        for network in networks.values() {
            if let Some(rx) = network.rx_bytes {
                network_rx_bytes += rx;
            }
            if let Some(tx) = network.tx_bytes {
                network_tx_bytes += tx;
            }
        }
    }

    let memory_percent = if memory_limit_bytes > 0 {
        (memory_usage_bytes as f64 / memory_limit_bytes as f64) * 100.0
    } else {
        0.0
    };

    Some(ResourceUsageSummary {
        cpu_percent,
        memory_usage_bytes,
        memory_limit_bytes,
        memory_percent,
        network_rx_bytes,
        network_tx_bytes,
        container_count: 1,
    })
}

fn calculate_cpu_percent(stats: &bollard::models::ContainerStatsResponse) -> f64 {
    let Some(cpu_stats) = &stats.cpu_stats else {
        return 0.0;
    };
    let Some(precpu_stats) = &stats.precpu_stats else {
        return 0.0;
    };

    let cpu_total = cpu_stats
        .cpu_usage
        .as_ref()
        .and_then(|usage| usage.total_usage)
        .unwrap_or(0);
    let precpu_total = precpu_stats
        .cpu_usage
        .as_ref()
        .and_then(|usage| usage.total_usage)
        .unwrap_or(0);
    let cpu_delta = cpu_total.saturating_sub(precpu_total);

    let system_total = cpu_stats.system_cpu_usage.unwrap_or(0);
    let presystem_total = precpu_stats.system_cpu_usage.unwrap_or(0);
    let system_delta = system_total.saturating_sub(presystem_total);

    if system_delta == 0 || cpu_delta == 0 {
        return 0.0;
    }

    let online_cpus = cpu_stats.online_cpus.unwrap_or(1) as f64;
    (cpu_delta as f64 / system_delta as f64) * online_cpus * 100.0
}

#[cfg(test)]
mod tests {
    use super::{calculate_cpu_percent, cleanup_old_stats};
    use crate::config;
    use crate::test_support::setup_test_state;
    use bollard::models::{ContainerCpuStats, ContainerCpuUsage, ContainerStatsResponse};
    use chrono::Utc;

    fn make_stats(
        cpu_total: u64,
        precpu_total: u64,
        system_total: u64,
        presystem_total: u64,
    ) -> ContainerStatsResponse {
        ContainerStatsResponse {
            name: None,
            id: None,
            read: None,
            preread: None,
            pids_stats: None,
            blkio_stats: None,
            num_procs: None,
            storage_stats: None,
            cpu_stats: Some(ContainerCpuStats {
                cpu_usage: Some(ContainerCpuUsage {
                    total_usage: Some(cpu_total),
                    percpu_usage: None,
                    usage_in_kernelmode: None,
                    usage_in_usermode: None,
                }),
                system_cpu_usage: Some(system_total),
                online_cpus: Some(2),
                throttling_data: None,
            }),
            precpu_stats: Some(ContainerCpuStats {
                cpu_usage: Some(ContainerCpuUsage {
                    total_usage: Some(precpu_total),
                    percpu_usage: None,
                    usage_in_kernelmode: None,
                    usage_in_usermode: None,
                }),
                system_cpu_usage: Some(presystem_total),
                online_cpus: Some(2),
                throttling_data: None,
            }),
            memory_stats: None,
            networks: None,
        }
    }

    #[test]
    fn calculate_cpu_percent_handles_zero_delta() {
        let stats = make_stats(100, 100, 200, 200);
        assert_eq!(calculate_cpu_percent(&stats), 0.0);
    }

    #[test]
    fn calculate_cpu_percent_computes_ratio() {
        let stats = make_stats(200, 100, 300, 100);
        let percent = calculate_cpu_percent(&stats);
        assert!((percent - 100.0).abs() < 0.0001);
    }

    #[tokio::test]
    async fn cleanup_old_stats_removes_expired_rows() {
        let state = setup_test_state().await;
        let now = Utc::now().timestamp();
        let retention_hours = config::stats_retention_hours() as i64;
        let old = now - (retention_hours + 1) * 3600;
        let recent = now - 3600;

        sqlx::query(
            r#"
            INSERT INTO docker_metrics_summary (
                created_at, cpu_percent, memory_usage_bytes, memory_limit_bytes, memory_percent,
                network_rx_bytes, network_tx_bytes, container_count
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(old)
        .bind(1.0)
        .bind(10_i64)
        .bind(20_i64)
        .bind(50.0)
        .bind(5_i64)
        .bind(6_i64)
        .bind(1_i64)
        .execute(&state.metadata_db)
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO docker_metrics_summary (
                created_at, cpu_percent, memory_usage_bytes, memory_limit_bytes, memory_percent,
                network_rx_bytes, network_tx_bytes, container_count
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(recent)
        .bind(2.0)
        .bind(11_i64)
        .bind(21_i64)
        .bind(52.0)
        .bind(7_i64)
        .bind(8_i64)
        .bind(2_i64)
        .execute(&state.metadata_db)
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO docker_metrics_container (
                container_id, created_at, cpu_percent, memory_usage_bytes, memory_limit_bytes,
                memory_percent, network_rx_bytes, network_tx_bytes
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("abc")
        .bind(old)
        .bind(1.0)
        .bind(10_i64)
        .bind(20_i64)
        .bind(50.0)
        .bind(5_i64)
        .bind(6_i64)
        .execute(&state.metadata_db)
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO docker_metrics_container (
                container_id, created_at, cpu_percent, memory_usage_bytes, memory_limit_bytes,
                memory_percent, network_rx_bytes, network_tx_bytes
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("abc")
        .bind(recent)
        .bind(2.0)
        .bind(11_i64)
        .bind(21_i64)
        .bind(52.0)
        .bind(7_i64)
        .bind(8_i64)
        .execute(&state.metadata_db)
        .await
        .unwrap();

        cleanup_old_stats(&state).await.unwrap();

        let summary_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM docker_metrics_summary")
            .fetch_one(&state.metadata_db)
            .await
            .unwrap();
        let container_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM docker_metrics_container")
                .fetch_one(&state.metadata_db)
                .await
                .unwrap();

        assert_eq!(summary_count, 1);
        assert_eq!(container_count, 1);
    }
}
