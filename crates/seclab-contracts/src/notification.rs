//! 通知历史契约：统一通知写入与查询模型。

use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use ts_rs::TS;

/// 通知类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export_to = "notification/")]
pub enum NotificationLevel {
    Success,
    Error,
    Warning,
    Info,
}

impl NotificationLevel {
    /// 返回稳定字符串表示，用于数据库与前端对齐。
    pub const fn as_str(self) -> &'static str {
        match self {
            NotificationLevel::Success => "success",
            NotificationLevel::Error => "error",
            NotificationLevel::Warning => "warning",
            NotificationLevel::Info => "info",
        }
    }
}

/// 单条通知历史记录。
#[derive(Debug, Clone, FromRow, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "notification/")]
pub struct NotificationRecord {
    pub id: i64,
    pub level: String,
    pub message: String,
    pub created_at: i64,
}

/// 创建通知时的请求体。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "notification/")]
pub struct NotificationCreatePayload {
    pub level: NotificationLevel,
    pub message: String,
}

/// 通知历史查询参数。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "notification/")]
pub struct NotificationQuery {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    #[ts(optional)]
    pub keyword: Option<String>,
    #[ts(optional)]
    pub level: Option<NotificationLevel>,
}

/// 批量删除通知的请求体。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "notification/")]
pub struct NotificationBatchDeletePayload {
    pub ids: Vec<i64>,
}

/// 通知历史分页返回。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "notification/")]
pub struct NotificationList {
    pub total: i64,
    pub page: u32,
    pub page_size: u32,
    pub records: Vec<NotificationRecord>,
}

fn default_page() -> u32 {
    1
}

fn default_page_size() -> u32 {
    20
}
