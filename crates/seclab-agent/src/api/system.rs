//! 系统信息 API：提供运行环境与系统摘要信息。

use crate::services::system_metrics;
use crate::state::AppState;
use crate::types::{ApiResponse, ApiResult};
use axum::{
    Json, Router,
    extract::State,
    response::{IntoResponse, Response},
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use std::{fs, net::Ipv4Addr, path::Path as FsPath, process::Command, sync::Arc};
use sysinfo::System;

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

/// 构建系统信息相关路由集合。
pub fn system_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/about", get(about))
        .route("/summary", get(summary))
        .route("/firewall/detect", get(detect_firewall))
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
    let output = Command::new("lsblk")
        .args(["--bytes", "--json", "--output", "TYPE,SIZE"])
        .output()?;
    if !output.status.success() {
        return Err(anyhow::anyhow!("lsblk failed"));
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    Ok(value
        .get("blockdevices")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|device| device.get("type").and_then(serde_json::Value::as_str) == Some("disk"))
        .filter_map(|device| {
            let size = device.get("size")?;
            size.as_u64()
                .or_else(|| size.as_str().and_then(|value| value.parse().ok()))
        })
        .fold(0_u64, u64::saturating_add))
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
