//! SecLab 管理共享契约。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 当前 SecLab 监听配置。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "seclab/")]
pub struct SeclabNetworkConfig {
    pub host: String,
    pub port: u16,
    pub public_host: Option<String>,
}

/// 更新 SecLab 监听配置后的返回载荷。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "seclab/")]
pub struct SeclabNetworkUpdateResult {
    pub host: String,
    pub port: u16,
    pub next_url: String,
}
