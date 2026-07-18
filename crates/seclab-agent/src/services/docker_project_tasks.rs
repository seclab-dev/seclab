//! Docker Compose 项目后台任务持久化、互斥、取消与命令执行。

use crate::api::docker::context::DockerOperationContext;
use crate::models::docker::{
    DockerActivityActorKind, DockerProjectTask, DockerProjectTaskOperation, DockerProjectTaskStage,
    DockerProjectTaskStatus,
};
use crate::state::DbPool;
use crate::types::{ApiError, ApiResult};
use once_cell::sync::Lazy;
use seclab_contracts::api::ErrorCode;
use serde_json::json;
use sqlx::Row;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const TASK_RETENTION_SECONDS: i64 = 24 * 60 * 60;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30 * 60);

static CANCELLATIONS: Lazy<Mutex<HashMap<String, CancellationToken>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static CREATE_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

/// 创建任务时允许保存的非敏感参数。
pub struct NewDockerProjectTask<'a> {
    pub project_name: &'a str,
    pub operation: DockerProjectTaskOperation,
    pub service_name: Option<&'a str>,
    pub replicas: Option<usize>,
    pub pull_images: bool,
}

/// Compose 命令执行结果。
pub enum ComposeCommandResult {
    Succeeded,
    Cancelled,
}

/// Agent 启动时关闭不可恢复的旧任务并清理过期记录。
pub async fn initialize(pool: &DbPool) -> ApiResult<()> {
    sqlx::query(
        "UPDATE docker_compose_project_tasks \
         SET status = 'failed', stage = 'interrupted', progress_percent = 100, \
             error_summary = 'Agent restarted before the task completed', finished_at = unixepoch() \
         WHERE status IN ('queued', 'running')",
    )
    .execute(pool)
    .await?;
    cleanup_expired(pool).await?;
    Ok(())
}

/// 每日清理超过保留期的项目任务。
pub fn spawn_retention_worker(pool: DbPool) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(24 * 60 * 60));
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(error) = cleanup_expired(&pool).await {
                tracing::error!(%error, "failed to clean expired Docker project tasks");
            }
        }
    });
}

/// 创建持久化任务，并为同一项目提供进程内互斥。
pub async fn create(
    pool: &DbPool,
    context: &DockerOperationContext,
    request: NewDockerProjectTask<'_>,
) -> ApiResult<DockerProjectTask> {
    let _guard = CREATE_LOCK.lock().await;
    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM docker_compose_project_tasks \
         WHERE project_name = ?1 AND status IN ('queued', 'running')",
    )
    .bind(request.project_name)
    .fetch_one(pool)
    .await?;
    if active > 0 {
        return Err(ApiError::conflict(
            ErrorCode::DockerProjectBusy,
            "Docker project already has an active task",
        ));
    }
    let id = Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO docker_compose_project_tasks (\
            id, project_name, operation, status, stage, progress_percent, \
            service_name, replicas, pull_images, actor_kind, actor_user_id, actor_name, client_ip, trace_id\
         ) VALUES (?1, ?2, ?3, 'queued', 'preparing', 0, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )
    .bind(&id)
    .bind(request.project_name)
    .bind(request.operation.as_str())
    .bind(request.service_name)
    .bind(request.replicas.map(|value| value as i64))
    .bind(request.pull_images)
    .bind(context.actor_kind.as_str())
    .bind(context.actor_user_id)
    .bind(&context.actor_name)
    .bind(&context.client_ip)
    .bind(&context.trace_id)
    .execute(pool)
    .await?;
    context
        .record_success(
            pool,
            "compose.task.submitted",
            Some(("composeProject", request.project_name)),
            json!({
                "taskId": id,
                "name": request.project_name,
                "operation": request.operation.as_str(),
                "service": request.service_name,
                "replicas": request.replicas,
                "pullImages": request.pull_images,
            }),
            false,
        )
        .await;
    let cancellation = CancellationToken::new();
    CANCELLATIONS.lock().await.insert(id.clone(), cancellation);
    get(pool, &id).await
}

/// 返回任务的取消令牌。
pub async fn cancellation(task_id: &str) -> Option<CancellationToken> {
    CANCELLATIONS.lock().await.get(task_id).cloned()
}

