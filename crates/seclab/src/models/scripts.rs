use crate::state::DbPool;
use serde::{Deserialize, Serialize};

/// 脚本库记录结构。
#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ScriptRecord {
    pub script_id: String,
    pub title: String,
    pub description: Option<String>,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

/// 脚本库新增或更新负载结构。
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScriptPayload {
    pub title: String,
    pub description: Option<String>,
    pub content: String,
}

/// 查询所有脚本列表。
pub async fn list_scripts(pool: &DbPool) -> sqlx::Result<Vec<ScriptRecord>> {
    sqlx::query_as::<_, ScriptRecord>(
        r#"SELECT script_id, title, description, content, created_at, updated_at
           FROM scripts
           ORDER BY created_at DESC"#,
    )
    .fetch_all(pool)
    .await
}

/// 获取单个脚本详情。
pub async fn get_script(pool: &DbPool, script_id: &str) -> sqlx::Result<Option<ScriptRecord>> {
    sqlx::query_as::<_, ScriptRecord>(
        r#"SELECT script_id, title, description, content, created_at, updated_at
           FROM scripts
           WHERE script_id = ?"#,
    )
    .bind(script_id)
    .fetch_optional(pool)
    .await
}

/// 创建新脚本。
pub async fn create_script(pool: &DbPool, record: &ScriptRecord) -> sqlx::Result<()> {
    sqlx::query(
        r#"INSERT INTO scripts (script_id, title, description, content, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?)"#,
    )
    .bind(&record.script_id)
    .bind(&record.title)
    .bind(&record.description)
    .bind(&record.content)
    .bind(&record.created_at)
    .bind(&record.updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// 更新现有脚本。
pub async fn update_script(pool: &DbPool, record: &ScriptRecord) -> sqlx::Result<()> {
    sqlx::query(
        r#"UPDATE scripts
           SET title = ?,
               description = ?,
               content = ?
           WHERE script_id = ?"#,
    )
    .bind(&record.title)
    .bind(&record.description)
    .bind(&record.content)
    .bind(&record.script_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// 删除脚本。
pub async fn delete_script(pool: &DbPool, script_id: &str) -> sqlx::Result<()> {
    sqlx::query(
        r#"DELETE FROM scripts
           WHERE script_id = ?"#,
    )
    .bind(script_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// 获取所有内存中嵌入的内置脚本。
pub fn get_builtin_scripts() -> Vec<ScriptRecord> {
    Vec::new()
}
