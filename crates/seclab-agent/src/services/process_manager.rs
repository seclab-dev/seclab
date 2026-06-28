//! 进程管理服务：采集主机进程、网络连接并执行进程信号控制。

use seclab_contracts::process::{NetworkConnection, NetworkSummary, ProcessItem};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct SignalExecutionResult {
    pub pid: u32,
    pub signal: String,
    pub success: bool,
    pub process_existed: bool,
    pub message: String,
}

/// 采集当前主机进程快照。
pub fn collect_processes() -> Result<Vec<ProcessItem>, String> {
    let connection_counts = collect_socket_counts();
    let output = Command::new("ps")
        .args([
            "-eo",
            "pid=,ppid=,comm=,nlwp=,user=,pcpu=,pmem=,stat=,lstart=,args=",
        ])
        .output()
        .map_err(|err| format!("failed to invoke ps command: {}", err))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if stderr.is_empty() {
            "failed to collect process list".to_string()
        } else {
            stderr
        };
        return Err(message);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut rows = stdout
        .lines()
        .filter_map(|line| parse_ps_process_line(line, &connection_counts))
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        b.cpu_percent
            .partial_cmp(&a.cpu_percent)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.memory_percent
                    .partial_cmp(&a.memory_percent)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    Ok(rows)
}

fn parse_ps_process_line(
    line: &str,
    connection_counts: &HashMap<u32, usize>,
) -> Option<ProcessItem> {
    let parts = line.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 13 {
        return None;
    }

    let pid = parts.first()?.parse::<u32>().ok()?;
    let parent_pid = parts.get(1)?.parse::<u32>().ok()?;
    let name = parts.get(2)?.to_string();
    let thread_count = parts
        .get(3)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let user = parts.get(4)?.to_string();
    let cpu_percent = parts
        .get(5)
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0);
    let memory_percent = parts
        .get(6)
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0);
    let status = normalize_process_status(parts.get(7)?).to_string();
    let start_time = parse_ps_lstart(parts.get(8..13)?.join(" ").as_str());
    let command = if parts.len() > 13 {
        parts[13..].join(" ")
    } else {
        name.clone()
    };

    Some(ProcessItem {
        pid,
        name,
        parent_pid,
        thread_count,
        user,
        cpu_percent,
        memory_percent,
        connection_count: connection_counts.get(&pid).copied().unwrap_or(0),
        status,
        start_time,
        command,
    })
}

fn normalize_process_status(value: &str) -> &'static str {
    if value.contains('L') {
        return "locked";
    }

    match value.chars().next().unwrap_or('S') {
        'R' => "running",
        'S' => "sleeping",
        'T' | 't' => "stopped",
        'I' => "idle",
        'D' => "waiting",
        'Z' => "zombie",
        _ => "sleeping",
    }
}

fn parse_ps_lstart(value: &str) -> i64 {
    use chrono::TimeZone;

    let parsed = match chrono::NaiveDateTime::parse_from_str(value, "%a %b %e %H:%M:%S %Y") {
        Ok(value) => value,
        Err(_) => return 0,
    };
    chrono::Local
        .from_local_datetime(&parsed)
        .single()
        .or_else(|| chrono::Local.from_local_datetime(&parsed).earliest())
        .or_else(|| chrono::Local.from_local_datetime(&parsed).latest())
        .map(|value| value.timestamp())
        .unwrap_or(0)
}

