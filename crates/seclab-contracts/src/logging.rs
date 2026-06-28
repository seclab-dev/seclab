//! 统一日志契约：日志记录、查询参数与列表返回。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::FromRow;
use ts_rs::TS;

use crate::telemetry::LogModule;

/// 平台日志记录。
#[derive(Debug, Clone, FromRow, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "logging/", optional_fields)]
pub struct PlatformLog {
    pub id: i64,
    pub user_id: Option<i64>,
    pub username: String,
    pub module: String,
    pub event: String,
    pub target_type: String,
    pub target_id: String,
    #[serde(with = "chrono::serde::ts_seconds")]
    #[ts(type = "number")]
    pub timestamp: DateTime<Utc>,
    pub status: String,
    pub client_ip: String,
    pub trace_id: String,
    pub source: String,
    pub request_path: String,
    pub method: String,
    #[sqlx(json)]
    #[ts(type = "Record<string, unknown>")]
    pub metadata: JsonValue,
}

/// 日志查询参数。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "logging/", optional_fields)]
pub struct PlatformLogQuery {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    pub modules: Option<Vec<LogModule>>,
    pub events: Option<Vec<String>>,
    pub event_prefixes: Option<Vec<String>>,
    pub statuses: Option<Vec<String>>,
    /// 起始时间戳，Unix epoch milliseconds。
    pub start_at: Option<i64>,
    /// 结束时间戳，Unix epoch milliseconds。
    pub end_at: Option<i64>,
    pub keyword: Option<String>,
}

/// 日志分页返回。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "logging/")]
pub struct PlatformLogList {
    pub total: i64,
    pub page: u32,
    pub page_size: u32,
    pub logs: Vec<PlatformLog>,
}

fn default_page() -> u32 {
    1
}

fn default_page_size() -> u32 {
    10
}
