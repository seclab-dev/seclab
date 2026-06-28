//! Agent 定时任务调度器：分钟级 cron 触发本地计划任务。

use crate::models::scheduled_tasks::{
    self, AgentScheduledTask, NewAgentTaskRun, UpdateAgentTaskRun,
};
use crate::state::AppState;
use chrono::{Timelike, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;

pub fn spawn_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            let now = Utc::now();
            let wait_nanos = u64::from(59u32 - now.second()) * 1_000_000_000
                + u64::from(1_000_000_000u32 - now.nanosecond());
            tokio::time::sleep(Duration::from_nanos(wait_nanos)).await;

            if let Err(err) = dispatch_due_tasks(Arc::clone(&state)).await {
                tracing::warn!("local task scheduler dispatch failed: {:?}", err);
            }
        }
    });
}

async fn dispatch_due_tasks(state: Arc<AppState>) -> anyhow::Result<()> {
    let now_ts = Utc::now().timestamp();
    let due_tasks = scheduled_tasks::list_due_tasks(&state.metadata_db, now_ts).await?;
    for task in due_tasks {
        enqueue_task_run(Arc::clone(&state), task, now_ts).await;
    }
    Ok(())
}

async fn enqueue_task_run(state: Arc<AppState>, task: AgentScheduledTask, triggered_at_ts: i64) {
    if task.no_overlap && !try_acquire_task_lock(&state, task.controller_task_id).await {
        tracing::info!(
            "task {} execution skipped due to no_overlap lock",
            task.controller_task_id
        );
        return;
    }

    tokio::spawn(async move {
        if let Err(err) = execute_task_once(Arc::clone(&state), &task, triggered_at_ts).await {
            tracing::warn!("execute task {} failed: {:?}", task.controller_task_id, err);
        }
        if task.no_overlap {
            release_task_lock(&state, task.controller_task_id).await;
        }
    });
}

async fn try_acquire_task_lock(state: &AppState, task_id: i64) -> bool {
    let mut guard = state.running_task_ids.lock().await;
    guard.insert(task_id)
}

async fn release_task_lock(state: &AppState, task_id: i64) {
    let mut guard = state.running_task_ids.lock().await;
    guard.remove(&task_id);
}

async fn execute_task_once(
    state: Arc<AppState>,
    task: &AgentScheduledTask,
    triggered_at_ts: i64,
) -> anyhow::Result<()> {
    let next_run_at = if task.enabled {
        scheduled_tasks::compute_next_run_at(&task.cron_expr, triggered_at_ts)?
    } else {
        None
    };

    scheduled_tasks::update_task_run_times(
        &state.metadata_db,
        task.controller_task_id,
        triggered_at_ts,
        next_run_at,
    )
    .await?;

    let run_id = uuid::Uuid::now_v7().to_string();
    let triggered_at_str = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);

    let new_run = NewAgentTaskRun {
        run_id: run_id.clone(),
        controller_task_id: task.controller_task_id,
        triggered_at: triggered_at_str.clone(),
        started_at: Some(triggered_at_str.clone()),
        finished_at: None,
        status: "running".to_string(),
        exit_code: None,
        stdout: None,
        stderr: None,
        error_message: None,
        trigger_source: "cron".to_string(),
    };

    scheduled_tasks::create_task_run(&state.metadata_db, &new_run).await?;

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
                started_at: Some(triggered_at_str.clone()),
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
            started_at: Some(triggered_at_str.clone()),
            finished_at: Some(Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)),
            status: "failed".to_string(),
            exit_code: None,
            stdout: None,
            stderr: None,
            error_message: Some(format!("Failed to spawn command: {err}")),
        },
    };

    scheduled_tasks::update_task_run(&state.metadata_db, &run_id, &update_payload).await?;

    // 清理历史记录，每个任务最多保留最近 500 条
    let _ =
        scheduled_tasks::cleanup_old_runs(&state.metadata_db, task.controller_task_id, 500).await;

    let log_excerpt =
        if let (Some(stdout), Some(stderr)) = (&update_payload.stdout, &update_payload.stderr) {
            Some(scheduled_tasks::merge_log_excerpt(stdout, stderr))
        } else {
            None
        };

    let report_payload = scheduled_tasks::TaskRunReportPayload {
        run_id: run_id.clone(),
        controller_task_id: task.controller_task_id,
        triggered_at: triggered_at_str,
        started_at: update_payload.started_at.clone(),
        finished_at: update_payload.finished_at.clone(),
        status: update_payload.status.clone(),
        exit_code: update_payload.exit_code,
        log_excerpt,
        error_message: update_payload.error_message.clone(),
        trigger_source: "cron".to_string(),
    };
    let _ = scheduled_tasks::get_task_run_reporter()
        .send(report_payload)
        .await;

    Ok(())
}
