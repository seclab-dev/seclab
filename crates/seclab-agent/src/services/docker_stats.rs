//! Docker 统计采样服务：定时采样并写入缓存。

use crate::config;
use crate::models::docker::{
    ContainerResourceUsageSample, HostResourceUsageSummary, ResourceSampleStatus,
};
use crate::state::AppState;
use bollard::models::ContainerSummaryStateEnum;
use bollard::query_parameters;
use chrono::Utc;
use futures_util::stream::{self, StreamExt};
use sqlx::FromRow;
use std::sync::Arc;
use tokio::time::{Duration, timeout};
use tracing::{debug, warn};

const CONTAINER_STATS_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, FromRow)]
struct LatestSummaryRow {
    created_at: i64,
    cpu_core_percent: f64,
    cpu_host_percent: f64,
    memory_working_set_bytes: i64,
    memory_limit_bytes: i64,
    memory_percent: f64,
    running_container_count: i64,
    sampled_container_count: i64,
}

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

/// 从缓存读取最近一次宿主机 Docker 资源汇总，并标注新鲜度。
pub async fn load_latest_summary(state: &AppState) -> anyhow::Result<HostResourceUsageSummary> {
    let row = sqlx::query_as::<_, LatestSummaryRow>(
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
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(&state.metadata_db)
    .await?;

    let Some(row) = row else {
        return Ok(HostResourceUsageSummary {
            status: ResourceSampleStatus::Unavailable,
            collected_at: None,
            running_container_count: 0,
            sampled_container_count: 0,
            cpu_host_percent: 0.0,
            cpu_core_percent: 0.0,
            memory_working_set_bytes: 0,
            memory_limit_bytes: 0,
            memory_percent: 0.0,
        });
    };

    let status = resolve_sample_status(
        Utc::now().timestamp(),
        row.created_at,
        row.running_container_count,
        row.sampled_container_count,
    );
    Ok(HostResourceUsageSummary {
        status,
        collected_at: Some(row.created_at),
        running_container_count: row.running_container_count.max(0) as usize,
        sampled_container_count: row.sampled_container_count.max(0) as usize,
        cpu_host_percent: row.cpu_host_percent,
        cpu_core_percent: row.cpu_core_percent,
        memory_working_set_bytes: row.memory_working_set_bytes.max(0) as u64,
        memory_limit_bytes: row.memory_limit_bytes.max(0) as u64,
        memory_percent: row.memory_percent,
    })
}

/// 根据采样时间与覆盖率判定资源数据状态。
fn resolve_sample_status(
    now: i64,
    collected_at: i64,
    running_container_count: i64,
    sampled_container_count: i64,
) -> ResourceSampleStatus {
    let stale_after = config::stats_sample_interval().as_secs() as i64 * 2;
    if now - collected_at > stale_after {
        ResourceSampleStatus::Stale
    } else if sampled_container_count < running_container_count {
        ResourceSampleStatus::Partial
    } else {
        ResourceSampleStatus::Fresh
    }
}

async fn collect_and_store_stats(state: &AppState) -> anyhow::Result<()> {
    let docker = match state.docker_client().await {
        Ok(client) => client,
        Err(_) => return Ok(()),
    };

    let (summary, samples) = collect_samples_and_summary(&docker).await?;
    let timestamp = Utc::now().timestamp();
    let mut tx = state.metadata_db.begin().await?;

    sqlx::query(
        r#"
        INSERT INTO docker_metrics_summary (
            created_at,
            cpu_core_percent,
            cpu_host_percent,
            memory_working_set_bytes,
            memory_limit_bytes,
            memory_percent,
            running_container_count,
            sampled_container_count
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(timestamp)
    .bind(summary.cpu_core_percent)
    .bind(summary.cpu_host_percent)
    .bind(summary.memory_working_set_bytes as i64)
    .bind(summary.memory_limit_bytes as i64)
    .bind(summary.memory_percent)
    .bind(summary.running_container_count as i64)
    .bind(summary.sampled_container_count as i64)
    .execute(&mut *tx)
    .await?;

    if !samples.is_empty() {
        for (id, sample) in samples {
            sqlx::query(
                r#"
                INSERT INTO docker_metrics_container (
                    container_id,
                    created_at,
                    cpu_core_percent,
                    memory_working_set_bytes,
                    memory_limit_bytes,
                    memory_percent,
                    network_rx_bytes,
                    network_tx_bytes
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(id)
            .bind(timestamp)
            .bind(sample.cpu_core_percent)
            .bind(sample.memory_working_set_bytes as i64)
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
        summary.sampled_container_count, timestamp
    );
    Ok(())
}

async fn collect_samples_and_summary(
    docker: &bollard::Docker,
) -> anyhow::Result<(
    HostResourceUsageSummary,
    Vec<(String, ContainerResourceUsageSample)>,
)> {
    let options = query_parameters::ListContainersOptionsBuilder::new()
        .all(true)
        .build();
    let containers = docker.list_containers(Some(options)).await?;
    let running_ids: Vec<String> = containers
        .into_iter()
        .filter(|container| container.state == Some(ContainerSummaryStateEnum::RUNNING))
        .filter_map(|container| container.id)
        .collect();

    let running_container_count = running_ids.len();
    let samples: Vec<(String, ContainerResourceUsageSample)> = stream::iter(running_ids)
        .map(|id| async {
            let summary = fetch_container_stats_snapshot(docker, &id).await;
            summary.map(|value| (id, value))
        })
        .buffer_unordered(6)
        .filter_map(|value| async move { value })
        .collect()
        .await;

    let sampled_container_count = samples.len();
    let mut cpu_core_percent = 0.0_f64;
    let mut memory_working_set_bytes = 0_u64;
    let mut memory_limit_bytes = 0_u64;

    for (_, sample) in &samples {
        cpu_core_percent += sample.cpu_core_percent;
        memory_working_set_bytes += sample.memory_working_set_bytes;
        memory_limit_bytes = memory_limit_bytes.max(sample.memory_limit_bytes);
    }

    let mut online_cpus = 1_u64;
    if let Ok(info) = docker.info().await {
        if let Some(mem_total) = info.mem_total
            && mem_total > 0
        {
            memory_limit_bytes = mem_total as u64;
        }
        if let Some(ncpu) = info.ncpu
            && ncpu > 0
        {
            online_cpus = ncpu as u64;
        }
    }

    let memory_percent = if memory_limit_bytes > 0 {
        ((memory_working_set_bytes as f64 / memory_limit_bytes as f64) * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    let cpu_host_percent = normalize_cpu_host_percent(cpu_core_percent, online_cpus);
    let status = if sampled_container_count < running_container_count {
        ResourceSampleStatus::Partial
    } else {
        ResourceSampleStatus::Fresh
    };

    let summary = HostResourceUsageSummary {
        status,
        collected_at: Some(Utc::now().timestamp()),
        running_container_count,
        sampled_container_count,
        cpu_host_percent,
        cpu_core_percent,
        memory_working_set_bytes,
        memory_limit_bytes,
        memory_percent,
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
) -> Option<ContainerResourceUsageSample> {
    let stats_options = container_stats_options();
    let mut stream = docker.stats(id, Some(stats_options));
    let sample = timeout(CONTAINER_STATS_TIMEOUT, stream.next())
        .await
        .ok()??
        .ok()?;

    let cpu_core_percent = calculate_cpu_percent(&sample);

    let (mut memory_working_set_bytes, mut memory_limit_bytes) = (0_u64, 0_u64);
    if let Some(memory_stats) = &sample.memory_stats {
        if let Some(usage) = memory_stats.usage {
            let reclaimable_cache = memory_stats
                .stats
                .as_ref()
                .and_then(|stats| {
                    stats
                        .get("total_inactive_file")
                        .or_else(|| stats.get("inactive_file"))
                        .or_else(|| stats.get("cache"))
                })
                .copied()
                .unwrap_or(0);
            memory_working_set_bytes = usage.saturating_sub(reclaimable_cache);
        }
        if let Some(limit) = memory_stats.limit {
            memory_limit_bytes = limit;
        }
    }

    let mut network_rx_bytes = 0_u64;
    let mut network_tx_bytes = 0_u64;
    if let Some(networks) = sample.networks {
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
        ((memory_working_set_bytes as f64 / memory_limit_bytes as f64) * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };

    Some(ContainerResourceUsageSample {
        cpu_core_percent,
        memory_working_set_bytes,
        memory_limit_bytes,
        memory_percent,
        network_rx_bytes,
        network_tx_bytes,
    })
}

/// 构造一次非流式双周期 Docker 统计请求。
fn container_stats_options() -> query_parameters::StatsOptions {
    query_parameters::StatsOptionsBuilder::new()
        .stream(false)
        .one_shot(false)
        .build()
}

/// 将 Docker 多核 CPU 百分比归一化为宿主机总容量占比。
fn normalize_cpu_host_percent(cpu_core_percent: f64, online_cpus: u64) -> f64 {
    (cpu_core_percent / online_cpus.max(1) as f64).clamp(0.0, 100.0)
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

    let online_cpus = cpu_stats
        .online_cpus
        .filter(|value| *value > 0)
        .or_else(|| {
            cpu_stats
                .cpu_usage
                .as_ref()
                .and_then(|usage| usage.percpu_usage.as_ref())
                .map(|usage| usage.len() as u32)
                .filter(|value| *value > 0)
        })
        .unwrap_or(1) as f64;
    (cpu_delta as f64 / system_delta as f64) * online_cpus * 100.0
}

#[cfg(test)]
mod tests {
    use super::{
        calculate_cpu_percent, cleanup_old_stats, container_stats_options,
        normalize_cpu_host_percent, resolve_sample_status,
    };
    use crate::config;
    use crate::models::docker::ResourceSampleStatus;
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
            os_type: None,
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

    #[test]
    fn calculate_cpu_percent_falls_back_to_per_cpu_usage_count() {
        let mut stats = make_stats(200, 100, 300, 100);
        let cpu_stats = stats.cpu_stats.as_mut().unwrap();
        cpu_stats.online_cpus = None;
        cpu_stats.cpu_usage.as_mut().unwrap().percpu_usage = Some(vec![50, 50, 50, 50]);

        let percent = calculate_cpu_percent(&stats);

        assert!((percent - 200.0).abs() < 0.0001);
    }

    #[test]
    fn container_stats_waits_for_two_non_streaming_samples() {
        let options = container_stats_options();
        assert!(!options.stream);
        assert!(!options.one_shot);
    }

    #[test]
    fn normalize_cpu_percent_uses_host_capacity() {
        assert_eq!(normalize_cpu_host_percent(400.0, 8), 50.0);
        assert_eq!(normalize_cpu_host_percent(900.0, 8), 100.0);
    }

    #[test]
    fn sample_status_distinguishes_partial_stale_and_idle() {
        assert_eq!(
            resolve_sample_status(100, 90, 2, 1),
            ResourceSampleStatus::Partial
        );
        assert_eq!(
            resolve_sample_status(1000, 0, 2, 2),
            ResourceSampleStatus::Stale
        );
        assert_eq!(
            resolve_sample_status(100, 90, 0, 0),
            ResourceSampleStatus::Fresh
        );
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
                created_at, cpu_core_percent, cpu_host_percent, memory_working_set_bytes,
                memory_limit_bytes, memory_percent, running_container_count, sampled_container_count
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(old)
        .bind(1.0)
        .bind(1.0)
        .bind(10_i64)
        .bind(20_i64)
        .bind(50.0)
        .bind(1_i64)
        .bind(1_i64)
        .execute(&state.metadata_db)
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO docker_metrics_summary (
                created_at, cpu_core_percent, cpu_host_percent, memory_working_set_bytes,
                memory_limit_bytes, memory_percent, running_container_count, sampled_container_count
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(recent)
        .bind(2.0)
        .bind(2.0)
        .bind(11_i64)
        .bind(21_i64)
        .bind(52.0)
        .bind(2_i64)
        .bind(2_i64)
        .execute(&state.metadata_db)
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO docker_metrics_container (
                container_id, created_at, cpu_core_percent, memory_working_set_bytes, memory_limit_bytes,
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
                container_id, created_at, cpu_core_percent, memory_working_set_bytes, memory_limit_bytes,
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
