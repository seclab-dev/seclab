//! 系统信息 API：提供运行环境与系统摘要信息。

use crate::services::system_metrics;
use crate::state::AppState;
use crate::types::{ApiResponse, ApiResult};
use axum::{
    Json, Router,
    extract::{Path, State},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap, fs, net::Ipv4Addr, path::Path as FsPath, process::Command, sync::Arc,
};
use sysinfo::System;

const DISK_MOUNT_ROOT: &str = "/mnt";
const SYSTEM_MOUNTPOINTS: &[&str] = &["/", "/boot", "/boot/efi", "/usr", "/var"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemHistoryQuery {
    pub hours: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectorStatusPayload {
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MountPartitionPayload {
    pub mountpoint: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatPartitionPayload {
    pub filesystem: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeDiskPayload {
    pub filesystem: Option<String>,
    pub mountpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    pub name: String,
    pub size_bytes: u64,
    pub partition_count: usize,
    pub disk_type: String,
    pub model: String,
    pub serial: String,
    pub partition_table: String,
    pub is_system_disk: bool,
    pub read_only: bool,
    pub partitions: Vec<DiskPartitionInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskPartitionInfo {
    pub name: String,
    pub size_bytes: u64,
    pub used_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub usage_percent: Option<f64>,
    pub mountpoint: String,
    pub filesystem: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemAboutInfo {
    pub hostname: String,
    pub os_name: String,
    pub os_version: String,
    pub kernel_version: String,
    pub architecture: String,
    pub cpu_model: String,
    pub cpu_vendor: String,
    pub cpu_physical_cores: usize,
    pub cpu_logical_cores: usize,
    pub cpu_frequency_mhz: u64,
    pub cpu_cache_l3: String,
    pub memory_total_bytes: u64,
    pub memory_available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub uptime_seconds: u64,
    pub boot_time_epoch: u64,
    pub timezone: String,
    pub locale: String,
    pub virtualization: String,
    pub primary_interface: String,
    pub mac_address: String,
    pub ipv4_addresses: Vec<String>,
    pub ipv6_addresses: Vec<String>,
    pub default_gateway: String,
    pub dns_servers: Vec<String>,
    pub disk_total_bytes: u64,
    pub seclab_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LsblkResult {
    #[serde(default)]
    blockdevices: Vec<LsblkDevice>,
}

#[derive(Debug, Clone, Deserialize)]
struct LsblkDevice {
    name: String,
    #[serde(default)]
    size: Option<LsblkSizeValue>,
    #[serde(rename = "type")]
    device_type: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    serial: Option<String>,
    #[serde(default)]
    pttype: Option<String>,
    #[serde(default)]
    fstype: Option<String>,
    #[serde(default)]
    mountpoint: Option<String>,
    #[serde(default)]
    pkname: Option<String>,
    #[serde(default)]
    rota: Option<bool>,
    #[serde(default)]
    tran: Option<String>,
    #[serde(default)]
    children: Vec<LsblkDevice>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum LsblkSizeValue {
    Number(u64),
    Text(String),
}

/// 返回主机系统资源概览（不包含 Docker 状态）。
pub async fn summary() -> ApiResult<Response> {
    let summary = tokio::task::spawn_blocking(system_metrics::collect_snapshot)
        .await
        .map_err(|err| crate::types::ApiError::Internal(err.to_string()))?
        .map_err(|err| crate::types::ApiError::Internal(err.to_string()))?;

    Ok(ApiResponse::success_with_raw("System summary loaded", Some(summary)).into_response())
}

/// 返回主机关于信息（系统与硬件概要）。
pub async fn about() -> ApiResult<Response> {
    let about = tokio::task::spawn_blocking(collect_about_info)
        .await
        .map_err(|err| crate::types::ApiError::Internal(err.to_string()))?;
    Ok(ApiResponse::success_with_raw("System about info loaded", Some(about)).into_response())
}

/// 返回主机磁盘清单（包含系统盘标记）。
pub async fn disks() -> ApiResult<Response> {
    let disks = tokio::task::spawn_blocking(collect_disks)
        .await
        .map_err(|err| crate::types::ApiError::Internal(err.to_string()))?
        .map_err(|err| crate::types::ApiError::Internal(err.to_string()))?;
    Ok(ApiResponse::success_with_raw("Disk list loaded", Some(disks)).into_response())
}

/// 挂载分区。
pub async fn mount_partition(
    Path((disk_name, partition_name)): Path<(String, String)>,
    payload: Option<Json<MountPartitionPayload>>,
) -> ApiResult<Response> {
    let mountpoint = payload
        .and_then(|value| value.0.mountpoint)
        .filter(|value| !value.trim().is_empty());
    tokio::task::spawn_blocking(move || {
        do_mount_partition(&disk_name, &partition_name, mountpoint)
    })
    .await
    .map_err(|err| crate::types::ApiError::Internal(err.to_string()))?
    .map_err(|err| crate::types::ApiError::BadRequest(err.to_string()))?;
    Ok(ApiResponse::ok("Partition mounted").into_response())
}

/// 卸载分区。
pub async fn unmount_partition(
    Path((disk_name, partition_name)): Path<(String, String)>,
) -> ApiResult<Response> {
    tokio::task::spawn_blocking(move || do_unmount_partition(&disk_name, &partition_name))
        .await
        .map_err(|err| crate::types::ApiError::Internal(err.to_string()))?
        .map_err(|err| crate::types::ApiError::BadRequest(err.to_string()))?;
    Ok(ApiResponse::ok("Partition unmounted").into_response())
}

/// 格式化分区。
pub async fn format_partition(
    Path((disk_name, partition_name)): Path<(String, String)>,
    payload: Option<Json<FormatPartitionPayload>>,
) -> ApiResult<Response> {
    let filesystem = payload
        .and_then(|value| value.0.filesystem)
        .unwrap_or_else(|| "ext4".to_string());
    tokio::task::spawn_blocking(move || {
        do_format_partition(&disk_name, &partition_name, &filesystem)
    })
    .await
    .map_err(|err| crate::types::ApiError::Internal(err.to_string()))?
    .map_err(|err| crate::types::ApiError::BadRequest(err.to_string()))?;
    Ok(ApiResponse::ok("Partition formatted").into_response())
}

/// 初始化磁盘分区并格式化挂载。
pub async fn initialize_disk(
    Path(disk_name): Path<String>,
    payload: Option<Json<InitializeDiskPayload>>,
) -> ApiResult<Response> {
    let payload = payload.map(|value| value.0);
    let filesystem = payload
        .as_ref()
        .and_then(|value| value.filesystem.clone())
        .unwrap_or_else(|| "ext4".to_string());
    let mountpoint = payload
        .and_then(|value| value.mountpoint)
        .filter(|value| !value.trim().is_empty());
    tokio::task::spawn_blocking(move || do_initialize_disk(&disk_name, &filesystem, mountpoint))
        .await
        .map_err(|err| crate::types::ApiError::Internal(err.to_string()))?
        .map_err(|err| crate::types::ApiError::BadRequest(err.to_string()))?;
    Ok(ApiResponse::ok("Disk initialized").into_response())
}

/// 返回系统监控历史记录，默认最近 7 天。
pub async fn history(
    State(state): State<Arc<AppState>>,
    payload: Option<Json<SystemHistoryQuery>>,
) -> ApiResult<Response> {
    let rows = system_metrics::fetch_history(&state, payload.and_then(|value| value.0.hours))
        .await
        .map_err(|err| crate::types::ApiError::Internal(err.to_string()))?;
    Ok(ApiResponse::success_with_raw("System metrics history loaded", Some(rows)).into_response())
}

/// 清空系统监控历史记录。
pub async fn clear_history(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    let affected = system_metrics::clear_history(&state)
        .await
        .map_err(|err| crate::types::ApiError::Internal(err.to_string()))?;
    let message = format!("Cleared {affected} system metrics records");
    Ok(ApiResponse::ok(&message).into_response())
}

/// 返回系统监控采集开关状态。
pub async fn collector_status(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    let enabled = state.system_metrics_enabled().await;
    let status = system_metrics::CollectorStatus { enabled };
    Ok(
        ApiResponse::success_with_raw("System metrics collector status loaded", Some(status))
            .into_response(),
    )
}

/// 更新系统监控采集开关状态。
pub async fn set_collector_status(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CollectorStatusPayload>,
) -> ApiResult<Response> {
    system_metrics::set_collector_enabled(&state, payload.enabled)
        .await
        .map_err(|err| crate::types::ApiError::Internal(err.to_string()))?;
    let status = system_metrics::CollectorStatus {
        enabled: payload.enabled,
    };
    Ok(
        ApiResponse::success_with_raw("System metrics collector status updated", Some(status))
            .into_response(),
    )
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallStatusInfo {
    pub installed: bool,
    pub firewall_type: Option<String>,
    pub version: Option<String>,
    pub active: bool,
    pub service_status: String,
    pub platform: String,
    pub primary_backend: Option<String>,
    pub detected_backends: Vec<FirewallBackendStatus>,
    pub rules: Vec<FirewallRuleSummary>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallBackendStatus {
    pub backend: String,
    pub installed: bool,
    pub active: bool,
    pub version: Option<String>,
    pub service_status: String,
    pub command_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallRuleSummary {
    pub backend: String,
    pub scope: String,
    pub action: String,
    pub protocol: Option<String>,
    pub port: Option<String>,
    pub source: Option<String>,
    pub raw: String,
}

fn command_path(command: &str, fallback_paths: &[&str]) -> Option<String> {
    if let Ok(output) = Command::new("sh")
        .args(["-c", &format!("command -v {command}")])
        .output()
        && output.status.success()
    {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(path);
        }
    }

    fallback_paths
        .iter()
        .find(|path| FsPath::new(path).exists())
        .map(|path| (*path).to_string())
}

fn systemd_service_status(service: &str) -> (bool, String) {
    if let Ok(output) = Command::new("systemctl")
        .arg("is-active")
        .arg(service)
        .output()
    {
        let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !status.is_empty() {
            return (status == "active", status);
        }
    }

    (false, "unknown".to_string())
}

fn first_stdout_line(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn collect_command_output(
    command: &str,
    args: &[&str],
    warnings: &mut Vec<String>,
) -> Option<String> {
    match Command::new(command).args(args).output() {
        Ok(output) if output.status.success() => {
            Some(String::from_utf8_lossy(&output.stdout).to_string())
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            warnings.push(format!(
                "{} {} failed: {}",
                command,
                args.join(" "),
                if stderr.is_empty() {
                    "non-zero exit status".to_string()
                } else {
                    stderr
                }
            ));
            None
        }
        Err(err) => {
            warnings.push(format!("{} {} failed: {}", command, args.join(" "), err));
            None
        }
    }
}

fn parse_ufw_rules(output: &str) -> Vec<FirewallRuleSummary> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with("Status:")
                && !line.starts_with("To ")
                && !line.starts_with("--")
        })
        .filter_map(|line| {
            let parts = line.split_whitespace().collect::<Vec<_>>();
            if parts.len() < 2 {
                return None;
            }

            let to_index = parts
                .iter()
                .position(|part| *part == "ALLOW" || *part == "DENY" || *part == "REJECT")?;
            let target = parts[..to_index].join(" ");
            let action = parts[to_index].to_string();
            let source = parts
                .iter()
                .position(|part| *part == "From")
                .and_then(|idx| parts.get(idx + 1..))
                .or_else(|| parts.get(to_index + 1..))
                .map(|rest| rest.join(" "));

            let (port, protocol) = split_port_protocol(&target);
            Some(FirewallRuleSummary {
                backend: "ufw".to_string(),
                scope: "status".to_string(),
                action,
                protocol,
                port,
                source,
                raw: line.to_string(),
            })
        })
        .collect()
}

fn parse_firewalld_rules(output: &str) -> Vec<FirewallRuleSummary> {
    let mut zone = "default".to_string();
    let mut rules = Vec::new();

    for line in output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(name) = line.strip_suffix(" (active)") {
            zone = name.to_string();
        } else if let Some(ports) = line.strip_prefix("ports:") {
            for port in ports.split_whitespace() {
                let (port, protocol) = split_port_protocol(port);
                rules.push(FirewallRuleSummary {
                    backend: "firewalld".to_string(),
                    scope: zone.clone(),
                    action: "allow".to_string(),
                    protocol,
                    port,
                    source: None,
                    raw: line.to_string(),
                });
            }
        } else if let Some(services) = line.strip_prefix("services:") {
            for service in services.split_whitespace() {
                rules.push(FirewallRuleSummary {
                    backend: "firewalld".to_string(),
                    scope: zone.clone(),
                    action: "service".to_string(),
                    protocol: None,
                    port: Some(service.to_string()),
                    source: None,
                    raw: line.to_string(),
                });
            }
        }
    }

    rules
}

fn parse_nftables_rules(output: &str) -> Vec<FirewallRuleSummary> {
    let mut table = "ruleset".to_string();
    let mut chain = "unknown".to_string();
    let mut rules = Vec::new();

    for line in output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(rest) = line.strip_prefix("table ") {
            table = rest.trim_end_matches('{').trim().to_string();
        } else if let Some(rest) = line.strip_prefix("chain ") {
            chain = rest
                .split_whitespace()
                .next()
                .unwrap_or("unknown")
                .to_string();
        } else if line.starts_with("type ") {
            continue;
        } else if line.contains(" accept")
            || line.ends_with("accept")
            || line.contains(" drop")
            || line.ends_with("drop")
        {
            let action = if line.contains("drop") {
                "drop"
            } else {
                "accept"
            };
            let protocol = if line.contains(" tcp ") || line.starts_with("tcp ") {
                Some("tcp".to_string())
            } else if line.contains(" udp ") || line.starts_with("udp ") {
                Some("udp".to_string())
            } else {
                None
            };
            let port = extract_after_token(line, "dport");
            rules.push(FirewallRuleSummary {
                backend: "nftables".to_string(),
                scope: format!("{table}/{chain}"),
                action: action.to_string(),
                protocol,
                port,
                source: extract_after_token(line, "saddr"),
                raw: line.to_string(),
            });
        }
    }

    rules
}

fn parse_iptables_rules(output: &str) -> Vec<FirewallRuleSummary> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("-A "))
        .map(|line| {
            let parts = line.split_whitespace().collect::<Vec<_>>();
            let scope = parts.get(1).unwrap_or(&"unknown").to_string();
            let action = token_value(&parts, "-j").unwrap_or_else(|| "rule".to_string());
            FirewallRuleSummary {
                backend: "iptables".to_string(),
                scope,
                action,
                protocol: token_value(&parts, "-p"),
                port: token_value(&parts, "--dport"),
                source: token_value(&parts, "-s"),
                raw: line.to_string(),
            }
        })
        .collect()
}

fn split_port_protocol(value: &str) -> (Option<String>, Option<String>) {
    if let Some((port, protocol)) = value.split_once('/') {
        (Some(port.to_string()), Some(protocol.to_string()))
    } else if value.trim().is_empty() {
        (None, None)
    } else {
        (Some(value.to_string()), None)
    }
}

fn token_value(parts: &[&str], token: &str) -> Option<String> {
    parts
        .iter()
        .position(|part| *part == token)
        .and_then(|index| parts.get(index + 1))
        .map(|value| (*value).to_string())
}

fn extract_after_token(line: &str, token: &str) -> Option<String> {
    let parts = line.split_whitespace().collect::<Vec<_>>();
    parts
        .iter()
        .position(|part| *part == token)
        .and_then(|index| parts.get(index + 1))
        .map(|value| (*value).to_string())
}

/// 探测主机防火墙安装状态、版本、活跃度与规则摘要。
pub async fn detect_firewall() -> ApiResult<Response> {
    let mut info = FirewallStatusInfo {
        installed: false,
        firewall_type: None,
        version: None,
        active: false,
        service_status: "not_installed".to_string(),
        platform: "linux".to_string(),
        primary_backend: None,
        detected_backends: Vec::new(),
        rules: Vec::new(),
        warnings: Vec::new(),
    };

    let ufw_path = command_path("ufw", &["/usr/sbin/ufw"]);
    if let Some(path) = ufw_path {
        let (active, service_status) = systemd_service_status("ufw");
        let version = first_stdout_line("ufw", &["--version"]);
        if let Some(output) = collect_command_output("ufw", &["status"], &mut info.warnings) {
            info.rules.extend(parse_ufw_rules(&output));
        }
        info.detected_backends.push(FirewallBackendStatus {
            backend: "ufw".to_string(),
            installed: true,
            active,
            version,
            service_status,
            command_path: Some(path),
        });
    }

    let firewalld_path = command_path(
        "firewall-cmd",
        &["/usr/bin/firewall-cmd", "/usr/sbin/firewall-cmd"],
    );
    if let Some(path) = firewalld_path {
        let (active, service_status) = systemd_service_status("firewalld");
        let version = first_stdout_line("firewall-cmd", &["--version"])
            .map(|version| format!("firewalld {version}"));
        if let Some(output) =
            collect_command_output("firewall-cmd", &["--list-all"], &mut info.warnings)
        {
            info.rules.extend(parse_firewalld_rules(&output));
        }
        info.detected_backends.push(FirewallBackendStatus {
            backend: "firewalld".to_string(),
            installed: true,
            active,
            version,
            service_status,
            command_path: Some(path),
        });
    }

    let nft_path = command_path("nft", &["/usr/sbin/nft", "/usr/bin/nft"]);
    if let Some(path) = nft_path {
        let version = first_stdout_line("nft", &["--version"]);
        if let Some(output) =
            collect_command_output("nft", &["list", "ruleset"], &mut info.warnings)
        {
            info.rules.extend(parse_nftables_rules(&output));
        }
        info.detected_backends.push(FirewallBackendStatus {
            backend: "nftables".to_string(),
            installed: true,
            active: true,
            version,
            service_status: "kernel_ruleset".to_string(),
            command_path: Some(path),
        });
    }

    let iptables_path = command_path("iptables", &["/usr/sbin/iptables", "/usr/bin/iptables"]);
    if let Some(path) = iptables_path {
        let version = first_stdout_line("iptables", &["--version"]);
        if let Some(output) = collect_command_output("iptables", &["-S"], &mut info.warnings) {
            info.rules.extend(parse_iptables_rules(&output));
        }
        info.detected_backends.push(FirewallBackendStatus {
            backend: "iptables".to_string(),
            installed: true,
            active: true,
            version,
            service_status: "available".to_string(),
            command_path: Some(path),
        });
    }

    if let Some(primary) = ["ufw", "firewalld", "nftables", "iptables"]
        .iter()
        .find_map(|backend| {
            info.detected_backends
                .iter()
                .find(|item| item.backend == *backend)
        })
    {
        info.installed = true;
        info.firewall_type = Some(primary.backend.clone());
        info.primary_backend = Some(primary.backend.clone());
        info.version = primary.version.clone();
        info.active = primary.active;
        info.service_status = primary.service_status.clone();
    }

    if info.detected_backends.len() > 1 {
        info.warnings
            .push("multiple_firewall_backends_detected".to_string());
    }

    Ok(
        ApiResponse::success_with_raw("Firewall status detected successfully", Some(info))
            .into_response(),
    )
}

/// 返回系统监控告警阈值配置。
pub async fn alert_thresholds(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    let thresholds = system_metrics::fetch_alert_thresholds(&state.metadata_db)
        .await
        .map_err(|err| crate::types::ApiError::Internal(err.to_string()))?;
    Ok(ApiResponse::success_with_raw("Alert thresholds loaded", Some(thresholds)).into_response())
}

/// 更新系统监控告警阈值配置。
pub async fn set_alert_thresholds_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<system_metrics::AlertThresholds>,
) -> ApiResult<Response> {
    system_metrics::set_alert_thresholds(&state.metadata_db, &payload)
        .await
        .map_err(|err| crate::types::ApiError::Internal(err.to_string()))?;
    let thresholds = system_metrics::fetch_alert_thresholds(&state.metadata_db)
        .await
        .map_err(|err| crate::types::ApiError::Internal(err.to_string()))?;
    Ok(ApiResponse::success_with_raw("Alert thresholds updated", Some(thresholds)).into_response())
}

/// 构建系统信息相关路由集合。
pub fn system_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/about", get(about))
        .route("/summary", get(summary))
        .route("/disks", get(disks))
        .route("/firewall/detect", get(detect_firewall))
        .route(
            "/disks/{disk}/partitions/{partition}/mount",
            post(mount_partition),
        )
        .route(
            "/disks/{disk}/partitions/{partition}/unmount",
            post(unmount_partition),
        )
        .route(
            "/disks/{disk}/partitions/{partition}/format",
            post(format_partition),
        )
        .route("/disks/{disk}/initialize", post(initialize_disk))
        .route("/history", post(history))
        .route("/history", delete(clear_history))
        .route("/collector/status", get(collector_status))
        .route("/collector/status", put(set_collector_status))
        .route("/alert/thresholds", get(alert_thresholds))
        .route("/alert/thresholds", put(set_alert_thresholds_handler))
        .route("/seclab-url", put(set_seclab_url))
}

fn collect_about_info() -> SystemAboutInfo {
    let mut system = System::new_all();
    system.refresh_cpu_usage();
    system.refresh_memory();

    let cpu_info = read_cpu_info();
    let (primary_interface, default_gateway) = read_default_route();
    let (ipv4_addresses, ipv6_addresses) = read_ip_addresses(&primary_interface);
    let mac_address = read_mac_address(&primary_interface).unwrap_or_else(|| "--".to_string());
    let dns_servers = read_dns_servers();
    let virtualization = detect_virtualization();

    let os_name = System::name().unwrap_or_else(|| "--".to_string());
    let os_version = read_os_pretty_name()
        .or_else(System::long_os_version)
        .unwrap_or_else(|| "--".to_string());
    let kernel_version = System::kernel_version().unwrap_or_else(|| "--".to_string());
    let hostname = System::host_name().unwrap_or_else(|| "--".to_string());
    let architecture = read_uname_m().unwrap_or_else(|| std::env::consts::ARCH.to_string());

    let cpus = system.cpus();
    let cpu_model = cpus
        .first()
        .map(|cpu| cpu.brand().trim())
        .filter(|value| !value.is_empty())
        .or(cpu_info.model_name.as_deref())
        .unwrap_or("--")
        .to_string();
    let cpu_vendor = cpus
        .first()
        .map(|cpu| cpu.vendor_id().trim())
        .filter(|value| !value.is_empty())
        .or(cpu_info.vendor_id.as_deref())
        .unwrap_or("--")
        .to_string();
    let cpu_frequency_mhz = cpus.first().map(|cpu| cpu.frequency()).unwrap_or(0);
    let cpu_physical_cores = system.physical_core_count().unwrap_or(0);
    let timezone = read_timezone().unwrap_or_else(|| "--".to_string());
    let locale = read_locale().unwrap_or_else(|| "--".to_string());
    let boot_time_epoch = System::boot_time();

    SystemAboutInfo {
        hostname,
        os_name,
        os_version,
        kernel_version,
        architecture,
        cpu_model,
        cpu_vendor,
        cpu_physical_cores,
        cpu_logical_cores: cpus.len(),
        cpu_frequency_mhz,
        cpu_cache_l3: cpu_info.cache_l3.unwrap_or_else(|| "--".to_string()),
        memory_total_bytes: system.total_memory(),
        memory_available_bytes: system.available_memory(),
        swap_total_bytes: system.total_swap(),
        swap_used_bytes: system.used_swap(),
        uptime_seconds: System::uptime(),
        boot_time_epoch,
        timezone,
        locale,
        virtualization,
        primary_interface: if primary_interface.is_empty() {
            "--".to_string()
        } else {
            primary_interface.clone()
        },
        mac_address,
        ipv4_addresses,
        ipv6_addresses,
        default_gateway: default_gateway.unwrap_or_else(|| "--".to_string()),
        dns_servers,
        disk_total_bytes: collect_disk_total_bytes().unwrap_or(0),
        seclab_url: crate::config::get().seclab_url.clone(),
    }
}

fn read_os_pretty_name() -> Option<String> {
    let content = fs::read_to_string("/etc/os-release").ok()?;
    for line in content.lines() {
        if let Some(value) = line.strip_prefix("PRETTY_NAME=") {
            let cleaned = value.trim().trim_matches('"').trim();
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }
    None
}

fn read_uname_m() -> Option<String> {
    let output = Command::new("uname").arg("-m").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

#[derive(Default)]
struct CpuInfo {
    model_name: Option<String>,
    vendor_id: Option<String>,
    cache_l3: Option<String>,
}

fn read_cpu_info() -> CpuInfo {
    let mut info = CpuInfo::default();
    let Ok(content) = fs::read_to_string("/proc/cpuinfo") else {
        return info;
    };
    for line in content.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let key = k.trim();
        let value = v.trim();
        if value.is_empty() {
            continue;
        }
        match key {
            "model name" if info.model_name.is_none() => info.model_name = Some(value.to_string()),
            "vendor_id" if info.vendor_id.is_none() => info.vendor_id = Some(value.to_string()),
            "cache size" if info.cache_l3.is_none() => info.cache_l3 = Some(value.to_string()),
            _ => {}
        }
        if info.model_name.is_some() && info.vendor_id.is_some() && info.cache_l3.is_some() {
            break;
        }
    }
    info
}

fn read_default_route() -> (String, Option<String>) {
    let Ok(content) = fs::read_to_string("/proc/net/route") else {
        return (String::new(), None);
    };
    for line in content.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 3 || cols[1] != "00000000" {
            continue;
        }
        let iface = cols[0].to_string();
        let gateway = parse_route_gateway(cols[2]);
        return (iface, gateway);
    }
    (String::new(), None)
}

fn parse_route_gateway(hex_value: &str) -> Option<String> {
    let value = u32::from_str_radix(hex_value, 16).ok()?;
    let bytes = value.to_le_bytes();
    Some(Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]).to_string())
}

fn read_ip_addresses(interface: &str) -> (Vec<String>, Vec<String>) {
    if interface.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let output = Command::new("ip")
        .args(["-o", "addr", "show", "dev", interface])
        .output();
    let Ok(output) = output else {
        return (Vec::new(), Vec::new());
    };
    if !output.status.success() {
        return (Vec::new(), Vec::new());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();
    for line in stdout.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 4 {
            continue;
        }
        if cols[2] == "inet" {
            if let Some(addr) = cols[3].split('/').next()
                && !addr.is_empty()
            {
                v4.push(addr.to_string());
            }
        } else if cols[2] == "inet6"
            && let Some(addr) = cols[3].split('/').next()
            && !addr.is_empty()
        {
            v6.push(addr.to_string());
        }
    }
    (v4, v6)
}

fn read_mac_address(interface: &str) -> Option<String> {
    if interface.is_empty() {
        return None;
    }
    let path = format!("/sys/class/net/{interface}/address");
    let content = fs::read_to_string(path).ok()?;
    let value = content.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn read_dns_servers() -> Vec<String> {
    let Ok(content) = fs::read_to_string("/etc/resolv.conf") else {
        return Vec::new();
    };
    let mut values = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        let Some(value) = trimmed.strip_prefix("nameserver") else {
            continue;
        };
        let dns = value.trim();
        if !dns.is_empty() {
            values.push(dns.to_string());
        }
    }
    values
}

fn read_timezone() -> Option<String> {
    if let Ok(content) = fs::read_to_string("/etc/timezone") {
        let value = content.trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    let localtime = fs::read_link("/etc/localtime").ok()?;
    let text = localtime.to_string_lossy();
    text.split("zoneinfo/")
        .nth(1)
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
}

fn read_locale() -> Option<String> {
    if let Ok(value) = std::env::var("LANG") {
        let lang = value.trim();
        if !lang.is_empty() {
            return Some(lang.to_string());
        }
    }
    let content = fs::read_to_string("/etc/locale.conf").ok()?;
    for line in content.lines() {
        if let Some(value) = line.strip_prefix("LANG=") {
            let lang = value.trim().trim_matches('"').trim();
            if !lang.is_empty() {
                return Some(lang.to_string());
            }
        }
    }
    None
}

fn detect_virtualization() -> String {
    if let Some(value) = detect_virtualization_via_systemd() {
        return value;
    }
    if FsPath::new("/.dockerenv").exists() || FsPath::new("/run/.containerenv").exists() {
        return "Container".to_string();
    }
    if let Ok(content) = fs::read_to_string("/proc/1/cgroup") {
        let lower = content.to_lowercase();
        if lower.contains("kubepods")
            || lower.contains("docker")
            || lower.contains("containerd")
            || lower.contains("lxc")
        {
            return "Container".to_string();
        }
    }
    if let Some(value) = detect_virtualization_via_dmi() {
        return value;
    }
    "--".to_string()
}

fn detect_virtualization_via_systemd() -> Option<String> {
    let output = Command::new("systemd-detect-virt").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_lowercase();
    if raw.is_empty() || raw == "none" {
        return None;
    }
    Some(match raw.as_str() {
        "kvm" | "qemu" => "KVM/QEMU".to_string(),
        "vmware" => "VMware".to_string(),
        "oracle" | "virtualbox" => "VirtualBox".to_string(),
        "microsoft" | "hyperv" => "Hyper-V".to_string(),
        "xen" => "Xen".to_string(),
        "lxc" | "lxc-libvirt" => "LXC".to_string(),
        "docker" | "podman" | "containerd" | "openvz" => "Container".to_string(),
        other => other.to_string(),
    })
}

fn detect_virtualization_via_dmi() -> Option<String> {
    let product_name = read_trimmed_file("/sys/class/dmi/id/product_name");
    let sys_vendor = read_trimmed_file("/sys/class/dmi/id/sys_vendor");
    let bios_vendor = read_trimmed_file("/sys/class/dmi/id/bios_vendor");
    let board_vendor = read_trimmed_file("/sys/class/dmi/id/board_vendor");

    let mut signals = String::new();
    for value in [&product_name, &sys_vendor, &bios_vendor, &board_vendor]
        .into_iter()
        .flatten()
    {
        signals.push_str(value);
        signals.push(' ');
    }
    let lower = signals.to_lowercase();
    if lower.is_empty() {
        return None;
    }
    if lower.contains("proxmox") || lower.contains("pve") {
        return Some("KVM/QEMU (PVE)".to_string());
    }
    if lower.contains("qemu") || lower.contains("kvm") {
        return Some("KVM/QEMU".to_string());
    }
    if lower.contains("vmware") {
        return Some("VMware".to_string());
    }
    if lower.contains("virtualbox") || lower.contains("oracle") {
        return Some("VirtualBox".to_string());
    }
    if lower.contains("hyper-v") || lower.contains("microsoft") {
        return Some("Hyper-V".to_string());
    }
    if lower.contains("xen") {
        return Some("Xen".to_string());
    }
    if lower.contains("hvm domu") || lower.contains("openstack") {
        return Some("KVM".to_string());
    }
    product_name.filter(|value| !value.is_empty())
}

fn read_trimmed_file(path: &str) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let value = content.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn collect_disk_total_bytes() -> anyhow::Result<u64> {
    let parsed = load_lsblk()?;
    let mut total = 0_u64;
    for device in parsed.blockdevices {
        if device.device_type == "disk" {
            total = total.saturating_add(parse_size_bytes(device.size.as_ref()));
        }
    }
    Ok(total)
}

fn collect_disks() -> anyhow::Result<Vec<DiskInfo>> {
    let output = Command::new("lsblk")
        .args([
            "--bytes",
            "--json",
            "--output",
            "NAME,SIZE,TYPE,MODEL,SERIAL,PTTYPE,FSTYPE,MOUNTPOINT,PKNAME,ROTA,TRAN",
        ])
        .output()?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "lsblk failed with status: {}",
            output.status
        ));
    }

    let parsed: LsblkResult = serde_json::from_slice(&output.stdout)?;
    let mut by_name = HashMap::<String, LsblkDevice>::new();
    flatten_devices(&parsed.blockdevices, &mut by_name);
    let system_disk_names = resolve_system_disk_names(&by_name);
    let read_only = is_wsl_runtime();

    let mut rows = Vec::new();
    for device in &parsed.blockdevices {
        collect_disk_rows(device, &system_disk_names, read_only, &mut rows);
    }
    Ok(rows)
}

