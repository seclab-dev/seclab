//! 进程管理共享契约。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

/// 进程列表项。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "process/")]
pub struct ProcessItem {
    pub pid: u32,
    pub name: String,
    pub parent_pid: u32,
    pub thread_count: usize,
    pub user: String,
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub connection_count: usize,
    pub status: String,
    pub start_time: i64,
    pub command: String,
}

/// 进程管理 WebSocket 信号请求。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "process/")]
pub struct ProcessManagerSignalRequest {
    pub request_id: String,
    pub pid: u32,
    pub signal: String,
}

/// 主机网络连接项。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "process/", optional_fields)]
pub struct NetworkConnection {
    pub protocol: String,
    pub local_address: String,
    pub remote_address: String,
    pub state: String,
    pub pid: Option<u32>,
    pub process_name: Option<String>,
}

/// 网络连接汇总。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "process/")]
pub struct NetworkSummary {
    pub total: usize,
    pub by_state: BTreeMap<String, usize>,
    pub by_protocol: BTreeMap<String, usize>,
}

/// 进程管理 WebSocket 当前订阅视图。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "process/")]
pub enum ProcessManagerActiveView {
    Process,
    Network,
}

/// 进程管理 WebSocket 客户端消息。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
#[ts(export_to = "process/")]
pub enum ProcessManagerClientMessage {
    SetActiveView(ProcessManagerActiveView),
    SendSignal(ProcessManagerSignalRequest),
}

/// 进程快照推送载荷。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "process/")]
pub struct ProcessSnapshot {
    pub processes: Vec<ProcessItem>,
    pub sampled_at: i64,
}

/// 网络连接快照推送载荷。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "process/")]
pub struct NetworkSnapshot {
    pub connections: Vec<NetworkConnection>,
    pub summary: NetworkSummary,
    pub sampled_at: i64,
}

/// 进程信号执行结果推送载荷。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "process/")]
pub struct SignalResult {
    pub request_id: String,
    pub pid: u32,
    pub signal: String,
    pub success: bool,
    pub process_existed: bool,
    pub message: String,
    pub sampled_at: i64,
}

/// 进程管理 WebSocket 错误载荷。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "process/", optional_fields)]
pub struct ProcessManagerError {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub message: String,
}

/// 进程管理 WebSocket 服务端消息。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", content = "payload", rename_all = "camelCase")]
#[ts(export_to = "process/")]
pub enum ProcessManagerServerMessage {
    ProcessSnapshot(ProcessSnapshot),
    NetworkSnapshot(NetworkSnapshot),
    SignalResult(SignalResult),
    Error(ProcessManagerError),
    Heartbeat,
}
