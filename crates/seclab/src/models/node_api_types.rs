//! 节点控制面 API 共享类型：创建与更新载荷。

use serde::{Deserialize, Serialize};

/// 新建节点时的请求载荷。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NodeCreatePayload {
    pub agent_id: Option<String>,
    pub name: Option<String>,
    pub group_id: Option<String>,
    pub description: Option<String>,
    pub addr: Option<String>,
    pub port: Option<String>,
    pub user: Option<String>,
    pub pwd: Option<String>,
    pub private_key: Option<String>,
    pub private_key_passphrase: Option<String>,
    pub auth_mode: Option<String>,
    pub status: Option<String>,
    pub version: Option<String>,
    pub tags: Option<Vec<String>>,
    pub metadata: Option<serde_json::Value>,
    pub install_dir: Option<String>,
    #[serde(rename = "servicePort", alias = "listenPort")]
    pub service_port: Option<String>,
    pub sync_enabled: Option<bool>,
}

/// 更新节点信息时的请求载荷。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NodeUpdatePayload {
    pub agent_id: Option<String>,
    pub name: Option<String>,
    pub group_id: Option<String>,
    pub description: Option<String>,
    pub addr: Option<String>,
    pub port: Option<String>,
    pub user: Option<String>,
    pub pwd: Option<String>,
    pub private_key: Option<String>,
    pub private_key_passphrase: Option<String>,
    pub auth_mode: Option<String>,
    pub status: Option<String>,
    pub version: Option<String>,
    pub tags: Option<Vec<String>>,
    pub metadata: Option<serde_json::Value>,
    pub install_dir: Option<String>,
    #[serde(rename = "servicePort", alias = "listenPort")]
    pub service_port: Option<String>,
    pub sync_enabled: Option<bool>,
    pub seclab_url: Option<String>,
}
