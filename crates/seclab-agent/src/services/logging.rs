//! 日志服务：统一写入与输出逻辑。

pub use crate::models::logging::{AgentLog, AgentLogModule, AgentLogStatus};
use crate::state::DbPool;
use crate::types::ApiError;
use chrono::{DateTime, Utc};
use seclab_contracts::api::ErrorCode;
use seclab_contracts::logging::{PlatformLogList, PlatformLogQuery};
use seclab_contracts::telemetry::PlatformLogEntryDraft;
use serde_json::json;
use sqlx::types::{Json, JsonValue};
use std::net::IpAddr;
use tracing::{error, info};

/// 平台日志输入结构体（PlatformLogEntry）。
///
/// 该结构体在服务层充当数据传输对象（DTO），也是用于链式配置的 **构建者模式（Builder Pattern）** 的起点。
///
/// 它旨在收集所有必要的平台事件字段，并提供一个简洁的 `finish()` 方法，
/// 通过非阻塞的后台任务将数据异步写入 `agent_logs` 数据库表。
///
/// # 核心职责
/// 1.  封装一条待记录的平台事件的所有信息。
/// 2.  提供链式方法，确保日志字段设置的安全性、可读性和灵活性。
/// 3.  通过 `finish` 方法，将 I/O 操作卸载到后台，保证 API 响应速度。
///
/// # 字段说明 (与数据库表 logs 对齐)
/// * `user_id`: 操作用户的 ID。
/// * `username`: 执行操作的用户名。
/// * `module`: 所属业务模块 (LogModule)。
/// * `event`: 平台事件名。
/// * `target_type`: 操作目标类型 (例如：User, Container)。
/// * `target_id`: 操作目标的唯一 ID 或名称。
/// * `status`: 操作结果 (LogStatus::Success 或 LogStatus::Failed)。
/// * `client_ip`: 客户端 IP 地址。
/// * `metadata`: 存储详细上下文信息（JSONB）。
///
/// # 示例 (Example)
///
/// 记录一次**成功的**用户登录事件:
/// ```rust
/// use serde_json::json;
/// use crate::db::DbPool;
/// use std::net::IpAddr;
///
/// // 假设我们已经获取了用户UID、客户端IP和数据库连接池
/// let user_id = "1";
/// let client_ip: IpAddr = "192.168.1.1".parse().unwrap();
/// let db_pool: &DbPool = &state.pool;
///
/// LoggerEntry::new("admin", "user_login", client_ip)
///     .module(LogModule::Auth)
///     .target_type("user")
///     .target_id(user_id)
///     .user_id(user_id)
///     .metadata(json!({
///         "message_key": "platformLog.auth.userLogin.success",
///         "token_expiry_minutes": 43200
///     }))
///     .set_success()
///     .finish(db_pool);
/// ```
#[derive(Debug)]
pub struct LoggerEntry {
    pub inner: PlatformLogEntryDraft,
}

impl LoggerEntry {
    /// 启动平台日志记录的构建流程（Builder Pattern）。
    ///
    /// 此方法使用最少的参数构造一个半成品的日志记录实例，并设置合理的默认值。
    /// 在调用 `finish` 写入数据库前，需要通过链式方法完善日志细节。
    ///
    /// # Parameters (参数)
    /// * `username` - 执行操作的用户名。
    /// * `event` - 平台事件名，例如 "user_login" 或 "docker_container_started"。
    /// * `client_ip` - 发起请求的客户端 IP 地址。
    ///
    /// # Defaults (默认初始化值)
    /// * `module`: `LogModule::System` (系统模块)。
    /// * `target_type`: 空字符串 `""` (待补充)。
    /// * `target_id`: 空字符串 `""` (待补充)。
    /// * `status`: `LogStatus::Failed` (失败)，强制调用者在成功路径上显式设置状态。
    /// * `user_id` 和 `metadata`: 均初始化为 `None`。
    ///
    /// # Returns (返回值)
    /// 返回 `Self` (PlatformLogRecordArgs) 实例，可继续通过链式方法配置。
    pub fn new(username: &str, event: &str, client_ip: IpAddr) -> Self {
        Self {
            inner: PlatformLogEntryDraft::new(username, event, client_ip).source("agent"),
        }
    }

