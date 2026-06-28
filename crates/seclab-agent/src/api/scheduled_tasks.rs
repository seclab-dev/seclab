//! Agent 本地定时任务 API 处理器。

use crate::models::scheduled_tasks::{
    self, AgentScheduledTask, AgentTaskStatus, NewAgentTaskRun, UpdateAgentTaskRun,
    UpsertTaskPayload,
};
use crate::state::AppState;
use crate::types::{ApiError, ApiResponse, ApiResult};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{delete, get, post, put},
};
use chrono::Utc;
use seclab_contracts::api::ErrorCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::process::Command;

#[derive(Debug, Deserialize)]
pub struct ListRunsParams {
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRunDto {
    pub run_id: String,
    pub controller_task_id: i64,
    pub triggered_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub status: String,
    pub exit_code: Option<i64>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub log_excerpt: Option<String>,
    pub error_message: Option<String>,
    pub trigger_source: String,
    pub created_at: String,
}

pub fn scheduled_task_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/status", get(list_all_status))
        .route("/{controller_task_id}", put(upsert))
        .route("/{controller_task_id}", delete(remove))
        .route("/{controller_task_id}/run", post(run_now))
        .route("/{controller_task_id}/runs", get(get_runs))
        .route("/{controller_task_id}/status", get(get_status))
}

/// 增加/更新定时任务
pub async fn upsert(
    State(state): State<Arc<AppState>>,
    Path(controller_task_id): Path<i64>,
    Json(payload): Json<UpsertTaskPayload>,
) -> ApiResult<Json<ApiResponse<()>>> {
    if payload.controller_task_id != controller_task_id {
        return Err(ApiError::bad_request(
            ErrorCode::BadRequest,
            "controller_task_id mismatch".to_string(),
        ));
    }

    if let Some(existing) = scheduled_tasks::get_task(&state.metadata_db, controller_task_id)
        .await
        .map_err(|err| ApiError::internal(format!("failed to check existing task: {err}")))?
        && payload.revision < existing.revision
        && !payload.force.unwrap_or(false)
    {
        return Err(ApiError::conflict(
            ErrorCode::TaskRevisionConflict,
            format!(
                "Task revision conflict: controller revision ({}) is less than existing local revision ({})",
                payload.revision, existing.revision
            ),
        ));
    }

    let next_run_at = if payload.enabled {
        match scheduled_tasks::compute_next_run_at(&payload.cron_expr, Utc::now().timestamp()) {
            Ok(next) => next,
            Err(err) => return Err(err),
        }
    } else {
        None
    };

    scheduled_tasks::upsert_task(&state.metadata_db, &payload, next_run_at)
        .await
        .map_err(|err| ApiError::internal(format!("failed to upsert task: {err}")))?;

    Ok(Json(ApiResponse::success_with_raw("Task synced", ())))
}

/// 删除定时任务
pub async fn remove(
    State(state): State<Arc<AppState>>,
    Path(controller_task_id): Path<i64>,
) -> ApiResult<Json<ApiResponse<bool>>> {
    let deleted = scheduled_tasks::delete_task(&state.metadata_db, controller_task_id)
        .await
        .map_err(|err| ApiError::internal(format!("failed to delete task: {err}")))?;

    Ok(Json(ApiResponse::success_with_raw("Task deleted", deleted)))
}