fn flatten_devices(devices: &[LsblkDevice], by_name: &mut HashMap<String, LsblkDevice>) {
    for device in devices {
        by_name.insert(device.name.clone(), device.clone());
        if !device.children.is_empty() {
            flatten_devices(&device.children, by_name);
        }
    }
}

fn collect_disk_rows(
    device: &LsblkDevice,
    system_disk_names: &[String],
    read_only: bool,
    rows: &mut Vec<DiskInfo>,
) {
    if device.device_type == "disk" {
        let transport = device
            .tran
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_uppercase());
        let disk_type = transport.unwrap_or_else(|| {
            if device.rota.unwrap_or(false) {
                "HDD".to_string()
            } else {
                "SSD".to_string()
            }
        });
        let partition_count = device
            .children
            .iter()
            .filter(|child| child.device_type == "part")
            .count();
        rows.push(DiskInfo {
            name: device.name.clone(),
            size_bytes: parse_size_bytes(device.size.as_ref()),
            partition_count,
            disk_type,
            model: normalize_optional_field(device.model.as_deref()),
            serial: normalize_optional_field(device.serial.as_deref()),
            partition_table: normalize_optional_field(device.pttype.as_deref()),
            is_system_disk: system_disk_names.iter().any(|item| item == &device.name),
            read_only,
            partitions: collect_partitions(device),
        });
    }
    for child in &device.children {
        collect_disk_rows(child, system_disk_names, read_only, rows);
    }
}

