//! Master 脚本运行协调器：从持久化队列向目标 Agent 幂等投递和取消。

use crate::{
    models::{
        logging::{LogModule, LogStatus, PlatformLogLevel},
        node_runtime_client::NodeRuntimeClient,
        scripts,
    },
    services::logging::PlatformLogEntry,
    state::AppState,
    types::{ApiError, ApiResult},
};
use seclab_contracts::{api::ApiResponse, scripts::AgentStartScriptRunRequest};
use std::{sync::Arc, time::Duration};

const AGENT_BASE: &str = "/api/v1/agent/script-runs";

/// 启动单实例持久化投递循环，服务重启后会继续处理 queued/cancelling 运行。
pub fn spawn_worker(state: Arc<AppState>) {
    let dispatch_state = Arc::clone(&state);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            if let Err(error) = dispatch_pending(&dispatch_state).await {
                tracing::warn!(%error, "script run dispatch cycle failed");
            }
        }
    });
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(60 * 60));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            if let Err(error) = scripts::cleanup(&state.metadata_db).await {
                tracing::warn!(%error, "script run retention cleanup failed");
            }
        }
    });
}

async fn dispatch_pending(state: &AppState) -> ApiResult<()> {
    for run in scripts::queued_runs(&state.metadata_db).await? {
        match run.status.as_str() {
            "queued" => {
                if let Err(error) = dispatch_start(state, &run).await {
                    tracing::warn!(run_id = %run.run_id, node_id = %run.node_id, %error, "script run delivery failed");
                    scripts::fail_dispatch(
                        &state.metadata_db,
                        &run.run_id,
                        "execution node became unavailable before the run started",
                    )
                    .await?;
                    record_terminal(state, &run.run_id).await?;
                }
            }
            "cancelling" => {
                if let Err(error) = dispatch_cancel(state, &run).await {
                    tracing::warn!(run_id = %run.run_id, node_id = %run.node_id, %error, "script run cancellation delivery will retry");
                }
            }
            _ => {}
        }
    }
    Ok(())
}

async fn record_terminal(state: &AppState, run_id: &str) -> ApiResult<()> {
    let Some(run) = scripts::claim_terminal_audit(&state.metadata_db, run_id).await? else {
        return Ok(());
    };
    let Ok(client_ip) = run.client_ip.parse() else {
        tracing::error!(%run_id, "trusted script run client IP is invalid");
        return Ok(());
    };
    PlatformLogEntry::new(&run.actor_name, "script_run_completed", client_ip)
        .user_id(run.actor_user_id)
        .module(LogModule::System)
        .target_type("script")
        .target_id(&run.script_id)
        .trace_id(&run.trace_id)
        .status(LogStatus::Failed)
        .level(PlatformLogLevel::Error)
        .metadata(serde_json::json!({
            "runId": run.run_id,
            "scriptName": run.script_name,
            "revision": run.script_revision,
            "nodeId": run.node_id,
            "result": run.status,
            "errorCode": run.error_code,
        }))
        .finish(&state.metadata_db);
    Ok(())
}

async fn dispatch_start(state: &AppState, run: &scripts::ScriptRunRow) -> ApiResult<()> {
    let client = NodeRuntimeClient::from_node_route(&state.metadata_db, Some(&run.node_id)).await?;
    let request = AgentStartScriptRunRequest {
        run_id: run.run_id.clone(),
        script_id: run.script_id.clone(),
        script_name: run.script_name.clone(),
        script_revision: run.script_revision,
        source_content: run.source_content.clone(),
        source_sha256: run.source_sha256.clone(),
        timeout_seconds: run.timeout_seconds.clamp(1, 86_400) as u32,
        ownership_kind: seclab_contracts::scripts::ScriptOwnershipKind::Custom,
    };
    let response: ApiResponse<serde_json::Value> = client
        .post_json(AGENT_BASE, &request)
        .await
        .map_err(external_error)?;
    if !response.success {
        return Err(ApiError::conflict(
            response
                .error_code
                .unwrap_or(seclab_contracts::api::ErrorCode::ScriptRunConflict),
            response.message,
        ));
    }
    scripts::mark_dispatched(&state.metadata_db, &run.run_id).await
}

async fn dispatch_cancel(state: &AppState, run: &scripts::ScriptRunRow) -> ApiResult<()> {
    let client = NodeRuntimeClient::from_node_route(&state.metadata_db, Some(&run.node_id)).await?;
    let response: ApiResponse<serde_json::Value> = client
        .post_json(
            &format!("{AGENT_BASE}/{}/cancel", run.run_id),
            &serde_json::json!({}),
        )
        .await
        .map_err(external_error)?;
    if response.success {
        Ok(())
    } else {
        Err(ApiError::conflict(
            response
                .error_code
                .unwrap_or(seclab_contracts::api::ErrorCode::ScriptRunNotCancellable),
            response.message,
        ))
    }
}

fn external_error(error: anyhow::Error) -> ApiError {
    ApiError::new(
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
        seclab_contracts::api::ErrorCode::ScriptNodeUnavailable,
        format!("script execution node is unavailable: {error}"),
    )
}