/// 将任务推进到运行阶段。
pub async fn update(
    pool: &DbPool,
    task_id: &str,
    stage: DockerProjectTaskStage,
    progress_percent: u8,
) -> ApiResult<()> {
    sqlx::query(
        "UPDATE docker_compose_project_tasks SET status = 'running', stage = ?2, \
         progress_percent = ?3, started_at = COALESCE(started_at, unixepoch()) WHERE id = ?1",
    )
    .bind(task_id)
    .bind(stage_as_str(stage))
    .bind(i64::from(progress_percent))
    .execute(pool)
    .await?;
    Ok(())
}

/// 将任务标记为成功。
pub async fn succeed(pool: &DbPool, task_id: &str) -> ApiResult<()> {
    finish(pool, task_id, "succeeded", "completed", None, None).await
}

/// 将任务标记为成功，并保留不影响业务结果的清理警告。
pub async fn succeed_with_warning(
    pool: &DbPool,
    task_id: &str,
    cleanup_warning: &str,
) -> ApiResult<()> {
    finish(
        pool,
        task_id,
        "succeeded",
        "completed",
        None,
        Some(cleanup_warning),
    )
    .await
}

/// 将任务标记为失败，并保存截断后的安全摘要。
pub async fn fail(
    pool: &DbPool,
    task_id: &str,
    error: &str,
    cleanup_warning: Option<&str>,
) -> ApiResult<()> {
    finish(
        pool,
        task_id,
        "failed",
        "completed",
        Some(error),
        cleanup_warning,
    )
    .await
}

/// 将任务标记为已取消。
pub async fn cancelled(
    pool: &DbPool,
    task_id: &str,
    cleanup_warning: Option<&str>,
) -> ApiResult<()> {
    finish(
        pool,
        task_id,
        "cancelled",
        "cancelled",
        None,
        cleanup_warning,
    )
    .await
}

async fn finish(
    pool: &DbPool,
    task_id: &str,
    status: &str,
    stage: &str,
    error: Option<&str>,
    cleanup_warning: Option<&str>,
) -> ApiResult<()> {
    sqlx::query(
        "UPDATE docker_compose_project_tasks SET status = ?2, stage = ?3, \
         progress_percent = 100, error_summary = ?4, cleanup_warning = ?5, \
         finished_at = unixepoch() WHERE id = ?1",
    )
    .bind(task_id)
    .bind(status)
    .bind(stage)
    .bind(error.map(sanitize_error))
    .bind(cleanup_warning.map(sanitize_error))
    .execute(pool)
    .await?;
    CANCELLATIONS.lock().await.remove(task_id);
    record_terminal_activity(pool, task_id).await;
    Ok(())
}

async fn record_terminal_activity(pool: &DbPool, task_id: &str) {
    let row = match sqlx::query(
        "SELECT project_name, operation, status, service_name, replicas, pull_images, \
         actor_kind, actor_user_id, actor_name, client_ip, trace_id, error_summary \
         FROM docker_compose_project_tasks WHERE id = ?1",
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return,
        Err(error) => {
            tracing::error!(%error, task_id, "failed to load Compose task audit context");
            return;
        }
    };
    let actor_kind = match row.try_get::<String, _>("actor_kind").as_deref() {
        Ok("user") => DockerActivityActorKind::User,
        Ok("system") => DockerActivityActorKind::System,
        _ => return,
    };
    let context = DockerOperationContext {
        actor_kind,
        actor_user_id: row.try_get("actor_user_id").ok().flatten(),
        actor_name: row
            .try_get("actor_name")
            .unwrap_or_else(|_| "unknown".to_string()),
        client_ip: row.try_get("client_ip").ok().flatten(),
        trace_id: row.try_get("trace_id").ok().flatten(),
    };
    let project_name: String = match row.try_get("project_name") {
        Ok(value) => value,
        Err(_) => return,
    };
    let operation: String = match row.try_get("operation") {
        Ok(value) => value,
        Err(_) => return,
    };
    let status: String = match row.try_get("status") {
        Ok(value) => value,
        Err(_) => return,
    };
    let params = json!({
        "taskId": task_id,
        "name": project_name,
        "operation": operation,
        "service": row.try_get::<Option<String>, _>("service_name").ok().flatten(),
        "replicas": row.try_get::<Option<i64>, _>("replicas").ok().flatten(),
        "pullImages": row.try_get::<bool, _>("pull_images").unwrap_or(false),
    });
    let event_code = format!("compose.{operation}.{status}");
    if status == "failed" {
        let error = row
            .try_get::<Option<String>, _>("error_summary")
            .ok()
            .flatten()
            .unwrap_or_else(|| "Docker project task failed".to_string());
        context
            .record_failure(
                pool,
                &event_code,
                Some(("composeProject", &project_name)),
                params,
                error,
            )
            .await;
    } else {
        context
            .record_success(
                pool,
                &event_code,
                Some(("composeProject", &project_name)),
                params,
                operation == "redeploy" || operation == "remove" || status == "cancelled",
            )
            .await;
    }
}

