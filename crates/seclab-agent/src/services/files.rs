//! 文件管理同步领域：路径校验、元数据、分页、文本读取与原子写入。

use crate::{config, state::DbPool, types::ApiError};
use axum::http::StatusCode;
use chrono::{DateTime, SecondsFormat, Utc};
use once_cell::sync::Lazy;
use seclab_contracts::{
    api::ErrorCode,
    files::{
        FileCapabilities, FileContent, FileEntryCounts, FileEntryDetail, FileEntryKind,
        FileEntrySummary, FileHome, FileListPage, FileManagement, FileManagementKind,
    },
};
use sqlx::Row;
use std::{
    cmp::Ordering,
    ffi::CString,
    io,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Component, Path, PathBuf},
    time::SystemTime,
};
use tokio::{io::AsyncWriteExt, sync::Mutex};
use uuid::Uuid;

/// 文本读取与保存的最大 UTF-8 字节数。
pub const MAX_TEXT_BYTES: u64 = 4 * 1024 * 1024;
pub const DEFAULT_PAGE_SIZE: u32 = 50;
pub const MAX_PAGE_SIZE: u32 = 500;

static CONTENT_WRITE_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

/// 文件列表排序字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileSortBy {
    Name,
    ModifiedAt,
    SizeBytes,
}

/// 文件列表排序方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileSortOrder {
    Asc,
    Desc,
}

/// 已校验的目录列表参数。
pub struct FileListOptions {
    pub path: PathBuf,
    pub page: u32,
    pub page_size: u32,
    pub sort_by: FileSortBy,
    pub sort_order: FileSortOrder,
    pub show_hidden: bool,
}

#[derive(Debug, Clone)]
struct ManagementRoot {
    path: PathBuf,
    kind: FileManagementKind,
    owner_name: Option<String>,
    manage_via: &'static str,
}

/// 校验并进行纯词法规范化；文件管理允许访问任意绝对路径。
pub fn normalize_absolute_path(value: &str) -> Result<PathBuf, ApiError> {
    let value = value.trim();
    if value.is_empty() || value.as_bytes().contains(&0) {
        return Err(invalid_path("path must not be empty or contain NUL"));
    }
    let path = Path::new(value);
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(invalid_path(
            "path must be absolute and must not contain parent references",
        ));
    }
    let mut normalized = PathBuf::from("/");
    for component in path.components() {
        if let Component::Normal(segment) = component {
            normalized.push(segment);
        }
    }
    Ok(normalized)
}

/// 返回 Agent 有效主目录。
pub fn home() -> FileHome {
    let path = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    FileHome {
        path: path.to_string_lossy().into_owned(),
    }
}

/// 返回稳定排序并分页的当前目录摘要。
pub async fn list(pool: &DbPool, options: FileListOptions) -> Result<FileListPage, ApiError> {
    let metadata = tokio::fs::symlink_metadata(&options.path)
        .await
        .map_err(|error| map_io_error(error, "list directory"))?;
    if !metadata.is_dir() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::FileTypeUnsupported,
            "path is not a directory",
        ));
    }

    let roots = management_roots(pool).await?;
    let mut counts = FileEntryCounts::default();
    let mut entries = Vec::new();
    let mut reader = tokio::fs::read_dir(&options.path)
        .await
        .map_err(|error| map_io_error(error, "open directory"))?;
    while let Some(entry) = reader
        .next_entry()
        .await
        .map_err(|error| map_io_error(error, "read directory"))?
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !options.show_hidden && name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let metadata = tokio::fs::symlink_metadata(&path)
            .await
            .map_err(|error| map_io_error(error, "read file metadata"))?;
        let summary = summary_from_metadata(&path, name, &metadata, &roots);
        match summary.kind {
            FileEntryKind::File => counts.file_count += 1,
            FileEntryKind::Directory => counts.directory_count += 1,
            FileEntryKind::Symlink => counts.symlink_count += 1,
            FileEntryKind::Other => counts.other_count += 1,
        }
        entries.push(summary);
    }

    entries.sort_by(|left, right| compare_entries(left, right, &options));
    let total = entries.len() as u64;
    let start = usize::try_from((options.page - 1).saturating_mul(options.page_size))
        .unwrap_or(usize::MAX)
        .min(entries.len());
    let end = start
        .saturating_add(options.page_size as usize)
        .min(entries.len());
    let entries = entries[start..end].to_vec();

    Ok(FileListPage {
        path: options.path.to_string_lossy().into_owned(),
        entries,
        page: options.page,
        page_size: options.page_size,
        total,
        counts,
        loaded_at: timestamp(SystemTime::now()).unwrap_or_default(),
    })
}

