//! 脚本库领域契约：脚本资产、单节点运行、输出与 Agent 上报。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 脚本资源归属。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scripts/")]
pub enum ScriptOwnershipKind {
    Custom,
    Compose,
    Suite,
    System,
}

/// 脚本运行状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scripts/")]
pub enum ScriptRunStatus {
    Queued,
    Starting,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
}

impl ScriptRunStatus {
    /// 判断运行是否进入终态。
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::TimedOut | Self::Cancelled
        )
    }
}

/// 输出流类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scripts/")]
pub enum ScriptOutputStream {
    Stdout,
    Stderr,
}

/// 脚本资源归属信息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scripts/", optional_fields)]
pub struct ScriptOwnership {
    pub kind: ScriptOwnershipKind,
    pub owner_id: Option<String>,
    pub owner_name: Option<String>,
    pub manager_path: Option<String>,
}

/// Master 统一计算的脚本能力。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scripts/")]
pub struct ScriptCapabilities {
    pub can_update: bool,
    pub can_remove: bool,
    pub can_clone: bool,
    pub can_run: bool,
}

/// 最近一次脚本运行摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scripts/", optional_fields)]
pub struct ScriptLastRun {
    pub run_id: String,
    pub node_id: String,
    pub status: ScriptRunStatus,
    pub finished_at: Option<String>,
}

/// 脚本列表摘要，不包含正文。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scripts/", optional_fields)]
pub struct ScriptSummary {
    pub script_id: String,
    pub name: String,
    pub description: Option<String>,
    #[ts(type = "\"shell\"")]
    pub language: String,
    pub revision: i64,
    #[ts(inline)]
    pub ownership: ScriptOwnership,
    #[ts(inline)]
    pub capabilities: ScriptCapabilities,
    pub last_run: Option<ScriptLastRun>,
    pub created_at: String,
    pub updated_at: String,
    pub updated_by: String,
}

/// 脚本正文元数据。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scripts/")]
pub struct ScriptSource {
    pub content: String,
    pub size_bytes: u64,
    pub sha256: String,
}

/// 脚本默认执行配置。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scripts/")]
pub struct ScriptExecutionDefaults {
    pub timeout_seconds: u32,
}

/// 按需加载的脚本详情。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scripts/")]
pub struct ScriptDetail {
    #[serde(flatten)]
    #[ts(flatten)]
    pub summary: ScriptSummary,
    #[ts(inline)]
    pub source: ScriptSource,
    #[ts(inline)]
    pub execution_defaults: ScriptExecutionDefaults,
}

/// 分页脚本列表。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scripts/")]
pub struct ScriptListPage {
    pub items: Vec<ScriptSummary>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
    pub loaded_at: String,
}

/// 创建脚本请求。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "scripts/", optional_fields)]
pub struct CreateScriptRequest {
    pub name: String,
    pub description: Option<String>,
    pub content: String,
    pub timeout_seconds: Option<u32>,
}

/// 更新脚本请求。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "scripts/", optional_fields)]
pub struct UpdateScriptRequest {
    pub expected_revision: i64,
    pub name: String,
    pub description: Option<String>,
    pub content: String,
    pub timeout_seconds: u32,
}

/// 创建单节点脚本运行请求。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "scripts/", optional_fields)]
pub struct CreateScriptRunRequest {
    pub node_id: String,
    pub timeout_seconds: Option<u32>,
}

/// 运行输出摘要。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scripts/")]
pub struct ScriptRunOutputSummary {
    pub available: bool,
    pub truncated: bool,
    pub size_bytes: u64,
}

/// 运行操作能力。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scripts/")]
pub struct ScriptRunCapabilities {
    pub can_cancel: bool,
}

/// 可恢复跟踪的脚本运行。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scripts/", optional_fields)]
pub struct ScriptRun {
    pub run_id: String,
    pub script_id: String,
    pub script_name: String,
    pub script_revision: i64,
    pub source_sha256: String,
    pub node_id: String,
    pub node_name: String,
    pub status: ScriptRunStatus,
    pub phase: Option<String>,
    pub queued_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub exit_code: Option<i32>,
    pub error_code: Option<String>,
    pub error_summary: Option<String>,
    #[ts(inline)]
    pub output: ScriptRunOutputSummary,
    #[ts(inline)]
    pub capabilities: ScriptRunCapabilities,
}

/// 分页脚本运行记录。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scripts/")]
pub struct ScriptRunPage {
    pub items: Vec<ScriptRun>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
    pub loaded_at: String,
}

/// 单个有序输出块。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scripts/")]
pub struct ScriptRunOutputChunk {
    pub sequence: u64,
    pub stream: ScriptOutputStream,
    pub content: String,
}

/// 游标分页运行输出。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "scripts/", optional_fields)]
pub struct ScriptRunOutputPage {
    pub run_id: String,
    pub items: Vec<ScriptRunOutputChunk>,
    pub next_cursor: Option<u64>,
    pub size_bytes: u64,
    pub truncated: bool,
}

/// Master 下发给 Agent 的脚本快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentStartScriptRunRequest {
    pub run_id: String,
    pub script_id: String,
    pub script_name: String,
    pub script_revision: i64,
    pub source_content: String,
    pub source_sha256: String,
    pub timeout_seconds: u32,
    pub ownership_kind: ScriptOwnershipKind,
}

/// Agent 向 Master 上报的运行事实。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentScriptRunReport {
    pub run_id: String,
    pub script_id: String,
    pub script_revision: i64,
    pub source_sha256: String,
    pub status: ScriptRunStatus,
    pub phase: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub exit_code: Option<i32>,
    pub error_code: Option<String>,
    pub error_summary: Option<String>,
    pub output_size_bytes: u64,
    pub output_truncated: bool,
    pub output_chunks: Vec<ScriptRunOutputChunk>,
}

/// Agent 批量上报请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentScriptRunReportBatch {
    pub reports: Vec<AgentScriptRunReport>,
}