/// 查询一个任务。
pub async fn get(pool: &DbPool, task_id: &str) -> ApiResult<DockerProjectTask> {
    let row = sqlx::query(
        "SELECT id, project_name, operation, status, stage, progress_percent, service_name, \
         replicas, pull_images, error_summary, cleanup_warning, created_at, started_at, finished_at \
         FROM docker_compose_project_tasks WHERE id = ?1",
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found(ErrorCode::TaskNotFound, "Docker project task not found"))?;
    row_to_task(&row)
}

/// 返回最近提交且仍在执行的创建或重新部署操作。
pub async fn latest_active_deployment(pool: &DbPool) -> ApiResult<Option<DockerProjectTask>> {
    let row = sqlx::query(
        "SELECT id, project_name, operation, status, stage, progress_percent, service_name, \
         replicas, pull_images, error_summary, cleanup_warning, created_at, started_at, finished_at \
         FROM docker_compose_project_tasks \
         WHERE status IN ('queued', 'running') AND operation IN ('create', 'redeploy') \
         ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(row_to_task).transpose()
}

/// 执行可取消、可超时且不会记录完整命令输出的 Compose 命令。
pub async fn run_compose_command(
    project: &str,
    compose_file: &Path,
    args: &[String],
    cancellation: &CancellationToken,
) -> ApiResult<ComposeCommandResult> {
    let mut command = Command::new("docker");
    command
        .args(["compose", "-f"])
        .arg(compose_file)
        .args(["-p", project])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let future = command.output();
    tokio::pin!(future);
    let output = tokio::select! {
        _ = cancellation.cancelled() => return Ok(ComposeCommandResult::Cancelled),
        result = tokio::time::timeout(COMMAND_TIMEOUT, &mut future) => {
            result.map_err(|_| ApiError::validation("Docker Compose command timed out"))??
        }
    };
    if !output.status.success() {
        return Err(ApiError::validation(sanitize_error(
            &String::from_utf8_lossy(&output.stderr),
        )));
    }
    Ok(ComposeCommandResult::Succeeded)
}

/// 清理超过保留期的终态任务。
pub async fn cleanup_expired(pool: &DbPool) -> ApiResult<()> {
    sqlx::query(
        "DELETE FROM docker_compose_project_tasks WHERE status IN ('succeeded', 'failed', 'cancelled') \
         AND created_at < unixepoch() - ?1",
    )
    .bind(TASK_RETENTION_SECONDS)
    .execute(pool)
    .await?;
    Ok(())
}

fn row_to_task(row: &sqlx::sqlite::SqliteRow) -> ApiResult<DockerProjectTask> {
    let operation: String = row.try_get("operation")?;
    let status: String = row.try_get("status")?;
    let stage: String = row.try_get("stage")?;
    Ok(DockerProjectTask {
        id: row.try_get("id")?,
        project_name: row.try_get("project_name")?,
        operation: parse_operation(&operation)?,
        status: parse_status(&status)?,
        stage: parse_stage(&stage)?,
        progress_percent: row.try_get::<i64, _>("progress_percent")? as u8,
        service_name: row.try_get("service_name")?,
        replicas: row
            .try_get::<Option<i64>, _>("replicas")?
            .map(|value| value as usize),
        pull_images: row.try_get("pull_images")?,
        error_summary: row.try_get("error_summary")?,
        cleanup_warning: row.try_get("cleanup_warning")?,
        created_at: row.try_get("created_at")?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
    })
}

fn parse_operation(value: &str) -> ApiResult<DockerProjectTaskOperation> {
    match value {
        "create" => Ok(DockerProjectTaskOperation::Create),
        "start" => Ok(DockerProjectTaskOperation::Start),
        "stop" => Ok(DockerProjectTaskOperation::Stop),
        "restart" => Ok(DockerProjectTaskOperation::Restart),
        "redeploy" => Ok(DockerProjectTaskOperation::Redeploy),
        "scale" => Ok(DockerProjectTaskOperation::Scale),
        "remove" => Ok(DockerProjectTaskOperation::Remove),
        _ => Err(ApiError::internal("invalid Docker project task operation")),
    }
}

fn parse_status(value: &str) -> ApiResult<DockerProjectTaskStatus> {
    match value {
        "queued" => Ok(DockerProjectTaskStatus::Queued),
        "running" => Ok(DockerProjectTaskStatus::Running),
        "succeeded" => Ok(DockerProjectTaskStatus::Succeeded),
        "failed" => Ok(DockerProjectTaskStatus::Failed),
        "cancelled" => Ok(DockerProjectTaskStatus::Cancelled),
        _ => Err(ApiError::internal("invalid Docker project task status")),
    }
}

fn parse_stage(value: &str) -> ApiResult<DockerProjectTaskStage> {
    match value {
        "validating" => Ok(DockerProjectTaskStage::Validating),
        "preparing" => Ok(DockerProjectTaskStage::Preparing),
        "pulling" => Ok(DockerProjectTaskStage::Pulling),
        "applying" => Ok(DockerProjectTaskStage::Applying),
        "verifying" => Ok(DockerProjectTaskStage::Verifying),
        "rolling_back" => Ok(DockerProjectTaskStage::RollingBack),
        "cleaning_up" => Ok(DockerProjectTaskStage::CleaningUp),
        "completed" => Ok(DockerProjectTaskStage::Completed),
        "cancelled" => Ok(DockerProjectTaskStage::Cancelled),
        "interrupted" => Ok(DockerProjectTaskStage::Interrupted),
        _ => Err(ApiError::internal("invalid Docker project task stage")),
    }
}

const fn stage_as_str(stage: DockerProjectTaskStage) -> &'static str {
    match stage {
        DockerProjectTaskStage::Validating => "validating",
        DockerProjectTaskStage::Preparing => "preparing",
        DockerProjectTaskStage::Pulling => "pulling",
        DockerProjectTaskStage::Applying => "applying",
        DockerProjectTaskStage::Verifying => "verifying",
        DockerProjectTaskStage::RollingBack => "rolling_back",
        DockerProjectTaskStage::CleaningUp => "cleaning_up",
        DockerProjectTaskStage::Completed => "completed",
        DockerProjectTaskStage::Cancelled => "cancelled",
        DockerProjectTaskStage::Interrupted => "interrupted",
    }
}