fn collect_socket_counts() -> HashMap<u32, usize> {
    let mut counts = HashMap::new();
    let proc_entries = match fs::read_dir("/proc") {
        Ok(entries) => entries,
        Err(_) => return counts,
    };

    for entry in proc_entries.flatten() {
        let pid = match entry.file_name().to_string_lossy().parse::<u32>() {
            Ok(value) => value,
            Err(_) => continue,
        };
        let fd_dir = format!("/proc/{}/fd", pid);
        let fd_entries = match fs::read_dir(fd_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        let mut count = 0_usize;
        for fd in fd_entries.flatten() {
            let link = match fs::read_link(fd.path()) {
                Ok(value) => value,
                Err(_) => continue,
            };
            if extract_socket_inode(&link.to_string_lossy()).is_some() {
                count += 1;
            }
        }
        if count > 0 {
            counts.insert(pid, count);
        }
    }

    counts
}

/// 采集当前主机 TCP/UDP 网络连接快照。
pub fn collect_network_connections() -> Result<Vec<NetworkConnection>, String> {
    let owners = collect_socket_owners();
    let mut rows = Vec::new();
    rows.extend(parse_proc_net_file("tcp", "/proc/net/tcp", false, &owners)?);
    rows.extend(parse_proc_net_file(
        "tcp6",
        "/proc/net/tcp6",
        true,
        &owners,
    )?);
    rows.extend(parse_proc_net_file("udp", "/proc/net/udp", false, &owners)?);
    rows.extend(parse_proc_net_file(
        "udp6",
        "/proc/net/udp6",
        true,
        &owners,
    )?);
    rows.sort_by(|a, b| {
        a.protocol
            .cmp(&b.protocol)
            .then_with(|| a.local_address.cmp(&b.local_address))
    });
    Ok(rows)
}

/// 根据网络连接快照生成汇总数据。
pub fn summarize_network_connections(rows: &[NetworkConnection]) -> NetworkSummary {
    let mut by_state = BTreeMap::new();
    let mut by_protocol = BTreeMap::new();
    for row in rows {
        *by_state.entry(row.state.clone()).or_insert(0) += 1;
        *by_protocol.entry(row.protocol.clone()).or_insert(0) += 1;
    }
    NetworkSummary {
        total: rows.len(),
        by_state,
        by_protocol,
    }
}

/// 向进程发送 TERM/KILL 信号。
pub fn send_signal(pid: u32, signal: &str) -> Result<SignalExecutionResult, String> {
    if pid == 0 {
        return Err("invalid process PID".to_string());
    }
    let signal = normalize_signal(signal)?;
    run_kill_command(pid, signal)
}

fn normalize_signal(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_uppercase();
    match normalized.as_str() {
        "TERM" | "SIGTERM" => Ok("TERM".to_string()),
        "KILL" | "SIGKILL" => Ok("KILL".to_string()),
        _ => Err("only TERM or KILL signal is supported".to_string()),
    }
}

fn run_kill_command(pid: u32, signal: String) -> Result<SignalExecutionResult, String> {
    let output = std::process::Command::new("kill")
        .args(["-s", &signal, &pid.to_string()])
        .output()
        .map_err(|err| format!("failed to invoke kill command: {}", err))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let lowered = stderr.to_ascii_lowercase();
        if lowered.contains("no such process") {
            return Ok(SignalExecutionResult {
                pid,
                signal,
                success: false,
                process_existed: false,
                message: "process no longer exists".to_string(),
            });
        }
        let message = if stderr.is_empty() {
            "failed to terminate process".to_string()
        } else {
            stderr
        };
        return Ok(SignalExecutionResult {
            pid,
            signal,
            success: false,
            process_existed: true,
            message,
        });
    }
    Ok(SignalExecutionResult {
        pid,
        signal,
        success: true,
        process_existed: true,
        message: "signal sent".to_string(),
    })
}

fn collect_socket_owners() -> HashMap<u64, (u32, String)> {
    let mut owners = HashMap::new();
    let proc_entries = match fs::read_dir("/proc") {
        Ok(entries) => entries,
        Err(_) => return owners,
    };

    for entry in proc_entries.flatten() {
        let pid = match entry.file_name().to_string_lossy().parse::<u32>() {
            Ok(value) => value,
            Err(_) => continue,
        };
        let process_name = read_process_name(pid);
        let fd_dir = format!("/proc/{}/fd", pid);
        let fd_entries = match fs::read_dir(fd_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for fd in fd_entries.flatten() {
            let link = match fs::read_link(fd.path()) {
                Ok(value) => value,
                Err(_) => continue,
            };
            let target = link.to_string_lossy();
            if let Some(inode) = extract_socket_inode(&target) {
                owners
                    .entry(inode)
                    .or_insert_with(|| (pid, process_name.clone()));
            }
        }
    }
    owners
}

fn read_process_name(pid: u32) -> String {
    let path = format!("/proc/{}/comm", pid);
    match fs::read_to_string(path) {
        Ok(value) => value.trim().to_string(),
        Err(_) => pid.to_string(),
    }
}

fn extract_socket_inode(target: &str) -> Option<u64> {
    if !target.starts_with("socket:[") || !target.ends_with(']') {
        return None;
    }
    let inode = &target[8..target.len().saturating_sub(1)];
    inode.parse::<u64>().ok()
}

fn parse_proc_net_file(
    protocol: &str,
    path: &str,
    ipv6: bool,
    owners: &HashMap<u64, (u32, String)>,
) -> Result<Vec<NetworkConnection>, String> {
    let content =
        fs::read_to_string(path).map_err(|err| format!("failed to read {}: {}", path, err))?;
    let mut rows = Vec::new();

    for line in content.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 10 {
            continue;
        }

        let local_address = parse_endpoint(cols[1], ipv6).unwrap_or_else(|| cols[1].to_string());
        let remote_address = parse_endpoint(cols[2], ipv6).unwrap_or_else(|| cols[2].to_string());
        let state = if protocol.starts_with("tcp") {
            tcp_state(cols[3]).to_string()
        } else {
            "NONE".to_string()
        };
        let inode = cols[9].parse::<u64>().ok();
        let owner = inode.and_then(|value| owners.get(&value));

        rows.push(NetworkConnection {
            protocol: protocol.to_ascii_uppercase(),
            local_address,
            remote_address,
            state,
            pid: owner.map(|(pid, _)| *pid),
            process_name: owner.map(|(_, name)| name.clone()),
        });
    }

    Ok(rows)
}