/// 判断当前进程是否运行在 Windows Subsystem for Linux 环境中。
fn is_wsl_runtime() -> bool {
    if std::env::var_os("WSL_INTEROP").is_some() || std::env::var_os("WSL_DISTRO_NAME").is_some() {
        return true;
    }

    ["/proc/sys/kernel/osrelease", "/proc/version"]
        .iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .any(|value| is_wsl_kernel_text(&value))
}

/// 根据内核版本文本识别 WSL 特征。
fn is_wsl_kernel_text(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    normalized.contains("microsoft") || normalized.contains("-wsl")
}

/// 阻止 WSL 环境执行会改变块设备状态的操作。
fn guard_disk_management_runtime() -> anyhow::Result<()> {
    if is_wsl_runtime() {
        return Err(anyhow::anyhow!(
            "disk management is read-only in WSL environments"
        ));
    }
    Ok(())
}

fn resolve_system_disk_names(by_name: &HashMap<String, LsblkDevice>) -> Vec<String> {
    let mut names = Vec::new();
    for device in by_name.values() {
        let runtime = resolve_partition_runtime(device);
        if runtime
            .mountpoints
            .iter()
            .any(|mountpoint| is_system_mountpoint(mountpoint))
            && let Some(name) = resolve_parent_disk_name(device, by_name)
            && !names.iter().any(|item| item == &name)
        {
            names.push(name);
        }
    }
    names
}