/// 返回单个文件系统条目的按需详情。
pub async fn detail(pool: &DbPool, path: &Path) -> Result<FileEntryDetail, ApiError> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|error| map_io_error(error, "read file metadata"))?;
    let roots = management_roots(pool).await?;
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/".to_string());
    let symlink_target = if metadata.file_type().is_symlink() {
        Some(
            tokio::fs::read_link(path)
                .await
                .map_err(|error| map_io_error(error, "read symbolic link"))?
                .to_string_lossy()
                .into_owned(),
        )
    } else {
        None
    };
    Ok(FileEntryDetail {
        summary: summary_from_metadata(path, name, &metadata, &roots),
        mode: metadata.mode(),
        uid: metadata.uid(),
        gid: metadata.gid(),
        user_name: None,
        group_name: None,
        symlink_target,
    })
}

/// 读取有限大小的 UTF-8 普通文件，符号链接会解析到最终目标。
pub async fn read_content(path: &Path) -> Result<FileContent, ApiError> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|error| map_io_error(error, "read file metadata"))?;
    ensure_regular_file(&metadata)?;
    if metadata.len() > MAX_TEXT_BYTES {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            ErrorCode::FileContentTooLarge,
            "text file exceeds the 4 MiB editor limit",
        ));
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|error| map_io_error(error, "read file"))?;
    let content = String::from_utf8(bytes).map_err(|_| {
        ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ErrorCode::FileTypeUnsupported,
            "file content is not UTF-8 text",
        )
    })?;
    Ok(FileContent {
        path: path.to_string_lossy().into_owned(),
        size_bytes: metadata.len(),
        revision: revision(&metadata),
        modified_at: metadata.modified().ok().and_then(timestamp),
        content,
        encoding: "utf8".to_string(),
    })
}

/// 以排他方式创建普通文件，不隐式创建父目录。
pub async fn create_file(
    pool: &DbPool,
    path: &Path,
    content: &str,
) -> Result<FileEntryDetail, ApiError> {
    ensure_text_size(content)?;
    let mut options = tokio::fs::OpenOptions::new();
    let mut file = options
        .write(true)
        .create_new(true)
        .open(path)
        .await
        .map_err(|error| map_io_error(error, "create file"))?;
    if let Err(error) = file.write_all(content.as_bytes()).await {
        drop(file);
        let _ = tokio::fs::remove_file(path).await;
        return Err(map_io_error(error, "write new file"));
    }
    if let Err(error) = file.sync_all().await {
        drop(file);
        let _ = tokio::fs::remove_file(path).await;
        return Err(map_io_error(error, "sync new file"));
    }
    detail(pool, path).await
}

