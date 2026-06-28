//! Docker 镜像 API：镜像列表与管理操作。

use crate::models::docker;
use crate::state::AppState;
use crate::types::{AgentError, ApiResponse, ApiResult};

use axum::{
    extract::{Multipart, Query, State},
    response::{IntoResponse, Response},
};

use bollard::query_parameters;
use serde_json::json;

use crate::services::logging::{AgentLogModule, LoggerEntry};
use bollard::query_parameters::ImportImageOptions;
use futures_util::{StreamExt, TryStreamExt};
use std::default::Default;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio_util::codec::{BytesCodec, FramedRead};
use tracing::info;
use uuid::Uuid;

/// 获取本地所有 Docker 镜像的摘要信息列表。
///
/// 此函数通过调用 `bollard` 的 `list_images` 方法与 Docker 守护进程通信，
/// 并将获取到的镜像列表作为 `ApiResponse` 返回。
pub async fn list_images(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    info!("Requesting docker images");
    let docker = state.docker_client().await?;
    let images: Vec<bollard::secret::ImageSummary> = docker
        .list_images(Some(query_parameters::ListImagesOptions::default()))
        .await?;
    Ok(ApiResponse::success_with_raw("Image list loaded", Some(images)).into_response())
}

/// 根据镜像 ID 或名称删除一个本地 Docker 镜像。
///
/// # 参数
/// - `state`: 共享的应用状态。
/// - `query`: 包含要删除镜像的 ID 或名称 (`docker::ImageRef`) 的查询参数。
///
/// # 处理流程
/// 1. 调用 `bollard` 的 `remove_image` 方法执行删除操作。
/// 2. 如果删除成功，记录一条平台日志 (`LoggerEntry`)。
/// 3. 将 Docker API 的返回结果封装在 `ApiResponse` 中返回给客户端。
pub async fn remove_image(
    State(state): State<Arc<AppState>>,
    Query(query): Query<docker::ImageRef>,
) -> ApiResult<Response> {
    info!("Requesting Remove {} image", query.name);

    let docker = state.docker_client().await?;
    let images = docker
        .remove_image(
            &query.id,
            Some(query_parameters::RemoveImageOptions::default()),
            None,
        )
        .await?;
    LoggerEntry::new(
        "docker",
        "docker_image_removed",
        IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
    )
    .set_success()
    .module(AgentLogModule::Docker)
    .metadata(json!({
        "message_key": "platformLog.docker.image.removed",
        "image_name": query.name
    }))
    .finish(&state.metadata_db);
    info!("{} image removed", query.name);

    Ok(ApiResponse::success_with_raw("Image removed", Some(images)).into_response())
}

/// 接收流式上传的镜像 tar 包，并导入到本地 Docker 中。
pub async fn load_image(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> ApiResult<Response> {
    info!("Starting docker image load");
    let mut temp_file_path = std::env::temp_dir();
    let file_name = format!("docker-image-{}.tar", Uuid::new_v4());
    temp_file_path.push(&file_name);

    let mut file = File::create(&temp_file_path)
        .await
        .map_err(AgentError::FileOperation)?;

    let mut has_file = false;
    while let Some(field) = multipart.next_field().await.map_err(AgentError::from)? {
        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
            has_file = true;
            let mut stream = field;
            while let Some(chunk) = stream.next().await {
                let bytes = chunk.map_err(|e| AgentError::FileUploadInvalid(e.to_string()))?;
                file.write_all(&bytes)
                    .await
                    .map_err(AgentError::FileOperation)?;
            }
        }
    }

    if !has_file {
        return Err(AgentError::BadRequest("missing upload file field".to_string()).into());
    }

    // 确保写入完成并关闭文件
    file.flush().await.map_err(AgentError::FileOperation)?;
    drop(file);

    // 重新以只读方式打开文件，用于 load_image
    let read_file = File::open(&temp_file_path)
        .await
        .map_err(AgentError::FileOperation)?;
    let byte_stream = FramedRead::new(read_file, BytesCodec::new()).map(|r| r.unwrap().freeze());

    let docker = state.docker_client().await?;
    let options = ImportImageOptions {
        ..Default::default()
    };

    let mut stream = docker.import_image_stream(options, byte_stream, None);
    let mut logs = Vec::new();

    // 收集 stream
    let load_result: Result<(), AgentError> = async {
        while let Some(msg) = stream
            .try_next()
            .await
            .map_err(|e| AgentError::DockerOperation(e.to_string()))?
        {
            if let Some(stream_msg) = msg.stream {
                logs.push(stream_msg);
            }
            if let Some(error_msg) = msg.error {
                return Err(AgentError::DockerOperation(error_msg));
            }
        }
        Ok(())
    }
    .await;

    // 清理临时文件
    if let Err(e) = tokio::fs::remove_file(&temp_file_path).await {
        tracing::error!("Failed to remove temp file {:?}: {}", temp_file_path, e);
    }

    load_result?;

    let log_output = logs.join("");

    LoggerEntry::new(
        "docker",
        "docker_image_loaded",
        IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
    )
    .set_success()
    .module(AgentLogModule::Docker)
    .metadata(json!({
        "message_key": "platformLog.docker.image.loaded",
    }))
    .finish(&state.metadata_db);

    Ok(
        ApiResponse::success_with_raw("Docker image loaded successfully", Some(log_output))
            .into_response(),
    )
}
