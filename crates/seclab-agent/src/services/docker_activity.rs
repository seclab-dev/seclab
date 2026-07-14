//! Docker 操作日志：负责规范化写入、筛选查询与保留期清理。

use crate::models::docker::{
    DockerActivityActor, DockerActivityActorKind, DockerActivityLevel, DockerActivityLogItem,
    DockerActivityLogPage, DockerActivityLogQuery, DockerActivityOutcome, DockerActivityTarget,
};
use crate::state::DbPool;
use crate::types::{ApiError, ApiResult};
use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use sqlx::{QueryBuilder, Row, Sqlite};
use std::time::Duration as StdDuration;

const MAX_PAGE_SIZE: u32 = 100;
const MAX_ERROR_CHARS: usize = 2_048;
const RETENTION_DAYS: i64 = 90;

/// 待写入的 Docker 操作日志。
#[derive(Debug, Clone)]
pub struct NewDockerActivity {
    pub actor_kind: DockerActivityActorKind,
    pub actor_name: String,
    pub client_ip: Option<String>,
    pub level: DockerActivityLevel,
    pub outcome: DockerActivityOutcome,
    pub event_code: String,
    pub target_kind: Option<String>,
    pub target_id: Option<String>,
    pub message_params: Value,
    pub error_message: Option<String>,
    pub trace_id: Option<String>,
}

/// 以 best-effort 方式写入一条 Docker 操作日志。
pub async fn record(pool: &DbPool, activity: NewDockerActivity) {
    let params = if activity.message_params.is_object() {
        activity.message_params
    } else {
        Value::Object(Default::default())
    };
    let error_message = activity.error_message.as_deref().map(sanitize_error);

    let result = sqlx::query(
        r#"
        INSERT INTO docker_activity_logs (
            actor_kind, actor_name, client_ip, level, outcome, event_code,
            target_kind, target_id, message_params, error_message, trace_id
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(activity.actor_kind.as_str())
    .bind(activity.actor_name.trim())
    .bind(activity.client_ip.unwrap_or_default())
    .bind(activity.level.as_str())
    .bind(activity.outcome.as_str())
    .bind(activity.event_code)
    .bind(activity.target_kind.unwrap_or_default())
    .bind(activity.target_id.unwrap_or_default())
    .bind(params.to_string())
    .bind(error_message)
    .bind(activity.trace_id.unwrap_or_default())
    .execute(pool)
    .await;

    if let Err(error) = result {
        tracing::error!(%error, "Failed to record Docker activity log");
    }
}

/// 查询 Docker 操作日志。
pub async fn query(
    pool: &DbPool,
    query: DockerActivityLogQuery,
) -> ApiResult<DockerActivityLogPage> {
    validate_query(&query)?;

    let page = query.page;
    let page_size = query.page_size;
    let offset = i64::from((page - 1) * page_size);
    let start_at = query.start_at.map(epoch_millis_to_rfc3339).transpose()?;
    let end_at = query.end_at.map(epoch_millis_to_rfc3339).transpose()?;
    let keyword = query
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());

    let mut count = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM docker_activity_logs");
    push_filters(
        &mut count,
        query.levels.as_deref(),
        query.actor_kinds.as_deref(),
        start_at.as_deref(),
        end_at.as_deref(),
        keyword,
    );
    let total: i64 = count.build_query_scalar().fetch_one(pool).await?;

    let mut data = QueryBuilder::<Sqlite>::new(
        "SELECT id, occurred_at, actor_kind, actor_name, client_ip, level, outcome, \
         event_code, target_kind, target_id, message_params, error_message, trace_id \
         FROM docker_activity_logs",
    );
    push_filters(
        &mut data,
        query.levels.as_deref(),
        query.actor_kinds.as_deref(),
        start_at.as_deref(),
        end_at.as_deref(),
        keyword,
    );
    data.push(" ORDER BY occurred_at DESC, id DESC LIMIT ")
        .push_bind(i64::from(page_size))
        .push(" OFFSET ")
        .push_bind(offset);

    let rows = data.build().fetch_all(pool).await?;
    let items = rows
        .into_iter()
        .map(|row| {
            let actor_kind = parse_actor_kind(row.get::<String, _>("actor_kind").as_str());
            let client_ip = non_empty(row.get("client_ip"));
            let target_kind: String = row.get("target_kind");
            let target_id: String = row.get("target_id");
            let occurred_at: DateTime<Utc> = row.get("occurred_at");
            DockerActivityLogItem {
                id: row.get("id"),
                occurred_at: occurred_at.timestamp(),
                actor: DockerActivityActor {
                    kind: actor_kind,
                    name: row.get("actor_name"),
                    client_ip,
                },
                level: parse_level(row.get::<String, _>("level").as_str()),
                outcome: parse_outcome(row.get::<String, _>("outcome").as_str()),
                event_code: row.get("event_code"),
                target: if target_kind.is_empty() && target_id.is_empty() {
                    None
                } else {
                    Some(DockerActivityTarget {
                        kind: target_kind,
                        id: target_id,
                    })
                },
                message_params: serde_json::from_str(
                    row.get::<String, _>("message_params").as_str(),
                )
                .unwrap_or_else(|_| Value::Object(Default::default())),
                error_message: row.get("error_message"),
                trace_id: non_empty(row.get("trace_id")),
            }
        })
        .collect();

    Ok(DockerActivityLogPage {
        total,
        page,
        page_size,
        items,
    })
}

/// 启动 Docker 操作日志保留期清理任务。
pub fn spawn_retention_worker(pool: DbPool) {
    tokio::spawn(async move {
        if let Err(error) = delete_expired(&pool, Utc::now()).await {
            tracing::error!(%error, "Failed to clean expired Docker activity logs");
        }
        let mut interval = tokio::time::interval(StdDuration::from_secs(24 * 60 * 60));
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(error) = delete_expired(&pool, Utc::now()).await {
                tracing::error!(%error, "Failed to clean expired Docker activity logs");
            }
        }
    });
}