/// 创建目录，可选择递归创建父目录。
pub async fn create_directory(
    pool: &DbPool,
    path: &Path,
    recursive: bool,
) -> Result<FileEntryDetail, ApiError> {
    if tokio::fs::symlink_metadata(path).await.is_ok() {
        return Err(ApiError::conflict(
            ErrorCode::FileAlreadyExists,
            "target file or directory already exists",
        ));
    }
    if !recursive {
        tokio::fs::create_dir(path)
            .await
            .map_err(|error| map_io_error(error, "create directory"))?;
        return detail(pool, path).await;
    }

    let mut missing = Vec::new();
    let mut cursor = path;
    while tokio::fs::symlink_metadata(cursor).await.is_err() {
        missing.push(cursor.to_path_buf());
        cursor = cursor
            .parent()
            .ok_or_else(|| invalid_path("directory has no existing parent"))?;
    }
    let mut created = Vec::new();
    for directory in missing.into_iter().rev() {
        if let Err(error) = tokio::fs::create_dir(&directory).await {
            for created_directory in created.into_iter().rev() {
                let _ = tokio::fs::remove_dir(created_directory).await;
            }
            return Err(map_io_error(error, "create directory"));
        }
        created.push(directory);
    }
    detail(pool, path).await
}

/// 使用同目录临时文件和 revision 校验原子更新文本内容。
pub async fn update_content(
    path: &Path,
    content: &str,
    expected_revision: &str,
) -> Result<FileContent, ApiError> {
    ensure_text_size(content)?;
    let _guard = CONTENT_WRITE_LOCK.lock().await;
    let target = if tokio::fs::symlink_metadata(path)
        .await
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        tokio::fs::canonicalize(path)
            .await
            .map_err(|error| map_io_error(error, "resolve symbolic link"))?
    } else {
        path.to_path_buf()
    };
    let metadata = tokio::fs::metadata(&target)
        .await
        .map_err(|error| map_io_error(error, "read file metadata"))?;
    ensure_regular_file(&metadata)?;
    if revision(&metadata) != expected_revision {
        return Err(ApiError::conflict(
            ErrorCode::FileChanged,
            "file changed since it was loaded",
        ));
    }
    let parent = target
        .parent()
        .ok_or_else(|| invalid_path("file has no parent"))?;
    let temporary = parent.join(format!(".seclab-write-{}", Uuid::now_v7()));
    let result = write_temporary(&temporary, &target, &metadata, content).await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temporary).await;
    }
    result?;
    let mut response = read_content(&target).await?;
    response.path = path.to_string_lossy().into_owned();
    Ok(response)
}

async fn write_temporary(
    temporary: &Path,
    target: &Path,
    metadata: &std::fs::Metadata,
    content: &str,
) -> Result<(), ApiError> {
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary)
        .await
        .map_err(|error| map_io_error(error, "create temporary file"))?;
    tokio::fs::set_permissions(temporary, metadata.permissions())
        .await
        .map_err(|error| map_io_error(error, "set temporary file permissions"))?;
    preserve_owner(temporary, metadata.uid(), metadata.gid())?;
    file.write_all(content.as_bytes())
        .await
        .map_err(|error| map_io_error(error, "write temporary file"))?;
    file.sync_all()
        .await
        .map_err(|error| map_io_error(error, "sync temporary file"))?;
    let current = tokio::fs::metadata(target)
        .await
        .map_err(|error| map_io_error(error, "recheck file metadata"))?;
    if revision(&current) != revision(metadata) {
        return Err(ApiError::conflict(
            ErrorCode::FileChanged,
            "file changed while it was being saved",
        ));
    }
    tokio::fs::rename(temporary, target)
        .await
        .map_err(|error| map_io_error(error, "commit file update"))?;
    sync_directory(target.parent().unwrap_or(Path::new("/"))).await
}

fn preserve_owner(path: &Path, uid: u32, gid: u32) -> Result<(), ApiError> {
    let path =
        CString::new(path.as_os_str().as_bytes()).map_err(|_| invalid_path("path contains NUL"))?;
    // SAFETY: CString guarantees a terminating NUL and the values originate from metadata.
    let result = unsafe { libc::chown(path.as_ptr(), uid, gid) };
    if result == 0 {
        Ok(())
    } else {
        Err(map_io_error(
            io::Error::last_os_error(),
            "preserve file owner",
        ))
    }
}