fn collect_partitions(device: &LsblkDevice) -> Vec<DiskPartitionInfo> {
    let mut rows = Vec::new();
    for child in &device.children {
        collect_partition_rows(child, &mut rows);
    }
    rows
}

fn collect_partition_rows(device: &LsblkDevice, rows: &mut Vec<DiskPartitionInfo>) {
    if device.device_type == "part" {
        let runtime = resolve_partition_runtime(device);
        let usage = runtime
            .primary_mount
            .as_deref()
            .and_then(fetch_partition_usage);
        rows.push(DiskPartitionInfo {
            name: device.name.clone(),
            size_bytes: parse_size_bytes(device.size.as_ref()),
            used_bytes: usage.map(|value| value.used_bytes),
            available_bytes: usage.map(|value| value.available_bytes),
            usage_percent: usage.map(|value| value.usage_percent),
            mountpoint: if runtime.mountpoints.is_empty() {
                normalize_optional_field(device.mountpoint.as_deref())
            } else {
                runtime.mountpoints.join(", ")
            },
            filesystem: if runtime.filesystems.is_empty() {
                normalize_optional_field(device.fstype.as_deref())
            } else {
                runtime.filesystems.join(", ")
            },
        });
    }
    for child in &device.children {
        collect_partition_rows(child, rows);
    }
}

