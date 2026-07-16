//! 脚本库仓储：脚本资产、修订、软删除、运行读模型与可靠 Agent 上报。

use crate::{
    state::DbPool,
    types::{ApiError, ApiResult, new_uuid_v7},
};
use chrono::{Duration, Utc};
use seclab_contracts::{
    api::ErrorCode,
    scripts::{
        AgentScriptRunReport, CreateScriptRequest, ScriptCapabilities, ScriptDetail,
        ScriptExecutionDefaults, ScriptLastRun, ScriptListPage, ScriptOutputStream,
        ScriptOwnership, ScriptOwnershipKind, ScriptRun, ScriptRunCapabilities,
        ScriptRunOutputChunk, ScriptRunOutputPage, ScriptRunOutputSummary, ScriptRunPage,
        ScriptRunStatus, ScriptSource, ScriptSummary, UpdateScriptRequest,
    },
};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, QueryBuilder, Row, Sqlite};

const ACTIVE_STATUSES: &[&str] = &["queued", "starting", "running", "cancelling"];

/// 可信脚本操作用户。
#[derive(Debug, Clone)]
pub struct ScriptActor {
    pub user_id: i64,
    pub name: String,
    pub client_ip: String,
    pub trace_id: String,
}

/// 脚本列表筛选条件。
pub struct ScriptListFilter<'a> {
    pub keyword: Option<&'a str>,
    pub page: u32,
    pub page_size: u32,
    pub sort_by: &'a str,
    pub sort_order: &'a str,
}

/// 运行列表筛选条件。
pub struct ScriptRunListFilter<'a> {
    pub script_id: Option<&'a str>,
    pub node_id: Option<&'a str>,
    pub status: Option<ScriptRunStatus>,
    pub page: u32,
    pub page_size: u32,
}

/// Master 脚本持久化行。
#[derive(Debug, Clone, FromRow)]
pub struct ScriptRow {
    pub script_id: String,
    pub name: String,
    pub description: Option<String>,
    pub language: String,
    pub content: String,
    pub content_size_bytes: i64,
    pub source_sha256: String,
    pub default_timeout_seconds: i64,
    pub revision: i64,
    pub ownership_kind: String,
    pub owner_id: Option<String>,
    pub owner_name: Option<String>,
    pub manager_path: Option<String>,
    pub updated_by_name: String,
    pub deleted_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub has_active_run: bool,
    pub last_run_id: Option<String>,
    pub last_run_node_id: Option<String>,
    pub last_run_status: Option<String>,
    pub last_run_finished_at: Option<String>,
}