async fn sync_directory(path: &Path) -> Result<(), ApiError> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || std::fs::File::open(path)?.sync_all())
        .await
        .map_err(|_| ApiError::internal("directory sync task failed"))?
        .map_err(|error| map_io_error(error, "sync parent directory"))
}

fn ensure_text_size(content: &str) -> Result<(), ApiError> {
    if content.len() as u64 <= MAX_TEXT_BYTES {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::PAYLOAD_TOO_LARGE,
        ErrorCode::FileContentTooLarge,
        "text content exceeds the 4 MiB editor limit",
    ))
}

fn ensure_regular_file(metadata: &std::fs::Metadata) -> Result<(), ApiError> {
    if metadata.is_file() {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        ErrorCode::FileTypeUnsupported,
        "operation requires a regular file",
    ))
}

fn summary_from_metadata(
    path: &Path,
    name: String,
    metadata: &std::fs::Metadata,
    roots: &[ManagementRoot],
) -> FileEntrySummary {
    let kind = kind(metadata);
    FileEntrySummary {
        name,
        path: path.to_string_lossy().into_owned(),
        kind,
        size_bytes: (kind == FileEntryKind::File).then_some(metadata.len()),
        modified_at: metadata.modified().ok().and_then(timestamp),
        created_at: metadata.created().ok().and_then(timestamp),
        revision: revision(metadata),
        management: management_for(path, roots),
        capabilities: capabilities_for(kind),
    }
}

fn kind(metadata: &std::fs::Metadata) -> FileEntryKind {
    let file_type = metadata.file_type();
    if file_type.is_file() {
        FileEntryKind::File
    } else if file_type.is_dir() {
        FileEntryKind::Directory
    } else if file_type.is_symlink() {
        FileEntryKind::Symlink
    } else {
        FileEntryKind::Other
    }
}

fn capabilities_for(kind: FileEntryKind) -> FileCapabilities {
    match kind {
        FileEntryKind::File => FileCapabilities {
            can_open: true,
            can_read: true,
            can_write: true,
            can_create_child: false,
            can_rename: true,
            can_copy: true,
            can_remove: true,
            can_upload: false,
            can_download: true,
        },
        FileEntryKind::Directory => FileCapabilities {
            can_open: true,
            can_read: true,
            can_write: false,
            can_create_child: true,
            can_rename: true,
            can_copy: true,
            can_remove: true,
            can_upload: true,
            can_download: false,
        },
        FileEntryKind::Symlink => FileCapabilities {
            can_open: true,
            can_read: true,
            can_write: true,
            can_create_child: false,
            can_rename: true,
            can_copy: true,
            can_remove: true,
            can_upload: false,
            can_download: false,
        },
        FileEntryKind::Other => FileCapabilities {
            can_rename: true,
            can_remove: true,
            ..FileCapabilities::default()
        },
    }
}

async fn management_roots(pool: &DbPool) -> Result<Vec<ManagementRoot>, ApiError> {
    let mut roots = Vec::new();
    let rows =
        sqlx::query("SELECT compose_dir, management_kind, owner_name FROM docker_compose_projects")
            .fetch_all(pool)
            .await
            .map_err(|error| ApiError::database(error.to_string()))?;
    for row in rows {
        let management: String = row.try_get("management_kind").unwrap_or_default();
        let (kind, manage_via) = match management.as_str() {
            "suite" => (FileManagementKind::Suite, "suiteCenter"),
            "system" => (FileManagementKind::System, "system"),
            _ => (FileManagementKind::Compose, "dockerProjects"),
        };
        roots.push(ManagementRoot {
            path: PathBuf::from(row.try_get::<String, _>("compose_dir").unwrap_or_default()),
            kind,
            owner_name: row.try_get("owner_name").ok().flatten(),
            manage_via,
        });
    }
    roots.push(ManagementRoot {
        path: config::data_dir(),
        kind: FileManagementKind::System,
        owner_name: Some("SecLab".to_string()),
        manage_via: "system",
    });
    if let Some(parent) = config::config_path().parent() {
        roots.push(ManagementRoot {
            path: parent.to_path_buf(),
            kind: FileManagementKind::System,
            owner_name: Some("SecLab".to_string()),
            manage_via: "system",
        });
    }
    roots.sort_by(|left, right| {
        right
            .path
            .components()
            .count()
            .cmp(&left.path.components().count())
    });
    Ok(roots)
}

