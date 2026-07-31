//! 本地 Agent 操作事件 API：通过 Unix Socket 向 Master 暴露持久 outbox。

use crate::{
    services::operation_outbox,
    state::AppState,
    types::{ApiError, ApiResponse, ApiResult},
};
use axum::{
    Json, Router,
    extract::{Query, State},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use seclab_contracts::api::ErrorCode;
use serde::Deserialize;
use std::sync::Arc;

const MAX_BATCH_SIZE: u32 = 100;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PendingQuery {
    #[serde(default = "default_batch_size")]
    limit: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AcknowledgeRequest {
    event_ids: Vec<String>,
}

fn default_batch_size() -> u32 {
    MAX_BATCH_SIZE
}

/// 构造仅供 Master 通过本地 Agent 通道消费的操作事件路由。
pub fn operation_event_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/pending", get(pending))
        .route("/acknowledge", post(acknowledge))
}

/// 返回当前允许投递的持久操作事件。
async fn pending(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PendingQuery>,
) -> ApiResult<Response> {
    ensure_local_mode(&state).await?;
    if query.limit == 0 || query.limit > MAX_BATCH_SIZE {
        return Err(ApiError::bad_request(
            ErrorCode::BadRequest,
            "operation event batch limit must be between 1 and 100",
        ));
    }
    let events = operation_outbox::pending(&state.metadata_db, query.limit).await?;
    Ok(
        ApiResponse::success_with_raw("Pending operation events loaded", Some(events))
            .into_response(),
    )
}

/// 确认 Master 已持久化的本地操作事件。
async fn acknowledge(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<AcknowledgeRequest>,
) -> ApiResult<Response> {
    ensure_local_mode(&state).await?;
    if payload.event_ids.is_empty()
        || payload.event_ids.len() > MAX_BATCH_SIZE as usize
        || payload
            .event_ids
            .iter()
            .any(|event_id| event_id.is_empty() || event_id.len() > 128)
    {
        return Err(ApiError::bad_request(
            ErrorCode::BadRequest,
            "operation event acknowledgement must contain 1 to 100 valid event ids",
        ));
    }
    operation_outbox::acknowledge(&state.metadata_db, &payload.event_ids).await?;
    Ok(
        ApiResponse::success_with_raw("Operation events acknowledged", Some(payload.event_ids))
            .into_response(),
    )
}

/// 防止远程 Agent 暴露由 runtime session 自行投递的 outbox。
async fn ensure_local_mode(state: &AppState) -> ApiResult<()> {
    let mode =
        sqlx::query_scalar::<_, String>("SELECT mode FROM agent_identity ORDER BY id LIMIT 1")
            .fetch_optional(&state.metadata_db)
            .await?;
    if mode.as_deref() != Some("local") {
        return Err(ApiError::forbidden(
            ErrorCode::AuthForbidden,
            "operation event outbox is only available in local Agent mode",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::setup_test_state;
    use seclab_contracts::logging::{
        AgentOperationEvent, OperationActor, OperationActorKind, OperationImpact, OperationModule,
        OperationOutcome,
    };
    use std::collections::BTreeMap;

    async fn state_with_mode(mode: &str) -> Arc<AppState> {
        let state = Arc::new(setup_test_state().await);
        sqlx::query("INSERT INTO agent_identity (mode) VALUES (?)")
            .bind(mode)
            .execute(&state.metadata_db)
            .await
            .unwrap();
        state
    }

    fn operation_event() -> AgentOperationEvent {
        AgentOperationEvent {
            event_id: "local-operation-event".to_string(),
            occurred_at: "2026-07-31T03:08:24Z".to_string(),
            module: OperationModule::Docker,
            event_code: "docker_compose_project_create".to_string(),
            actor: OperationActor {
                kind: OperationActorKind::System,
                user_id: None,
                display_name: "system".to_string(),
            },
            client_ip: None,
            target: None,
            outcome: OperationOutcome::Success,
            impact: OperationImpact::Info,
            trace_id: "local-operation-trace".to_string(),
            task_id: Some("local-operation-task".to_string()),
            parameters: BTreeMap::new(),
            error_code: None,
            error_summary: None,
        }
    }

    #[tokio::test]
    async fn local_mode_exposes_and_acknowledges_pending_events() {
        let state = state_with_mode("local").await;
        operation_outbox::enqueue(&state.metadata_db, &operation_event())
            .await
            .unwrap();

        let response = pending(
            State(Arc::clone(&state)),
            Query(PendingQuery {
                limit: MAX_BATCH_SIZE,
            }),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        acknowledge(
            State(Arc::clone(&state)),
            Json(AcknowledgeRequest {
                event_ids: vec!["local-operation-event".to_string()],
            }),
        )
        .await
        .unwrap();
        assert!(
            operation_outbox::pending(&state.metadata_db, MAX_BATCH_SIZE)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn remote_mode_cannot_expose_operation_outbox() {
        let state = state_with_mode("remote").await;
        assert!(ensure_local_mode(&state).await.is_err());
    }
}