/// 立即运行任务并同步返回结果
pub async fn run_now(
    State(state): State<Arc<AppState>>,
    Path(controller_task_id): Path<i64>,
) -> ApiResult<Json<ApiResponse<TaskRunDto>>> {
    let task = scheduled_tasks::get_task(&state.metadata_db, controller_task_id)
        .await
        .map_err(|err| ApiError::internal(format!("failed to query task: {err}")))?
        .ok_or_else(|| ApiError::not_found(ErrorCode::TaskNotFound, "Task not found"))?;

    let run_id = uuid::Uuid::now_v7().to_string();
    let triggered_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);

    let new_run = NewAgentTaskRun {
        run_id: run_id.clone(),
        controller_task_id,
        triggered_at: triggered_at.clone(),
        started_at: Some(triggered_at.clone()),
        finished_at: None,
        status: "running".to_string(),
        exit_code: None,
        stdout: None,
        stderr: None,
        error_message: None,
        trigger_source: "manual".to_string(),
    };

    scheduled_tasks::create_task_run(&state.metadata_db, &new_run)
        .await
        .map_err(|err| ApiError::Internal(format!("failed to create task run: {err}")))?;

    let timeout_secs = task.timeout_secs.clamp(1, 86_400) as u64;

    let output_result = Command::new("/usr/bin/timeout")
        .arg(format!("{}s", timeout_secs))
        .arg("/bin/bash")
        .arg("-lc")
        .arg(&task.command)
        .output()
        .await;

    let update_payload = match output_result {
        Ok(output) => {
            let exit_code = output.status.code().unwrap_or(-1) as i64;
            let timed_out = exit_code == 124;
            let status = if timed_out {
                "timeout".to_string()
            } else if exit_code == 0 {
                "success".to_string()
            } else {
                "failed".to_string()
            };
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            UpdateAgentTaskRun {
                started_at: Some(triggered_at.clone()),
                finished_at: Some(Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)),
                status,
                exit_code: Some(exit_code),
                stdout: Some(stdout),
                stderr: Some(stderr),
                error_message: if timed_out {
                    Some("Task execution timed out".to_string())
                } else {
                    None
                },
            }
        }
        Err(err) => UpdateAgentTaskRun {
            started_at: Some(triggered_at.clone()),
            finished_at: Some(Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)),
            status: "failed".to_string(),
            exit_code: None,
            stdout: None,
            stderr: None,
            error_message: Some(format!("Failed to spawn command: {err}")),
        },
    };

    scheduled_tasks::update_task_run(&state.metadata_db, &run_id, &update_payload)
        .await
        .map_err(|err| ApiError::Internal(format!("failed to update task run: {err}")))?;

    // 无论结果如何，更新本地任务的 last_run_at，保留 next_run_at 不变
    let _ = scheduled_tasks::update_task_run_times(
        &state.metadata_db,
        controller_task_id,
        Utc::now().timestamp(),
        task.next_run_at,
    )
    .await;

    // 清理历史记录，每个任务最多保留最近 500 条
    let _ = scheduled_tasks::cleanup_old_runs(&state.metadata_db, controller_task_id, 500).await;

    let log_excerpt_val = scheduled_tasks::merge_log_excerpt(
        update_payload.stdout.as_deref().unwrap_or(""),
        update_payload.stderr.as_deref().unwrap_or(""),
    );

    let report_payload = scheduled_tasks::TaskRunReportPayload {
        run_id: run_id.clone(),
        controller_task_id,
        triggered_at: triggered_at.clone(),
        started_at: update_payload.started_at.clone(),
        finished_at: update_payload.finished_at.clone(),
        status: update_payload.status.clone(),
        exit_code: update_payload.exit_code,
        log_excerpt: Some(log_excerpt_val.clone()),
        error_message: update_payload.error_message.clone(),
        trigger_source: "manual".to_string(),
    };
    let _ = scheduled_tasks::get_task_run_reporter()
        .send(report_payload)
        .await;

    let updated_run = scheduled_tasks::list_task_runs(&state.metadata_db, controller_task_id, 1)
        .await
        .map_err(|err| ApiError::Internal(format!("failed to query updated task run: {err}")))?
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::Internal("Task run not found after update".to_string()))?;

    let dto = TaskRunDto {
        run_id: updated_run.run_id,
        controller_task_id: updated_run.controller_task_id,
        triggered_at: updated_run.triggered_at,
        started_at: updated_run.started_at,
        finished_at: updated_run.finished_at,
        status: updated_run.status,
        exit_code: updated_run.exit_code,
        stdout: updated_run.stdout,
        stderr: updated_run.stderr,
        log_excerpt: Some(log_excerpt_val),
        error_message: updated_run.error_message,
        trigger_source: updated_run.trigger_source,
        created_at: updated_run.created_at,
    };

    Ok(Json(ApiResponse::success_with_raw(
        "Task run executed",
        dto,
    )))
}

/// 查询执行历史，按需生成 log_excerpt
pub async fn get_runs(
    State(state): State<Arc<AppState>>,
    Path(controller_task_id): Path<i64>,
    Query(params): Query<ListRunsParams>,
) -> ApiResult<Json<ApiResponse<Vec<TaskRunDto>>>> {
    let limit = params.limit.unwrap_or(50).clamp(1, 500);
    let runs = scheduled_tasks::list_task_runs(&state.metadata_db, controller_task_id, limit)
        .await
        .map_err(|err| ApiError::internal(format!("failed to list runs: {err}")))?;

    let dtos = runs
        .into_iter()
        .map(|run| {
            let log_excerpt = scheduled_tasks::merge_log_excerpt(
                run.stdout.as_deref().unwrap_or(""),
                run.stderr.as_deref().unwrap_or(""),
            );
            TaskRunDto {
                run_id: run.run_id,
                controller_task_id: run.controller_task_id,
                triggered_at: run.triggered_at,
                started_at: run.started_at,
                finished_at: run.finished_at,
                status: run.status,
                exit_code: run.exit_code,
                stdout: run.stdout,
                stderr: run.stderr,
                log_excerpt: Some(log_excerpt),
                error_message: run.error_message,
                trigger_source: run.trigger_source,
                created_at: run.created_at,
            }
        })
        .collect();

    Ok(Json(ApiResponse::success_with_raw(
        "Task runs retrieved",
        dtos,
    )))
}

/// 查询单个定时任务状态
pub async fn get_status(
    State(state): State<Arc<AppState>>,
    Path(controller_task_id): Path<i64>,
) -> ApiResult<Json<ApiResponse<AgentScheduledTask>>> {
    let task = scheduled_tasks::get_task(&state.metadata_db, controller_task_id)
        .await
        .map_err(|err| ApiError::internal(format!("failed to query task: {err}")))?
        .ok_or_else(|| ApiError::not_found(ErrorCode::TaskNotFound, "Task not found"))?;

    Ok(Json(ApiResponse::success_with_raw(
        "Task status retrieved",
        task,
    )))
}

/// 批量查询所有定时任务状态
pub async fn list_all_status(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<ApiResponse<Vec<AgentTaskStatus>>>> {
    let list = scheduled_tasks::list_all_task_status(&state.metadata_db)
        .await
        .map_err(|err| ApiError::internal(format!("failed to query task statuses: {err}")))?;

    Ok(Json(ApiResponse::success_with_raw(
        "All task statuses retrieved",
        list,
    )))
}