#[derive(Debug, Clone)]
struct PartitionRuntime {
    mountpoints: Vec<String>,
    filesystems: Vec<String>,
    primary_mount: Option<String>,
}

fn resolve_partition_runtime(device: &LsblkDevice) -> PartitionRuntime {
    let mut mountpoints = Vec::<String>::new();
    let mut filesystems = Vec::<String>::new();
    collect_runtime_from_tree(device, &mut mountpoints, &mut filesystems);
    dedup_keep_order(&mut mountpoints);
    dedup_keep_order(&mut filesystems);
    let primary_mount = mountpoints
        .iter()
        .find(|mount| !is_special_mountpoint(mount))
        .cloned();
    PartitionRuntime {
        mountpoints,
        filesystems,
        primary_mount,
    }
}

fn collect_runtime_from_tree(
    device: &LsblkDevice,
    mountpoints: &mut Vec<String>,
    filesystems: &mut Vec<String>,
) {
    if let Some(mountpoint) = device
        .mountpoint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        mountpoints.push(mountpoint.to_string());
    }
    if let Some(filesystem) = device
        .fstype
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        filesystems.push(filesystem.to_string());
    }
    for child in &device.children {
        collect_runtime_from_tree(child, mountpoints, filesystems);
    }
}

fn dedup_keep_order(values: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::<String>::new();
    values.retain(|value| seen.insert(value.clone()));
}

fn is_special_mountpoint(mountpoint: &str) -> bool {
    mountpoint.starts_with('[') && mountpoint.ends_with(']')
}

fn is_system_mountpoint(mountpoint: &str) -> bool {
    let normalized = normalize_path_text(mountpoint);
    SYSTEM_MOUNTPOINTS
        .iter()
        .any(|system_mount| normalized == *system_mount)
}

fn normalize_path_text(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed == "/" {
        return "/".to_string();
    }
    trimmed.trim_end_matches('/').to_string()
}

fn active_mountpoints(device: &LsblkDevice) -> Vec<String> {
    resolve_partition_runtime(device)
        .mountpoints
        .into_iter()
        .filter(|mountpoint| !is_special_mountpoint(mountpoint))
        .collect()
}

fn has_active_mountpoint(device: &LsblkDevice) -> bool {
    !active_mountpoints(device).is_empty()
}

fn filesystems(device: &LsblkDevice) -> Vec<String> {
    resolve_partition_runtime(device).filesystems
}

fn has_filesystem(device: &LsblkDevice) -> bool {
    !filesystems(device).is_empty()
}

fn has_system_mountpoint(device: &LsblkDevice) -> bool {
    active_mountpoints(device)
        .iter()
        .any(|mountpoint| is_system_mountpoint(mountpoint))
}

#[derive(Debug, Clone, Copy)]
struct PartitionUsage {
    used_bytes: u64,
    available_bytes: u64,
    usage_percent: f64,
}

