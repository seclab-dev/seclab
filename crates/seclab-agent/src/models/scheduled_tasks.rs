//! 本地计划任务模型与执行历史模型的数据访问。

use crate::state::DbPool;
use chrono::{TimeZone, Utc};
use cron::Schedule;
use seclab_api::error::{ApiError, ApiResult};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AgentScheduledTask {
    pub controller_task_id: i64,
    pub revision: i64,
    pub name: String,
    pub command: String,
    pub cron_expr: String,
    pub enabled: bool,
    pub timeout_secs: i64,
    pub no_overlap: bool,
    pub last_run_at: Option<i64>,
    pub next_run_at: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskRun {
    pub run_id: String,
    pub controller_task_id: i64,
    pub triggered_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub status: String,
    pub exit_code: Option<i64>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub error_message: Option<String>,
    pub trigger_source: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskStatus {
    pub controller_task_id: i64,
    pub next_run_at: Option<i64>,
    pub last_run_at: Option<i64>,
    pub last_status: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertTaskPayload {
    pub controller_task_id: i64,
    pub revision: i64,
    pub name: String,
    pub command: String,
    pub cron_expr: String,
    pub enabled: bool,
    pub timeout_secs: i64,
    pub no_overlap: bool,
    pub force: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct NewAgentTaskRun {
    pub run_id: String,
    pub controller_task_id: i64,
    pub triggered_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub status: String,
    pub exit_code: Option<i64>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub error_message: Option<String>,
    pub trigger_source: String,
}

#[derive(Debug, Clone)]
pub struct UpdateAgentTaskRun {
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub status: String,
    pub exit_code: Option<i64>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub error_message: Option<String>,
}

/// 插入或更新本地任务副本，始终信任主控 revision。
pub async fn upsert_task(
    pool: &DbPool,
    payload: &UpsertTaskPayload,
    next_run_at: Option<i64>,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO scheduled_tasks (
            controller_task_id,
            revision,
            name,
            command,
            cron_expr,
            enabled,
            timeout_secs,
            no_overlap,
            next_run_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ON CONFLICT(controller_task_id) DO UPDATE SET
            revision = excluded.revision,
            name = excluded.name,
            command = excluded.command,
            cron_expr = excluded.cron_expr,
            enabled = excluded.enabled,
            timeout_secs = excluded.timeout_secs,
            no_overlap = excluded.no_overlap,
            next_run_at = excluded.next_run_at
        "#,
    )
    .bind(payload.controller_task_id)
    .bind(payload.revision)
    .bind(&payload.name)
    .bind(&payload.command)
    .bind(&payload.cron_expr)
    .bind(if payload.enabled { 1 } else { 0 })
    .bind(payload.timeout_secs)
    .bind(if payload.no_overlap { 1 } else { 0 })
    .bind(next_run_at)
    .execute(pool)
    .await?;

    Ok(())
}

/// 删除本地任务定义（保留历史）。
pub async fn delete_task(pool: &DbPool, controller_task_id: i64) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM scheduled_tasks WHERE controller_task_id = ?")
        .bind(controller_task_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// 查询单个任务。
pub async fn get_task(
    pool: &DbPool,
    controller_task_id: i64,
) -> sqlx::Result<Option<AgentScheduledTask>> {
    sqlx::query_as::<_, AgentScheduledTask>(
        r#"
        SELECT
            controller_task_id,
            revision,
            name,
            command,
            cron_expr,
            enabled,
            timeout_secs,
            no_overlap,
            last_run_at,
            next_run_at,
            created_at,
            updated_at
        FROM scheduled_tasks
        WHERE controller_task_id = ?
        "#,
    )
    .bind(controller_task_id)
    .fetch_optional(pool)
    .await
}

/// 查询到期任务（enabled=1 AND next_run_at <= now）。
pub async fn list_due_tasks(pool: &DbPool, now: i64) -> sqlx::Result<Vec<AgentScheduledTask>> {
    sqlx::query_as::<_, AgentScheduledTask>(
        r#"
        SELECT
            controller_task_id,
            revision,
            name,
            command,
            cron_expr,
            enabled,
            timeout_secs,
            no_overlap,
            last_run_at,
            next_run_at,
            created_at,
            updated_at
        FROM scheduled_tasks
        WHERE enabled = 1
          AND next_run_at IS NOT NULL
          AND next_run_at <= ?
        ORDER BY next_run_at ASC, controller_task_id ASC
        "#,
    )
    .bind(now)
    .fetch_all(pool)
    .await
}

/// 批量返回所有任务的 controller_task_id、next_run_at、last_run_at、last_status。
pub async fn list_all_task_status(pool: &DbPool) -> sqlx::Result<Vec<AgentTaskStatus>> {
    sqlx::query_as::<_, AgentTaskStatus>(
        r#"
        SELECT
            t.controller_task_id,
            t.next_run_at,
            t.last_run_at,
            (
                SELECT r.status
                FROM task_runs r
                WHERE r.controller_task_id = t.controller_task_id
                ORDER BY r.created_at DESC, r.run_id DESC
                LIMIT 1
            ) AS last_status
        FROM scheduled_tasks t
        "#,
    )
    .fetch_all(pool)
    .await
}

/// 更新执行时间。
pub async fn update_task_run_times(
    pool: &DbPool,
    controller_task_id: i64,
    last_run_at: i64,
    next_run_at: Option<i64>,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        UPDATE scheduled_tasks
        SET last_run_at = ?, next_run_at = ?
        WHERE controller_task_id = ?
        "#,
    )
    .bind(last_run_at)
    .bind(next_run_at)
    .bind(controller_task_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// 创建执行记录。
pub async fn create_task_run(pool: &DbPool, payload: &NewAgentTaskRun) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO task_runs (
            run_id,
            controller_task_id,
            triggered_at,
            started_at,
            finished_at,
            status,
            exit_code,
            stdout,
            stderr,
            error_message,
            trigger_source
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        "#,
    )
    .bind(&payload.run_id)
    .bind(payload.controller_task_id)
    .bind(&payload.triggered_at)
    .bind(&payload.started_at)
    .bind(&payload.finished_at)
    .bind(&payload.status)
    .bind(payload.exit_code)
    .bind(&payload.stdout)
    .bind(&payload.stderr)
    .bind(&payload.error_message)
    .bind(&payload.trigger_source)
    .execute(pool)
    .await?;
    Ok(())
}

/// 更新执行记录。
pub async fn update_task_run(
    pool: &DbPool,
    run_id: &str,
    payload: &UpdateAgentTaskRun,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        UPDATE task_runs
        SET
            started_at = ?,
            finished_at = ?,
            status = ?,
            exit_code = ?,
            stdout = ?,
            stderr = ?,
            error_message = ?
        WHERE run_id = ?
        "#,
    )
    .bind(&payload.started_at)
    .bind(&payload.finished_at)
    .bind(&payload.status)
    .bind(payload.exit_code)
    .bind(&payload.stdout)
    .bind(&payload.stderr)
    .bind(&payload.error_message)
    .bind(run_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// 查询执行历史。
pub async fn list_task_runs(
    pool: &DbPool,
    controller_task_id: i64,
    limit: i64,
) -> sqlx::Result<Vec<AgentTaskRun>> {
    let limit = limit.clamp(1, 500);
    sqlx::query_as::<_, AgentTaskRun>(
        r#"
        SELECT
            run_id,
            controller_task_id,
            triggered_at,
            started_at,
            finished_at,
            status,
            exit_code,
            stdout,
            stderr,
            error_message,
            trigger_source,
            created_at
        FROM task_runs
        WHERE controller_task_id = ?
        ORDER BY created_at DESC, run_id DESC
        LIMIT ?
        "#,
    )
    .bind(controller_task_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// 清理超出 500 条上限的历史记录。
pub async fn cleanup_old_runs(
    pool: &DbPool,
    controller_task_id: i64,
    keep: i64,
) -> sqlx::Result<u64> {
    let result = sqlx::query(
        r#"
        DELETE FROM task_runs
        WHERE controller_task_id = ?1
          AND run_id NOT IN (
              SELECT run_id FROM task_runs
              WHERE controller_task_id = ?1
              ORDER BY created_at DESC, run_id DESC
              LIMIT ?2
          )
        "#,
    )
    .bind(controller_task_id)
    .bind(keep)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

// --- Cron 辅助函数 ---

pub fn validate_cron_expr(cron_expr: &str) -> ApiResult<()> {
    let minute_expr = normalize_minute_cron_expr(cron_expr)?;
    let schedule_expr = format!("0 {minute_expr}");
    Schedule::from_str(&schedule_expr)
        .map(|_| ())
        .map_err(|err| ApiError::BadRequest(format!("invalid cron expression: {err}")))
}

pub fn compute_next_run_at(cron_expr: &str, from_ts: i64) -> ApiResult<Option<i64>> {
    let minute_expr = normalize_minute_cron_expr(cron_expr)?;
    let schedule_expr = format!("0 {minute_expr}");
    let schedule = Schedule::from_str(&schedule_expr)
        .map_err(|err| ApiError::BadRequest(format!("invalid cron expression: {err}")))?;
    let base = Utc
        .timestamp_opt(from_ts, 0)
        .single()
        .unwrap_or_else(Utc::now);
    Ok(schedule.after(&base).next().map(|next| next.timestamp()))
}

fn normalize_minute_cron_expr(cron_expr: &str) -> ApiResult<String> {
    let parts: Vec<&str> = cron_expr.split_whitespace().collect();
    match parts.as_slice() {
        [minute, hour, day, month, weekday] => {
            Ok(format!("{minute} {hour} {day} {month} {weekday}"))
        }
        [second, minute, hour, day, month, weekday] if *second == "0" => {
            Ok(format!("{minute} {hour} {day} {month} {weekday}"))
        }
        [_, ..] if parts.len() == 6 => Err(ApiError::BadRequest(
            "only minute-level cron expressions are supported; seconds-level triggers are not supported".to_string(),
        )),
        _ => Err(ApiError::BadRequest(
            "cron expression must contain 5 fields: minute hour day month weekday".to_string(),
        )),
    }
}

// --- 上报与日志合并辅助函数 ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRunReportPayload {
    pub run_id: String,
    pub controller_task_id: i64,
    pub triggered_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub status: String,
    pub exit_code: Option<i64>,
    pub log_excerpt: Option<String>,
    pub error_message: Option<String>,
    pub trigger_source: String,
}

#[allow(clippy::type_complexity)]
static TASK_RUN_CHANNEL: once_cell::sync::Lazy<(
    tokio::sync::mpsc::Sender<TaskRunReportPayload>,
    tokio::sync::Mutex<Option<tokio::sync::mpsc::Receiver<TaskRunReportPayload>>>,
)> = once_cell::sync::Lazy::new(|| {
    let (tx, rx) = tokio::sync::mpsc::channel(1000);
    (tx, tokio::sync::Mutex::new(Some(rx)))
});

pub fn get_task_run_reporter() -> tokio::sync::mpsc::Sender<TaskRunReportPayload> {
    TASK_RUN_CHANNEL.0.clone()
}

pub async fn take_task_run_receiver() -> Option<tokio::sync::mpsc::Receiver<TaskRunReportPayload>> {
    TASK_RUN_CHANNEL.1.lock().await.take()
}

pub fn merge_log_excerpt(stdout: &str, stderr: &str) -> String {
    let stdout_text = stdout.trim();
    let stderr_text = stderr.trim();
    let merged = if stdout_text.is_empty() && stderr_text.is_empty() {
        String::new()
    } else if stderr_text.is_empty() {
        stdout_text.to_string()
    } else if stdout_text.is_empty() {
        format!("[stderr]\n{stderr_text}")
    } else {
        format!("[stdout]\n{stdout_text}\n\n[stderr]\n{stderr_text}")
    };
    truncate_chars(&merged, 2000)
}

pub fn truncate_chars(input: &str, limit: usize) -> String {
    if input.chars().count() <= limit {
        return input.to_string();
    }
    let mut text: String = input.chars().take(limit).collect();
    text.push_str("...");
    text
}

/// 获取本地所有的计划任务定义
pub async fn list_all_tasks(pool: &DbPool) -> sqlx::Result<Vec<AgentScheduledTask>> {
    sqlx::query_as::<_, AgentScheduledTask>(
        r#"
        SELECT
            controller_task_id,
            revision,
            name,
            command,
            cron_expr,
            enabled,
            timeout_secs,
            no_overlap,
            last_run_at,
            next_run_at,
            created_at,
            updated_at
        FROM scheduled_tasks
        "#,
    )
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::setup_test_db;

    #[tokio::test]
    async fn test_scheduled_task_crud() {
        let pool = setup_test_db().await;

        let payload = UpsertTaskPayload {
            controller_task_id: 42,
            revision: 1,
            name: "test task".to_string(),
            command: "echo 123".to_string(),
            cron_expr: "*/5 * * * *".to_string(),
            enabled: true,
            timeout_secs: 30,
            no_overlap: true,
            force: None,
        };

        // Test upsert
        upsert_task(&pool, &payload, Some(1000)).await.unwrap();

        // Test get
        let task = get_task(&pool, 42).await.unwrap().unwrap();
        assert_eq!(task.name, "test task");
        assert_eq!(task.revision, 1);
        assert_eq!(task.next_run_at, Some(1000));

        // Test list due
        let due = list_due_tasks(&pool, 1000).await.unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].controller_task_id, 42);

        let due_empty = list_due_tasks(&pool, 999).await.unwrap();
        assert!(due_empty.is_empty());

        // Test update run times
        update_task_run_times(&pool, 42, 1000, Some(2000))
            .await
            .unwrap();
        let updated_task = get_task(&pool, 42).await.unwrap().unwrap();
        assert_eq!(updated_task.last_run_at, Some(1000));
        assert_eq!(updated_task.next_run_at, Some(2000));

        // Test status list
        let status_list = list_all_task_status(&pool).await.unwrap();
        assert_eq!(status_list.len(), 1);
        assert_eq!(status_list[0].controller_task_id, 42);
        assert_eq!(status_list[0].next_run_at, Some(2000));
        assert!(status_list[0].last_status.is_none());

        // Test task runs
        let run_payload = NewAgentTaskRun {
            run_id: "run-uuid-1".to_string(),
            controller_task_id: 42,
            triggered_at: "2026-06-24T16:00:00Z".to_string(),
            started_at: Some("2026-06-24T16:00:01Z".to_string()),
            finished_at: None,
            status: "running".to_string(),
            exit_code: None,
            stdout: None,
            stderr: None,
            error_message: None,
            trigger_source: "cron".to_string(),
        };

        create_task_run(&pool, &run_payload).await.unwrap();

        let status_list2 = list_all_task_status(&pool).await.unwrap();
        assert_eq!(status_list2[0].last_status.as_deref(), Some("running"));

        let update_run_payload = UpdateAgentTaskRun {
            started_at: Some("2026-06-24T16:00:01Z".to_string()),
            finished_at: Some("2026-06-24T16:00:05Z".to_string()),
            status: "success".to_string(),
            exit_code: Some(0),
            stdout: Some("hello".to_string()),
            stderr: Some("".to_string()),
            error_message: None,
        };

        update_task_run(&pool, "run-uuid-1", &update_run_payload)
            .await
            .unwrap();

        let runs = list_task_runs(&pool, 42, 10).await.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, "success");
        assert_eq!(runs[0].exit_code, Some(0));
        assert_eq!(runs[0].stdout.as_deref(), Some("hello"));

        // Test cleanup_old_runs
        // Create 3 more runs
        for i in 2..=4 {
            let rp = NewAgentTaskRun {
                run_id: format!("run-uuid-{}", i),
                controller_task_id: 42,
                triggered_at: format!("2026-06-24T16:00:0{}Z", i),
                started_at: Some(format!("2026-06-24T16:00:0{}Z", i)),
                finished_at: None,
                status: "success".to_string(),
                exit_code: Some(0),
                stdout: None,
                stderr: None,
                error_message: None,
                trigger_source: "cron".to_string(),
            };
            create_task_run(&pool, &rp).await.unwrap();
        }

        let runs_before = list_task_runs(&pool, 42, 10).await.unwrap();
        assert_eq!(runs_before.len(), 4);

        cleanup_old_runs(&pool, 42, 2).await.unwrap();

        let runs_after = list_task_runs(&pool, 42, 10).await.unwrap();
        assert_eq!(runs_after.len(), 2);
        // The kept runs should be the latest ones (run-uuid-4 and run-uuid-3)
        assert_eq!(runs_after[0].run_id, "run-uuid-4");
        assert_eq!(runs_after[1].run_id, "run-uuid-3");

        // Test delete
        delete_task(&pool, 42).await.unwrap();
        assert!(get_task(&pool, 42).await.unwrap().is_none());
    }

    #[test]
    fn test_cron_helpers() {
        assert!(validate_cron_expr("*/5 * * * *").is_ok());
        assert!(validate_cron_expr("0 0 1 1 *").is_ok());
        assert!(validate_cron_expr("invalid").is_err());
        assert!(validate_cron_expr("0 */5 * * * *").is_ok()); // with leading 0 second

        // UTC next run calculation
        // 2026-06-24T16:59:05Z is timestamp 1782310745
        // cron "*/5 * * * *" starting from 1782310745 should trigger at 1782310800 (17:00:00)
        let next = compute_next_run_at("*/5 * * * *", 1782310745).unwrap();
        assert_eq!(next, Some(1782310800));
    }
}