    /// 可选的 user_id 字段 (操作用户/目标用户的 UID)。
    pub fn user_id(mut self, id: i64) -> Self {
        self.inner = self.inner.user_id(id);
        self
    }

    /// 设置所属模块。
    pub fn module(mut self, module: AgentLogModule) -> Self {
        self.inner = self.inner.module(module);
        self
    }

    /// 操作目标类型。
    pub fn target_type(mut self, target_type: &str) -> Self {
        self.inner = self.inner.target_type(target_type);
        self
    }

    /// 操作目标的唯一 ID 或名称。
    pub fn target_id(mut self, id: &str) -> Self {
        self.inner = self.inner.target_id(id);
        self
    }

    /// 请求 trace_id。
    pub fn trace_id(mut self, trace_id: &str) -> Self {
        self.inner = self.inner.trace_id(trace_id);
        self
    }

    /// 请求上下文（方法 + 路径）。
    pub fn request(mut self, method: &str, request_path: &str) -> Self {
        self.inner = self.inner.request(method, request_path);
        self
    }

    /// 操作结果。
    pub fn status(mut self, status: AgentLogStatus) -> Self {
        self.inner = self.inner.status(status);
        self
    }

    /// 设置状态为成功
    pub fn set_success(mut self) -> Self {
        self.inner = self.inner.set_success();
        self
    }

    /// 可选的 metadata 字段 (额外信息，如请求体、错误详情)。
    pub fn metadata(mut self, data: JsonValue) -> Self {
        self.inner = self.inner.metadata(data);
        self
    }

    /// 将日志写入数据库。它会立即返回，不等待数据库操作完成。
    pub fn finish(self, pool: &DbPool) {
        log_async(pool, self);
    }
}

/// 在后台异步记录一条平台日志。
///
/// 该函数封装了 `tokio::spawn`、连接池克隆以及后台的错误日志记录，从而简化调用方的代码。
fn log_async(pool: &DbPool, args: LoggerEntry) {
    let pool_clone = pool.clone();

    // 提取关键信息用于外部日志，因为 args 的所有权将被移动
    let module = args.inner.module.as_str();
    let event = args.inner.event.clone();
    let status_str = args.inner.status.as_str().to_string();
    let ip = args.inner.client_ip;

    // 立即启动后台任务，使用 tokio::spawn 实现非阻塞
    tokio::spawn(async move {
        // 在实际执行前记录日志调用，便于调试
        info!(
            module,
            event,
            status = %status_str,
            client_ip = %ip,
            "Platform log write scheduled"
        );

        // 调用核心的记录函数
        if let Err(e) = record_log(&pool_clone, args).await {
            error!("Failed to record platform log in background: {:?}", e);
        }
    });
}

