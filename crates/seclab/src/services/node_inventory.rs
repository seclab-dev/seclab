//! 节点库存服务：维护 `nodes` 表并提供控制面读模型。

use crate::models::node_api_types::{NodeCreatePayload, NodeUpdatePayload};
use crate::models::nodes::{NodeStatus, get_node_by_id};
use crate::state::DbPool;
use sqlx::Row;

fn normalize_name(name: Option<&str>, node_id: &str) -> String {
    name.unwrap_or(node_id)
        .trim()
        .to_lowercase()
        .replace(' ', "-")
}

fn encode_labels_from_payload(tags: Option<Vec<String>>) -> String {
    serde_json::to_string(&tags.unwrap_or_default()).unwrap_or_else(|_| "[]".to_string())
}

fn encode_metadata_from_payload(metadata: Option<serde_json::Value>) -> String {
    serde_json::to_string(&metadata.unwrap_or_else(|| serde_json::json!({})))
        .unwrap_or_else(|_| "{}".to_string())
}

/// 节点列表响应：供 `/nodes` 控制面 API 使用。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeSummary {
    pub node_id: String,
    pub name: String,
    pub group_name: String,
    pub description: Option<String>,
    pub address: Option<String>,
    pub service_port: Option<String>,
    pub status: String,
    pub tags: Vec<String>,
    pub metadata: Option<serde_json::Value>,
    pub last_seen_at: Option<String>,
}

/// 从 `nodes + node_provisioning` 聚合节点列表响应。
pub async fn list_node_summaries(pool: &DbPool) -> sqlx::Result<Vec<NodeSummary>> {
    let rows = sqlx::query(
        r#"
        SELECT
            n.node_id,
            n.name,
            n.group_name,
            n.description,
            n.status,
            n.labels,
            n.metadata,
            n.last_seen_at,
            p.ssh_addr,
            p.ssh_port,
            p.expected_listen_port
        FROM nodes n
        LEFT JOIN node_provisioning p
          ON p.node_id = n.node_id
        ORDER BY n.created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut nodes = Vec::with_capacity(rows.len());
    for row in rows {
        let metadata_raw: String = row.try_get("metadata")?;
        let metadata = serde_json::from_str::<serde_json::Value>(&metadata_raw).ok();
        let labels_raw: String = row.try_get("labels")?;
        let tags = serde_json::from_str::<Vec<String>>(&labels_raw).unwrap_or_default();
        let ssh_addr: Option<String> = row.try_get("ssh_addr")?;
        let ssh_port: Option<i64> = row.try_get("ssh_port")?;
        let address = match (ssh_addr, ssh_port) {
            (Some(addr), Some(port)) => Some(format!("{addr}:{port}")),
            (Some(addr), None) => Some(addr),
            _ => None,
        };

        nodes.push(NodeSummary {
            node_id: row.try_get("node_id")?,
            name: row.try_get("name")?,
            group_name: row.try_get("group_name")?,
            description: row.try_get("description")?,
            address,
            service_port: row
                .try_get::<Option<i64>, _>("expected_listen_port")?
                .map(|value| value.to_string()),
            status: row.try_get("status")?,
            tags,
            metadata,
            last_seen_at: row.try_get("last_seen_at")?,
        });
    }

    Ok(nodes)
}

/// 读取单个节点摘要。
pub async fn get_node_summary(pool: &DbPool, node_id: &str) -> sqlx::Result<Option<NodeSummary>> {
    let nodes = list_node_summaries(pool).await?;
    Ok(nodes.into_iter().find(|node| node.node_id == node_id))
}

/// 直接根据节点创建载荷写入 `nodes` 主记录。
pub async fn create_node_from_payload(
    pool: &DbPool,
    node_id: &str,
    payload: &NodeCreatePayload,
    status: NodeStatus,
) -> sqlx::Result<()> {
    let normalized_name = normalize_name(payload.name.as_deref(), node_id);
    let group_name = payload
        .group_id
        .clone()
        .unwrap_or_else(|| "default".to_string());

    sqlx::query(
        r#"
        INSERT INTO nodes (
            node_id,
            name,
            normalized_name,
            group_name,
            labels,
            description,
            status,
            schedulable,
            metadata,
            registered_at,
            last_seen_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(node_id)
    .bind(payload.name.clone().unwrap_or_else(|| node_id.to_string()))
    .bind(normalized_name)
    .bind(group_name)
    .bind(encode_labels_from_payload(payload.tags.clone()))
    .bind(payload.description.clone())
    .bind(status.as_str())
    .bind(if matches!(status, NodeStatus::Retired) {
        0_i64
    } else {
        1_i64
    })
    .bind(encode_metadata_from_payload(payload.metadata.clone()))
    .bind(Option::<String>::None)
    .bind(Option::<String>::None)
    .execute(pool)
    .await?;

    Ok(())
}

/// 直接根据节点更新载荷更新 `nodes` 主记录。
pub async fn update_node_from_payload(
    pool: &DbPool,
    node_id: &str,
    payload: &NodeUpdatePayload,
) -> sqlx::Result<()> {
    let normalized_name = payload
        .name
        .as_deref()
        .map(|name| normalize_name(Some(name), node_id));

    sqlx::query(
        r#"
        UPDATE nodes
        SET
            name = COALESCE(?, name),
            normalized_name = COALESCE(?, normalized_name),
            group_name = COALESCE(?, group_name),
            labels = COALESCE(?, labels),
            description = COALESCE(?, description),
            metadata = COALESCE(?, metadata)
        WHERE node_id = ?
        "#,
    )
    .bind(payload.name.clone())
    .bind(normalized_name)
    .bind(payload.group_id.clone())
    .bind(
        payload
            .tags
            .clone()
            .map(|tags| encode_labels_from_payload(Some(tags))),
    )
    .bind(payload.description.clone())
    .bind(
        payload
            .metadata
            .clone()
            .map(|metadata| encode_metadata_from_payload(Some(metadata))),
    )
    .bind(node_id)
    .execute(pool)
    .await?;

    Ok(())
}

/// 删除节点主记录。
pub async fn delete_node(pool: &DbPool, node_id: &str) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        DELETE FROM nodes
        WHERE node_id = ?
        "#,
    )
    .bind(node_id)
    .execute(pool)
    .await?;

    Ok(())
}

/// 读取节点展示名称。
pub async fn get_node_display_name(pool: &DbPool, node_id: &str) -> sqlx::Result<Option<String>> {
    Ok(get_node_by_id(pool, node_id).await?.map(|node| node.name))
}

/// 检查节点名称是否冲突（注意忽略当前节点）
pub async fn check_name_conflict(
    pool: &DbPool,
    name: &str,
    exclude_node_id: Option<&str>,
) -> sqlx::Result<bool> {
    let normalized = normalize_name(Some(name), "");

    let query_str = if exclude_node_id.is_some() {
        "SELECT 1 FROM nodes WHERE normalized_name = ? AND node_id != ? LIMIT 1"
    } else {
        "SELECT 1 FROM nodes WHERE normalized_name = ? LIMIT 1"
    };

    let mut query = sqlx::query(query_str).bind(normalized);
    if let Some(node_id) = exclude_node_id {
        query = query.bind(node_id);
    }

    let row = query.fetch_optional(pool).await?;
    Ok(row.is_some())
}