async fn delete_expired(pool: &DbPool, now: DateTime<Utc>) -> Result<u64, sqlx::Error> {
    let cutoff = now - Duration::days(RETENTION_DAYS);
    let result = sqlx::query("DELETE FROM docker_activity_logs WHERE occurred_at < ?")
        .bind(cutoff.to_rfc3339())
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

fn validate_query(query: &DockerActivityLogQuery) -> ApiResult<()> {
    if query.page == 0 {
        return Err(ApiError::BadRequest(
            "page must be greater than zero".to_string(),
        ));
    }
    if query.page_size == 0 || query.page_size > MAX_PAGE_SIZE {
        return Err(ApiError::BadRequest(
            "pageSize must be between 1 and 100".to_string(),
        ));
    }
    if let (Some(start), Some(end)) = (query.start_at, query.end_at)
        && start > end
    {
        return Err(ApiError::BadRequest(
            "startAt must not be later than endAt".to_string(),
        ));
    }
    Ok(())
}

fn epoch_millis_to_rfc3339(value: i64) -> ApiResult<String> {
    DateTime::<Utc>::from_timestamp_millis(value)
        .map(|date| date.to_rfc3339())
        .ok_or_else(|| ApiError::BadRequest("invalid activity log time".to_string()))
}

fn push_filters<'a>(
    builder: &mut QueryBuilder<'a, Sqlite>,
    levels: Option<&'a [DockerActivityLevel]>,
    actor_kinds: Option<&'a [DockerActivityActorKind]>,
    start_at: Option<&'a str>,
    end_at: Option<&'a str>,
    keyword: Option<&'a str>,
) {
    let mut has_filter = false;

    let push_connector = |builder: &mut QueryBuilder<'a, Sqlite>, has_filter: &mut bool| {
        builder.push(if *has_filter { " AND " } else { " WHERE " });
        *has_filter = true;
    };

    if let Some(levels) = levels.filter(|values| !values.is_empty()) {
        push_connector(builder, &mut has_filter);
        builder.push("level IN (");
        let mut values = builder.separated(", ");
        for level in levels {
            values.push_bind(level.as_str());
        }
        values.push_unseparated(")");
    }
    if let Some(actor_kinds) = actor_kinds.filter(|values| !values.is_empty()) {
        push_connector(builder, &mut has_filter);
        builder.push("actor_kind IN (");
        let mut values = builder.separated(", ");
        for kind in actor_kinds {
            values.push_bind(kind.as_str());
        }
        values.push_unseparated(")");
    }
    if let Some(value) = start_at {
        push_connector(builder, &mut has_filter);
        builder.push("occurred_at >= ").push_bind(value);
    }
    if let Some(value) = end_at {
        push_connector(builder, &mut has_filter);
        builder.push("occurred_at <= ").push_bind(value);
    }
    if let Some(value) = keyword {
        push_connector(builder, &mut has_filter);
        let pattern = format!("%{value}%");
        builder
            .push("(actor_name LIKE ")
            .push_bind(pattern.clone())
            .push(" OR event_code LIKE ")
            .push_bind(pattern.clone())
            .push(" OR target_id LIKE ")
            .push_bind(pattern.clone())
            .push(" OR message_params LIKE ")
            .push_bind(pattern.clone())
            .push(" OR error_message LIKE ")
            .push_bind(pattern)
            .push(")");
    }
}