/// Master 脚本运行持久化行。
#[derive(Debug, Clone, FromRow)]
pub struct ScriptRunRow {
    pub run_id: String,
    pub script_id: String,
    pub script_name: String,
    pub script_revision: i64,
    pub source_sha256: String,
    pub source_content: String,
    pub timeout_seconds: i64,
    pub node_id: String,
    pub node_name: String,
    pub status: String,
    pub phase: Option<String>,
    pub queued_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub exit_code: Option<i32>,
    pub error_code: Option<String>,
    pub error_summary: Option<String>,
    pub output_size_bytes: i64,
    pub output_truncated: bool,
    pub actor_user_id: i64,
    pub actor_name: String,
    pub client_ip: String,
    pub trace_id: String,
    pub terminal_logged_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

const SCRIPT_SELECT: &str = r#"
SELECT s.script_id, s.name, s.description, s.language, s.content, s.content_size_bytes,
       s.source_sha256, s.default_timeout_seconds, s.revision, s.ownership_kind,
       s.owner_id, s.owner_name, s.manager_path, s.updated_by_name, s.deleted_at,
       s.created_at, s.updated_at,
       EXISTS(SELECT 1 FROM script_runs ar WHERE ar.script_id = s.script_id
              AND ar.status IN ('queued','starting','running','cancelling')) AS has_active_run,
       lr.run_id AS last_run_id, lr.node_id AS last_run_node_id,
       lr.status AS last_run_status, lr.finished_at AS last_run_finished_at
FROM scripts s
LEFT JOIN script_runs lr ON lr.run_id = (
    SELECT r.run_id FROM script_runs r WHERE r.script_id = s.script_id
    ORDER BY r.queued_at DESC, r.run_id DESC LIMIT 1
)"#;

const SCRIPT_LIST_SELECT: &str = r#"
SELECT s.script_id, s.name, s.description, s.language, '' AS content, 0 AS content_size_bytes,
       '' AS source_sha256, s.default_timeout_seconds, s.revision, s.ownership_kind,
       s.owner_id, s.owner_name, s.manager_path, s.updated_by_name, s.deleted_at,
       s.created_at, s.updated_at,
       EXISTS(SELECT 1 FROM script_runs ar WHERE ar.script_id = s.script_id
              AND ar.status IN ('queued','starting','running','cancelling')) AS has_active_run,
       lr.run_id AS last_run_id, lr.node_id AS last_run_node_id,
       lr.status AS last_run_status, lr.finished_at AS last_run_finished_at
FROM scripts s
LEFT JOIN script_runs lr ON lr.run_id = (
    SELECT r.run_id FROM script_runs r WHERE r.script_id = s.script_id
    ORDER BY r.queued_at DESC, r.run_id DESC LIMIT 1
)"#;

/// 校验并规范化脚本名称。
pub fn normalize_name(name: &str) -> ApiResult<(String, String)> {
    let value = name.trim();
    if value.is_empty() || value.chars().count() > 80 || value.chars().any(char::is_control) {
        return Err(ApiError::bad_request(
            ErrorCode::ScriptInvalidName,
            "script name is invalid",
        ));
    }
    Ok((value.to_string(), value.to_lowercase()))
}

/// 校验脚本说明、正文和超时。
pub fn validate_fields(description: Option<&str>, content: &str, timeout: u32) -> ApiResult<()> {
    if description.is_some_and(|v| v.chars().count() > 500 || v.chars().any(|c| c == '\0')) {
        return Err(ApiError::bad_request(
            ErrorCode::ScriptInvalidDescription,
            "script description is invalid",
        ));
    }
    let size = content.len();
    if content.trim().is_empty() || size > 262_144 || content.contains('\0') {
        return Err(ApiError::bad_request(
            ErrorCode::ScriptInvalidContent,
            "script content is invalid",
        ));
    }
    if !(1..=86_400).contains(&timeout) {
        return Err(ApiError::bad_request(
            ErrorCode::ScriptInvalidContent,
            "script timeout is invalid",
        ));
    }
    Ok(())
}

/// 计算正文 SHA-256。
pub fn source_sha256(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

/// 分页查询未删除的自定义脚本。
pub async fn list(pool: &DbPool, filter: &ScriptListFilter<'_>) -> ApiResult<ScriptListPage> {
    let mut count = QueryBuilder::<Sqlite>::new(
        "SELECT COUNT(*) FROM scripts s WHERE s.deleted_at IS NULL AND s.ownership_kind = 'custom'",
    );
    push_script_filter(&mut count, filter.keyword);
    let total: i64 = count.build_query_scalar().fetch_one(pool).await?;

    let mut query = QueryBuilder::<Sqlite>::new(SCRIPT_LIST_SELECT);
    query.push(" WHERE s.deleted_at IS NULL AND s.ownership_kind = 'custom'");
    push_script_filter(&mut query, filter.keyword);
    let sort = match filter.sort_by {
        "name" => "s.name_key",
        _ => "s.updated_at",
    };
    let order = if filter.sort_order == "asc" {
        "ASC"
    } else {
        "DESC"
    };
    query
        .push(format!(
            " ORDER BY {sort} {order}, s.script_id {order} LIMIT "
        ))
        .push_bind(i64::from(filter.page_size))
        .push(" OFFSET ")
        .push_bind(i64::from((filter.page - 1) * filter.page_size));
    let rows: Vec<ScriptRow> = query.build_query_as().fetch_all(pool).await?;
    Ok(ScriptListPage {
        items: rows.into_iter().map(summary).collect::<ApiResult<_>>()?,
        page: filter.page,
        page_size: filter.page_size,
        total: total.max(0) as u64,
        loaded_at: now(),
    })
}

fn push_script_filter<'a>(query: &mut QueryBuilder<'a, Sqlite>, keyword: Option<&'a str>) {
    if let Some(keyword) = keyword.filter(|v| !v.is_empty()) {
        let pattern = format!("%{}%", keyword.to_lowercase());
        query
            .push(" AND (s.name_key LIKE ")
            .push_bind(pattern.clone())
            .push(" OR lower(COALESCE(s.description, '')) LIKE ")
            .push_bind(pattern)
            .push(")");
    }
}

/// 查询活动脚本详情。
pub async fn detail(pool: &DbPool, script_id: &str) -> ApiResult<ScriptDetail> {
    let row = script_row(pool, script_id, false)
        .await?
        .ok_or_else(script_not_found)?;
    detail_from_row(row)
}

/// 创建自定义脚本。
pub async fn create(
    pool: &DbPool,
    request: &CreateScriptRequest,
    actor: &ScriptActor,
) -> ApiResult<ScriptDetail> {
    let timeout = request.timeout_seconds.unwrap_or(300);
    let (name, name_key) = normalize_name(&request.name)?;
    validate_fields(request.description.as_deref(), &request.content, timeout)?;
    let id = new_uuid_v7();
    let timestamp = now();
    let result = sqlx::query("INSERT INTO scripts (script_id,name,name_key,description,language,content,content_size_bytes,source_sha256,default_timeout_seconds,revision,ownership_kind,created_by_user_id,created_by_name,updated_by_user_id,updated_by_name,created_at,updated_at) VALUES (?,?,?,?, 'shell', ?,?,?,?,1,'custom',?,?,?,?,?,?)")
        .bind(&id).bind(name).bind(name_key).bind(trim_description(request.description.as_deref()))
        .bind(&request.content).bind(request.content.len() as i64).bind(source_sha256(&request.content))
        .bind(i64::from(timeout)).bind(actor.user_id).bind(&actor.name).bind(actor.user_id)
        .bind(&actor.name).bind(&timestamp).bind(&timestamp).execute(pool).await;
    if let Err(error) = result {
        if is_unique(&error) {
            return Err(ApiError::conflict(
                ErrorCode::ScriptAlreadyExists,
                "script name already exists",
            ));
        }
        return Err(error.into());
    }
    detail(pool, &id).await
}

/// 以 revision 乐观锁更新脚本。
pub async fn update(
    pool: &DbPool,
    script_id: &str,
    request: &UpdateScriptRequest,
    actor: &ScriptActor,
) -> ApiResult<ScriptDetail> {
    let (name, name_key) = normalize_name(&request.name)?;
    validate_fields(
        request.description.as_deref(),
        &request.content,
        request.timeout_seconds,
    )?;
    let result = sqlx::query("UPDATE scripts SET name=?,name_key=?,description=?,content=?,content_size_bytes=?,source_sha256=?,default_timeout_seconds=?,revision=revision+1,updated_by_user_id=?,updated_by_name=?,updated_at=? WHERE script_id=? AND deleted_at IS NULL AND ownership_kind='custom' AND revision=?")
        .bind(name).bind(name_key).bind(trim_description(request.description.as_deref()))
        .bind(&request.content).bind(request.content.len() as i64).bind(source_sha256(&request.content))
        .bind(i64::from(request.timeout_seconds)).bind(actor.user_id).bind(&actor.name).bind(now())
        .bind(script_id).bind(request.expected_revision).execute(pool).await;
    match result {
        Ok(done) if done.rows_affected() == 1 => detail(pool, script_id).await,
        Ok(_) => {
            if script_row(pool, script_id, false).await?.is_none() {
                Err(script_not_found())
            } else {
                Err(ApiError::conflict(
                    ErrorCode::ScriptRevisionConflict,
                    "script revision changed",
                ))
            }
        }
        Err(error) if is_unique(&error) => Err(ApiError::conflict(
            ErrorCode::ScriptAlreadyExists,
            "script name already exists",
        )),
        Err(error) => Err(error.into()),
    }
}

/// 软删除脚本，保留终态运行历史。
pub async fn remove(pool: &DbPool, script_id: &str) -> ApiResult<()> {
    let row = script_row(pool, script_id, false)
        .await?
        .ok_or_else(script_not_found)?;
    if row.ownership_kind != "custom" {
        return Err(ApiError::conflict(
            ErrorCode::ScriptProtected,
            "script is managed",
        ));
    }
    if row.has_active_run {
        return Err(ApiError::conflict(
            ErrorCode::ScriptInUse,
            "script has an active run",
        ));
    }
    sqlx::query(
        "UPDATE scripts SET deleted_at=?, updated_at=? WHERE script_id=? AND deleted_at IS NULL",
    )
    .bind(now())
    .bind(now())
    .bind(script_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// 创建或恢复幂等的一次运行。
pub async fn create_run(
    pool: &DbPool,
    script_id: &str,
    node_id: &str,
    node_name: &str,
    timeout_override: Option<u32>,
    idempotency_key: &str,
    actor: &ScriptActor,
) -> ApiResult<(ScriptRunRow, bool)> {
    if let Some(existing) = run_by_idempotency(pool, idempotency_key).await? {
        if existing.script_id != script_id || existing.node_id != node_id {
            return Err(ApiError::conflict(
                ErrorCode::ScriptRunConflict,
                "idempotency key was already used for another script run",
            ));
        }
        return Ok((existing, false));
    }
    let script = script_row(pool, script_id, false)
        .await?
        .ok_or_else(script_not_found)?;
    let timeout = timeout_override.unwrap_or(script.default_timeout_seconds as u32);
    validate_fields(script.description.as_deref(), &script.content, timeout)?;
    let run_id = new_uuid_v7();
    let timestamp = now();
    let guard = format!("{script_id}:{node_id}");
    let result = sqlx::query("INSERT INTO script_runs (run_id,idempotency_key,script_id,script_name,script_revision,source_sha256,source_content,timeout_seconds,node_id,node_name,status,phase,queued_at,overlap_guard,actor_user_id,actor_name,client_ip,trace_id,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?,'queued','queued',?,?,?,?,?,?,?,?)")
        .bind(&run_id).bind(idempotency_key).bind(script_id).bind(&script.name).bind(script.revision)
        .bind(&script.source_sha256).bind(&script.content).bind(i64::from(timeout)).bind(node_id).bind(node_name)
        .bind(&timestamp).bind(guard).bind(actor.user_id).bind(&actor.name).bind(&actor.client_ip)
        .bind(&actor.trace_id).bind(&timestamp).bind(&timestamp).execute(pool).await;
    if let Err(error) = result {
        if is_unique(&error) {
            if let Some(existing) = run_by_idempotency(pool, idempotency_key).await? {
                return Ok((existing, false));
            }
            return Err(ApiError::conflict(
                ErrorCode::ScriptRunConflict,
                "script is already running on this node",
            ));
        }
        return Err(error.into());
    }
    Ok((required_run(pool, &run_id).await?, true))
}

/// 分页查询全局运行记录。
pub async fn list_runs(
    pool: &DbPool,
    filter: &ScriptRunListFilter<'_>,
) -> ApiResult<ScriptRunPage> {
    let mut count = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM script_runs WHERE 1=1");
    push_run_filter(&mut count, filter);
    let total: i64 = count.build_query_scalar().fetch_one(pool).await?;
    let mut query = QueryBuilder::<Sqlite>::new("SELECT * FROM script_runs WHERE 1=1");
    push_run_filter(&mut query, filter);
    query
        .push(" ORDER BY queued_at DESC, run_id DESC LIMIT ")
        .push_bind(i64::from(filter.page_size))
        .push(" OFFSET ")
        .push_bind(i64::from((filter.page - 1) * filter.page_size));
    let rows: Vec<ScriptRunRow> = query.build_query_as().fetch_all(pool).await?;
    Ok(ScriptRunPage {
        items: rows.into_iter().map(run_dto).collect::<ApiResult<_>>()?,
        page: filter.page,
        page_size: filter.page_size,
        total: total.max(0) as u64,
        loaded_at: now(),
    })
}

fn push_run_filter<'a>(query: &mut QueryBuilder<'a, Sqlite>, filter: &ScriptRunListFilter<'a>) {
    if let Some(id) = filter.script_id {
        query.push(" AND script_id=").push_bind(id);
    }
    if let Some(id) = filter.node_id {
        query.push(" AND node_id=").push_bind(id);
    }
    if let Some(status) = filter.status {
        query
            .push(" AND status=")
            .push_bind(run_status_text(status));
    }
}

/// 查询运行 DTO。
pub async fn run(pool: &DbPool, run_id: &str) -> ApiResult<ScriptRun> {
    run_dto(required_run(pool, run_id).await?)
}

/// 查询输出游标页。
pub async fn output(
    pool: &DbPool,
    run_id: &str,
    cursor: u64,
    limit: u32,
) -> ApiResult<ScriptRunOutputPage> {
    let run = required_run(pool, run_id).await?;
    let rows = sqlx::query("SELECT sequence,stream,content FROM script_run_output_chunks WHERE run_id=? AND sequence>=? ORDER BY sequence LIMIT ?")
        .bind(run_id).bind(cursor as i64).bind(i64::from(limit) + 1).fetch_all(pool).await?;
    let has_more = rows.len() > limit as usize;
    let items = rows
        .into_iter()
        .take(limit as usize)
        .map(|row| {
            Ok(ScriptRunOutputChunk {
                sequence: row.try_get::<i64, _>("sequence")?.max(0) as u64,
                stream: stream_from_text(row.try_get("stream")?)?,
                content: row.try_get("content")?,
            })
        })
        .collect::<ApiResult<Vec<_>>>()?;
    let next_cursor = if has_more {
        items.last().map(|v| v.sequence + 1)
    } else {
        None
    };
    Ok(ScriptRunOutputPage {
        run_id: run_id.to_string(),
        items,
        next_cursor,
        size_bytes: run.output_size_bytes.max(0) as u64,
        truncated: run.output_truncated,
    })
}

/// 请求取消活动运行。
pub async fn request_cancel(pool: &DbPool, run_id: &str) -> ApiResult<ScriptRunRow> {
    let run = required_run(pool, run_id).await?;
    if !ACTIVE_STATUSES.contains(&run.status.as_str()) {
        return Err(ApiError::conflict(
            ErrorCode::ScriptRunNotCancellable,
            "script run is not cancellable",
        ));
    }
    sqlx::query("UPDATE script_runs SET status='cancelling',phase='cancelling',updated_at=? WHERE run_id=? AND status IN ('queued','starting','running','cancelling')")
        .bind(now()).bind(run_id).execute(pool).await?;
    required_run(pool, run_id).await
}

/// 获取等待投递的运行。
pub async fn queued_runs(pool: &DbPool) -> ApiResult<Vec<ScriptRunRow>> {
    Ok(sqlx::query_as("SELECT * FROM script_runs WHERE status IN ('queued','cancelling') ORDER BY queued_at LIMIT 100").fetch_all(pool).await?)
}

/// Agent 已确认接收运行后，将 Master 投递状态推进为 starting。
pub async fn mark_dispatched(pool: &DbPool, run_id: &str) -> ApiResult<()> {
    sqlx::query("UPDATE script_runs SET status='starting',phase='starting',updated_at=? WHERE run_id=? AND status='queued'")
        .bind(now()).bind(run_id).execute(pool).await?;
    Ok(())
}

/// 节点在投递阶段失联时，将尚未启动的运行收敛为可追踪失败终态。
pub async fn fail_dispatch(pool: &DbPool, run_id: &str, summary: &str) -> ApiResult<()> {
    let timestamp = now();
    sqlx::query("UPDATE script_runs SET status='failed',phase=NULL,finished_at=?,error_code='SCRIPT_NODE_UNAVAILABLE',error_summary=?,overlap_guard=NULL,updated_at=? WHERE run_id=? AND status='queued'")
        .bind(&timestamp).bind(summary).bind(&timestamp).bind(run_id).execute(pool).await?;
    Ok(())
}

/// 保存并校验 Agent 运行上报。
pub async fn save_report(
    pool: &DbPool,
    session_node_id: &str,
    report: &AgentScriptRunReport,
) -> ApiResult<()> {
    let current = required_run(pool, &report.run_id).await?;
    if current.node_id != session_node_id || current.script_id != report.script_id {
        return Err(ApiError::conflict(
            ErrorCode::ScriptRunConflict,
            "script run does not belong to this node",
        ));
    }
    if current.script_revision != report.script_revision
        || current.source_sha256 != report.source_sha256
    {
        return Err(ApiError::conflict(
            ErrorCode::ScriptRunConflict,
            "script run report snapshot does not match",
        ));
    }
    validate_report_output(report)?;
    let current_status = run_status_from_text(&current.status)?;
    if current_status.is_terminal() {
        return Ok(());
    }
    if !transition_allowed(current_status, report.status) {
        return Err(ApiError::conflict(
            ErrorCode::ScriptRunConflict,
            "invalid script run status transition",
        ));
    }
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE script_runs SET status=?,phase=?,started_at=COALESCE(?,started_at),finished_at=?,exit_code=?,error_code=?,error_summary=?,output_size_bytes=?,output_truncated=?,overlap_guard=CASE WHEN ? IN ('succeeded','failed','timed_out','cancelled') THEN NULL ELSE overlap_guard END,updated_at=? WHERE run_id=?")
        .bind(run_status_text(report.status)).bind(if report.status.is_terminal(){None}else{report.phase.as_deref()})
        .bind(&report.started_at).bind(&report.finished_at).bind(report.exit_code).bind(&report.error_code)
        .bind(&report.error_summary).bind(report.output_size_bytes.min(262_144) as i64).bind(report.output_truncated)
        .bind(run_status_text(report.status)).bind(now()).bind(&report.run_id).execute(&mut *tx).await?;
    if !report.output_chunks.is_empty() {
        sqlx::query("DELETE FROM script_run_output_chunks WHERE run_id=?")
            .bind(&report.run_id)
            .execute(&mut *tx)
            .await?;
        for chunk in &report.output_chunks {
            sqlx::query("INSERT INTO script_run_output_chunks (run_id,sequence,stream,content,size_bytes) VALUES (?,?,?,?,?)")
                .bind(&report.run_id).bind(chunk.sequence as i64).bind(stream_text(chunk.stream)).bind(&chunk.content)
                .bind(chunk.content.len() as i64).execute(&mut *tx).await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

fn validate_report_output(report: &AgentScriptRunReport) -> ApiResult<()> {
    if report
        .error_summary
        .as_ref()
        .is_some_and(|value| value.len() > 500)
        || report
            .error_code
            .as_ref()
            .is_some_and(|value| value.len() > 80)
        || report.output_chunks.len() > 100
    {
        return Err(ApiError::conflict(
            ErrorCode::ScriptRunConflict,
            "script run report contains invalid summary fields",
        ));
    }
    let mut size = 0_u64;
    for (index, chunk) in report.output_chunks.iter().enumerate() {
        if chunk.sequence != index as u64 {
            return Err(ApiError::conflict(
                ErrorCode::ScriptRunConflict,
                "script run output sequence is invalid",
            ));
        }
        size = size
            .checked_add(chunk.content.len() as u64)
            .ok_or_else(|| {
                ApiError::conflict(
                    ErrorCode::ScriptRunConflict,
                    "script run output is too large",
                )
            })?;
    }
    if size > 262_144 || size != report.output_size_bytes {
        return Err(ApiError::conflict(
            ErrorCode::ScriptRunConflict,
            "script run output size does not match",
        ));
    }
    Ok(())
}

/// 原子领取一次终态操作日志。
pub async fn claim_terminal_audit(pool: &DbPool, run_id: &str) -> ApiResult<Option<ScriptRunRow>> {
    let timestamp = now();
    let done = sqlx::query("UPDATE script_runs SET terminal_logged_at=? WHERE run_id=? AND terminal_logged_at IS NULL AND status IN ('succeeded','failed','timed_out','cancelled')")
        .bind(&timestamp).bind(run_id).execute(pool).await?;
    if done.rows_affected() == 0 {
        return Ok(None);
    }
    Ok(Some(required_run(pool, run_id).await?))
}

/// 清理到期运行，并物理清理无历史引用的软删除脚本。
pub async fn cleanup(pool: &DbPool) -> ApiResult<()> {
    let cutoff = (Utc::now() - Duration::days(30)).to_rfc3339();
    sqlx::query("DELETE FROM script_runs WHERE status IN ('succeeded','failed','timed_out','cancelled') AND (finished_at < ? OR run_id IN (SELECT run_id FROM script_runs r2 WHERE r2.script_id=script_runs.script_id ORDER BY queued_at DESC LIMIT -1 OFFSET 100))")
        .bind(cutoff).execute(pool).await?;
    sqlx::query("DELETE FROM scripts WHERE deleted_at IS NOT NULL AND NOT EXISTS(SELECT 1 FROM script_runs r WHERE r.script_id=scripts.script_id)").execute(pool).await?;
    Ok(())
}

async fn script_row(
    pool: &DbPool,
    script_id: &str,
    include_deleted: bool,
) -> ApiResult<Option<ScriptRow>> {
    let mut query = QueryBuilder::<Sqlite>::new(SCRIPT_SELECT);
    query.push(" WHERE s.script_id=").push_bind(script_id);
    if !include_deleted {
        query.push(" AND s.deleted_at IS NULL");
    }
    Ok(query.build_query_as().fetch_optional(pool).await?)
}

async fn required_run(pool: &DbPool, run_id: &str) -> ApiResult<ScriptRunRow> {
    sqlx::query_as("SELECT * FROM script_runs WHERE run_id=?")
        .bind(run_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ApiError::not_found(ErrorCode::ScriptRunNotFound, "script run not found"))
}

async fn run_by_idempotency(pool: &DbPool, key: &str) -> ApiResult<Option<ScriptRunRow>> {
    Ok(
        sqlx::query_as("SELECT * FROM script_runs WHERE idempotency_key=?")
            .bind(key)
            .fetch_optional(pool)
            .await?,
    )
}

fn summary(row: ScriptRow) -> ApiResult<ScriptSummary> {
    let ownership = ownership_from_text(&row.ownership_kind)?;
    let last_run = match (row.last_run_id, row.last_run_node_id, row.last_run_status) {
        (Some(run_id), Some(node_id), Some(status)) => Some(ScriptLastRun {
            run_id,
            node_id,
            status: run_status_from_text(&status)?,
            finished_at: row.last_run_finished_at,
        }),
        _ => None,
    };
    Ok(ScriptSummary {
        script_id: row.script_id,
        name: row.name,
        description: row.description,
        language: row.language,
        revision: row.revision,
        ownership: ScriptOwnership {
            kind: ownership,
            owner_id: row.owner_id,
            owner_name: row.owner_name,
            manager_path: row.manager_path,
        },
        capabilities: capabilities(ownership, row.deleted_at.is_some(), row.has_active_run),
        last_run,
        created_at: row.created_at,
        updated_at: row.updated_at,
        updated_by: row.updated_by_name,
    })
}

fn detail_from_row(row: ScriptRow) -> ApiResult<ScriptDetail> {
    let source = ScriptSource {
        content: row.content.clone(),
        size_bytes: row.content_size_bytes.max(0) as u64,
        sha256: row.source_sha256.clone(),
    };
    let defaults = ScriptExecutionDefaults {
        timeout_seconds: row.default_timeout_seconds.clamp(1, 86_400) as u32,
    };
    Ok(ScriptDetail {
        summary: summary(row)?,
        source,
        execution_defaults: defaults,
    })
}

/// 将持久化行转换为公共运行 DTO。
pub fn run_dto(row: ScriptRunRow) -> ApiResult<ScriptRun> {
    let status = run_status_from_text(&row.status)?;
    Ok(ScriptRun {
        run_id: row.run_id,
        script_id: row.script_id,
        script_name: row.script_name,
        script_revision: row.script_revision,
        source_sha256: row.source_sha256,
        node_id: row.node_id,
        node_name: row.node_name,
        status,
        phase: if status.is_terminal() {
            None
        } else {
            row.phase
        },
        queued_at: row.queued_at,
        started_at: row.started_at,
        finished_at: row.finished_at,
        exit_code: row.exit_code,
        error_code: row.error_code,
        error_summary: row.error_summary,
        output: ScriptRunOutputSummary {
            available: row.output_size_bytes > 0,
            truncated: row.output_truncated,
            size_bytes: row.output_size_bytes.max(0) as u64,
        },
        capabilities: ScriptRunCapabilities {
            can_cancel: !status.is_terminal(),
        },
    })
}

/// 统一计算脚本能力。
pub fn capabilities(
    kind: ScriptOwnershipKind,
    deleted: bool,
    active_run: bool,
) -> ScriptCapabilities {
    let custom = kind == ScriptOwnershipKind::Custom && !deleted;
    ScriptCapabilities {
        can_update: custom,
        can_remove: custom && !active_run,
        can_clone: custom,
        can_run: custom,
    }
}

fn ownership_from_text(value: &str) -> ApiResult<ScriptOwnershipKind> {
    match value {
        "custom" => Ok(ScriptOwnershipKind::Custom),
        "compose" => Ok(ScriptOwnershipKind::Compose),
        "suite" => Ok(ScriptOwnershipKind::Suite),
        "system" => Ok(ScriptOwnershipKind::System),
        _ => Err(ApiError::internal("invalid script ownership")),
    }
}
fn run_status_text(value: ScriptRunStatus) -> &'static str {
    match value {
        ScriptRunStatus::Queued => "queued",
        ScriptRunStatus::Starting => "starting",
        ScriptRunStatus::Running => "running",
        ScriptRunStatus::Cancelling => "cancelling",
        ScriptRunStatus::Succeeded => "succeeded",
        ScriptRunStatus::Failed => "failed",
        ScriptRunStatus::TimedOut => "timed_out",
        ScriptRunStatus::Cancelled => "cancelled",
    }
}
pub fn run_status_from_text(value: &str) -> ApiResult<ScriptRunStatus> {
    match value {
        "queued" => Ok(ScriptRunStatus::Queued),
        "starting" => Ok(ScriptRunStatus::Starting),
        "running" => Ok(ScriptRunStatus::Running),
        "cancelling" => Ok(ScriptRunStatus::Cancelling),
        "succeeded" => Ok(ScriptRunStatus::Succeeded),
        "failed" => Ok(ScriptRunStatus::Failed),
        "timed_out" => Ok(ScriptRunStatus::TimedOut),
        "cancelled" => Ok(ScriptRunStatus::Cancelled),
        _ => Err(ApiError::internal("invalid script run status")),
    }
}
fn stream_text(value: ScriptOutputStream) -> &'static str {
    match value {
        ScriptOutputStream::Stdout => "stdout",
        ScriptOutputStream::Stderr => "stderr",
    }
}
fn stream_from_text(value: &str) -> ApiResult<ScriptOutputStream> {
    match value {
        "stdout" => Ok(ScriptOutputStream::Stdout),
        "stderr" => Ok(ScriptOutputStream::Stderr),
        _ => Err(ApiError::internal("invalid script output stream")),
    }
}
fn transition_allowed(from: ScriptRunStatus, to: ScriptRunStatus) -> bool {
    from == to
        || matches!(
            (from, to),
            (
                ScriptRunStatus::Queued,
                ScriptRunStatus::Starting
                    | ScriptRunStatus::Running
                    | ScriptRunStatus::Cancelling
                    | ScriptRunStatus::Succeeded
                    | ScriptRunStatus::Failed
                    | ScriptRunStatus::TimedOut
                    | ScriptRunStatus::Cancelled
            ) | (
                ScriptRunStatus::Starting,
                ScriptRunStatus::Running
                    | ScriptRunStatus::Cancelling
                    | ScriptRunStatus::Succeeded
                    | ScriptRunStatus::Failed
                    | ScriptRunStatus::TimedOut
                    | ScriptRunStatus::Cancelled
            ) | (
                ScriptRunStatus::Running,
                ScriptRunStatus::Cancelling
                    | ScriptRunStatus::Succeeded
                    | ScriptRunStatus::Failed
                    | ScriptRunStatus::TimedOut
                    | ScriptRunStatus::Cancelled
            ) | (
                ScriptRunStatus::Cancelling,
                ScriptRunStatus::Succeeded
                    | ScriptRunStatus::Failed
                    | ScriptRunStatus::TimedOut
                    | ScriptRunStatus::Cancelled
            )
        )
}
fn trim_description(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}
fn script_not_found() -> ApiError {
    ApiError::not_found(ErrorCode::ScriptNotFound, "script not found")
}
fn is_unique(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.is_unique_violation())
}
fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor() -> ScriptActor {
        ScriptActor {
            user_id: 1,
            name: "tester".into(),
            client_ip: "127.0.0.1".into(),
            trace_id: "trace".into(),
        }
    }
    fn request(name: &str) -> CreateScriptRequest {
        CreateScriptRequest {
            name: name.into(),
            description: Some("demo".into()),
            content: "echo ok".into(),
            timeout_seconds: Some(30),
        }
    }

    #[tokio::test]
    async fn script_crud_is_revisioned_unique_and_soft_deleted() {
        let pool = crate::test_support::setup_test_db().await;
        let first = create(&pool, &request("Alpha"), &actor()).await.unwrap();
        assert!(create(&pool, &request(" alpha "), &actor()).await.is_err());
        let updated = update(
            &pool,
            &first.summary.script_id,
            &UpdateScriptRequest {
                expected_revision: 1,
                name: "Beta".into(),
                description: None,
                content: "echo changed".into(),
                timeout_seconds: 40,
            },
            &actor(),
        )
        .await
        .unwrap();
        assert_eq!(updated.summary.revision, 2);
        assert!(
            update(
                &pool,
                &first.summary.script_id,
                &UpdateScriptRequest {
                    expected_revision: 1,
                    name: "Late".into(),
                    description: None,
                    content: "echo late".into(),
                    timeout_seconds: 40
                },
                &actor()
            )
            .await
            .is_err()
        );
        remove(&pool, &first.summary.script_id).await.unwrap();
        assert!(detail(&pool, &first.summary.script_id).await.is_err());
    }

    #[tokio::test]
    async fn run_is_idempotent_and_blocks_overlap_and_delete() {
        let pool = crate::test_support::setup_test_db().await;
        let script = create(&pool, &request("Run"), &actor()).await.unwrap();
        let (run, created) = create_run(
            &pool,
            &script.summary.script_id,
            "local",
            "local",
            None,
            "key-1",
            &actor(),
        )
        .await
        .unwrap();
        assert!(created);
        let (same, created) = create_run(
            &pool,
            &script.summary.script_id,
            "local",
            "local",
            None,
            "key-1",
            &actor(),
        )
        .await
        .unwrap();
        assert!(!created);
        assert_eq!(same.run_id, run.run_id);
        assert!(
            create_run(
                &pool,
                &script.summary.script_id,
                "local",
                "local",
                None,
                "key-2",
                &actor()
            )
            .await
            .is_err()
        );
        assert!(remove(&pool, &script.summary.script_id).await.is_err());
    }

    #[tokio::test]
    async fn agent_report_validates_snapshot_output_and_terminal_monotonicity() {
        let pool = crate::test_support::setup_test_db().await;
        let script = create(&pool, &request("Report"), &actor()).await.unwrap();
        let (run_row, _) = create_run(
            &pool,
            &script.summary.script_id,
            "local",
            "Local Node",
            None,
            "report-key",
            &actor(),
        )
        .await
        .unwrap();
        let report = AgentScriptRunReport {
            run_id: run_row.run_id.clone(),
            script_id: run_row.script_id.clone(),
            script_revision: run_row.script_revision,
            source_sha256: run_row.source_sha256.clone(),
            status: ScriptRunStatus::Succeeded,
            phase: None,
            started_at: Some(now()),
            finished_at: Some(now()),
            exit_code: Some(0),
            error_code: None,
            error_summary: None,
            output_size_bytes: 2,
            output_truncated: false,
            output_chunks: vec![ScriptRunOutputChunk {
                sequence: 0,
                stream: ScriptOutputStream::Stdout,
                content: "ok".into(),
            }],
        };
        let mut invalid = report.clone();
        invalid.source_sha256 = "0".repeat(64);
        assert!(save_report(&pool, "local", &invalid).await.is_err());
        let mut invalid = report.clone();
        invalid.output_size_bytes = 3;
        assert!(save_report(&pool, "local", &invalid).await.is_err());
        save_report(&pool, "local", &report).await.unwrap();
        let mut stale = report;
        stale.status = ScriptRunStatus::Running;
        stale.finished_at = None;
        stale.output_size_bytes = 0;
        stale.output_chunks.clear();
        save_report(&pool, "local", &stale).await.unwrap();
        assert_eq!(
            run(&pool, &run_row.run_id).await.unwrap().status,
            ScriptRunStatus::Succeeded
        );
    }
}
