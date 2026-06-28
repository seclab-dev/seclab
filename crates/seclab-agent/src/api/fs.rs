//! 文件系统 API：目录浏览、读取与上传等文件操作接口。

use crate::state::AppState;
use crate::types::{ApiError, ApiResponse, ApiResult};
use axum::{
    Json, Router,
    extract::{Multipart, Query},
    http::header,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;
use tracing::info;

/// 目录列表查询参数。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    pub path: String,
    #[serde(default)]
    pub recursive: bool,
    #[serde(default)]
    #[serde(alias = "show_hidden")]
    pub show_hidden: bool,
}

/// 文件系统条目返回结构。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsEntry {
    pub name: String,
    pub path: String,
    pub entry_type: String,
    pub size: u64,
    pub modified: Option<u64>,
    pub created: Option<u64>,
}

/// 读取文件内容的查询参数。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadQuery {
    pub path: String,
}

/// 文件读取结果与文本标记。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadResponse {
    pub path: String,
    pub content: String,
    pub is_text: bool,
}

/// 写入文件的请求参数。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteRequest {
    pub path: String,
    pub content: String,
    #[serde(default)]
    #[serde(alias = "create_if_missing")]
    pub create_if_missing: bool,
    #[serde(default)]
    pub overwrite: bool,
}

/// 创建目录的请求参数。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MkdirRequest {
    pub path: String,
    #[serde(default)]
    pub recursive: bool,
}

/// 删除文件或目录的请求参数。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveRequest {
    pub path: String,
    #[serde(default)]
    pub recursive: bool,
}

/// 重命名文件或目录的请求参数。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameRequest {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub overwrite: bool,
}

/// 复制文件或目录的请求参数。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyRequest {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub overwrite: bool,
}

/// 上传文件时的查询参数。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadQuery {
    pub path: String,
    #[serde(default)]
    pub overwrite: bool,
}

/// 上传结果，包含已保存文件列表。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadResult {
    pub saved: Vec<String>,
}

/// 返回用户主目录路径的响应。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeResponse {
    pub path: String,
}

fn normalize_absolute_path(path: &str) -> ApiResult<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("path must not be empty".to_string()));
    }
    let path_buf = PathBuf::from(trimmed);
    if !path_buf.is_absolute() {
        return Err(ApiError::BadRequest("path must be absolute".to_string()));
    }
    if path_buf
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(ApiError::BadRequest(
            "path contains invalid parent directory reference".to_string(),
        ));
    }
    Ok(path_buf)
}

fn file_name_only(name: &str) -> ApiResult<&str> {
    let file_name = Path::new(name)
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(|| ApiError::BadRequest("invalid file name".to_string()))?;
    Ok(file_name)
}

fn to_timestamp(time: Option<SystemTime>) -> Option<u64> {
    time.and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

/// 列出目录内容，支持递归与隐藏文件控制。
pub async fn list_entries(Query(query): Query<ListQuery>) -> ApiResult<Response> {
    let root = normalize_absolute_path(&query.path)?;
    info!("Requesting file list: {}", root.display());

    let mut results = Vec::new();
    let mut queue = VecDeque::new();
    let root_meta = tokio::fs::symlink_metadata(&root)
        .await
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => ApiError::ResourceNotFound,
            _ => ApiError::Io(err),
        })?;

    if root_meta.is_dir() {
        queue.push_back(root);
    } else {
        let name = root
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or_default()
            .to_string();
        if query.show_hidden || !name.starts_with('.') {
            results.push(FsEntry {
                name,
                path: root.to_string_lossy().to_string(),
                entry_type: "file".to_string(),
                size: root_meta.len(),
                modified: to_timestamp(root_meta.modified().ok()),
                created: to_timestamp(root_meta.created().ok()),
            });
        }
        return Ok(
            ApiResponse::success_with_raw("File list loaded", Some(results)).into_response(),
        );
    }

    while let Some(current) = queue.pop_front() {
        let mut read_dir = tokio::fs::read_dir(&current).await.map_err(ApiError::Io)?;
        while let Some(entry) = read_dir.next_entry().await.map_err(ApiError::Io)? {
            let path = entry.path();
            let name = entry.file_name().to_str().unwrap_or_default().to_string();

            if !query.show_hidden && name.starts_with('.') {
                continue;
            }

            let metadata = tokio::fs::symlink_metadata(&path)
                .await
                .map_err(ApiError::Io)?;
            let file_type = metadata.file_type();
            let entry_type = if file_type.is_dir() {
                "dir"
            } else if file_type.is_file() {
                "file"
            } else if file_type.is_symlink() {
                "symlink"
            } else {
                "other"
            };

            let size = if file_type.is_file() {
                metadata.len()
            } else {
                0
            };
            let modified = to_timestamp(metadata.modified().ok());
            let created = to_timestamp(metadata.created().ok());

            results.push(FsEntry {
                name: name.clone(),
                path: path.to_string_lossy().to_string(),
                entry_type: entry_type.to_string(),
                size,
                modified,
                created,
            });

            if query.recursive && file_type.is_dir() {
                queue.push_back(path);
            }
        }
    }

    Ok(ApiResponse::success_with_raw("File list loaded", Some(results)).into_response())
}