fn fetch_partition_usage(mountpoint: &str) -> Option<PartitionUsage> {
    let output = Command::new("df")
        .args(["-B1", "--output=used,avail,pcent,target", mountpoint])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let line = stdout
        .lines()
        .skip(1)
        .find(|item| !item.trim().is_empty())?;
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() < 4 {
        return None;
    }
    let used_bytes = cols.first()?.parse::<u64>().ok()?;
    let available_bytes = cols.get(1)?.parse::<u64>().ok()?;
    let usage_percent = cols
        .get(2)?
        .trim()
        .trim_end_matches('%')
        .parse::<f64>()
        .ok()?;
    Some(PartitionUsage {
        used_bytes,
        available_bytes,
        usage_percent,
    })
}

fn resolve_parent_disk_name(
    device: &LsblkDevice,
    by_name: &HashMap<String, LsblkDevice>,
) -> Option<String> {
    if device.device_type == "disk" {
        return Some(device.name.clone());
    }
    if let Some(parent) = device.pkname.as_deref().and_then(|name| by_name.get(name)) {
        return resolve_parent_disk_name(parent, by_name);
    }
    guess_disk_name(&device.name)
}

fn guess_disk_name(name: &str) -> Option<String> {
    if (name.starts_with("nvme") || name.starts_with("mmcblk"))
        && let Some(pos) = name.rfind('p')
    {
        let suffix = &name[pos + 1..];
        if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
            return Some(name[..pos].to_string());
        }
    }
    let split_at = name
        .char_indices()
        .find_map(|(idx, ch)| ch.is_ascii_digit().then_some(idx))?;
    Some(name[..split_at].to_string())
}

fn normalize_optional_field(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("--")
        .to_string()
}

fn parse_size_bytes(value: Option<&LsblkSizeValue>) -> u64 {
    match value {
        Some(LsblkSizeValue::Number(size)) => *size,
        Some(LsblkSizeValue::Text(size)) => size.trim().parse::<u64>().unwrap_or(0),
        None => 0,
    }
}

fn do_mount_partition(
    disk_name: &str,
    partition_name: &str,
    mountpoint: Option<String>,
) -> anyhow::Result<()> {
    guard_disk_management_runtime()?;
    let context = resolve_partition_context(disk_name, partition_name)?;
    guard_partition_context(&context)?;
    if has_active_mountpoint(&context.partition) {
        return Err(anyhow::anyhow!("partition is already mounted"));
    }
    if !has_filesystem(&context.partition) {
        return Err(anyhow::anyhow!(
            "target partition has no filesystem; format it before mounting"
        ));
    }
    let target_mount = match mountpoint {
        Some(custom) => normalize_mountpoint(&custom)?,
        None => default_mountpoint(&context.partition_name),
    };
    std::fs::create_dir_all(&target_mount)?;
    run_command(
        "mount",
        &[context.partition_path.as_str(), target_mount.as_str()],
    )?;
    Ok(())
}

fn do_unmount_partition(disk_name: &str, partition_name: &str) -> anyhow::Result<()> {
    guard_disk_management_runtime()?;
    let context = resolve_partition_context(disk_name, partition_name)?;
    guard_partition_context(&context)?;
    let mounts = active_mountpoints(&context.partition);
    if mounts.is_empty() {
        return Err(anyhow::anyhow!("partition is not mounted"));
    }
    if mounts.len() > 1 {
        return Err(anyhow::anyhow!(
            "partition has multiple mountpoints; manual review required"
        ));
    }
    let current_mount = mounts[0].clone();
    if !is_allowed_managed_mountpoint(&current_mount) {
        return Err(anyhow::anyhow!(
            "only mountpoints under /mnt can be unmounted from disk manager"
        ));
    }
    run_command("umount", &[current_mount.as_str()])?;
    remove_empty_managed_mountpoint(&current_mount)?;
    Ok(())
}

fn do_format_partition(
    disk_name: &str,
    partition_name: &str,
    filesystem: &str,
) -> anyhow::Result<()> {
    guard_disk_management_runtime()?;
    let context = resolve_partition_context(disk_name, partition_name)?;
    guard_partition_context(&context)?;
    if has_active_mountpoint(&context.partition) {
        return Err(anyhow::anyhow!("unmount partition before formatting"));
    }
    if filesystem != "ext4" {
        return Err(anyhow::anyhow!("only ext4 formatting is supported"));
    }
    run_command("mkfs.ext4", &["-F", context.partition_path.as_str()])?;
    Ok(())
}

#[derive(Debug, Clone)]
struct PartitionContext {
    partition_name: String,
    partition_path: String,
    partition: LsblkDevice,
    is_system_disk: bool,
}

fn guard_partition_context(context: &PartitionContext) -> anyhow::Result<()> {
    if context.is_system_disk || has_system_mountpoint(&context.partition) {
        return Err(anyhow::anyhow!("system disk operation is forbidden"));
    }
    Ok(())
}

fn resolve_partition_context(
    disk_name: &str,
    partition_name: &str,
) -> anyhow::Result<PartitionContext> {
    let disk_name = sanitize_device_name(disk_name)?;
    let partition_name = sanitize_device_name(partition_name)?;
    let parsed = load_lsblk()?;
    let mut by_name = HashMap::<String, LsblkDevice>::new();
    flatten_devices(&parsed.blockdevices, &mut by_name);
    let system_disks = resolve_system_disk_names(&by_name);

    let disk = by_name
        .get(&disk_name)
        .ok_or_else(|| anyhow::anyhow!("disk not found: {}", disk_name))?;
    if disk.device_type != "disk" {
        return Err(anyhow::anyhow!(
            "target is not a disk device: {}",
            disk_name
        ));
    }
    let partition = by_name
        .get(&partition_name)
        .ok_or_else(|| anyhow::anyhow!("partition not found: {}", partition_name))?;
    if partition.device_type != "part" {
        return Err(anyhow::anyhow!(
            "target is not a partition device: {}",
            partition_name
        ));
    }
    let parent_disk = resolve_parent_disk_name(partition, &by_name).ok_or_else(|| {
        anyhow::anyhow!(
            "failed to resolve parent disk for partition: {}",
            partition_name
        )
    })?;
    if parent_disk != disk_name {
        return Err(anyhow::anyhow!("partition does not belong to target disk"));
    }
    let is_system_disk = system_disks.iter().any(|name| name == &disk_name);

    Ok(PartitionContext {
        partition_name: partition_name.clone(),
        partition_path: format!("/dev/{}", partition_name),
        partition: partition.clone(),
        is_system_disk,
    })
}

fn load_lsblk() -> anyhow::Result<LsblkResult> {
    let output = Command::new("lsblk")
        .args([
            "--bytes",
            "--json",
            "--output",
            "NAME,SIZE,TYPE,MODEL,SERIAL,PTTYPE,FSTYPE,MOUNTPOINT,PKNAME,ROTA,TRAN",
        ])
        .output()?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "lsblk failed with status: {}",
            output.status
        ));
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn sanitize_device_name(input: &str) -> anyhow::Result<String> {
    let value = input.trim().trim_start_matches("/dev/");
    if value.is_empty() {
        return Err(anyhow::anyhow!("device name must not be empty"));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(anyhow::anyhow!("invalid device name: {}", input));
    }
    Ok(value.to_string())
}

fn normalize_mountpoint(input: &str) -> anyhow::Result<String> {
    let value = normalize_path_text(input);
    if value.contains("/../") || value.ends_with("/..") || value == ".." || value.contains("/./") {
        return Err(anyhow::anyhow!("invalid mount directory"));
    }
    normalize_managed_mountpoint(&value)
}

fn default_mountpoint(partition_name: &str) -> String {
    format!("{DISK_MOUNT_ROOT}/{partition_name}")
}

fn is_allowed_managed_mountpoint(mountpoint: &str) -> bool {
    let normalized = normalize_path_text(mountpoint);
    normalized
        .strip_prefix(DISK_MOUNT_ROOT)
        .is_some_and(|suffix| suffix.starts_with('/') && suffix.len() > 1)
}

