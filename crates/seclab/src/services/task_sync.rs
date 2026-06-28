//! 主控到 Agent 定时任务同步与代理查询服务。

use crate::models::node_runtime_client::NodeRuntimeClient;
use crate::models::task_scheduler::ScheduledTask;
use crate::state::AppState;
use chrono::Utc;
use seclab_api::response::ApiResponse;
use serde::{Deserialize, Serialize};

use seclab_contracts::api::ErrorCode;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentUpsertTaskPayload {
    controller_task_id: i64,
    revision: i64,
    name: String,
    command: String,
    cron_expr: String,
    enabled: bool,
    timeout_secs: i64,
    no_overlap: bool,
    force: bool,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AgentTaskDto {
    pub next_run_at: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskRunDto {
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

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskStatusDto {
    pub controller_task_id: i64,
    pub next_run_at: Option<i64>,
    pub last_run_at: Option<i64>,
    pub last_status: Option<String>,
}

/// 同步任务定义到指定的 Agent
pub async fn sync_task_to_agent(
    state: &AppState,
    task: &ScheduledTask,
    force: bool,
) -> anyhow::Result<()> {
    let client_res =
        NodeRuntimeClient::from_node_route(&state.metadata_db, Some(&task.agent_id)).await;
    let client = match client_res {
        Ok(c) => c,
        Err(err) => {
            let err_str = err.to_string();
            let _ = crate::models::task_scheduler::update_sync_status(
                &state.metadata_db,
                task.id,
                "target_offline",
                Some(&err_str),
                None,
                task.next_run_at,
            )
            .await;
            return Err(err.into());
        }
    };

    let payload = AgentUpsertTaskPayload {
        controller_task_id: task.id,
        revision: task.revision,
        name: task.name.clone(),
        command: task.command.clone(),
        cron_expr: task.cron_expr.clone(),
        enabled: task.enabled,
        timeout_secs: task.timeout_secs,
        no_overlap: task.no_overlap,
        force,
    };

    let path = format!("/api/v1/agent/tasks/{}", task.id);
    let sync_res: Result<ApiResponse<()>, _> = client.put_json(&path, &payload).await;

    match sync_res {
        Ok(resp) if resp.success => {
            let now_str = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);

            // 获取同步后 Agent 计算的下一次运行时间
            let status_path = format!("/api/v1/agent/tasks/{}/status", task.id);
            let agent_task: Result<ApiResponse<AgentTaskDto>, _> =
                client.get_json(&status_path).await;
            let next_run_at = agent_task
                .ok()
                .and_then(|r| r.data)
                .and_then(|t| t.next_run_at);

            let _ = crate::models::task_scheduler::update_sync_status(
                &state.metadata_db,
                task.id,
                "synced",
                None,
                Some(&now_str),
                next_run_at,
            )
            .await;
            Ok(())
        }
        Ok(resp) => {
            let is_conflict = resp.error_code == Some(ErrorCode::TaskRevisionConflict);
            let sync_status = if is_conflict { "conflict" } else { "failed" };
            let _ = crate::models::task_scheduler::update_sync_status(
                &state.metadata_db,
                task.id,
                sync_status,
                Some(&resp.message),
                None,
                task.next_run_at,
            )
            .await;
            Err(anyhow::anyhow!("sync failed: {}", resp.message))
        }
        Err(err) => {
            let err_str = err.to_string();
            let is_conflict = err_str.contains("TASK_REVISION_CONFLICT");
            let sync_status = if is_conflict { "conflict" } else { "failed" };
            let _ = crate::models::task_scheduler::update_sync_status(
                &state.metadata_db,
                task.id,
                sync_status,
                Some(&err_str),
                None,
                task.next_run_at,
            )
            .await;
            Err(err)
        }
    }
}

/// 从 Agent 删除任务副本
pub async fn delete_task_from_agent(
    state: &AppState,
    task_id: i64,
    agent_id: &str,
) -> anyhow::Result<()> {
    let client = NodeRuntimeClient::from_node_route(&state.metadata_db, Some(agent_id)).await?;
    let path = format!("/api/v1/agent/tasks/{}", task_id);
    let resp: ApiResponse<bool> = client.delete_json(&path).await?;
    if !resp.success {
        return Err(anyhow::anyhow!(
            "failed to delete task from agent: {}",
            resp.message
        ));
    }
    Ok(())
}

/// 发送立即执行命令到 Agent 并同步等待结果
pub async fn run_task_on_agent(
    state: &AppState,
    task: &ScheduledTask,
) -> anyhow::Result<AgentTaskRunDto> {
    let client =
        NodeRuntimeClient::from_node_route(&state.metadata_db, Some(&task.agent_id)).await?;
    let path = format!("/api/v1/agent/tasks/{}/run", task.id);
    let resp: ApiResponse<AgentTaskRunDto> =
        client.post_json(&path, &serde_json::json!({})).await?;
    if !resp.success {
        return Err(anyhow::anyhow!(
            "failed to run task on agent: {}",
            resp.message
        ));
    }
    let run = resp
        .data
        .ok_or_else(|| anyhow::anyhow!("agent returned empty task run data"))?;

    // 缓存写入本地 task_runs 作为备份
    let triggered_at_ts = parse_iso_time(&run.triggered_at).unwrap_or(0);
    let started_at_ts = run.started_at.as_deref().and_then(parse_iso_time);
    let finished_at_ts = run.finished_at.as_deref().and_then(parse_iso_time);

    let _ = crate::models::task_scheduler::create_task_run(
        &state.metadata_db,
        &crate::models::task_scheduler::NewTaskRun {
            run_id: Some(run.run_id.clone()),
            task_id: task.id,
            agent_id: task.agent_id.clone(),
            triggered_at: triggered_at_ts,
            started_at: started_at_ts,
            finished_at: finished_at_ts,
            status: run.status.clone(),
            exit_code: run.exit_code,
            log_excerpt: run.log_excerpt.clone(),
            error_message: run.error_message.clone(),
        },
    )
    .await;

    // 同时更新主控的 last_run_at 缓存，保留 next_run_at
    let _ = crate::models::task_scheduler::update_task_run_times(
        &state.metadata_db,
        task.id,
        triggered_at_ts,
        task.next_run_at,
    )
    .await;

    Ok(run)
}

/// 代理查询 Agent 侧的任务执行历史
pub async fn fetch_task_runs_from_agent(
    state: &AppState,
    task: &ScheduledTask,
    limit: i64,
) -> anyhow::Result<Vec<AgentTaskRunDto>> {
    let client =
        NodeRuntimeClient::from_node_route(&state.metadata_db, Some(&task.agent_id)).await?;
    let path = format!("/api/v1/agent/tasks/{}/runs?limit={}", task.id, limit);
    let resp: ApiResponse<Vec<AgentTaskRunDto>> = client.get_json(&path).await?;
    if !resp.success {
        return Err(anyhow::anyhow!(
            "failed to fetch runs from agent: {}",
            resp.message
        ));
    }
    Ok(resp.data.unwrap_or_default())
}

/// 批量查询 Agent 所有任务的状态
pub async fn fetch_tasks_status_from_agent(
    state: &AppState,
    node_id: &str,
) -> anyhow::Result<Vec<AgentTaskStatusDto>> {
    let client = NodeRuntimeClient::from_node_route(&state.metadata_db, Some(node_id)).await?;
    let path = "/api/v1/agent/tasks/status";
    let resp: ApiResponse<Vec<AgentTaskStatusDto>> = client.get_json(path).await?;
    if !resp.success {
        return Err(anyhow::anyhow!(
            "failed to fetch task statuses from agent: {}",
            resp.message
        ));
    }
    Ok(resp.data.unwrap_or_default())
}

pub fn parse_iso_time(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp())
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskRunReportDto {
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

/// 保存 Agent 上送的定时任务执行记录到主控本地缓存
pub async fn save_reported_task_runs(
    state: &AppState,
    agent_id: &str,
    runs: Vec<AgentTaskRunReportDto>,
) -> anyhow::Result<()> {
    for run in runs {
        let triggered_at_ts = parse_iso_time(&run.triggered_at).unwrap_or(0);
        let started_at_ts = run.started_at.as_deref().and_then(parse_iso_time);
        let finished_at_ts = run.finished_at.as_deref().and_then(parse_iso_time);

        let new_run = crate::models::task_scheduler::NewTaskRun {
            run_id: Some(run.run_id),
            task_id: run.controller_task_id,
            agent_id: agent_id.to_string(),
            triggered_at: triggered_at_ts,
            started_at: started_at_ts,
            finished_at: finished_at_ts,
            status: run.status,
            exit_code: run.exit_code,
            log_excerpt: run.log_excerpt,
            error_message: run.error_message,
        };

        let _ = crate::models::task_scheduler::create_task_run(&state.metadata_db, &new_run).await;
    }
    Ok(())
}

/// 在节点上线时，扫描并自动同步该 Agent 节点下所有非 'synced' 状态的任务
pub async fn reconcile_agent_tasks(state: &AppState, agent_id: &str) -> anyhow::Result<()> {
    let tasks =
        crate::models::task_scheduler::list_tasks(&state.metadata_db, Some(agent_id)).await?;
    for task in tasks {
        if task.sync_status != "synced" {
            if let Err(err) = sync_task_to_agent(state, &task, false).await {
                tracing::warn!(
                    "Reconcile task {} to agent {} failed: {}",
                    task.id,
                    agent_id,
                    err
                );
            } else {
                tracing::info!("Reconcile task {} to agent {} succeeded", task.id, agent_id);
            }
        }
    }
    Ok(())
}

use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::Notify;

fn sync_notify() -> &'static Notify {
    static NOTIFY: OnceLock<Notify> = OnceLock::new();
    NOTIFY.get_or_init(Notify::new)
}

/// 触发立即执行一次主控同步队列的检查消费
pub fn trigger_sync_queue_check() {
    sync_notify().notify_one();
}

/// 启动后台任务消费同步变更操作队列
pub fn spawn_sync_queue_worker(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            if let Err(err) = process_sync_queue(&state).await {
                tracing::error!("Failed to process task sync queue: {:?}", err);
            }

            tokio::select! {
                _ = sync_notify().notified() => {}
                _ = tokio::time::sleep(Duration::from_secs(5)) => {}
            }
        }
    });
}

