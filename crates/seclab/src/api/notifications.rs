//! 通知历史 API：通知记录的写入、分页查询与清空。

use crate::state::AppState;
use crate::types::{ApiResponse, ApiResult};
use axum::{
    Json, Router,
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
};
use seclab_contracts::notification::{
    NotificationBatchDeletePayload, NotificationCreatePayload, NotificationList, NotificationQuery,
    NotificationRecord,
};
use std::sync::Arc;

/// 挂载通知历史相关路由。
pub fn notifications_router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/",
            get(list_notifications)
                .post(create_notification)
                .delete(clear_notifications),
        )
        .route("/batch", axum::routing::delete(batch_delete_notifications))
}

/// 分页查询通知历史。
async fn list_notifications(
    State(state): State<Arc<AppState>>,
    Query(query): Query<NotificationQuery>,
) -> ApiResult<impl IntoResponse> {
    let page = query.page.max(1);
    let page_size = query.page_size.max(1);
    let offset = ((page - 1) * page_size) as i64;
    let limit = page_size as i64;

    let mut qb_count =
        sqlx::QueryBuilder::new("SELECT COUNT(*) FROM notification_history WHERE 1=1");
    let mut qb_select = sqlx::QueryBuilder::new(
        "SELECT id, level, message, created_at FROM notification_history WHERE 1=1",
    );

    if let Some(level) = &query.level {
        qb_count.push(" AND level = ");
        qb_count.push_bind(level.as_str());
        qb_select.push(" AND level = ");
        qb_select.push_bind(level.as_str());
    }

    if let Some(keyword) = &query.keyword {
        #[allow(clippy::collapsible_if)]
        if !keyword.trim().is_empty() {
            let like_term = format!("%{}%", keyword);
            qb_count.push(" AND message LIKE ");
            qb_count.push_bind(like_term.clone());
            qb_select.push(" AND message LIKE ");
            qb_select.push_bind(like_term);
        }
    }

    let total = qb_count
        .build_query_scalar::<i64>()
        .fetch_one(&state.metadata_db)
        .await?;

    qb_select.push(" ORDER BY created_at DESC, id DESC LIMIT ");
    qb_select.push_bind(limit);
    qb_select.push(" OFFSET ");
    qb_select.push_bind(offset);

    let records = qb_select
        .build_query_as::<NotificationRecord>()
        .fetch_all(&state.metadata_db)
        .await?;

    Ok(ApiResponse::success_with_raw(
        "Notification history loaded",
        NotificationList {
            total,
            page,
            page_size,
            records,
        },
    ))
}

/// 写入一条通知历史。
async fn create_notification(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<NotificationCreatePayload>,
) -> ApiResult<impl IntoResponse> {
    sqlx::query(
        r#"
        INSERT INTO notification_history (level, message)
        VALUES (?, ?)
        "#,
    )
    .bind(payload.level.as_str())
    .bind(payload.message)
    .execute(&state.metadata_db)
    .await?;

    Ok(ApiResponse::ok("Notification recorded"))
}

/// 清空通知历史。
async fn clear_notifications(State(state): State<Arc<AppState>>) -> ApiResult<impl IntoResponse> {
    sqlx::query("DELETE FROM notification_history")
        .execute(&state.metadata_db)
        .await?;

    Ok(ApiResponse::ok("Notification history cleared"))
}

/// 批量删除通知历史。
async fn batch_delete_notifications(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<NotificationBatchDeletePayload>,
) -> ApiResult<impl IntoResponse> {
    if payload.ids.is_empty() {
        return Ok(ApiResponse::ok("No notifications to delete"));
    }

    let mut qb = sqlx::QueryBuilder::new("DELETE FROM notification_history WHERE id IN (");
    let mut separated = qb.separated(", ");
    for id in &payload.ids {
        separated.push_bind(id);
    }
    separated.push_unseparated(")");

    qb.build().execute(&state.metadata_db).await?;

    Ok(ApiResponse::ok("Notifications deleted"))
}