fn remove_empty_managed_mountpoint(mountpoint: &str) -> anyhow::Result<()> {
    if !is_allowed_managed_mountpoint(mountpoint) {
        return Ok(());
    }
    remove_empty_directory(FsPath::new(mountpoint), mountpoint)
}

fn remove_empty_directory(path: &FsPath, display_path: &str) -> anyhow::Result<()> {
    match std::fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::DirectoryNotEmpty => Ok(()),
        Err(err) => Err(anyhow::anyhow!(
            "partition unmounted but failed to remove empty mount directory {}: {}",
            display_path,
            err
        )),
    }
}

fn normalize_managed_mountpoint(input: &str) -> anyhow::Result<String> {
    let value = normalize_path_text(input);
    let relative = match value.strip_prefix(DISK_MOUNT_ROOT) {
        Some(rest) if rest.starts_with('/') => rest.trim_start_matches('/'),
        Some("") => "",
        _ => value.trim_start_matches('/'),
    };
    if relative.is_empty() {
        return Err(anyhow::anyhow!("mount directory must be below /mnt"));
    }
    if relative
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(anyhow::anyhow!("invalid mount directory"));
    }
    Ok(format!("{DISK_MOUNT_ROOT}/{relative}"))
}

fn run_command(cmd: &str, args: &[&str]) -> anyhow::Result<()> {
    let output = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|err| classify_spawn_error(cmd, err))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let reason = classify_command_failure(cmd, output.status.code(), &stderr);
    Err(anyhow::anyhow!(reason))
}

fn classify_spawn_error(cmd: &str, err: std::io::Error) -> anyhow::Error {
    if err.kind() == std::io::ErrorKind::NotFound {
        return anyhow::anyhow!(
            "command not found: {}; install the required system command and retry",
            cmd
        );
    }
    if err.kind() == std::io::ErrorKind::PermissionDenied {
        return anyhow::anyhow!(
            "permission denied: cannot execute {}; run with elevated privileges and retry",
            cmd
        );
    }
    anyhow::anyhow!("failed to start {}: {}", cmd, err)
}

fn classify_command_failure(cmd: &str, status_code: Option<i32>, stderr: &str) -> String {
    let text = stderr.to_lowercase();
    if text.contains("permission denied")
        || text.contains("operation not permitted")
        || text.contains("must be superuser")
        || text.contains("must be root")
    {
        return format!(
            "permission denied: failed to execute {}; run with elevated privileges and retry",
            cmd
        );
    }
    if text.contains("not found")
        || text.contains("no such file or directory")
        || text.contains("command not found")
    {
        return format!(
            "command not found: failed to execute {}; install the required system command and retry",
            cmd
        );
    }
    if text.contains("device or resource busy")
        || text.contains("target is busy")
        || text.contains("resource busy")
        || text.contains("busy")
    {
        return "device is busy; stop the owning process or unmount it before retrying".to_string();
    }
    if stderr.is_empty() {
        return format!("{} failed with exit code {:?}", cmd, status_code);
    }
    stderr.to_string()
}

fn do_initialize_disk(
    disk_name: &str,
    filesystem: &str,
    mountpoint: Option<String>,
) -> anyhow::Result<()> {
    guard_disk_management_runtime()?;
    if filesystem != "ext4" {
        return Err(anyhow::anyhow!("only ext4 initialization is supported"));
    }

    let disk_name = sanitize_device_name(disk_name)?;
    let disk_path = format!("/dev/{}", disk_name);

    let parsed = load_lsblk()?;
    let mut by_name = HashMap::<String, LsblkDevice>::new();
    flatten_devices(&parsed.blockdevices, &mut by_name);
    let system_disks = resolve_system_disk_names(&by_name);
    if system_disks.iter().any(|name| name == &disk_name) {
        return Err(anyhow::anyhow!("system disk operation is forbidden"));
    }

    let disk = by_name
        .get(&disk_name)
        .ok_or_else(|| anyhow::anyhow!("disk not found: {}", disk_name))?;
    if disk.device_type != "disk" {
        return Err(anyhow::anyhow!(
            "target is not a disk device: {}",
            disk_name
        ));
    }
    if has_system_mountpoint(disk) || has_active_mountpoint(disk) {
        return Err(anyhow::anyhow!(
            "target disk has active mountpoints; manual review required"
        ));
    }

    run_command("parted", &["-s", disk_path.as_str(), "mklabel", "gpt"])?;
    run_command(
        "parted",
        &["-s", disk_path.as_str(), "mkpart", "primary", "0%", "100%"],
    )?;
    let _ = Command::new("partprobe").arg(disk_path.as_str()).output();
    std::thread::sleep(std::time::Duration::from_millis(800));

    let refreshed = load_lsblk()?;
    let mut refreshed_map = HashMap::<String, LsblkDevice>::new();
    flatten_devices(&refreshed.blockdevices, &mut refreshed_map);
    let refreshed_disk = refreshed_map
        .get(&disk_name)
        .ok_or_else(|| anyhow::anyhow!("target disk not found after partition creation"))?;
    let refreshed_system_disks = resolve_system_disk_names(&refreshed_map);
    if refreshed_system_disks.iter().any(|name| name == &disk_name)
        || has_system_mountpoint(refreshed_disk)
        || has_active_mountpoint(refreshed_disk)
    {
        return Err(anyhow::anyhow!("system disk operation is forbidden"));
    }
    let partitions: Vec<LsblkDevice> = refreshed_disk
        .children
        .iter()
        .filter(|child| child.device_type == "part")
        .cloned()
        .collect();

    if partitions.len() != 1 {
        return Err(anyhow::anyhow!(
            "disk initialization requires exactly one target partition"
        ));
    }
    let target = partitions
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no initializable partition found"))?;

    if has_system_mountpoint(&target) {
        return Err(anyhow::anyhow!("system disk operation is forbidden"));
    }
    if has_active_mountpoint(&target) {
        return Err(anyhow::anyhow!(
            "target partition is already mounted; unmount it before initialization"
        ));
    }
    let partition_path = format!("/dev/{}", target.name);
    run_command("mkfs.ext4", &["-F", partition_path.as_str()])?;
    let target_mount = match mountpoint {
        Some(custom) => normalize_mountpoint(&custom)?,
        None => default_mountpoint(&target.name),
    };
    std::fs::create_dir_all(&target_mount)?;
    run_command("mount", &[partition_path.as_str(), target_mount.as_str()])?;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeclabUrlPayload {
    pub seclab_url: String,
}

pub async fn set_seclab_url(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SeclabUrlPayload>,
) -> ApiResult<impl IntoResponse> {
    let parsed = reqwest::Url::parse(&payload.seclab_url).map_err(|err| {
        crate::types::ApiError::BadRequest(format!("seclabUrl must be a valid HTTPS URL: {}", err))
    })?;

    if parsed.scheme() != "https" {
        return Err(crate::types::ApiError::BadRequest(
            "seclabUrl must use HTTPS".to_string(),
        ));
    }

    if parsed.host_str().is_none() {
        return Err(crate::types::ApiError::BadRequest(
            "seclabUrl must include a host".to_string(),
        ));
    }

    if (parsed.path() != "" && parsed.path() != "/")
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(crate::types::ApiError::BadRequest(
            "seclabUrl must not include path, query, or fragment".to_string(),
        ));
    }

    let trimmed_url = payload.seclab_url.trim_end_matches('/').to_string();

    // 如果传入的 seclab_url 与当前本地配置的 seclab_url 完全一致，直接跳过重启与更新
    let current_url = crate::config::get().seclab_url.as_deref().unwrap_or("");
    if current_url.trim_end_matches('/') == trimmed_url {
        tracing::info!(
            "seclabUrl is unchanged ({}). Skipping restart and configuration update.",
            trimmed_url
        );
        return Ok(ApiResponse::success_with_raw(
            "seclabUrl is unchanged, no restart needed",
            Some(()),
        ));
    }

    // 更新本地数据库
    sqlx::query("UPDATE agent_identity SET seclab_url = ?1")
        .bind(&trimmed_url)
        .execute(&state.metadata_db)
        .await
        .map_err(|err| {
            crate::types::ApiError::Internal(format!("Failed to update database: {}", err))
        })?;

    // 更新 agent.toml 配置文件
    update_toml_seclab_url(&trimmed_url).map_err(|err| {
        crate::types::ApiError::Internal(format!("Failed to update agent.toml: {}", err))
    })?;

    // 延迟退出，触发外部进程管理器重启进程
    tokio::spawn(async move {
        tracing::info!(
            "seclabUrl has been updated by control plane to {}. Restarting agent to apply changes...",
            trimmed_url
        );
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        std::process::exit(0);
    });

    Ok(ApiResponse::success_with_raw(
        "seclabUrl updated successfully, agent is restarting",
        Some(()),
    ))
}