fn management_for(path: &Path, roots: &[ManagementRoot]) -> FileManagement {
    if let Some(root) = roots.iter().find(|root| path.starts_with(&root.path)) {
        return FileManagement {
            kind: root.kind,
            owner_name: root.owner_name.clone(),
            manage_via: Some(root.manage_via.to_string()),
        };
    }
    FileManagement {
        kind: FileManagementKind::Custom,
        owner_name: None,
        manage_via: Some("fileManager".to_string()),
    }
}

fn compare_entries(
    left: &FileEntrySummary,
    right: &FileEntrySummary,
    options: &FileListOptions,
) -> Ordering {
    let directory_order = matches!(right.kind, FileEntryKind::Directory)
        .cmp(&matches!(left.kind, FileEntryKind::Directory));
    if directory_order != Ordering::Equal {
        return directory_order;
    }
    let value_order = match options.sort_by {
        FileSortBy::Name => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
        FileSortBy::ModifiedAt => compare_optional(&left.modified_at, &right.modified_at),
        FileSortBy::SizeBytes => compare_optional(&left.size_bytes, &right.size_bytes),
    };
    let value_order = if options.sort_order == FileSortOrder::Desc {
        value_order.reverse()
    } else {
        value_order
    };
    value_order.then_with(|| left.path.cmp(&right.path))
}