/// 读取文件内容并识别是否为文本。
pub async fn read_file(Query(query): Query<ReadQuery>) -> ApiResult<Response> {
    let path = normalize_absolute_path(&query.path)?;
    info!("Requesting file read: {}", path.display());

    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => ApiError::ResourceNotFound,
            _ => ApiError::Io(err),
        })?;
    match String::from_utf8(bytes) {
        Ok(content) => Ok(ApiResponse::success_with_raw(
            "File read",
            Some(ReadResponse {
                path: path.to_string_lossy().to_string(),
                content,
                is_text: true,
            }),
        )
        .into_response()),
        Err(_) => Ok(ApiResponse::success_with_raw(
            "File preview is not supported",
            Some(ReadResponse {
                path: path.to_string_lossy().to_string(),
                content: String::new(),
                is_text: false,
            }),
        )
        .into_response()),
    }
}

/// 写入文件内容，支持创建与覆盖控制。
pub async fn write_file(Json(payload): Json<WriteRequest>) -> ApiResult<Response> {
    let path = normalize_absolute_path(&payload.path)?;
    info!("Requesting file write: {}", path.display());

    let exists = tokio::fs::metadata(&path).await;
    if exists.is_err() && !payload.create_if_missing {
        return Err(ApiError::ResourceNotFound);
    }
    if exists.is_ok() && !payload.overwrite {
        return Err(ApiError::BadRequest("file already exists".to_string()));
    }

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(ApiError::Io)?;
    }

    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).truncate(true);
    if payload.create_if_missing || exists.is_err() {
        options.create(true);
    }

    let mut file = options.open(&path).await.map_err(ApiError::Io)?;
    file.write_all(payload.content.as_bytes())
        .await
        .map_err(ApiError::Io)?;

    Ok(ApiResponse::ok("File written").into_response())
}

/// 创建目录，支持递归创建。
pub async fn mkdir(Json(payload): Json<MkdirRequest>) -> ApiResult<Response> {
    let path = normalize_absolute_path(&payload.path)?;
    info!("Requesting mkdir: {}", path.display());

    if payload.recursive {
        tokio::fs::create_dir_all(&path)
            .await
            .map_err(ApiError::Io)?;
    } else {
        tokio::fs::create_dir(&path).await.map_err(ApiError::Io)?;
    }
    Ok(ApiResponse::ok("Directory created").into_response())
}

/// 删除文件或目录，支持递归删除。
pub async fn remove_path(Json(payload): Json<RemoveRequest>) -> ApiResult<Response> {
    let path = normalize_absolute_path(&payload.path)?;
    info!("Requesting remove: {}", path.display());

    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => ApiError::ResourceNotFound,
            _ => ApiError::Io(err),
        })?;

    if metadata.is_dir() {
        if payload.recursive {
            tokio::fs::remove_dir_all(&path)
                .await
                .map_err(ApiError::Io)?;
        } else {
            tokio::fs::remove_dir(&path).await.map_err(ApiError::Io)?;
        }
    } else {
        tokio::fs::remove_file(&path).await.map_err(ApiError::Io)?;
    }

    Ok(ApiResponse::ok("Path removed").into_response())
}

