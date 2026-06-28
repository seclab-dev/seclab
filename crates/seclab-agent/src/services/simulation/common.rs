//! 仿真运行器通用数据结构与日志上报工具。

use seclab_security::client::build_mtls_client;
use std::path::PathBuf;
use tracing::{error, info};

/// 仿真交互审计日志上报草稿。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimLogDraft {
    pub rule_id: String,
    pub node_id: String,
    pub client_ip: String,
    pub client_port: u16,
    pub server_port: u16,
    pub event_type: String, // 'connection', 'http_request', 'exploit_attempt'
    pub detail_summary: String,
    pub payload_hex: Option<String>,
}

/// 异步向控制端上报审计日志的辅助方法。
pub(super) fn report_sim_log_async(callback_url: String, draft: SimLogDraft) {
    let callback_url = crate::config::adjust_callback_url(&callback_url);
    tokio::spawn(async move {
        let client = build_mtls_client("seclab");
        if let Ok(c) = client {
            let res = c.post(&callback_url).json(&draft).send().await;
            if let Err(err) = res {
                error!(
                    "Failed to report simulation audit log to control plane: {:?}",
                    err
                );
            }
        }
    });
}

/// 物理临时 PCAP 文件防线清理 Guard，在 Drop 时自动清理物理临时文件，100% 杜绝磁盘垃圾残留。
pub struct CleanupGuard {
    pub path: PathBuf,
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        if self.path.exists() {
            let path = self.path.clone();
            if let Err(err) = std::fs::remove_file(&path) {
                error!(
                    "CleanupGuard failed to remove temporary PCAP file '{:?}': {:?}",
                    path, err
                );
            } else {
                info!(
                    "CleanupGuard successfully removed temporary PCAP file '{:?}'",
                    path
                );
            }
        }
    }
}
