//! 数据库：单库连接池初始化与迁移执行。

use crate::config;
use crate::state::DbPool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::str::FromStr;

/// 初始化 `seclab` 服务的单一数据库连接池。
///
/// 当前 `seclab` 服务采用单服务单库设计，所有核心业务、平台日志与通知历史
/// 统一落在同一个 SQLite 文件中管理。
pub async fn init_db_pool() -> DbPool {
    let db_path = config::data_dir().join("seclab.db");
    create_pool(db_path.to_string_lossy().as_ref()).await
}

/// 创建一个 SQLite 连接池。
///
/// 此函数负责：
/// 1. 确保数据库文件所在的目录存在。
/// 2. 使用优化的 SQLite 连接选项 (`sqlite_options`) 创建一个 `SqlitePool`。
/// 3. 在连接池创建后，自动运行数据库迁移脚本。
///
/// # 参数
/// - `path`: 数据库文件的路径。
///
/// # 返回
/// 一个初始化完成并已运行迁移的 `DbPool` (`sqlx::Pool<Sqlite>`)。
async fn create_pool(path: &str) -> DbPool {
    // 创建目录
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent).unwrap();
    }

    let url = format!("sqlite:{}", path);

    let pool = SqlitePoolOptions::new()
        .max_connections(1) // SQLite 在 WAL 模式下单连接性能已经很好
        .connect_with(sqlite_options(&url))
        .await
        .unwrap();

    tracing::info!("Running database migrations for {}...", path);
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    tracing::info!("Database migrations for {} completed.", path);

    pool
}

/// 为 SQLite 连接提供一组优化的默认选项。
///
/// 这组选项旨在提高并发性能和数据安全性，适用于服务端应用。
///
/// # 配置选项
/// - **`journal_mode(SqliteJournalMode::Wal)`**: 启用预写日志（Write-Ahead Logging）模式。
///   这是 SQLite 在高并发读写场景下的首选模式，能显著提高性能。
/// - **`synchronous(SqliteSynchronous::Normal)`**: 设置同步等级为 "Normal"。
///   在 WAL 模式下，这是性能和安全性之间的良好平衡。
/// - **`foreign_keys(true)`**: 强制执行外键约束，保证数据完整性。
/// - **`auto_vacuum(sqlx::sqlite::SqliteAutoVacuum::Incremental)`**: 启用增量 vacuum，
///   有助于在删除数据后自动回收数据库空间。
/// - **`busy_timeout(std::time::Duration::from_secs(5))`**: 当数据库被锁定时，
///   设置一个 5 秒的等待超时，以减少因并发写入导致的 "database is locked" 错误。
///
/// # 参数
/// - `path`: 数据库文件的路径，用于构建连接选项。
///
/// # 返回
/// 一个配置好的 `SqliteConnectOptions` 实例。
fn sqlite_options(path: &str) -> SqliteConnectOptions {
    SqliteConnectOptions::from_str(&format!("sqlite:{path}"))
        .unwrap()
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .auto_vacuum(sqlx::sqlite::SqliteAutoVacuum::Incremental)
        .busy_timeout(std::time::Duration::from_secs(5))
}
