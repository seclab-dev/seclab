//! 在线升级 API：发布版本同步、计划创建与计划状态查询。

use crate::api::auth::AuthenticatedAdmin;
use crate::services::logging::OperationEventBuilder;
use crate::services::upgrades::{self, UpgradePlanCreatePayload};
use crate::state::AppState;
use crate::types::{ApiResponse, ApiResult};
use axum::{
    Json, Router,
    extract::{Multipart, Path, State, connect_info::ConnectInfo},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use std::net::SocketAddr;
use std::sync::Arc;

/// 查询已同步的发布版本。
pub async fn list_releases(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    let releases = upgrades::list_synced_releases(&state.metadata_db).await?;
    Ok(ApiResponse::success_with_raw("Upgrade releases loaded", Some(releases)).into_response())
}

/// 从 GitHub 同步发布版本。
pub async fn sync_releases(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    let releases = upgrades::sync_github_releases(&state.metadata_db).await?;
    Ok(ApiResponse::success_with_raw("Upgrade releases synced", Some(releases)).into_response())
}

/// 上传本地完整版本包。
pub async fn upload_release(
    State(state): State<Arc<AppState>>,
    admin: AuthenticatedAdmin,
    ConnectInfo(conn): ConnectInfo<SocketAddr>,
    mut multipart: Multipart,
) -> ApiResult<Response> {
    let release_res = upgrades::upload_release_package(&state.metadata_db, &mut multipart).await;
    match &release_res {
        Ok(release) => {
            OperationEventBuilder::new(&admin.username, "upgrade_release_upload", conn.ip())
                .module(crate::models::logging::LogModule::System)
                .target_type("upgrade_release")
                .target_id(&release.version)
                .set_success()
                .metadata(serde_json::json!({
                    "version": release.version,
                    "channel": release.channel,
                    "source": release.source,
                }))
                .finish(&state.metadata_db);
        }
        Err(err) => {
            OperationEventBuilder::new(&admin.username, "upgrade_release_upload", conn.ip())
                .module(crate::models::logging::LogModule::System)
                .target_type("upgrade_release")
                .status(crate::models::logging::LogStatus::Failed)
                .metadata(serde_json::json!({
                    "error": err.to_string(),
                }))
                .finish(&state.metadata_db);
        }
    }
    let release = upgrades::build_release_view(release_res?);
    Ok(ApiResponse::success_with_raw("Upgrade release uploaded", Some(release)).into_response())
}

/// 创建升级计划。
pub async fn create_plan(
    State(state): State<Arc<AppState>>,
    admin: AuthenticatedAdmin,
    Json(payload): Json<UpgradePlanCreatePayload>,
) -> ApiResult<Response> {
    let detail =
        upgrades::create_plan(&state.metadata_db, payload, admin.id, &admin.username).await?;
    Ok(ApiResponse::success_with_raw("Upgrade plan created", Some(detail)).into_response())
}

/// 启动升级计划。
pub async fn start_plan(
    State(state): State<Arc<AppState>>,
    Path(plan_id): Path<String>,
) -> ApiResult<Response> {
    let detail = upgrades::start_plan(&state.metadata_db, &plan_id).await?;
    Ok(ApiResponse::success_with_raw("Upgrade plan started", Some(detail)).into_response())
}

/// 查询升级计划详情。
pub async fn detail(
    State(state): State<Arc<AppState>>,
    Path(plan_id): Path<String>,
) -> ApiResult<Response> {
    let detail = upgrades::detail(&state.metadata_db, &plan_id).await?;
    Ok(ApiResponse::success_with_raw("Upgrade plan loaded", Some(detail)).into_response())
}

/// 获取最近的升级计划。
pub async fn latest_plan(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    let detail = upgrades::get_latest_plan(&state.metadata_db).await?;
    Ok(ApiResponse::success_with_raw("Latest upgrade plan loaded", Some(detail)).into_response())
}

/// 取消升级计划。
pub async fn cancel_plan(
    State(state): State<Arc<AppState>>,
    Path(plan_id): Path<String>,
) -> ApiResult<Response> {
    let detail = upgrades::cancel_plan(&state.metadata_db, &plan_id).await?;
    Ok(ApiResponse::success_with_raw("Upgrade plan canceled", Some(detail)).into_response())
}

/// 删除发布版本及其缓存。
pub async fn delete_release(
    State(state): State<Arc<AppState>>,
    admin: AuthenticatedAdmin,
    ConnectInfo(conn): ConnectInfo<SocketAddr>,
    Path(version): Path<String>,
) -> ApiResult<Response> {
    let res = upgrades::delete_release(&state.metadata_db, &version).await;
    match &res {
        Ok(_) => {
            OperationEventBuilder::new(&admin.username, "upgrade_release_delete", conn.ip())
                .module(crate::models::logging::LogModule::System)
                .target_type("upgrade_release")
                .target_id(&version)
                .set_success()
                .metadata(serde_json::json!({
                    "version": version,
                }))
                .finish(&state.metadata_db);
        }
        Err(err) => {
            OperationEventBuilder::new(&admin.username, "upgrade_release_delete", conn.ip())
                .module(crate::models::logging::LogModule::System)
                .target_type("upgrade_release")
                .target_id(&version)
                .status(crate::models::logging::LogStatus::Failed)
                .metadata(serde_json::json!({
                    "version": version,
                    "error": err.to_string(),
                }))
                .finish(&state.metadata_db);
        }
    }
    res?;
    Ok(ApiResponse::success_with_raw("Upgrade release deleted", Some(())).into_response())
}

/// 在线升级 API 路由。
pub fn upgrades_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/releases/list", get(list_releases))
        .route("/releases/sync", post(sync_releases))
        .route("/releases/upload", post(upload_release))
        .route("/release/{version}/delete", delete(delete_release))
        .route("/plan/create", post(create_plan))
        .route("/plan/latest", get(latest_plan))
        .route("/plan/{plan_id}/start", post(start_plan))
        .route("/plan/{plan_id}/detail", get(detail))
        .route("/plan/{plan_id}/cancel", post(cancel_plan))
}
