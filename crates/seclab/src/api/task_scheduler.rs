//! 计划任务 API：多节点任务定义、触发与执行历史。

use crate::models::task_scheduler::{
    NewTask, UpdateTask, delete_task, get_task_by_id, increment_revision, list_task_runs,
    list_tasks, set_task_enabled, update_task,
};
use crate::services::task_scheduler::{compute_next_run_at, validate_cron_expr};
use crate::state::AppState;
use crate::types::{ApiError, ApiResponse, ApiResult};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use chrono::Utc;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksQuery {
    pub agent_id: Option<String>,
    pub node_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertTaskPayload {
    pub name: String,
    pub agent_id: Option<String>,
    pub node_id: Option<String>,
    pub command: String,
    pub cron_expr: String,
    pub enabled: Option<bool>,
    pub timeout_secs: Option<i64>,
    pub no_overlap: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToggleTaskPayload {
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRunsQuery {
    pub limit: Option<i64>,
}

pub fn task_scheduler_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", put(update).delete(remove))
        .route("/{id}/toggle", post(toggle))
        .route("/{id}/sync", post(sync))
        .route("/{id}/run", post(run_once))
        .route("/{id}/runs", get(list_runs))
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListTasksQuery>,
) -> ApiResult<Response> {
    let node_id = query.node_id.as_deref().or(query.agent_id.as_deref());
    let mut tasks = list_tasks(&state.metadata_db, node_id).await?;

    // 批量调用 fetch_tasks_status_from_agent 合并 Agent 侧实际的运行状态和 next_run_at
    let agent_ids: std::collections::HashSet<String> =
        tasks.iter().map(|t| t.agent_id.clone()).collect();
    for agent_id in agent_ids {
        if let Ok(agent_statuses) =
            crate::services::task_sync::fetch_tasks_status_from_agent(&state, &agent_id).await
        {
            let status_map: std::collections::HashMap<
                i64,
                crate::services::task_sync::AgentTaskStatusDto,
            > = agent_statuses
                .into_iter()
                .map(|s| (s.controller_task_id, s))
                .collect();

            for task in tasks.iter_mut() {
                if task.agent_id == agent_id
                    && let Some(agent_status) = status_map.get(&task.id)
                {
                    task.next_run_at = agent_status.next_run_at;
                    task.last_run_at = agent_status.last_run_at;
                    task.last_status = agent_status.last_status.clone();
                }
            }
        }
    }

    Ok(ApiResponse::success_with_raw("Scheduled tasks loaded", Some(tasks)).into_response())
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UpsertTaskPayload>,
) -> ApiResult<Response> {
    validate_create_or_update_payload(&payload)?;

    let enabled = payload.enabled.unwrap_or(true);
    let timeout_secs = payload.timeout_secs.unwrap_or(60).clamp(1, 86_400);
    let no_overlap = payload.no_overlap.unwrap_or(true);
    let next_run_at = if enabled {
        compute_next_run_at(&payload.cron_expr, Utc::now().timestamp())?
    } else {
        None
    };

    let agent_id = resolve_node_id(&payload)?.to_string();

    let task_id = crate::models::task_scheduler::create_task(
        &state.metadata_db,
        &NewTask {
            name: payload.name.trim().to_string(),
            agent_id: agent_id.clone(),
            command: payload.command.trim().to_string(),
            cron_expr: payload.cron_expr.trim().to_string(),
            enabled,
            timeout_secs,
            no_overlap,
            next_run_at,
        },
    )
    .await?;

    let task = get_task_by_id(&state.metadata_db, task_id)
        .await?
        .ok_or(ApiError::NotFound)?;

    // 写入同步队列并触发
    let _ = crate::models::task_scheduler::queue_sync_op(
        &state.metadata_db,
        task_id,
        &task.agent_id,
        "upsert",
        task.revision,
    )
    .await;
    crate::services::task_sync::trigger_sync_queue_check();

    // 重新获取同步后带有 sync_status 的最新定时任务记录
    let latest_task = get_task_by_id(&state.metadata_db, task_id)
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(ApiResponse::success_with_raw("Scheduled task created", Some(latest_task)).into_response())
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<UpsertTaskPayload>,
) -> ApiResult<Response> {
    validate_create_or_update_payload(&payload)?;

    let old_task = get_task_by_id(&state.metadata_db, id)
        .await?
        .ok_or(ApiError::NotFound)?;

    let enabled = payload.enabled.unwrap_or(true);
    let timeout_secs = payload.timeout_secs.unwrap_or(60).clamp(1, 86_400);
    let no_overlap = payload.no_overlap.unwrap_or(true);
    let next_run_at = if enabled {
        compute_next_run_at(&payload.cron_expr, Utc::now().timestamp())?
    } else {
        None
    };

    let target_agent_id = resolve_node_id(&payload)?.to_string();

    let updated = update_task(
        &state.metadata_db,
        id,
        &UpdateTask {
            name: payload.name.trim().to_string(),
            agent_id: target_agent_id.clone(),
            command: payload.command.trim().to_string(),
            cron_expr: payload.cron_expr.trim().to_string(),
            enabled,
            timeout_secs,
            no_overlap,
            next_run_at,
        },
    )
    .await?;

    if !updated {
        return Err(ApiError::NotFound);
    }

    // 递增 revision 修订版本
    let _ = increment_revision(&state.metadata_db, id).await?;

    let latest_task = get_task_by_id(&state.metadata_db, id)
        .await?
        .ok_or(ApiError::NotFound)?;

    // 节点发生变化时，先旧 Agent 标记删除
    if old_task.agent_id != latest_task.agent_id {
        let _ = crate::models::task_scheduler::queue_sync_op(
            &state.metadata_db,
            id,
            &old_task.agent_id,
            "delete",
            latest_task.revision,
        )
        .await;
    }

    // 同步写入队列到最新指定的 Agent 上
    let _ = crate::models::task_scheduler::queue_sync_op(
        &state.metadata_db,
        id,
        &latest_task.agent_id,
        "upsert",
        latest_task.revision,
    )
    .await;
    crate::services::task_sync::trigger_sync_queue_check();

    // 重新获取带有最新 sync_status 的记录返回
    let final_task = get_task_by_id(&state.metadata_db, id)
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(ApiResponse::success_with_raw("Scheduled task updated", Some(final_task)).into_response())
}

pub async fn remove(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<Response> {
    let task = get_task_by_id(&state.metadata_db, id)
        .await?
        .ok_or(ApiError::NotFound)?;

    // 写入删除操作到队列
    let _ = crate::models::task_scheduler::queue_sync_op(
        &state.metadata_db,
        id,
        &task.agent_id,
        "delete",
        task.revision,
    )
    .await;
    crate::services::task_sync::trigger_sync_queue_check();

    // 然后删除主控控制面本地定义
    let deleted = delete_task(&state.metadata_db, id).await?;
    if !deleted {
        return Err(ApiError::NotFound);
    }
    Ok(ApiResponse::ok("Scheduled task deleted").into_response())
}

pub async fn toggle(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<ToggleTaskPayload>,
) -> ApiResult<Response> {
    let task = get_task_by_id(&state.metadata_db, id)
        .await?
        .ok_or(ApiError::NotFound)?;

    let next_run_at = if payload.enabled {
        compute_next_run_at(&task.cron_expr, Utc::now().timestamp())?
    } else {
        None
    };

    let updated = set_task_enabled(&state.metadata_db, id, payload.enabled, next_run_at).await?;
    if !updated {
        return Err(ApiError::NotFound);
    }

    // 递增修订号
    let _ = increment_revision(&state.metadata_db, id).await?;

    let latest = get_task_by_id(&state.metadata_db, id)
        .await?
        .ok_or(ApiError::NotFound)?;

    // 写入同步队列启用/禁用状态并触发
    let _ = crate::models::task_scheduler::queue_sync_op(
        &state.metadata_db,
        id,
        &latest.agent_id,
        "upsert",
        latest.revision,
    )
    .await;
    crate::services::task_sync::trigger_sync_queue_check();

    let final_latest = get_task_by_id(&state.metadata_db, id)
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(
        ApiResponse::success_with_raw("Scheduled task enabled state updated", Some(final_latest))
            .into_response(),
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncParams {
    pub force: Option<bool>,
}

pub async fn sync(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Query(params): Query<SyncParams>,
) -> ApiResult<Response> {
    let task = get_task_by_id(&state.metadata_db, id)
        .await?
        .ok_or(ApiError::NotFound)?;

    let force = params.force.unwrap_or(false);
    if force {
        crate::services::task_sync::sync_task_to_agent(&state, &task, true)
            .await
            .map_err(|err| ApiError::internal(format!("failed to sync task to agent: {err}")))?;
    } else {
        crate::models::task_scheduler::queue_sync_op(
            &state.metadata_db,
            id,
            &task.agent_id,
            "upsert",
            task.revision,
        )
        .await?;
        crate::services::task_sync::trigger_sync_queue_check();
    }

    let final_task = get_task_by_id(&state.metadata_db, id)
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(
        ApiResponse::success_with_raw("Scheduled task synchronized", Some(final_task))
            .into_response(),
    )
}

pub async fn run_once(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<Response> {
    let task = get_task_by_id(&state.metadata_db, id)
        .await?
        .ok_or(ApiError::NotFound)?;

    // 调用 Agent 执行并同步返回结果
    let result = crate::services::task_sync::run_task_on_agent(&state, &task)
        .await
        .map_err(|err| {
            ApiError::internal(format!("failed to proxy task execution to agent: {err}"))
        })?;

    Ok(ApiResponse::success_with_raw("Task executed", Some(result)).into_response())
}

pub async fn list_runs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Query(query): Query<TaskRunsQuery>,
) -> ApiResult<Response> {
    let task = get_task_by_id(&state.metadata_db, id)
        .await?
        .ok_or(ApiError::NotFound)?;

    let limit = query.limit.unwrap_or(50).clamp(1, 200);

    // 优先代理查询 Agent，Agent 不可达时返回主控本地缓存
    let runs =
        match crate::services::task_sync::fetch_task_runs_from_agent(&state, &task, limit).await {
            Ok(agent_runs) => {
                let mapped: Vec<serde_json::Value> = agent_runs
                    .into_iter()
                    .enumerate()
                    .map(|(idx, run)| {
                        let triggered_at_ts = parse_iso_time(&run.triggered_at).unwrap_or(0);
                        let started_at_ts = run.started_at.as_deref().and_then(parse_iso_time);
                        let finished_at_ts = run.finished_at.as_deref().and_then(parse_iso_time);
                        serde_json::json!({
                            "id": idx as i64 + 1,
                            "taskId": task.id,
                            "agentId": task.agent_id.clone(),
                            "triggeredAt": triggered_at_ts,
                            "startedAt": started_at_ts,
                            "finishedAt": finished_at_ts,
                            "status": run.status,
                            "exitCode": run.exit_code,
                            "logExcerpt": run.log_excerpt,
                            "errorMessage": run.error_message,
                            "createdAt": run.created_at,
                        })
                    })
                    .collect();
                mapped
            }
            Err(_) => {
                let local_runs = list_task_runs(&state.metadata_db, id, limit).await?;
                let mapped: Vec<serde_json::Value> = local_runs
                    .into_iter()
                    .map(|run| {
                        serde_json::json!({
                            "id": run.id,
                            "taskId": run.task_id,
                            "agentId": run.agent_id,
                            "triggeredAt": run.triggered_at,
                            "startedAt": run.started_at,
                            "finishedAt": run.finished_at,
                            "status": run.status,
                            "exitCode": run.exit_code,
                            "logExcerpt": run.log_excerpt,
                            "errorMessage": run.error_message,
                            "createdAt": run.created_at,
                        })
                    })
                    .collect();
                mapped
            }
        };

    Ok(ApiResponse::success_with_raw("Task run history loaded", Some(runs)).into_response())
}

fn validate_create_or_update_payload(payload: &UpsertTaskPayload) -> ApiResult<()> {
    if payload.name.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "task name must not be empty".to_string(),
        ));
    }
    if resolve_node_id(payload)?.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "node_id must not be empty".to_string(),
        ));
    }
    if payload.command.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "command must not be empty".to_string(),
        ));
    }
    validate_cron_expr(payload.cron_expr.trim())
}

fn resolve_node_id(payload: &UpsertTaskPayload) -> ApiResult<&str> {
    payload
        .node_id
        .as_deref()
        .or(payload.agent_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::BadRequest("node_id must not be empty".to_string()))
}

fn parse_iso_time(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp())
}