fn parse_endpoint(value: &str, ipv6: bool) -> Option<String> {
    let (ip_hex, port_hex) = value.split_once(':')?;
    let port = u16::from_str_radix(port_hex, 16).ok()?;
    if ipv6 {
        let ip = parse_ipv6(ip_hex)?;
        Some(format!("[{}]:{}", ip, port))
    } else {
        let ip = parse_ipv4(ip_hex)?;
        Some(format!("{}:{}", ip, port))
    }
}

fn parse_ipv4(value: &str) -> Option<Ipv4Addr> {
    if value.len() != 8 {
        return None;
    }
    let raw = u32::from_str_radix(value, 16).ok()?;
    let bytes = raw.to_le_bytes();
    Some(Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]))
}

fn parse_ipv6(value: &str) -> Option<Ipv6Addr> {
    if value.len() != 32 {
        return None;
    }
    let mut bytes = [0_u8; 16];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let text = std::str::from_utf8(chunk).ok()?;
        bytes[index] = u8::from_str_radix(text, 16).ok()?;
    }
    for chunk in bytes.chunks_exact_mut(4) {
        chunk.reverse();
    }
    Some(Ipv6Addr::from(bytes))
}

fn tcp_state(code: &str) -> &'static str {
    match code {
        "01" => "ESTABLISHED",
        "02" => "SYN_SENT",
        "03" => "SYN_RECV",
        "04" => "FIN_WAIT1",
        "05" => "FIN_WAIT2",
        "06" => "TIME_WAIT",
        "07" => "CLOSE",
        "08" => "CLOSE_WAIT",
        "09" => "LAST_ACK",
        "0A" => "LISTEN",
        "0B" => "CLOSING",
        _ => "UNKNOWN",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_process_status, parse_ps_process_line, send_signal, summarize_network_connections,
    };
    use seclab_contracts::process::NetworkConnection;
    use std::collections::HashMap;

    #[test]
    fn send_signal_rejects_invalid_signal() {
        let err = send_signal(1, "STOP").expect_err("invalid signal should fail");
        assert_eq!(err, "only TERM or KILL signal is supported");
    }

    #[test]
    fn summarize_network_connections_groups_by_state_and_protocol() {
        let rows = vec![
            NetworkConnection {
                protocol: "TCP".to_string(),
                local_address: "127.0.0.1:1".to_string(),
                remote_address: "0.0.0.0:0".to_string(),
                state: "LISTEN".to_string(),
                pid: Some(1),
                process_name: Some("a".to_string()),
            },
            NetworkConnection {
                protocol: "TCP".to_string(),
                local_address: "127.0.0.1:2".to_string(),
                remote_address: "0.0.0.0:0".to_string(),
                state: "LISTEN".to_string(),
                pid: Some(2),
                process_name: Some("b".to_string()),
            },
        ];

        let summary = summarize_network_connections(&rows);

        assert_eq!(summary.total, 2);
        assert_eq!(summary.by_protocol.get("TCP"), Some(&2));
        assert_eq!(summary.by_state.get("LISTEN"), Some(&2));
    }

    #[test]
    fn parse_ps_process_line_reads_structured_columns() {
        let mut connection_counts = HashMap::new();
        connection_counts.insert(1234, 7);
        let row = parse_ps_process_line(
            " 1234 1 nginx 4 www-data 1.5 2.5 Ss Mon May 11 12:57:23 2026 nginx: worker process",
            &connection_counts,
        )
        .expect("ps row should parse");

        assert_eq!(row.pid, 1234);
        assert_eq!(row.parent_pid, 1);
        assert_eq!(row.name, "nginx");
        assert_eq!(row.thread_count, 4);
        assert_eq!(row.user, "www-data");
        assert_eq!(row.cpu_percent, 1.5);
        assert_eq!(row.memory_percent, 2.5);
        assert_eq!(row.connection_count, 7);
        assert_eq!(row.status, "sleeping");
        assert_eq!(row.command, "nginx: worker process");
    }

    #[test]
    fn normalize_process_status_maps_ps_codes_to_display_keys() {
        assert_eq!(normalize_process_status("R+"), "running");
        assert_eq!(normalize_process_status("Ss"), "sleeping");
        assert_eq!(normalize_process_status("T"), "stopped");
        assert_eq!(normalize_process_status("I"), "idle");
        assert_eq!(normalize_process_status("D"), "waiting");
        assert_eq!(normalize_process_status("SLl"), "locked");
        assert_eq!(normalize_process_status("Z"), "zombie");
        assert_eq!(normalize_process_status("?"), "sleeping");
    }
}