fn sanitize_error(value: &str) -> String {
    let mut sanitized = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(1000)
        .collect::<String>();
    for key in ["password", "token", "authorization"] {
        sanitized = redact_assignment(&sanitized, key);
    }
    sanitized.chars().take(1000).collect()
}

fn redact_assignment(value: &str, key: &str) -> String {
    let mut output = value.to_string();
    let mut search_from = 0;
    loop {
        let lower = output.to_ascii_lowercase();
        let Some(relative) = lower[search_from..].find(key) else {
            break;
        };
        let start = search_from + relative + key.len();
        let Some(separator) = lower[start..].find(['=', ':']) else {
            break;
        };
        let value_start = start + separator + 1;
        let value_end = lower[value_start..]
            .find([',', '&', ' '])
            .map(|offset| value_start + offset)
            .unwrap_or(output.len());
        output.replace_range(value_start..value_end, "[REDACTED]");
        search_from = value_start + "[REDACTED]".len();
        if search_from >= output.len() {
            break;
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_and_limits_command_errors() {
        let source = format!("  token=secret password:guess\n{}", "x".repeat(2000));
        let result = sanitize_error(&source);
        assert!(!result.contains('\n'));
        assert!(!result.contains("secret"));
        assert!(!result.contains("guess"));
        assert_eq!(result.chars().count(), 1000);
    }
}