/// 重命名或移动文件与目录。
pub async fn rename_path(Json(payload): Json<RenameRequest>) -> ApiResult<Response> {
    let from = normalize_absolute_path(&payload.from)?;
    let to = normalize_absolute_path(&payload.to)?;
    info!("Requesting rename: {} -> {}", from.display(), to.display());

    if payload.overwrite
        && let Ok(meta) = tokio::fs::metadata(&to).await
    {
        if meta.is_dir() {
            tokio::fs::remove_dir_all(&to).await.map_err(ApiError::Io)?;
        } else {
            tokio::fs::remove_file(&to).await.map_err(ApiError::Io)?;
        }
    }

    tokio::fs::rename(&from, &to).await.map_err(ApiError::Io)?;
    Ok(ApiResponse::ok("Path renamed").into_response())
}

/// 复制文件或目录（简单版，目前仅支持单文件复制）。
pub async fn copy_path(Json(payload): Json<CopyRequest>) -> ApiResult<Response> {
    let from = normalize_absolute_path(&payload.from)?;
    let to = normalize_absolute_path(&payload.to)?;
    info!("Requesting copy: {} -> {}", from.display(), to.display());

    if payload.overwrite
        && let Ok(meta) = tokio::fs::metadata(&to).await
    {
        if meta.is_dir() {
            tokio::fs::remove_dir_all(&to).await.map_err(ApiError::Io)?;
        } else {
            tokio::fs::remove_file(&to).await.map_err(ApiError::Io)?;
        }
    }

    tokio::fs::copy(&from, &to).await.map_err(ApiError::Io)?;
    Ok(ApiResponse::ok("Path copied").into_response())
}

/// 接收多段上传并写入目标目录。
pub async fn upload(
    Query(query): Query<UploadQuery>,
    mut multipart: Multipart,
) -> ApiResult<Response> {
    let target_dir = normalize_absolute_path(&query.path)?;
    info!("Requesting upload to: {}", target_dir.display());

    let dir_meta = tokio::fs::metadata(&target_dir)
        .await
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => ApiError::ResourceNotFound,
            _ => ApiError::Io(err),
        })?;
    if !dir_meta.is_dir() {
        return Err(ApiError::BadRequest(
            "target path is not a directory".to_string(),
        ));
    }

    let mut saved = Vec::new();

    while let Some(field) = multipart.next_field().await.map_err(ApiError::Multipart)? {
        let file_name = field
            .file_name()
            .ok_or(ApiError::MissingFileName)
            .and_then(file_name_only)?;
        let dest = target_dir.join(file_name);

        if !query.overwrite && tokio::fs::metadata(&dest).await.is_ok() {
            return Err(ApiError::BadRequest("file already exists".to_string()));
        }

        let data = field.bytes().await.map_err(ApiError::Multipart)?;
        tokio::fs::write(&dest, &data).await.map_err(ApiError::Io)?;
        saved.push(dest.to_string_lossy().to_string());
    }

    Ok(
        ApiResponse::success_with_raw("Upload completed", Some(UploadResult { saved }))
            .into_response(),
    )
}

/// 以附件形式下载指定文件。
pub async fn download(Query(query): Query<ReadQuery>) -> ApiResult<Response> {
    let path = normalize_absolute_path(&query.path)?;
    info!("Requesting download: {}", path.display());

    let data = tokio::fs::read(&path)
        .await
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => ApiError::ResourceNotFound,
            _ => ApiError::Io(err),
        })?;

    let file_name = path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("download");

    let response = Response::builder()
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", file_name),
        )
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(axum::body::Body::from(data))
        .map_err(ApiError::AxumError)?;

    Ok(response)
}

/// 返回当前用户的主目录路径。
pub async fn home_dir() -> ApiResult<Response> {
    let home = dirs::home_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/".to_string());
    let path = if home.is_empty() { "/" } else { home.as_str() };
    Ok(ApiResponse::success_with_raw(
        "Home directory loaded",
        Some(HomeResponse {
            path: path.to_string(),
        }),
    )
    .into_response())
}

/// 构建文件系统相关路由集合。
pub fn fs_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/ls", get(list_entries))
        .route("/home", get(home_dir))
        .route("/read", get(read_file))
        .route("/write", post(write_file))
        .route("/mkdir", post(mkdir))
        .route("/remove", post(remove_path))
        .route("/rename", post(rename_path))
        .route("/copy", post(copy_path))
        .route("/upload", post(upload))
        .route("/download", get(download))
}
