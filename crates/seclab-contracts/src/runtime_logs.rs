//! 运行日志查询契约：读取 `tracing` 轮转日志文件。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 运行日志文件摘要。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "runtime_logs/")]
pub struct RuntimeLogFile {
    pub service: String,
    pub node_id: Option<String>,
    pub node_name: Option<String>,
    pub file_name: String,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
}

/// 运行日志查询参数。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "runtime_logs/", optional_fields)]
pub struct RuntimeLogQuery {
    pub service: String,
    pub node_id: Option<String>,
    pub file_name: String,
    pub level: Option<String>,
    pub target: Option<String>,
    pub keyword: Option<String>,
    pub cursor: Option<u64>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

/// 运行日志单行结果。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "runtime_logs/", optional_fields)]
pub struct RuntimeLogLine {
    pub offset: u64,
    pub timestamp: Option<String>,
    pub level: Option<String>,
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub message: String,
    pub parse_error: bool,
}

/// 运行日志查询结果。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "runtime_logs/", optional_fields)]
pub struct RuntimeLogQueryResult {
    pub lines: Vec<RuntimeLogLine>,
    pub next_cursor: Option<u64>,
    pub has_more: bool,
}

fn default_limit() -> u32 {
    200
}
