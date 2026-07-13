//! Docker 镜像获取任务 API。

use crate::services::image_acquisition::ImageTask;
use crate::state::AppState;
use crate::types::{ApiResponse, ApiResult};
use axum::{
    Json, Router,
    extract::{Path, State},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use bollard::Docker;
use bollard::query_parameters::ListImagesOptions;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateImageTaskRequest {
    pub node_id: String,
    pub image_ref: String,
    pub source_mode: String,
}

pub fn docker_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/image-tasks", post(create_image_task))
        .route("/image-tasks/{task_id}/progress", get(image_task_progress))
        .route("/image-tasks/{task_id}", delete(cancel_image_task))
        .route("/local-images", get(list_local_images))
}

fn local_docker() -> ApiResult<Docker> {
    Docker::connect_with_local_defaults().map_err(|err| {
        seclab_api::error::ApiError::bad_gateway(
            seclab_contracts::api::ErrorCode::DockerUnavailable,
            format!("Local docker daemon is not available: {err}"),
        )
    })
}

pub async fn list_local_images() -> ApiResult<Response> {
    let images = local_docker()?
        .list_images(Some(ListImagesOptions::default()))
        .await
        .map_err(|err| {
            seclab_api::error::ApiError::internal(format!("failed to list local images: {err}"))
        })?;
    Ok(ApiResponse::success_with_raw("Local images loaded", Some(images)).into_response())
}

pub async fn create_image_task(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateImageTaskRequest>,
) -> ApiResult<Response> {
    let node_id = payload.node_id.trim();
    let image_ref = payload.image_ref.trim();
    if node_id.is_empty() || image_ref.is_empty() {
        return Err(seclab_api::error::ApiError::bad_request(
            seclab_contracts::api::ErrorCode::BadRequest,
            "nodeId and imageRef must not be empty".to_string(),
        ));
    }
    if payload.source_mode != "controller-first" {
        return Err(seclab_api::error::ApiError::bad_request(
            seclab_contracts::api::ErrorCode::BadRequest,
            "sourceMode must be controller-first".to_string(),
        ));
    }
    let task = state.image_acquisition.start(
        Arc::clone(&state),
        node_id.to_string(),
        image_ref.to_string(),
    );
    Ok(ApiResponse::success_with_raw("Image task started", Some(task)).into_response())
}

pub async fn image_task_progress(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> ApiResult<Response> {
    let task = find_task(&state, &task_id)?;
    Ok(ApiResponse::success_with_raw("Image task progress loaded", Some(task)).into_response())
}

pub async fn cancel_image_task(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> ApiResult<Response> {
    let task = state
        .image_acquisition
        .cancel(&task_id)
        .ok_or(seclab_api::error::ApiError::NotFound)?;
    Ok(
        ApiResponse::success_with_raw("Image task cancellation requested", Some(task))
            .into_response(),
    )
}

fn find_task(state: &AppState, task_id: &str) -> ApiResult<ImageTask> {
    state
        .image_acquisition
        .get(task_id)
        .ok_or(seclab_api::error::ApiError::NotFound)
}
