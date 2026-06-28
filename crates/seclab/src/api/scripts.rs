//! 脚本库管理 API：提供脚本列表、详情、增删改查。

use crate::models::scripts::{
    ScriptPayload, ScriptRecord, create_script, delete_script, get_script, list_scripts,
    update_script,
};
use crate::state::AppState;
use crate::types::{ApiError, ApiResponse, ApiResult, new_uuid_v7};
use axum::{
    Json, Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{delete, get, put},
};
use std::sync::Arc;

/// 构建脚本库相关的路由集合。
pub fn scripts_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{script_id}/detail", get(detail))
        .route("/{script_id}/update", put(update))
        .route("/{script_id}/remove", delete(remove))
}

/// 查询脚本列表。
pub async fn list(State(state): State<Arc<AppState>>) -> ApiResult<impl IntoResponse> {
    let mut db_scripts = list_scripts(&state.metadata_db).await?;
    let builtin = crate::models::scripts::get_builtin_scripts();
    db_scripts.retain(|s| !builtin.iter().any(|b| b.script_id == s.script_id));
    let mut all_scripts = builtin;
    all_scripts.append(&mut db_scripts);
    Ok(ApiResponse::success_with_raw("Scripts list loaded", all_scripts).into_response())
}

/// 查询单个脚本详情。
pub async fn detail(
    State(state): State<Arc<AppState>>,
    Path(script_id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let builtin = crate::models::scripts::get_builtin_scripts();
    if let Some(script) = builtin.into_iter().find(|s| s.script_id == script_id) {
        return Ok(ApiResponse::success_with_raw("Script details loaded", script).into_response());
    }
    let script = get_script(&state.metadata_db, &script_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(ApiResponse::success_with_raw("Script details loaded", script).into_response())
}

/// 新增脚本。
pub async fn create(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ScriptPayload>,
) -> ApiResult<impl IntoResponse> {
    if payload.title.trim().is_empty() {
        return Err(ApiError::BadRequest("Title is required".to_string()));
    }
    if payload.content.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "Script content is required".to_string(),
        ));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let script_id = new_uuid_v7();
    let record = ScriptRecord {
        script_id: script_id.clone(),
        title: payload.title,
        description: payload.description,
        content: payload.content,
        created_at: now.clone(),
        updated_at: now,
    };

    create_script(&state.metadata_db, &record).await?;
    Ok(ApiResponse::success_with_raw("Script created successfully", record).into_response())
}

/// 更新现有脚本。
pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(script_id): Path<String>,
    Json(payload): Json<ScriptPayload>,
) -> ApiResult<impl IntoResponse> {
    if payload.title.trim().is_empty() {
        return Err(ApiError::BadRequest("Title is required".to_string()));
    }
    if payload.content.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "Script content is required".to_string(),
        ));
    }

    let builtin = crate::models::scripts::get_builtin_scripts();
    if builtin.iter().any(|s| s.script_id == script_id) {
        return Err(ApiError::BadRequest(
            "Built-in scripts cannot be modified".to_string(),
        ));
    }

    let existing = get_script(&state.metadata_db, &script_id)
        .await?
        .ok_or(ApiError::NotFound)?;

    let now = chrono::Utc::now().to_rfc3339();
    let record = ScriptRecord {
        script_id,
        title: payload.title,
        description: payload.description,
        content: payload.content,
        created_at: existing.created_at,
        updated_at: now,
    };

    update_script(&state.metadata_db, &record).await?;
    Ok(ApiResponse::success_with_raw("Script updated successfully", record).into_response())
}

/// 删除脚本。
pub async fn remove(
    State(state): State<Arc<AppState>>,
    Path(script_id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let builtin = crate::models::scripts::get_builtin_scripts();
    if builtin.iter().any(|s| s.script_id == script_id) {
        return Err(ApiError::BadRequest(
            "Built-in scripts cannot be deleted".to_string(),
        ));
    }

    get_script(&state.metadata_db, &script_id)
        .await?
        .ok_or(ApiError::NotFound)?;

    delete_script(&state.metadata_db, &script_id).await?;
    Ok(ApiResponse::ok("Script deleted successfully").into_response())
}