async fn process_sync_queue(state: &AppState) -> anyhow::Result<()> {
    use crate::models::task_scheduler::{
        get_task_by_id, list_pending_sync_ops, update_sync_op_status,
    };

    let pending_ops = list_pending_sync_ops(&state.metadata_db).await?;
    for op in pending_ops {
        let _ = update_sync_op_status(&state.metadata_db, op.id, "processing", None).await;

        let result = if op.op_type == "upsert" {
            let task_opt = get_task_by_id(&state.metadata_db, op.task_id).await?;
            if let Some(task) = task_opt {
                sync_task_to_agent(state, &task, false).await
            } else {
                Ok(())
            }
        } else if op.op_type == "delete" {
            delete_task_from_agent(state, op.task_id, &op.agent_id).await
        } else {
            Err(anyhow::anyhow!("Unknown task sync op_type: {}", op.op_type))
        };

        match result {
            Ok(_) => {
                let _ = sqlx::query("DELETE FROM task_sync_ops WHERE id = ?")
                    .bind(op.id)
                    .execute(&state.metadata_db)
                    .await;
            }
            Err(err) => {
                let err_str = err.to_string();
                let _ = update_sync_op_status(&state.metadata_db, op.id, "failed", Some(&err_str))
                    .await;
            }
        }
    }
    Ok(())
}