fn sanitize_error(value: &str) -> String {
    let mut sanitized: String = value.chars().take(MAX_ERROR_CHARS).collect();
    for key in ["password", "token", "authorization"] {
        sanitized = redact_assignment(&sanitized, key);
    }
    sanitized.chars().take(MAX_ERROR_CHARS).collect()
}

fn redact_assignment(value: &str, key: &str) -> String {
    let mut output = value.to_string();
    let mut search_from = 0;
    loop {
        let lower = output.to_ascii_lowercase();
        let Some(relative) = lower[search_from..].find(key) else {
            break;
        };
        let start = search_from + relative + key.len();
        let suffix = &lower[start..];
        let Some(separator) = suffix.find(['=', ':']) else {
            break;
        };
        let value_start = start + separator + 1;
        let value_end = lower[value_start..]
            .find([',', '&', ' ', '\n'])
            .map(|offset| value_start + offset)
            .unwrap_or(output.len());
        output.replace_range(value_start..value_end, "[REDACTED]");
        search_from = value_start + "[REDACTED]".len();
        if search_from >= lower.len() {
            break;
        }
    }
    output
}

fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn parse_actor_kind(value: &str) -> DockerActivityActorKind {
    match value {
        "system" => DockerActivityActorKind::System,
        _ => DockerActivityActorKind::User,
    }
}

fn parse_level(value: &str) -> DockerActivityLevel {
    match value {
        "warning" => DockerActivityLevel::Warning,
        "error" => DockerActivityLevel::Error,
        _ => DockerActivityLevel::Info,
    }
}

fn parse_outcome(value: &str) -> DockerActivityOutcome {
    match value {
        "failure" => DockerActivityOutcome::Failure,
        _ => DockerActivityOutcome::Success,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::setup_test_db;
    use serde_json::json;

    fn activity(target: &str, level: DockerActivityLevel) -> NewDockerActivity {
        NewDockerActivity {
            actor_kind: DockerActivityActorKind::User,
            actor_name: "admin".to_string(),
            client_ip: Some("127.0.0.1".to_string()),
            level,
            outcome: DockerActivityOutcome::Success,
            event_code: "container.start".to_string(),
            target_kind: Some("container".to_string()),
            target_id: Some(target.to_string()),
            message_params: json!({ "name": target }),
            error_message: None,
            trace_id: Some(format!("trace-{target}")),
        }
    }

    #[tokio::test]
    async fn query_filters_and_orders_deterministically() {
        let pool = setup_test_db().await;
        record(&pool, activity("first", DockerActivityLevel::Info)).await;
        record(&pool, activity("second", DockerActivityLevel::Warning)).await;

        let page = query(
            &pool,
            DockerActivityLogQuery {
                page: 1,
                page_size: 20,
                levels: Some(vec![DockerActivityLevel::Warning]),
                actor_kinds: None,
                start_at: None,
                end_at: None,
                keyword: Some("second".to_string()),
            },
        )
        .await
        .unwrap();

        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].target.as_ref().unwrap().id, "second");
    }

    #[tokio::test]
    async fn deletes_only_records_older_than_ninety_days() {
        let pool = setup_test_db().await;
        let now = Utc::now();
        for (target, occurred_at) in [
            ("expired", now - Duration::days(91)),
            ("boundary", now - Duration::days(90)),
            ("fresh", now - Duration::days(1)),
        ] {
            record(&pool, activity(target, DockerActivityLevel::Info)).await;
            sqlx::query("UPDATE docker_activity_logs SET occurred_at = ? WHERE target_id = ?")
                .bind(occurred_at.to_rfc3339())
                .bind(target)
                .execute(&pool)
                .await
                .unwrap();
        }

        assert_eq!(delete_expired(&pool, now).await.unwrap(), 1);
    }

    #[test]
    fn sanitizes_secrets_and_limits_error_length() {
        let value = format!("token=secret password:guess {}", "x".repeat(3_000));
        let sanitized = sanitize_error(&value);
        assert!(!sanitized.contains("secret"));
        assert!(!sanitized.contains("guess"));
        assert!(sanitized.chars().count() <= MAX_ERROR_CHARS);
    }
}