fn compare_optional<T: Ord>(left: &Option<T>, right: &Option<T>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

pub fn revision(metadata: &std::fs::Metadata) -> String {
    format!(
        "{:x}-{:x}-{:x}-{:x}-{:x}",
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec()
    )
}

fn timestamp(value: SystemTime) -> Option<String> {
    let value: DateTime<Utc> = value.into();
    Some(value.to_rfc3339_opts(SecondsFormat::Millis, true))
}

fn invalid_path(message: &'static str) -> ApiError {
    ApiError::bad_request(ErrorCode::FileInvalidPath, message)
}

pub fn map_io_error(error: io::Error, operation: &'static str) -> ApiError {
    match error.kind() {
        io::ErrorKind::NotFound => {
            ApiError::not_found(ErrorCode::FileNotFound, "file or directory does not exist")
        }
        io::ErrorKind::PermissionDenied => ApiError::forbidden(
            ErrorCode::FilePermissionDenied,
            "permission denied by the target filesystem",
        ),
        io::ErrorKind::AlreadyExists => ApiError::conflict(
            ErrorCode::FileAlreadyExists,
            "target file or directory already exists",
        ),
        _ if matches!(error.raw_os_error(), Some(libc::ENOSPC | libc::EDQUOT)) => ApiError::new(
            StatusCode::INSUFFICIENT_STORAGE,
            ErrorCode::FileStorageExhausted,
            "target filesystem has insufficient storage",
        ),
        _ => {
            tracing::error!(%error, operation, "file operation failed");
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::FileOperationFailed,
                "file operation failed",
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        async fn new(label: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("seclab-files-{label}-{}", Uuid::now_v7()));
            tokio::fs::create_dir(&path).await.unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn normalizes_absolute_paths_without_parent_references() {
        assert_eq!(
            normalize_absolute_path("//tmp/./demo").unwrap(),
            PathBuf::from("/tmp/demo")
        );
        for invalid in ["", "relative", "/tmp/../etc"] {
            assert!(normalize_absolute_path(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn special_files_do_not_advertise_content_operations() {
        let capabilities = capabilities_for(FileEntryKind::Other);
        assert!(!capabilities.can_read);
        assert!(!capabilities.can_copy);
        assert!(!capabilities.can_download);
        assert!(capabilities.can_remove);
        assert!(!capabilities_for(FileEntryKind::Symlink).can_download);
    }

    #[test]
    fn management_classification_uses_the_most_specific_root_without_restricting_capabilities() {
        let roots = vec![
            ManagementRoot {
                path: PathBuf::from("/srv/compose/suite"),
                kind: FileManagementKind::Suite,
                owner_name: Some("scanner".to_string()),
                manage_via: "suiteCenter",
            },
            ManagementRoot {
                path: PathBuf::from("/srv/compose"),
                kind: FileManagementKind::Compose,
                owner_name: Some("project".to_string()),
                manage_via: "dockerProjects",
            },
            ManagementRoot {
                path: PathBuf::from("/opt/seclab"),
                kind: FileManagementKind::System,
                owner_name: Some("SecLab".to_string()),
                manage_via: "system",
            },
        ];
        assert_eq!(
            management_for(Path::new("/srv/compose/suite/config"), &roots).kind,
            FileManagementKind::Suite
        );
        assert_eq!(
            management_for(Path::new("/srv/compose/project.yml"), &roots).kind,
            FileManagementKind::Compose
        );
        assert_eq!(
            management_for(Path::new("/opt/seclab/config.toml"), &roots).kind,
            FileManagementKind::System
        );
        assert_eq!(
            management_for(Path::new("/home/admin/file"), &roots).kind,
            FileManagementKind::Custom
        );
        assert!(capabilities_for(FileEntryKind::File).can_remove);
    }

    #[tokio::test]
    async fn list_uses_stable_server_pagination_and_real_counts() {
        let pool = crate::test_support::setup_test_db().await;
        let root = TestDirectory::new("list").await;
        tokio::fs::create_dir(root.0.join("directory"))
            .await
            .unwrap();
        tokio::fs::write(root.0.join("b.txt"), b"bb").await.unwrap();
        tokio::fs::write(root.0.join("a.txt"), b"a").await.unwrap();
        tokio::fs::write(root.0.join(".hidden"), b"hidden")
            .await
            .unwrap();

        let page = list(
            &pool,
            FileListOptions {
                path: root.0.clone(),
                page: 1,
                page_size: 2,
                sort_by: FileSortBy::Name,
                sort_order: FileSortOrder::Asc,
                show_hidden: false,
            },
        )
        .await
        .unwrap();
        assert_eq!(page.total, 3);
        assert_eq!(page.counts.directory_count, 1);
        assert_eq!(page.counts.file_count, 2);
        assert_eq!(page.entries[0].name, "directory");
        assert_eq!(page.entries[1].name, "a.txt");

        let second = list(
            &pool,
            FileListOptions {
                path: root.0.clone(),
                page: 2,
                page_size: 2,
                sort_by: FileSortBy::Name,
                sort_order: FileSortOrder::Asc,
                show_hidden: false,
            },
        )
        .await
        .unwrap();
        assert_eq!(second.entries[0].name, "b.txt");
    }

    #[tokio::test]
    async fn atomic_update_rejects_stale_revision_without_overwriting() {
        let root = TestDirectory::new("revision").await;
        let path = root.0.join("document.txt");
        tokio::fs::write(&path, b"first").await.unwrap();
        let stale_revision = revision(&tokio::fs::metadata(&path).await.unwrap());
        tokio::fs::write(&path, b"external-change").await.unwrap();

        let error = update_content(&path, "editor-change", &stale_revision)
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::FileChanged);
        assert_eq!(
            tokio::fs::read_to_string(&path).await.unwrap(),
            "external-change"
        );
        let temporary_count = std::fs::read_dir(&root.0)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".seclab-write-")
            })
            .count();
        assert_eq!(temporary_count, 0);
    }
}
