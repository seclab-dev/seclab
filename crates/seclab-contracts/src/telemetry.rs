//! 埋点共享契约：统一事件来源与平台事件载荷结构。

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::fmt::{Display, Formatter};
use std::net::IpAddr;
use ts_rs::TS;

/// 事件来源端。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TelemetrySource {
    SecLab,
    Agent,
    Frontend,
}

impl TelemetrySource {
    /// 返回稳定字符串，便于日志与链路追踪复用。
    pub const fn as_str(self) -> &'static str {
        match self {
            TelemetrySource::SecLab => "seclab",
            TelemetrySource::Agent => "agent",
            TelemetrySource::Frontend => "frontend",
        }
    }
}

/// 统一平台/埋点事件包装结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformEventEnvelope<T> {
    pub event_name: String,
    pub trace_id: String,
    pub source: TelemetrySource,
    pub payload: T,
}

/// 统一日志状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "telemetry/")]
pub enum LogStatus {
    Success,
    Failed,
}

/// 操作日志的影响级别，与业务终态相互独立。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, sqlx::Type)]
#[serde(rename_all = "UPPERCASE")]
#[sqlx(type_name = "TEXT", rename_all = "UPPERCASE")]
#[ts(export_to = "telemetry/")]
pub enum PlatformLogLevel {
    Info,
    Warning,
    Error,
}

impl PlatformLogLevel {
    /// 返回持久化使用的固定级别字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            PlatformLogLevel::Info => "INFO",
            PlatformLogLevel::Warning => "WARNING",
            PlatformLogLevel::Error => "ERROR",
        }
    }
}

impl LogStatus {
    /// 返回持久化使用的固定状态字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            LogStatus::Success => "SUCCESS",
            LogStatus::Failed => "FAILED",
        }
    }
}

impl Display for LogStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// 统一日志业务模块。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "telemetry/")]
pub enum LogModule {
    System,
    Auth,
    Docker,
    File,
    Process,
}

impl LogModule {
    /// 返回持久化使用的固定模块字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            LogModule::System => "System",
            LogModule::Auth => "Auth",
            LogModule::Docker => "Docker",
            LogModule::File => "File",
            LogModule::Process => "Process",
        }
    }
}

impl Display for LogModule {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// 统一日志事件输入模型。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformLogEntryDraft {
    pub user_id: Option<i64>,
    pub username: String,
    pub module: LogModule,
    pub event: String,
    pub target_type: String,
    pub target_id: String,
    pub status: LogStatus,
    pub level: PlatformLogLevel,
    pub client_ip: IpAddr,
    pub trace_id: String,
    pub source: String,
    pub request_path: String,
    pub method: String,
    pub metadata: Option<JsonValue>,
}

impl PlatformLogEntryDraft {
    /// 使用基础上下文构建统一日志草稿。
    pub fn new(username: &str, event: &str, client_ip: IpAddr) -> Self {
        Self {
            user_id: None,
            username: username.to_string(),
            module: LogModule::System,
            event: event.to_string(),
            target_type: String::new(),
            target_id: String::new(),
            status: LogStatus::Failed,
            level: PlatformLogLevel::Error,
            client_ip,
            trace_id: String::new(),
            source: String::new(),
            request_path: String::new(),
            method: String::new(),
            metadata: None,
        }
    }

    pub fn user_id(mut self, id: i64) -> Self {
        self.user_id = Some(id);
        self
    }

    pub fn module(mut self, module: LogModule) -> Self {
        self.module = module;
        self
    }

    pub fn target_type(mut self, target_type: &str) -> Self {
        self.target_type = target_type.to_string();
        self
    }

    pub fn target_id(mut self, target_id: &str) -> Self {
        self.target_id = target_id.to_string();
        self
    }

    pub fn trace_id(mut self, trace_id: &str) -> Self {
        self.trace_id = trace_id.to_string();
        self
    }

    pub fn source(mut self, source: &str) -> Self {
        self.source = source.to_string();
        self
    }

    pub fn request(mut self, method: &str, request_path: &str) -> Self {
        self.method = method.to_string();
        self.request_path = request_path.to_string();
        self
    }

    pub fn status(mut self, status: LogStatus) -> Self {
        self.status = status;
        self.level = match status {
            LogStatus::Success => PlatformLogLevel::Info,
            LogStatus::Failed => PlatformLogLevel::Error,
        };
        self
    }

    pub fn set_success(mut self) -> Self {
        self.status = LogStatus::Success;
        self.level = PlatformLogLevel::Info;
        self
    }

    pub fn level(mut self, level: PlatformLogLevel) -> Self {
        self.level = level;
        self
    }

    pub fn metadata(mut self, data: JsonValue) -> Self {
        self.metadata = Some(data);
        self
    }
}
