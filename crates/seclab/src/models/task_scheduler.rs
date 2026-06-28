//! 计划任务模型：任务定义与执行记录的数据访问。

use crate::state::DbPool;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTask {
    pub id: i64,
    pub name: String,
    pub agent_id: String,
    pub command: String,
    pub cron_expr: String,
    pub enabled: bool,
    pub timeout_secs: i64,
    pub no_overlap: bool,
    pub last_run_at: Option<i64>,
    pub next_run_at: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
    pub sync_status: String,
    pub sync_error: Option<String>,
    pub synced_at: Option<String>,
    pub revision: i64,
    pub last_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TaskRun {
    pub id: i64,
    pub task_id: i64,
    pub agent_id: String,
    pub triggered_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub status: String,
    pub exit_code: Option<i64>,
    pub log_excerpt: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct NewTask {
    pub name: String,
    pub agent_id: String,
    pub command: String,
    pub cron_expr: String,
    pub enabled: bool,
    pub timeout_secs: i64,
    pub no_overlap: bool,
    pub next_run_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct UpdateTask {
    pub name: String,
    pub agent_id: String,
    pub command: String,
    pub cron_expr: String,
    pub enabled: bool,
    pub timeout_secs: i64,
    pub no_overlap: bool,
    pub next_run_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct NewTaskRun {
    pub run_id: Option<String>,
    pub task_id: i64,
    pub agent_id: String,
    pub triggered_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub status: String,
    pub exit_code: Option<i64>,
    pub log_excerpt: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UpdateTaskRun {
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub status: String,
    pub exit_code: Option<i64>,
    pub log_excerpt: Option<String>,
    pub error_message: Option<String>,
}

pub async fn list_tasks(pool: &DbPool, agent_id: Option<&str>) -> sqlx::Result<Vec<ScheduledTask>> {
    match agent_id {
        Some(value) => {
            sqlx::query_as::<_, ScheduledTask>(
                r#"
                SELECT
                    t.id,
                    t.name,
                    t.agent_id,
                    t.command,
                    t.cron_expr,
                    t.enabled,
                    t.timeout_secs,
                    t.no_overlap,
                    t.last_run_at,
                    t.next_run_at,
                    t.created_at,
                    t.updated_at,
                    t.sync_status,
                    t.sync_error,
                    t.synced_at,
                    t.revision,
                    (
                        SELECT r.status
                        FROM task_runs r
                        WHERE r.task_id = t.id
                        ORDER BY r.id DESC
                        LIMIT 1
                    ) AS last_status
                FROM scheduled_tasks t
                WHERE t.agent_id = ?
                ORDER BY t.id DESC
                "#,
            )
            .bind(value)
            .fetch_all(pool)
            .await
        }
        None => {
            sqlx::query_as::<_, ScheduledTask>(
                r#"
                SELECT
                    t.id,
                    t.name,
                    t.agent_id,
                    t.command,
                    t.cron_expr,
                    t.enabled,
                    t.timeout_secs,
                    t.no_overlap,
                    t.last_run_at,
                    t.next_run_at,
                    t.created_at,
                    t.updated_at,
                    t.sync_status,
                    t.sync_error,
                    t.synced_at,
                    t.revision,
                    (
                        SELECT r.status
                        FROM task_runs r
                        WHERE r.task_id = t.id
                        ORDER BY r.id DESC
                        LIMIT 1
                    ) AS last_status
                FROM scheduled_tasks t
                ORDER BY t.id DESC
                "#,
            )
            .fetch_all(pool)
            .await
        }
    }
}

pub async fn get_task_by_id(pool: &DbPool, id: i64) -> sqlx::Result<Option<ScheduledTask>> {
    sqlx::query_as::<_, ScheduledTask>(
        r#"
        SELECT
            t.id,
            t.name,
            t.agent_id,
            t.command,
            t.cron_expr,
            t.enabled,
            t.timeout_secs,
            t.no_overlap,
            t.last_run_at,
            t.next_run_at,
            t.created_at,
            t.updated_at,
            t.sync_status,
            t.sync_error,
            t.synced_at,
            t.revision,
            (
                SELECT r.status
                FROM task_runs r
                WHERE r.task_id = t.id
                ORDER BY r.id DESC
                LIMIT 1
            ) AS last_status
        FROM scheduled_tasks t
        WHERE t.id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn create_task(pool: &DbPool, payload: &NewTask) -> sqlx::Result<i64> {
    let result = sqlx::query(
        r#"
        INSERT INTO scheduled_tasks (
            name,
            agent_id,
            command,
            cron_expr,
            enabled,
            timeout_secs,
            no_overlap,
            next_run_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&payload.name)
    .bind(&payload.agent_id)
    .bind(&payload.command)
    .bind(&payload.cron_expr)
    .bind(if payload.enabled { 1 } else { 0 })
    .bind(payload.timeout_secs)
    .bind(if payload.no_overlap { 1 } else { 0 })
    .bind(payload.next_run_at)
    .execute(pool)
    .await?;

    Ok(result.last_insert_rowid())
}

pub async fn update_task(pool: &DbPool, id: i64, payload: &UpdateTask) -> sqlx::Result<bool> {
    let result = sqlx::query(
        r#"
        UPDATE scheduled_tasks
        SET
            name = ?,
            agent_id = ?,
            command = ?,
            cron_expr = ?,
            enabled = ?,
            timeout_secs = ?,
            no_overlap = ?,
            next_run_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&payload.name)
    .bind(&payload.agent_id)
    .bind(&payload.command)
    .bind(&payload.cron_expr)
    .bind(if payload.enabled { 1 } else { 0 })
    .bind(payload.timeout_secs)
    .bind(if payload.no_overlap { 1 } else { 0 })
    .bind(payload.next_run_at)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn delete_task(pool: &DbPool, id: i64) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM scheduled_tasks WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn set_task_enabled(
    pool: &DbPool,
    id: i64,
    enabled: bool,
    next_run_at: Option<i64>,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        r#"
        UPDATE scheduled_tasks
        SET enabled = ?, next_run_at = ?
        WHERE id = ?
        "#,
    )
    .bind(if enabled { 1 } else { 0 })
    .bind(next_run_at)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn update_task_run_times(
    pool: &DbPool,
    task_id: i64,
    last_run_at: i64,
    next_run_at: Option<i64>,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        UPDATE scheduled_tasks
        SET last_run_at = ?, next_run_at = ?
        WHERE id = ?
        "#,
    )
    .bind(last_run_at)
    .bind(next_run_at)
    .bind(task_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn create_task_run(pool: &DbPool, payload: &NewTaskRun) -> sqlx::Result<i64> {
    let result = sqlx::query(
        r#"
        INSERT INTO task_runs (
            run_id,
            task_id,
            agent_id,
            triggered_at,
            started_at,
            finished_at,
            status,
            exit_code,
            log_excerpt,
            error_message
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(run_id) DO UPDATE SET
            started_at = excluded.started_at,
            finished_at = excluded.finished_at,
            status = excluded.status,
            exit_code = excluded.exit_code,
            log_excerpt = excluded.log_excerpt,
            error_message = excluded.error_message
        "#,
    )
    .bind(payload.run_id.as_deref())
    .bind(payload.task_id)
    .bind(&payload.agent_id)
    .bind(payload.triggered_at)
    .bind(payload.started_at)
    .bind(payload.finished_at)
    .bind(&payload.status)
    .bind(payload.exit_code)
    .bind(payload.log_excerpt.as_deref())
    .bind(payload.error_message.as_deref())
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn update_task_run(
    pool: &DbPool,
    run_id: i64,
    payload: &UpdateTaskRun,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        UPDATE task_runs
        SET
            started_at = ?,
            finished_at = ?,
            status = ?,
            exit_code = ?,
            log_excerpt = ?,
            error_message = ?
        WHERE id = ?
        "#,
    )
    .bind(payload.started_at)
    .bind(payload.finished_at)
    .bind(&payload.status)
    .bind(payload.exit_code)
    .bind(payload.log_excerpt.as_deref())
    .bind(payload.error_message.as_deref())
    .bind(run_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_task_runs(pool: &DbPool, task_id: i64, limit: i64) -> sqlx::Result<Vec<TaskRun>> {
    let limit = limit.clamp(1, 200);
    sqlx::query_as::<_, TaskRun>(
        r#"
        SELECT
            id,
            task_id,
            agent_id,
            triggered_at,
            started_at,
            finished_at,
            status,
            exit_code,
            log_excerpt,
            error_message,
            created_at
        FROM task_runs
        WHERE task_id = ?
        ORDER BY id DESC
        LIMIT ?
        "#,
    )
    .bind(task_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

pub async fn list_due_tasks(pool: &DbPool, now_ts: i64) -> sqlx::Result<Vec<ScheduledTask>> {
    sqlx::query_as::<_, ScheduledTask>(
        r#"
        SELECT
            t.id,
            t.name,
            t.agent_id,
            t.command,
            t.cron_expr,
            t.enabled,
            t.timeout_secs,
            t.no_overlap,
            t.last_run_at,
            t.next_run_at,
            t.created_at,
            t.updated_at,
            t.sync_status,
            t.sync_error,
            t.synced_at,
            t.revision,
            (
                SELECT r.status
                FROM task_runs r
                WHERE r.task_id = t.id
                ORDER BY r.id DESC
                LIMIT 1
            ) AS last_status
        FROM scheduled_tasks t
        WHERE t.enabled = 1
          AND t.next_run_at IS NOT NULL
          AND t.next_run_at <= ?
        ORDER BY t.next_run_at ASC, t.id ASC
        "#,
    )
    .bind(now_ts)
    .fetch_all(pool)
    .await
}

pub async fn update_sync_status(
    pool: &DbPool,
    id: i64,
    status: &str,
    error: Option<&str>,
    synced_at: Option<&str>,
    next_run_at: Option<i64>,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        r#"
        UPDATE scheduled_tasks
        SET sync_status = ?, sync_error = ?, synced_at = ?, next_run_at = ?
        WHERE id = ?
        "#,
    )
    .bind(status)
    .bind(error)
    .bind(synced_at)
    .bind(next_run_at)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn increment_revision(pool: &DbPool, id: i64) -> sqlx::Result<i64> {
    sqlx::query(
        r#"
        UPDATE scheduled_tasks
        SET revision = revision + 1
        WHERE id = ?
        "#,
    )
    .bind(id)
    .execute(pool)
    .await?;

    let rev: i64 = sqlx::query_scalar("SELECT revision FROM scheduled_tasks WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await?;
    Ok(rev)
}

#[derive(Debug, Clone, FromRow)]
pub struct TaskSyncOp {
    pub id: i64,
    pub task_id: i64,
    pub agent_id: String,
    pub op_type: String,
    pub revision: i64,
    pub status: String,
}

pub async fn queue_sync_op(
    pool: &DbPool,
    task_id: i64,
    agent_id: &str,
    op_type: &str,
    revision: i64,
) -> sqlx::Result<()> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    // 折叠该任务未完成的同步操作
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM task_sync_ops WHERE task_id = ? AND status IN ('pending', 'failed') LIMIT 1"
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await?;

    if let Some(op_id) = existing {
        sqlx::query(
            "UPDATE task_sync_ops SET agent_id = ?, op_type = ?, revision = ?, status = 'pending', error_message = NULL, updated_at = ? WHERE id = ?"
        )
        .bind(agent_id)
        .bind(op_type)
        .bind(revision)
        .bind(&now)
        .bind(op_id)
        .execute(pool)
        .await?;
    } else {
        sqlx::query(
            "INSERT INTO task_sync_ops (task_id, agent_id, op_type, revision, status, created_at, updated_at) VALUES (?, ?, ?, ?, 'pending', ?, ?)"
        )
        .bind(task_id)
        .bind(agent_id)
        .bind(op_type)
        .bind(revision)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn list_pending_sync_ops(pool: &DbPool) -> sqlx::Result<Vec<TaskSyncOp>> {
    sqlx::query_as::<_, TaskSyncOp>(
        "SELECT id, task_id, agent_id, op_type, revision, status FROM task_sync_ops WHERE status IN ('pending', 'failed') ORDER BY id ASC"
    )
    .fetch_all(pool)
    .await
}

pub async fn update_sync_op_status(
    pool: &DbPool,
    id: i64,
    status: &str,
    error_message: Option<&str>,
) -> sqlx::Result<()> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    sqlx::query(
        "UPDATE task_sync_ops SET status = ?, error_message = ?, updated_at = ? WHERE id = ?",
    )
    .bind(status)
    .bind(error_message)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}
