//! 节点任务模型：`node_tasks` 表的数据结构与基础写入方法。

use crate::state::DbPool;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// 节点任务记录。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct NodeTaskRecord {
    pub task_id: String,
    pub node_id: String,
    pub session_id: Option<String>,
    pub task_type: String,
    pub scheduled_at: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub status: String,
    pub payload: String,
    pub result_summary: Option<String>,
    pub error_detail: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 写入节点任务记录。
pub async fn insert_node_task(pool: &DbPool, record: &NodeTaskRecord) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO node_tasks (
            task_id,
            node_id,
            session_id,
            task_type,
            scheduled_at,
            started_at,
            finished_at,
            status,
            payload,
            result_summary,
            error_detail
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&record.task_id)
    .bind(&record.node_id)
    .bind(&record.session_id)
    .bind(&record.task_type)
    .bind(&record.scheduled_at)
    .bind(&record.started_at)
    .bind(&record.finished_at)
    .bind(&record.status)
    .bind(&record.payload)
    .bind(&record.result_summary)
    .bind(&record.error_detail)
    .execute(pool)
    .await?;

    Ok(())
}