fn update_toml_seclab_url(new_url: &str) -> anyhow::Result<()> {
    let path = crate::config::config_path();
    if !path.exists() {
        return Err(anyhow::anyhow!("agent.toml not found at {:?}", path));
    }
    let content = fs::read_to_string(&path)?;
    let mut value: toml::Value = toml::from_str(&content)?;
    if let Some(table) = value.as_table_mut() {
        table.insert(
            "seclabUrl".to_string(),
            toml::Value::String(new_url.to_string()),
        );
    } else {
        return Err(anyhow::anyhow!("agent.toml root is not a table"));
    }
    let new_content = toml::to_string(&value)?;
    fs::write(&path, new_content)?;
    Ok(())
}

#[cfg(test)]
mod firewall_tests {
    use super::*;

    #[test]
    fn parses_ufw_status_rules() {
        let output = r#"
Status: active

To                         Action      From
--                         ------      ----
22/tcp                     ALLOW       Anywhere
8080                       DENY        10.0.0.0/8
"#;

        let rules = parse_ufw_rules(output);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].backend, "ufw");
        assert_eq!(rules[0].action, "ALLOW");
        assert_eq!(rules[0].protocol.as_deref(), Some("tcp"));
        assert_eq!(rules[0].port.as_deref(), Some("22"));
        assert_eq!(rules[1].source.as_deref(), Some("10.0.0.0/8"));
    }

    #[test]
    fn parses_firewalld_zone_rules() {
        let output = r#"
public (active)
  services: ssh dhcpv6-client
  ports: 8080/tcp 5353/udp
"#;

        let rules = parse_firewalld_rules(output);
        assert_eq!(rules.len(), 4);
        assert_eq!(rules[0].scope, "public");
        assert_eq!(rules[0].action, "service");
        assert_eq!(rules[2].port.as_deref(), Some("8080"));
        assert_eq!(rules[2].protocol.as_deref(), Some("tcp"));
    }

    #[test]
    fn parses_nftables_ruleset_summary() {
        let output = r#"
table inet filter {
  chain input {
    type filter hook input priority filter; policy drop;
    tcp dport 22 accept
    ip saddr 10.0.0.0/8 udp dport 53 accept
  }
}
"#;

        let rules = parse_nftables_rules(output);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].backend, "nftables");
        assert_eq!(rules[0].scope, "inet filter/input");
        assert_eq!(rules[0].port.as_deref(), Some("22"));
        assert_eq!(rules[1].source.as_deref(), Some("10.0.0.0/8"));
    }

    #[test]
    fn parses_iptables_rules() {
        let output = r#"
-P INPUT ACCEPT
-A INPUT -p tcp -s 192.168.1.0/24 --dport 22 -j ACCEPT
-A INPUT -p udp --dport 53 -j DROP
"#;

        let rules = parse_iptables_rules(output);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].scope, "INPUT");
        assert_eq!(rules[0].action, "ACCEPT");
        assert_eq!(rules[0].protocol.as_deref(), Some("tcp"));
        assert_eq!(rules[0].port.as_deref(), Some("22"));
        assert_eq!(rules[0].source.as_deref(), Some("192.168.1.0/24"));
    }
}

#[cfg(test)]
mod disk_safety_tests {
    use super::*;

    fn test_device(
        name: &str,
        device_type: &str,
        mountpoint: Option<&str>,
        fstype: Option<&str>,
        pkname: Option<&str>,
        children: Vec<LsblkDevice>,
    ) -> LsblkDevice {
        LsblkDevice {
            name: name.to_string(),
            size: None,
            device_type: device_type.to_string(),
            model: None,
            serial: None,
            pttype: None,
            fstype: fstype.map(str::to_string),
            mountpoint: mountpoint.map(str::to_string),
            pkname: pkname.map(str::to_string),
            rota: None,
            tran: None,
            children,
        }
    }

    #[test]
    fn resolves_system_disk_from_critical_mountpoints() {
        let boot_partition = test_device(
            "sda1",
            "part",
            Some("/boot/efi"),
            Some("vfat"),
            Some("sda"),
            vec![],
        );
        let data_partition = test_device(
            "sdb1",
            "part",
            Some("/mnt/data/sdb1"),
            Some("ext4"),
            Some("sdb"),
            vec![],
        );
        let system_disk = test_device("sda", "disk", None, None, None, vec![boot_partition]);
        let data_disk = test_device("sdb", "disk", None, None, None, vec![data_partition]);

        let mut by_name = HashMap::<String, LsblkDevice>::new();
        flatten_devices(&[system_disk, data_disk], &mut by_name);

        let system_disks = resolve_system_disk_names(&by_name);
        assert_eq!(system_disks, vec!["sda".to_string()]);
    }

    #[test]
    fn detects_wsl_kernel_signatures() {
        assert!(is_wsl_kernel_text("5.15.167.4-microsoft-standard-WSL2"));
        assert!(is_wsl_kernel_text("6.6.87.2-microsoft-standard-WSL2"));
        assert!(!is_wsl_kernel_text("6.8.0-60-generic"));
    }

    #[test]
    fn normalizes_mountpoints_under_mnt_root() {
        assert_eq!(normalize_mountpoint("test1").unwrap(), "/mnt/test1");
        assert_eq!(normalize_mountpoint("/test1").unwrap(), "/mnt/test1");
        assert_eq!(normalize_mountpoint("/mnt/test1/").unwrap(), "/mnt/test1");
        assert_eq!(
            normalize_mountpoint("data/test1").unwrap(),
            "/mnt/data/test1"
        );
        assert!(normalize_mountpoint("/mnt/../root").is_err());
        assert!(normalize_mountpoint("data/./test1").is_err());
        assert!(normalize_mountpoint("/mnt").is_err());
    }

    #[test]
    fn remove_empty_managed_mountpoint_ignores_non_empty_directory() {
        let base = std::env::temp_dir().join(format!("seclab-disk-test-{}", std::process::id()));
        let mount_dir = base.join("mnt").join("data1");
        std::fs::create_dir_all(&mount_dir).unwrap();
        std::fs::write(mount_dir.join("keep.txt"), "data").unwrap();

        remove_empty_directory(&mount_dir, mount_dir.to_str().unwrap()).unwrap();
        assert!(mount_dir.exists());

        std::fs::remove_file(mount_dir.join("keep.txt")).unwrap();
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn detects_active_and_system_mountpoints_from_descendants() {
        let partition = test_device("vda1", "part", Some("/"), Some("ext4"), Some("vda"), vec![]);
        let disk = test_device("vda", "disk", None, None, None, vec![partition]);

        assert!(has_active_mountpoint(&disk));
        assert!(has_system_mountpoint(&disk));
    }

    #[test]
    fn rejects_system_partition_context() {
        let partition = test_device("vda1", "part", Some("/"), Some("ext4"), Some("vda"), vec![]);
        let context = PartitionContext {
            partition_name: "vda1".to_string(),
            partition_path: "/dev/vda1".to_string(),
            partition,
            is_system_disk: false,
        };

        assert!(guard_partition_context(&context).is_err());
    }
}
