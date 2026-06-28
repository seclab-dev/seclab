//! 主机系统监控采样服务：定时采样并写入缓存。

use crate::config;
use crate::services::settings;
use crate::state::AppState;
use crate::state::DbPool;
use chrono::Utc;
use seclab_contracts::types::HostSystemSummary;
use sqlx::FromRow;
use std::sync::Arc;
use sysinfo::{Networks, System};
use tracing::{debug, warn};

#[derive(Debug, Clone, serde::Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SystemMetricPoint {
    pub created_at: i64,
    pub load_avg_1: f64,
    pub load_avg_5: f64,
    pub load_avg_15: f64,
    pub cpu_percent: f64,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub memory_percent: f64,
    pub disk_read_bytes: u64,
    pub disk_write_bytes: u64,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectorStatus {
    pub enabled: bool,
}

const COLLECTOR_ENABLED_KEY: &str = "system_metrics.collector_enabled";

/// 启动后台采样任务，定期收集系统监控数据并清理过期记录。
pub fn spawn_collector(state: Arc<AppState>) {
    let interval = config::system_metrics_sample_interval();
    let interval_secs = interval.as_secs().max(1);
    let cleanup_every = (3600 / interval_secs).max(1);

    tokio::spawn(async move {
        debug!(
            "System metrics sampling interval set to {:?} seconds",
            interval
        );
        let mut ticker = tokio::time::interval(interval);
        let mut cleanup_ticks = 0_u64;

        loop {
            ticker.tick().await;

            if state.system_metrics_enabled().await
                && let Err(err) = collect_and_store(&state).await
            {
                warn!("System metrics sampling failed: {}", err);
            }

            cleanup_ticks += 1;
            if cleanup_ticks >= cleanup_every {
                cleanup_ticks = 0;
                if let Err(err) = cleanup_old_stats(&state).await {
                    warn!("System metrics cleanup failed: {}", err);
                }
            }
        }
    });
}

pub async fn init_collector_enabled(pool: &crate::state::DbPool) -> anyhow::Result<bool> {
    settings::get_bool_or_default(pool, COLLECTOR_ENABLED_KEY, true).await
}

pub async fn set_collector_enabled(state: &AppState, enabled: bool) -> anyhow::Result<()> {
    settings::set_bool(&state.metadata_db, COLLECTOR_ENABLED_KEY, enabled).await?;
    state.set_system_metrics_enabled(enabled).await;
    Ok(())
}

/// 告警阈值配置。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertThresholds {
    /// CPU 使用率警告阈值（百分比），默认 80。
    pub cpu_warning: f64,
    /// CPU 使用率危险阈值（百分比），默认 95。
    pub cpu_danger: f64,
    /// 内存使用率警告阈值（百分比），默认 80。
    pub memory_warning: f64,
    /// 内存使用率危险阈值（百分比），默认 95。
    pub memory_danger: f64,
}

const ALERT_CPU_WARNING_KEY: &str = "system_metrics.alert.cpu_warning";
const ALERT_CPU_DANGER_KEY: &str = "system_metrics.alert.cpu_danger";
const ALERT_MEMORY_WARNING_KEY: &str = "system_metrics.alert.memory_warning";
const ALERT_MEMORY_DANGER_KEY: &str = "system_metrics.alert.memory_danger";

/// 读取告警阈值配置，不存在时自动写入默认值。
pub async fn fetch_alert_thresholds(pool: &DbPool) -> anyhow::Result<AlertThresholds> {
    let cpu_warning = settings::get_string_or_default(pool, ALERT_CPU_WARNING_KEY, "80")
        .await?
        .parse::<f64>()
        .unwrap_or(80.0);
    let cpu_danger = settings::get_string_or_default(pool, ALERT_CPU_DANGER_KEY, "95")
        .await?
        .parse::<f64>()
        .unwrap_or(95.0);
    let memory_warning = settings::get_string_or_default(pool, ALERT_MEMORY_WARNING_KEY, "80")
        .await?
        .parse::<f64>()
        .unwrap_or(80.0);
    let memory_danger = settings::get_string_or_default(pool, ALERT_MEMORY_DANGER_KEY, "95")
        .await?
        .parse::<f64>()
        .unwrap_or(95.0);
    Ok(AlertThresholds {
        cpu_warning,
        cpu_danger,
        memory_warning,
        memory_danger,
    })
}

/// 写入告警阈值配置，所有值钳制在 0-100 范围。
pub async fn set_alert_thresholds(
    pool: &DbPool,
    thresholds: &AlertThresholds,
) -> anyhow::Result<()> {
    let cpu_warning = thresholds.cpu_warning.clamp(0.0, 100.0);
    let cpu_danger = thresholds.cpu_danger.clamp(0.0, 100.0);
    let memory_warning = thresholds.memory_warning.clamp(0.0, 100.0);
    let memory_danger = thresholds.memory_danger.clamp(0.0, 100.0);

    settings::set_string(pool, ALERT_CPU_WARNING_KEY, &cpu_warning.to_string()).await?;
    settings::set_string(pool, ALERT_CPU_DANGER_KEY, &cpu_danger.to_string()).await?;
    settings::set_string(pool, ALERT_MEMORY_WARNING_KEY, &memory_warning.to_string()).await?;
    settings::set_string(pool, ALERT_MEMORY_DANGER_KEY, &memory_danger.to_string()).await?;
    Ok(())
}