/// 核心日志记录函数，异步安全。
///
/// 此函数在业务逻辑的 tokio::spawn 块中调用，以避免阻塞主请求线程。
async fn record_log(pool: &DbPool, args: LoggerEntry) -> Result<(), ApiError> {
    let status = args.inner.status.as_str();
    let client_ip = args.inner.client_ip.to_string();
    let metadata_value = args.inner.metadata.unwrap_or_else(|| json!({}));
    let metadata_json = Json(metadata_value);

    let result = sqlx::query(
        r#"
    INSERT INTO platform_logs (
        user_id, username, module, event, target_type, target_id, status, client_ip,
        trace_id, source, request_path, method, metadata
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    "#,
    )
    .bind(args.inner.user_id)
    .bind(&args.inner.username)
    .bind(args.inner.module.as_str())
    .bind(&args.inner.event)
    .bind(&args.inner.target_type)
    .bind(&args.inner.target_id)
    .bind(status)
    .bind(&client_ip)
    .bind(&args.inner.trace_id)
    .bind(&args.inner.source)
    .bind(&args.inner.request_path)
    .bind(&args.inner.method)
    .bind(&metadata_json)
    .execute(pool)
    .await;

    // 平台日志写入失败不应影响主业务流程，仅打印错误
    if let Err(e) = result {
        error!("Failed to record logs log: {:?}", e);
    }

    Ok(())
}

pub type LogPayload = PlatformLogQuery;

/// 将前端传入的 Unix epoch milliseconds 转换为数据库使用的 UTC RFC3339 字符串。
fn epoch_millis_to_rfc3339(value: i64) -> Result<String, ApiError> {
    DateTime::<Utc>::from_timestamp_millis(value)
        .map(|dt| dt.to_rfc3339())
        .ok_or_else(|| ApiError::bad_request(ErrorCode::BadRequest, "invalid log time range"))
}

/// 从数据库获取平台日志列表，支持分页和按模块过滤。
pub async fn fetch_agent_logs(
    pool: &DbPool,
    payload: LogPayload,
) -> Result<PlatformLogList, ApiError> {
    let LogPayload {
        page,
        page_size,
        modules,
        events,
        event_prefixes,
        statuses,
        start_at,
        end_at,
        keyword,
    } = payload;

    let offset = ((page.saturating_sub(1)) * page_size) as i64;
    let limit = page_size as i64;

    let module_filters: Vec<String> = modules
        .as_ref()
        .map(|values| {
            values
                .iter()
                .map(|module| module.as_str().to_string())
                .collect()
        })
        .unwrap_or_default();
    let event_filters = events.unwrap_or_default();
    let event_prefix_filters: Vec<String> = event_prefixes
        .unwrap_or_default()
        .into_iter()
        .map(|prefix| format!("{prefix}%"))
        .collect();
    let status_filters = statuses.unwrap_or_default();
    let start_time = start_at.map(epoch_millis_to_rfc3339).transpose()?;
    let end_time = end_at.map(epoch_millis_to_rfc3339).transpose()?;
    let keyword_filter = keyword.and_then(|value| {
        let trimmed = value.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(format!("%{trimmed}%"))
        }
    });

    let mut conditions: Vec<String> = Vec::new();
    if !module_filters.is_empty() {
        conditions.push(format!(
            "module IN ({})",
            build_placeholders(module_filters.len())
        ));
    }
    if !event_filters.is_empty() {
        conditions.push(format!(
            "event IN ({})",
            build_placeholders(event_filters.len())
        ));
    }
    if !event_prefix_filters.is_empty() {
        let prefixes = (0..event_prefix_filters.len())
            .map(|_| "event LIKE ?")
            .collect::<Vec<_>>()
            .join(" OR ");
        conditions.push(format!("({prefixes})"));
    }
    if !status_filters.is_empty() {
        conditions.push(format!(
            "status IN ({})",
            build_placeholders(status_filters.len())
        ));
    }
    if start_time.is_some() {
        conditions.push("timestamp >= ?".to_string());
    }
    if end_time.is_some() {
        conditions.push("timestamp <= ?".to_string());
    }
    if keyword_filter.is_some() {
        conditions.push(
            "(username LIKE ? OR event LIKE ? OR target_id LIKE ? OR trace_id LIKE ? OR request_path LIKE ?)"
                .to_string(),
        );
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let count_query = format!("SELECT COUNT(*) FROM platform_logs{where_clause}");
    let data_query = format!(
        r#"
            SELECT
                id, user_id, username, module, event, target_type, target_id, timestamp, status, client_ip,
                trace_id, source, request_path, method, metadata
            FROM platform_logs
            {where_clause}
            ORDER BY timestamp DESC
            LIMIT ? OFFSET ?
        "#
    );

    let mut count_query_builder = sqlx::query_scalar::<_, i64>(&count_query);
    for filter in &module_filters {
        count_query_builder = count_query_builder.bind(filter);
    }
    for filter in &event_filters {
        count_query_builder = count_query_builder.bind(filter);
    }
    for filter in &event_prefix_filters {
        count_query_builder = count_query_builder.bind(filter);
    }
    for filter in &status_filters {
        count_query_builder = count_query_builder.bind(filter);
    }
    if let Some(value) = &start_time {
        count_query_builder = count_query_builder.bind(value);
    }
    if let Some(value) = &end_time {
        count_query_builder = count_query_builder.bind(value);
    }
    if let Some(value) = &keyword_filter {
        for _ in 0..5 {
            count_query_builder = count_query_builder.bind(value);
        }
    }
    let total = count_query_builder.fetch_one(pool).await?;

    let mut data_query_builder = sqlx::query_as::<_, AgentLog>(&data_query);
    for filter in &module_filters {
        data_query_builder = data_query_builder.bind(filter);
    }
    for filter in &event_filters {
        data_query_builder = data_query_builder.bind(filter);
    }
    for filter in &event_prefix_filters {
        data_query_builder = data_query_builder.bind(filter);
    }
    for filter in &status_filters {
        data_query_builder = data_query_builder.bind(filter);
    }
    if let Some(value) = &start_time {
        data_query_builder = data_query_builder.bind(value);
    }
    if let Some(value) = &end_time {
        data_query_builder = data_query_builder.bind(value);
    }
    if let Some(value) = &keyword_filter {
        for _ in 0..5 {
            data_query_builder = data_query_builder.bind(value);
        }
    }

    let logs: Vec<AgentLog> = data_query_builder
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

    Ok(PlatformLogList {
        total,
        page,
        page_size,
        logs,
    })
}

fn build_placeholders(len: usize) -> String {
    (0..len).map(|_| "?").collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::{LogPayload, fetch_agent_logs};
    use crate::models::logging::AgentLogModule;
    use crate::test_support::setup_test_db;
    async fn insert_log(
        pool: &crate::state::DbPool,
        module: &str,
        timestamp: &str,
        target_id: &str,
    ) {
        sqlx::query(
            r#"
            INSERT INTO platform_logs (
                user_id, username, module, event, target_type, target_id, status, client_ip,
                trace_id, source, request_path, method, metadata, timestamp
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(1_i64)
        .bind("tester")
        .bind(module)
        .bind("test_event")
        .bind("Target")
        .bind(target_id)
        .bind("SUCCESS")
        .bind("127.0.0.1")
        .bind(format!("trace-{target_id}"))
        .bind("agent_api")
        .bind("/api/v1/agent/docker/logs")
        .bind("POST")
        .bind("{}")
        .bind(timestamp)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn fetch_agent_logs_orders_and_paginates() {
        let pool = setup_test_db().await;
        insert_log(&pool, "Docker", "2024-01-02T00:00:00Z", "t1").await;
        insert_log(&pool, "System", "2024-01-01T00:00:00Z", "t2").await;
        insert_log(&pool, "Auth", "2024-01-03T00:00:00Z", "t3").await;

        let result = fetch_agent_logs(
            &pool,
            LogPayload {
                page: 1,
                page_size: 2,
                modules: None,
                events: None,
                event_prefixes: None,
                statuses: None,
                start_at: None,
                end_at: None,
                keyword: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(result.total, 3);
        assert_eq!(result.logs.len(), 2);
        assert_eq!(result.logs[0].target_id, "t3");
        assert_eq!(result.logs[1].target_id, "t1");
    }

    #[tokio::test]
    async fn fetch_agent_logs_filters_modules() {
        let pool = setup_test_db().await;
        insert_log(&pool, "Docker", "2024-01-02T00:00:00Z", "t1").await;
        insert_log(&pool, "System", "2024-01-01T00:00:00Z", "t2").await;

        let result = fetch_agent_logs(
            &pool,
            LogPayload {
                page: 1,
                page_size: 10,
                modules: Some(vec![AgentLogModule::Docker]),
                events: None,
                event_prefixes: None,
                statuses: None,
                start_at: None,
                end_at: None,
                keyword: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(result.total, 1);
        assert_eq!(result.logs.len(), 1);
        assert_eq!(result.logs[0].module, "Docker");
        assert_eq!(result.logs[0].target_id, "t1");
    }
}