pub fn collect_snapshot() -> anyhow::Result<HostSystemSummary> {
    let mut system = System::new_all();
    system.refresh_cpu_usage();
    std::thread::sleep(std::time::Duration::from_millis(120));
    system.refresh_cpu_usage();
    system.refresh_memory();

    let load_avg = System::load_average();
    let cpu_percent = system.global_cpu_info().cpu_usage() as f64;
    let memory_total_bytes = system.total_memory();
    let memory_used_bytes = system.used_memory();
    let memory_percent = if memory_total_bytes > 0 {
        (memory_used_bytes as f64 / memory_total_bytes as f64) * 100.0
    } else {
        0.0
    };

    let (disk_read_bytes, disk_write_bytes) = collect_disk_io_totals();

    let mut networks = Networks::new_with_refreshed_list();
    networks.refresh();
    let mut network_rx_bytes = 0_u64;
    let mut network_tx_bytes = 0_u64;
    for (_name, data) in &networks {
        network_rx_bytes += data.total_received();
        network_tx_bytes += data.total_transmitted();
    }

    Ok(HostSystemSummary {
        version: env!("CARGO_PKG_VERSION").to_string(),
        cpu_percent: cpu_percent as f32,
        memory_used_bytes,
        memory_total_bytes,
        memory_percent,
        load_avg_1: load_avg.one,
        load_avg_5: load_avg.five,
        load_avg_15: load_avg.fifteen,
        disk_read_bytes,
        disk_write_bytes,
        network_rx_bytes,
        network_tx_bytes,
        collected_at: Utc::now().timestamp(),
    })
}

fn collect_disk_io_totals() -> (u64, u64) {
    #[cfg(target_os = "linux")]
    {
        let content = std::fs::read_to_string("/proc/diskstats").unwrap_or_default();
        let mut read_sectors = 0_u64;
        let mut write_sectors = 0_u64;

        for line in content.lines() {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 14 {
                continue;
            }
            let Some(name) = cols.get(2) else {
                continue;
            };
            if name.starts_with("loop")
                || name.starts_with("ram")
                || name.starts_with("zram")
                || name.starts_with("fd")
                || name.starts_with("sr")
            {
                continue;
            }
            let read = cols.get(5).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            let write = cols.get(9).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            read_sectors = read_sectors.saturating_add(read);
            write_sectors = write_sectors.saturating_add(write);
        }

        // Linux 块设备统计通常以 512-byte sectors 表示。
        (
            read_sectors.saturating_mul(512),
            write_sectors.saturating_mul(512),
        )
    }

    #[cfg(not(target_os = "linux"))]
    {
        (0, 0)
    }
}

pub async fn fetch_history(
    state: &AppState,
    hours: Option<i64>,
) -> anyhow::Result<Vec<SystemMetricPoint>> {
    let retention_hours = (config::system_metrics_retention_days() as i64) * 24;
    let hours = hours.unwrap_or(retention_hours).clamp(1, retention_hours);
    let cutoff = Utc::now().timestamp() - (hours * 3600);

    let rows = sqlx::query_as::<_, SystemMetricPoint>(
        r#"
        SELECT
            created_at,
            load_avg_1,
            load_avg_5,
            load_avg_15,
            cpu_percent,
            memory_used_bytes,
            memory_total_bytes,
            memory_percent,
            disk_read_bytes,
            disk_write_bytes,
            network_rx_bytes,
            network_tx_bytes
        FROM system_metrics
        WHERE created_at >= ?
        ORDER BY created_at ASC
        "#,
    )
    .bind(cutoff)
    .fetch_all(&state.metadata_db)
    .await?;

    Ok(rows)
}

pub async fn clear_history(state: &AppState) -> anyhow::Result<u64> {
    let result = sqlx::query("DELETE FROM system_metrics")
        .execute(&state.metadata_db)
        .await?;
    Ok(result.rows_affected())
}

async fn collect_and_store(state: &AppState) -> anyhow::Result<()> {
    let summary = collect_snapshot()?;

    sqlx::query(
        r#"
        INSERT INTO system_metrics (
            created_at,
            load_avg_1,
            load_avg_5,
            load_avg_15,
            cpu_percent,
            memory_used_bytes,
            memory_total_bytes,
            memory_percent,
            disk_read_bytes,
            disk_write_bytes,
            network_rx_bytes,
            network_tx_bytes
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(summary.collected_at)
    .bind(summary.load_avg_1)
    .bind(summary.load_avg_5)
    .bind(summary.load_avg_15)
    .bind(summary.cpu_percent as f64)
    .bind(summary.memory_used_bytes as i64)
    .bind(summary.memory_total_bytes as i64)
    .bind(summary.memory_percent)
    .bind(summary.disk_read_bytes as i64)
    .bind(summary.disk_write_bytes as i64)
    .bind(summary.network_rx_bytes as i64)
    .bind(summary.network_tx_bytes as i64)
    .execute(&state.metadata_db)
    .await?;

    Ok(())
}

async fn cleanup_old_stats(state: &AppState) -> anyhow::Result<()> {
    let retention_seconds = (config::system_metrics_retention_days() as i64) * 24 * 3600;
    let cutoff = Utc::now().timestamp() - retention_seconds;
    sqlx::query("DELETE FROM system_metrics WHERE created_at < ?")
        .bind(cutoff)
        .execute(&state.metadata_db)
        .await?;
    Ok(())
}
