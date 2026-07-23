//! 操作审计服务：事件归一化、best-effort 队列、查询与保留清理。

use std::{collections::BTreeMap, net::IpAddr, sync::OnceLock};

use axum::http::HeaderMap;
use chrono::{DateTime, Duration, Utc};
use seclab_contracts::logging::{
    AgentOperationEvent, OperationActor, OperationActorKind, OperationImpact,
    OperationLogCapabilities, OperationLogDetail, OperationLogPage, OperationLogQuery,
    OperationLogSummary, OperationModule, OperationOrigin, OperationOriginKind, OperationOutcome,
    OperationParameterValue, OperationTarget,
};
use seclab_contracts::notification::{
    NotificationAttentionLevel, NotificationCategory, NotificationCode,
};
use serde_json::Value;
use sqlx::{FromRow, QueryBuilder, Sqlite, Transaction};
use tokio::sync::mpsc;
use tracing::{error, warn};

use crate::{
    models::logging::{LogModule, LogStatus, PlatformLogLevel},
    state::DbPool,
    types::{ApiError, new_uuid_v7},
};

const QUEUE_CAPACITY: usize = 2_048;
const MAX_ERROR_SUMMARY_BYTES: usize = 512;
const RETENTION_DAYS: i64 = 180;
static WRITER: OnceLock<mpsc::Sender<StoredOperationEvent>> = OnceLock::new();

/// 初始化进程内唯一的操作日志写入队列。
pub fn init_operation_log_writer(pool: DbPool) {
    let (sender, mut receiver) = mpsc::channel::<StoredOperationEvent>(QUEUE_CAPACITY);
    if WRITER.set(sender).is_err() {
        return;
    }
    tokio::spawn(async move {
        while let Some(mut event) = receiver.recv().await {
            enrich_target_display_name(&pool, &mut event).await;
            if let Err(err) = insert_event(&pool, &event).await {
                error!(event_id = %event.event_id, error = %err, "Operation audit event write failed");
            }
        }
    });
}

/// 操作审计事件构建器。写入前会按注册策略收紧字段与敏感信息。
#[derive(Debug)]
pub struct OperationEventBuilder {
    event: StoredOperationEvent,
    raw_parameters: Option<Value>,
}

impl OperationEventBuilder {
    /// 创建默认失败事件，成功路径必须显式设置结果。
    pub fn new(display_name: &str, event_code: &str, client_ip: IpAddr) -> Self {
        let module = module_for_event(event_code);
        Self {
            event: StoredOperationEvent {
                event_id: new_uuid_v7(),
                occurred_at: Utc::now().to_rfc3339(),
                module,
                event_code: normalize_event_code(event_code),
                actor_kind: if display_name == "anonymous" {
                    "anonymous"
                } else if display_name == "system" {
                    "system"
                } else if display_name == "runtime-agent" {
                    "agent"
                } else {
                    "user"
                }
                .to_string(),
                actor_user_id: None,
                actor_display_name: truncate(display_name, 128),
                origin_kind: "master".to_string(),
                origin_node_id: None,
                origin_node_name: None,
                target_kind: None,
                target_id: None,
                target_display_name: None,
                target_ownership: None,
                outcome: OperationOutcome::Failure,
                impact: OperationImpact::Error,
                trace_id: new_uuid_v7(),
                task_id: None,
                client_ip: client_ip.to_string(),
                request_method: None,
                route_template: None,
                parameters_json: "{}".to_string(),
                error_code: None,
                error_summary: None,
            },
            raw_parameters: None,
        }
    }

    pub fn user_id(mut self, id: i64) -> Self {
        self.event.actor_user_id = Some(id);
        self
    }
    pub fn module(mut self, module: LogModule) -> Self {
        self.event.module = match module {
            LogModule::Auth => OperationModule::Auth,
            LogModule::Docker => OperationModule::Docker,
            LogModule::File => OperationModule::Files,
            LogModule::Process => OperationModule::Processes,
            LogModule::System => module_for_event(&self.event.event_code),
        };
        self
    }
    pub fn target_type(mut self, value: &str) -> Self {
        self.event.target_kind = non_empty(value, 64);
        self
    }
    pub fn target_id(mut self, value: &str) -> Self {
        self.event.target_id = non_empty(value, 256);
        self
    }
    pub fn target_display_name(mut self, value: &str) -> Self {
        self.event.target_display_name = non_empty(value, 256);
        self
    }
    pub fn task_id(mut self, value: &str) -> Self {
        self.event.task_id = non_empty(value, 128);
        self
    }
    pub fn trace_id(mut self, value: &str) -> Self {
        if !value.trim().is_empty() {
            self.event.trace_id = truncate(value.trim(), 128);
        }
        self
    }
    pub fn source(mut self, value: &str) -> Self {
        if value == "agent" {
            self.event.origin_kind = "agent".to_string();
            self.event.actor_kind = "agent".to_string();
        }
        self
    }
    pub fn request(mut self, method: &str, route_template: &str) -> Self {
        self.event.request_method = safe_method(method);
        self.event.route_template = safe_route_template(route_template);
        self
    }
    pub fn status(mut self, status: LogStatus) -> Self {
        match status {
            LogStatus::Success => self = self.set_success(),
            LogStatus::Failed => {
                self.event.outcome = OperationOutcome::Failure;
                self.event.impact = OperationImpact::Error;
            }
        }
        self
    }
    pub fn level(mut self, level: PlatformLogLevel) -> Self {
        if !matches!(
            self.event.outcome,
            OperationOutcome::Failure | OperationOutcome::TimedOut
        ) {
            self.event.impact = match level {
                PlatformLogLevel::Info => OperationImpact::Info,
                PlatformLogLevel::Warning => OperationImpact::Warning,
                PlatformLogLevel::Error => OperationImpact::Error,
            };
        }
        self
    }
    pub fn set_success(mut self) -> Self {
        self.event.outcome = OperationOutcome::Success;
        self.event.impact = if is_high_impact(&self.event.event_code) {
            OperationImpact::Warning
        } else {
            OperationImpact::Info
        };
        self
    }
    /// 设置完整终态；失败与超时始终归为错误影响。
    pub fn outcome(mut self, outcome: OperationOutcome) -> Self {
        self.event.outcome = outcome;
        self.event.impact = match outcome {
            OperationOutcome::Failure | OperationOutcome::TimedOut => OperationImpact::Error,
            OperationOutcome::Partial => OperationImpact::Warning,
            OperationOutcome::Canceled => OperationImpact::Info,
            OperationOutcome::Success => {
                if is_high_impact(&self.event.event_code) {
                    OperationImpact::Warning
                } else {
                    OperationImpact::Info
                }
            }
        };
        self
    }
    pub fn metadata(mut self, data: Value) -> Self {
        self.raw_parameters = Some(data);
        self
    }

    /// 非阻塞提交到统一队列；队列不可用时只记录运行警告，不改变业务结果。
    pub fn finish(mut self, _pool: &DbPool) {
        let (parameters, error_code, error_summary) =
            sanitize_parameters(self.raw_parameters.take());
        self.event.parameters_json =
            serde_json::to_string(&parameters).unwrap_or_else(|_| "{}".to_string());
        normalize_operation_target(&mut self.event, &parameters);
        self.event.error_code = error_code;
        self.event.error_summary = error_summary;
        match WRITER.get() {
            Some(sender) if sender.try_send(self.event).is_ok() => {}
            Some(_) => warn!("Operation audit queue is full; event was dropped"),
            None => warn!("Operation audit queue is not initialized; event was dropped"),
        }
    }
}

pub fn operation_log_success(
    username: &str,
    event: &str,
    client_ip: IpAddr,
) -> OperationEventBuilder {
    OperationEventBuilder::new(username, event, client_ip).set_success()
}

pub fn operation_log_failure(
    username: &str,
    event: &str,
    client_ip: IpAddr,
) -> OperationEventBuilder {
    OperationEventBuilder::new(username, event, client_ip)
}

/// 外部请求不能指定可信 trace id；Master 始终生成本地 UUIDv7。
pub fn resolve_trace_id(_headers: &HeaderMap) -> String {
    new_uuid_v7()
}

/// 查询操作日志摘要。
pub async fn query_operation_logs(
    pool: &DbPool,
    query: OperationLogQuery,
) -> Result<OperationLogPage, ApiError> {
    validate_query(&query)?;
    let offset = i64::from((query.page - 1) * query.page_size);
    let mut count = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM operation_logs");
    push_filters(&mut count, &query);
    let total = count.build_query_scalar::<i64>().fetch_one(pool).await?;

    let mut rows = QueryBuilder::<Sqlite>::new("SELECT * FROM operation_logs");
    push_filters(&mut rows, &query);
    rows.push(" ORDER BY occurred_at DESC, event_id DESC LIMIT ")
        .push_bind(i64::from(query.page_size))
        .push(" OFFSET ")
        .push_bind(offset);
    let items = rows
        .build_query_as::<OperationLogRow>()
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(OperationLogRow::into_summary)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(OperationLogPage {
        total,
        page: query.page,
        page_size: query.page_size,
        items,
    })
}

/// 按事件 ID 查询安全详情。
pub async fn get_operation_log(
    pool: &DbPool,
    event_id: &str,
) -> Result<Option<OperationLogDetail>, ApiError> {
    let row =
        sqlx::query_as::<_, OperationLogRow>("SELECT * FROM operation_logs WHERE event_id = ?")
            .bind(event_id)
            .fetch_optional(pool)
            .await?;
    row.map(OperationLogRow::into_detail).transpose()
}

/// 以机器认证得到的节点身份幂等接收 Agent 事件。
pub async fn store_agent_event(
    pool: &DbPool,
    node_id: &str,
    node_name: Option<&str>,
    event: AgentOperationEvent,
) -> Result<(), ApiError> {
    let parameters = sanitize_parameter_map(event.parameters);
    let mut stored = StoredOperationEvent {
        event_id: truncate(&event.event_id, 128),
        occurred_at: event.occurred_at,
        module: event.module,
        event_code: normalize_event_code(&event.event_code),
        actor_kind: actor_kind_value(event.actor.kind).to_string(),
        actor_user_id: event.actor.user_id,
        actor_display_name: truncate(&event.actor.display_name, 128),
        origin_kind: "agent".to_string(),
        origin_node_id: Some(truncate(node_id, 128)),
        origin_node_name: node_name.map(|v| truncate(v, 128)),
        target_kind: event.target.as_ref().map(|v| truncate(&v.kind, 64)),
        target_id: event.target.as_ref().map(|v| truncate(&v.id, 256)),
        target_display_name: event
            .target
            .as_ref()
            .and_then(|v| v.display_name.as_deref())
            .map(|v| truncate(v, 256)),
        target_ownership: event
            .target
            .as_ref()
            .and_then(|v| v.ownership.as_deref())
            .map(|v| truncate(v, 32)),
        outcome: event.outcome,
        impact: impact_for_outcome(event.outcome, event.impact),
        trace_id: truncate(&event.trace_id, 128),
        task_id: event.task_id.map(|v| truncate(&v, 128)),
        client_ip: sanitize_reported_client_ip(event.client_ip.as_deref()).unwrap_or_default(),
        request_method: None,
        route_template: None,
        parameters_json: serde_json::to_string(&parameters).unwrap_or_else(|_| "{}".to_string()),
        error_code: event.error_code.map(|v| truncate(&v, 128)),
        error_summary: event.error_summary.map(|v| redact_error(&v)),
    };
    normalize_operation_target(&mut stored, &parameters);
    enrich_target_display_name(pool, &mut stored).await;
    insert_event(pool, &stored).await
}

/// 分批删除超过 180 天的事件，避免长事务。
pub async fn prune_expired_operation_logs(pool: &DbPool) -> Result<u64, ApiError> {
    let cutoff = (Utc::now() - Duration::days(RETENTION_DAYS)).to_rfc3339();
    let result = sqlx::query("DELETE FROM operation_logs WHERE event_id IN (SELECT event_id FROM operation_logs WHERE occurred_at < ? ORDER BY occurred_at LIMIT 1000)")
        .bind(cutoff).execute(pool).await?;
    Ok(result.rows_affected())
}

/// 每日分批执行 180 天保留清理。
pub fn spawn_retention_worker(pool: DbPool) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
        loop {
            interval.tick().await;
            loop {
                match prune_expired_operation_logs(&pool).await {
                    Ok(1_000) => continue,
                    Ok(_) => break,
                    Err(error) => {
                        error!(%error, "Operation log retention cleanup failed");
                        break;
                    }
                }
            }
        }
    });
}

#[derive(Debug)]
struct StoredOperationEvent {
    event_id: String,
    occurred_at: String,
    module: OperationModule,
    event_code: String,
    actor_kind: String,
    actor_user_id: Option<i64>,
    actor_display_name: String,
    origin_kind: String,
    origin_node_id: Option<String>,
    origin_node_name: Option<String>,
    target_kind: Option<String>,
    target_id: Option<String>,
    target_display_name: Option<String>,
    target_ownership: Option<String>,
    outcome: OperationOutcome,
    impact: OperationImpact,
    trace_id: String,
    task_id: Option<String>,
    client_ip: String,
    request_method: Option<String>,
    route_template: Option<String>,
    parameters_json: String,
    error_code: Option<String>,
    error_summary: Option<String>,
}

#[derive(Debug, FromRow)]
struct OperationLogRow {
    event_id: String,
    occurred_at: String,
    module: String,
    event_code: String,
    actor_kind: String,
    actor_user_id: Option<i64>,
    actor_display_name: String,
    client_ip: String,
    origin_kind: String,
    origin_node_id: Option<String>,
    origin_node_name: Option<String>,
    target_kind: Option<String>,
    target_id: Option<String>,
    target_display_name: Option<String>,
    target_ownership: Option<String>,
    outcome: String,
    impact: String,
    trace_id: String,
    task_id: Option<String>,
    request_method: Option<String>,
    route_template: Option<String>,
    parameters_json: String,
    error_code: Option<String>,
    error_summary: Option<String>,
}

impl OperationLogRow {
    fn into_summary(self) -> Result<OperationLogSummary, ApiError> {
        let target = match (self.target_kind.clone(), self.target_id.clone()) {
            (Some(kind), Some(id)) => Some(OperationTarget {
                kind,
                id,
                display_name: self.target_display_name.clone(),
                ownership: self.target_ownership.clone(),
            }),
            _ => None,
        };
        Ok(OperationLogSummary {
            event_id: self.event_id,
            occurred_at: self.occurred_at,
            module: parse_module(&self.module)?,
            event_code: self.event_code,
            actor: OperationActor {
                kind: parse_actor_kind(&self.actor_kind)?,
                user_id: self.actor_user_id,
                display_name: self.actor_display_name,
            },
            client_ip: non_empty(&self.client_ip, 64),
            origin: OperationOrigin {
                kind: parse_origin_kind(&self.origin_kind)?,
                node_id: self.origin_node_id,
                node_name: self.origin_node_name,
            },
            target,
            outcome: parse_outcome(&self.outcome)?,
            impact: parse_impact(&self.impact)?,
            trace_id: self.trace_id,
            task_id: self.task_id,
            capabilities: OperationLogCapabilities {
                can_view_details: true,
            },
        })
    }
    fn into_detail(self) -> Result<OperationLogDetail, ApiError> {
        let request_method = self.request_method.clone();
        let route_template = self.route_template.clone();
        let error_code = self.error_code.clone();
        let error_summary = self.error_summary.clone();
        let parameters = serde_json::from_str(&self.parameters_json).unwrap_or_default();
        Ok(OperationLogDetail {
            summary: self.into_summary()?,
            request_method,
            route_template,
            parameters,
            error_code,
            error_summary,
        })
    }
}

async fn insert_event(pool: &DbPool, event: &StoredOperationEvent) -> Result<(), ApiError> {
    let mut transaction = pool.begin().await?;
    let result = sqlx::query("INSERT OR IGNORE INTO operation_logs (event_id,occurred_at,module,event_code,actor_kind,actor_user_id,actor_display_name,origin_kind,origin_node_id,origin_node_name,target_kind,target_id,target_display_name,target_ownership,outcome,impact,trace_id,task_id,client_ip,request_method,route_template,parameters_json,error_code,error_summary) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(&event.event_id).bind(&event.occurred_at).bind(event.module.as_str()).bind(&event.event_code)
        .bind(&event.actor_kind).bind(event.actor_user_id).bind(&event.actor_display_name).bind(&event.origin_kind)
        .bind(&event.origin_node_id).bind(&event.origin_node_name).bind(&event.target_kind).bind(&event.target_id)
        .bind(&event.target_display_name).bind(&event.target_ownership).bind(event.outcome.as_str()).bind(event.impact.as_str())
        .bind(&event.trace_id).bind(&event.task_id).bind(&event.client_ip).bind(&event.request_method).bind(&event.route_template)
        .bind(&event.parameters_json).bind(&event.error_code).bind(&event.error_summary).execute(&mut *transaction).await?;
    if result.rows_affected() > 0 {
        project_notification(&mut transaction, event).await?;
    }
    transaction.commit().await?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum NotificationRecipientPolicy {
    Actor,
    ScheduledTaskOwner,
    AllActiveAdmins,
}

#[derive(Debug, Clone, Copy)]
struct NotificationRegistration {
    code: NotificationCode,
    category: NotificationCategory,
    attention: NotificationAttentionPolicy,
    include_outcome: bool,
    recipients: NotificationRecipientPolicy,
    allowed_parameters: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
enum NotificationAttentionPolicy {
    OutcomeBased,
    Fixed(NotificationAttentionLevel),
}

/// 在操作事件事务内按严格注册表创建个人通知投影。
async fn project_notification(
    transaction: &mut Transaction<'_, Sqlite>,
    event: &StoredOperationEvent,
) -> Result<(), ApiError> {
    let Some(registration) = notification_registration(event) else {
        return Ok(());
    };
    let recipients = notification_recipients(transaction, event, registration.recipients).await?;
    if recipients.is_empty() {
        warn!(event_id = %event.event_id, event_code = %event.event_code, "Notification event has no trusted recipient");
        return Ok(());
    }
    let parameters =
        serde_json::from_str::<BTreeMap<String, OperationParameterValue>>(&event.parameters_json)
            .unwrap_or_default()
            .into_iter()
            .filter(|(key, _)| registration.allowed_parameters.contains(&key.as_str()))
            .collect::<BTreeMap<_, _>>();
    let parameters_json = serde_json::to_string(&parameters).unwrap_or_else(|_| "{}".to_string());
    let attention_level = notification_attention_level(registration.attention, event.outcome);
    let outcome = registration.include_outcome.then(|| event.outcome.as_str());
    for recipient_user_id in recipients {
        let notification_id = new_uuid_v7();
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO user_notifications (
                notification_id, recipient_user_id, operation_event_id, created_at,
                code, category, attention_level, outcome, source_module, source_node_id,
                source_node_name, subject_kind, subject_id, subject_display_name,
                task_id, parameters_json, error_code, error_summary, trace_id,
                read_at, archived_at, state_changed_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL, ?)
            "#,
        )
        .bind(notification_id)
        .bind(recipient_user_id)
        .bind(&event.event_id)
        .bind(&event.occurred_at)
        .bind(registration.code.as_str())
        .bind(registration.category.as_str())
        .bind(attention_level.as_str())
        .bind(outcome)
        .bind(event.module.as_str())
        .bind(&event.origin_node_id)
        .bind(&event.origin_node_name)
        .bind(&event.target_kind)
        .bind(&event.target_id)
        .bind(&event.target_display_name)
        .bind(&event.task_id)
        .bind(&parameters_json)
        .bind(&event.error_code)
        .bind(&event.error_summary)
        .bind(&event.trace_id)
        .bind(&event.occurred_at)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

fn notification_registration(event: &StoredOperationEvent) -> Option<NotificationRegistration> {
    const TASK_PARAMETERS: &[&str] = &[
        "nodeId",
        "nodeName",
        "suiteName",
        "scriptName",
        "taskName",
        "projectName",
        "name",
        "operation",
        "result",
        "targetPath",
        "mountPath",
        "imageRef",
        "image",
        "version",
        "completedItemCount",
        "failedItemCount",
        "sizeBytes",
        "transferredBytes",
    ];
    const SYSTEM_PARAMETERS: &[&str] = &["nodeId", "nodeName", "reason"];
    const SECURITY_PARAMETERS: &[&str] = &["username", "clientIp", "lockoutSeconds"];
    let event_code = event.event_code.as_str();
    let task = |code, recipients| NotificationRegistration {
        code,
        category: NotificationCategory::Task,
        attention: NotificationAttentionPolicy::OutcomeBased,
        include_outcome: true,
        recipients,
        allowed_parameters: TASK_PARAMETERS,
    };
    match event_code {
        "node_deploy" => Some(task(
            NotificationCode::NodeDeploymentFinished,
            NotificationRecipientPolicy::Actor,
        )),
        "suite_install_completed" | "suite_install_failed" | "suite_install_canceled" => {
            Some(task(
                NotificationCode::SuiteInstallationFinished,
                NotificationRecipientPolicy::Actor,
            ))
        }
        "script_run_completed" => Some(task(
            NotificationCode::ScriptRunFinished,
            NotificationRecipientPolicy::Actor,
        )),
        "scheduled_task_run_completed" => Some(task(
            NotificationCode::ScheduledTaskRunFinished,
            NotificationRecipientPolicy::ScheduledTaskOwner,
        )),
        "disk_operation_finished" => Some(task(
            NotificationCode::DiskOperationFinished,
            NotificationRecipientPolicy::Actor,
        )),
        "node_offline" => Some(NotificationRegistration {
            code: NotificationCode::NodeOffline,
            category: NotificationCategory::System,
            attention: NotificationAttentionPolicy::Fixed(NotificationAttentionLevel::Warning),
            include_outcome: false,
            recipients: NotificationRecipientPolicy::AllActiveAdmins,
            allowed_parameters: SYSTEM_PARAMETERS,
        }),
        "node_recovered" => Some(NotificationRegistration {
            code: NotificationCode::NodeRecovered,
            category: NotificationCategory::System,
            attention: NotificationAttentionPolicy::Fixed(NotificationAttentionLevel::Info),
            include_outcome: false,
            recipients: NotificationRecipientPolicy::AllActiveAdmins,
            allowed_parameters: SYSTEM_PARAMETERS,
        }),
        "login_lockout" => Some(NotificationRegistration {
            code: NotificationCode::LoginLockout,
            category: NotificationCategory::Security,
            attention: NotificationAttentionPolicy::Fixed(NotificationAttentionLevel::Warning),
            include_outcome: false,
            recipients: NotificationRecipientPolicy::AllActiveAdmins,
            allowed_parameters: SECURITY_PARAMETERS,
        }),
        "upgrade_plan_succeeded" | "upgrade_plan_failed" | "upgrade_plan_canceled" => Some(task(
            NotificationCode::UpgradePlanFinished,
            NotificationRecipientPolicy::Actor,
        )),
        _ if event_code.starts_with("scheduled_task_") && event_code.ends_with("_completed") => {
            Some(task(
                NotificationCode::ScheduledTaskOperationFinished,
                NotificationRecipientPolicy::Actor,
            ))
        }
        _ if event_code.starts_with("file_task_") && !event_code.ends_with("_submitted") => {
            Some(task(
                NotificationCode::FileTaskFinished,
                NotificationRecipientPolicy::Actor,
            ))
        }
        _ if event_code.starts_with("file_transfer_") && !event_code.ends_with("_submitted") => {
            Some(task(
                NotificationCode::FileTransferFinished,
                NotificationRecipientPolicy::Actor,
            ))
        }
        _ if event_code.starts_with("docker_image_")
            || (event.module == OperationModule::Docker
                && event.task_id.is_some()
                && matches!(event_code, "image_pull" | "image_pull_cancelled")) =>
        {
            Some(task(
                NotificationCode::DockerImageTaskFinished,
                NotificationRecipientPolicy::Actor,
            ))
        }
        _ if event.module == OperationModule::Docker
            && event_code.starts_with("compose_")
            && ["_succeeded", "_failed", "_cancelled"]
                .iter()
                .any(|suffix| event_code.ends_with(suffix)) =>
        {
            Some(task(
                NotificationCode::DockerProjectTaskFinished,
                NotificationRecipientPolicy::Actor,
            ))
        }
        _ => None,
    }
}

async fn notification_recipients(
    transaction: &mut Transaction<'_, Sqlite>,
    event: &StoredOperationEvent,
    policy: NotificationRecipientPolicy,
) -> Result<Vec<i64>, ApiError> {
    match policy {
        NotificationRecipientPolicy::Actor => Ok(event.actor_user_id.into_iter().collect()),
        NotificationRecipientPolicy::ScheduledTaskOwner => {
            if let Some(user_id) = event.actor_user_id {
                return Ok(vec![user_id]);
            }
            let owner = match event.target_id.as_deref() {
                Some(task_id) => {
                    sqlx::query_scalar::<_, i64>(
                        "SELECT created_by_user_id FROM scheduled_tasks WHERE task_id = ?",
                    )
                    .bind(task_id)
                    .fetch_optional(&mut **transaction)
                    .await?
                }
                None => None,
            };
            Ok(owner.into_iter().collect())
        }
        NotificationRecipientPolicy::AllActiveAdmins => Ok(sqlx::query_scalar::<_, i64>(
            "SELECT id FROM users WHERE status = 'active' ORDER BY id",
        )
        .fetch_all(&mut **transaction)
        .await?),
    }
}

const fn notification_attention_level(
    policy: NotificationAttentionPolicy,
    outcome: OperationOutcome,
) -> NotificationAttentionLevel {
    match policy {
        NotificationAttentionPolicy::Fixed(level) => level,
        NotificationAttentionPolicy::OutcomeBased => match outcome {
            OperationOutcome::Success | OperationOutcome::Canceled => {
                NotificationAttentionLevel::Info
            }
            OperationOutcome::Failure | OperationOutcome::Partial | OperationOutcome::TimedOut => {
                NotificationAttentionLevel::Warning
            }
        },
    }
}

fn validate_query(query: &OperationLogQuery) -> Result<(), ApiError> {
    if query.page == 0 || !matches!(query.page_size, 20 | 50 | 100) {
        return Err(ApiError::BadRequest(
            "invalid operation log pagination".to_string(),
        ));
    }
    for count in [
        query.modules.as_ref().map_or(0, Vec::len),
        query.event_codes.as_ref().map_or(0, Vec::len),
        query.outcomes.as_ref().map_or(0, Vec::len),
        query.impacts.as_ref().map_or(0, Vec::len),
        query.user_ids.as_ref().map_or(0, Vec::len),
        query.node_ids.as_ref().map_or(0, Vec::len),
    ] {
        if count > 20 {
            return Err(ApiError::BadRequest(
                "too many operation log filters".to_string(),
            ));
        }
    }
    if query
        .keyword
        .as_ref()
        .is_some_and(|v| v.chars().count() > 100)
    {
        return Err(ApiError::BadRequest(
            "operation log keyword is too long".to_string(),
        ));
    }
    let from = query
        .occurred_from
        .as_deref()
        .map(DateTime::parse_from_rfc3339)
        .transpose()
        .map_err(|_| ApiError::BadRequest("invalid occurredFrom".to_string()))?;
    let to = query
        .occurred_to
        .as_deref()
        .map(DateTime::parse_from_rfc3339)
        .transpose()
        .map_err(|_| ApiError::BadRequest("invalid occurredTo".to_string()))?;
    if from.zip(to).is_some_and(|(from, to)| from > to) {
        return Err(ApiError::BadRequest(
            "occurredFrom must not exceed occurredTo".to_string(),
        ));
    }
    Ok(())
}

fn push_filters(builder: &mut QueryBuilder<'_, Sqlite>, query: &OperationLogQuery) {
    builder.push(" WHERE 1 = 1");
    macro_rules! list_filter {
        ($field:literal, $values:expr, $map:expr) => {
            if let Some(values) = $values {
                if !values.is_empty() {
                    builder.push(concat!(" AND ", $field, " IN ("));
                    let mut first = true;
                    for value in values {
                        if !first {
                            builder.push(",");
                        }
                        first = false;
                        builder.push_bind(($map)(value));
                    }
                    builder.push(")");
                }
            }
        };
    }
    list_filter!("module", &query.modules, |v: &OperationModule| v
        .as_str()
        .to_string());
    list_filter!("event_code", &query.event_codes, |v: &String| v.clone());
    list_filter!("outcome", &query.outcomes, |v: &OperationOutcome| v
        .as_str()
        .to_string());
    list_filter!("impact", &query.impacts, |v: &OperationImpact| v
        .as_str()
        .to_string());
    list_filter!("actor_user_id", &query.user_ids, |v: &i64| *v);
    list_filter!("origin_node_id", &query.node_ids, |v: &String| v.clone());
    if let Some(value) = &query.occurred_from {
        builder
            .push(" AND occurred_at >= ")
            .push_bind(value.clone());
    }
    if let Some(value) = &query.occurred_to {
        builder
            .push(" AND occurred_at <= ")
            .push_bind(value.clone());
    }
    if let Some(value) = query
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        let pattern = format!("%{value}%");
        builder
            .push(" AND (actor_display_name LIKE ")
            .push_bind(pattern.clone())
            .push(" OR event_code LIKE ")
            .push_bind(pattern.clone())
            .push(" OR target_id LIKE ")
            .push_bind(pattern.clone())
            .push(" OR trace_id LIKE ")
            .push_bind(pattern)
            .push(")");
    }
}

fn module_for_event(event: &str) -> OperationModule {
    if event.starts_with("user_") || event.starts_with("auth_") {
        OperationModule::Auth
    } else if event.starts_with("node_") || event.starts_with("runtime_") {
        OperationModule::Nodes
    } else if event.starts_with("suite_") || event.starts_with("image_") {
        OperationModule::Suites
    } else if event.starts_with("docker_") {
        OperationModule::Docker
    } else if event.starts_with("file_") {
        OperationModule::Files
    } else if event.starts_with("process_") {
        OperationModule::Processes
    } else if event.starts_with("disk_") {
        OperationModule::Disks
    } else if event.starts_with("monitoring_") {
        OperationModule::Monitoring
    } else if event.starts_with("script_") {
        OperationModule::Scripts
    } else if event.starts_with("scheduled_task_") {
        OperationModule::ScheduledTasks
    } else if event.starts_with("upgrade_") {
        OperationModule::Upgrades
    } else if event.starts_with("terminal_") {
        OperationModule::Terminal
    } else {
        OperationModule::Settings
    }
}

fn normalize_event_code(value: &str) -> String {
    truncate(&value.trim().to_ascii_lowercase().replace('-', "_"), 128)
}
fn is_high_impact(event: &str) -> bool {
    [
        "delete",
        "remove",
        "uninstall",
        "erase",
        "reset",
        "terminate",
        "retire",
        "force",
    ]
    .iter()
    .any(|v| event.contains(v))
}
fn impact_for_outcome(outcome: OperationOutcome, requested: OperationImpact) -> OperationImpact {
    if outcome == OperationOutcome::Failure || outcome == OperationOutcome::TimedOut {
        OperationImpact::Error
    } else {
        requested
    }
}
fn safe_method(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_uppercase();
    matches!(value.as_str(), "POST" | "PUT" | "PATCH" | "DELETE").then_some(value)
}
fn safe_route_template(value: &str) -> Option<String> {
    let path = value.split('?').next().unwrap_or("").trim();
    path.starts_with("/api/").then(|| truncate(path, 256))
}
fn non_empty(value: &str, max: usize) -> Option<String> {
    (!value.trim().is_empty()).then(|| truncate(value.trim(), max))
}
fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

fn sanitize_reported_client_ip(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() || value.len() > 64 || value.parse::<IpAddr>().is_err() {
        return None;
    }
    Some(value.to_string())
}
fn redact_error(value: &str) -> String {
    let lowered = value.to_ascii_lowercase();
    if ["token", "password", "authorization", "secret", "cookie"]
        .iter()
        .any(|v| lowered.contains(v))
    {
        "Sensitive error details were redacted".to_string()
    } else {
        truncate(value, MAX_ERROR_SUMMARY_BYTES)
    }
}
fn sanitize_parameters(
    raw: Option<Value>,
) -> (
    BTreeMap<String, OperationParameterValue>,
    Option<String>,
    Option<String>,
) {
    let mut safe = BTreeMap::new();
    let mut error_code = None;
    let mut error_summary = None;
    if let Some(Value::Object(values)) = raw {
        for (key, value) in values {
            let normalized = key.to_ascii_lowercase();
            if [
                "password",
                "token",
                "authorization",
                "secret",
                "cookie",
                "command",
                "environment",
                "requestbody",
            ]
            .iter()
            .any(|v| normalized.contains(v))
            {
                continue;
            }
            if normalized == "errorcode" {
                error_code = value.as_str().map(|v| truncate(v, 128));
                continue;
            }
            if normalized == "error" || normalized == "errorsummary" {
                error_summary = Some(redact_error(value.as_str().unwrap_or("Operation failed")));
                continue;
            }
            if let Some(value) = parameter_from_json(value) {
                safe.insert(truncate(&key, 64), value);
            }
        }
    }
    (safe, error_code, error_summary)
}
fn sanitize_parameter_map(
    values: BTreeMap<String, OperationParameterValue>,
) -> BTreeMap<String, OperationParameterValue> {
    values
        .into_iter()
        .filter(|(key, _)| {
            ![
                "password",
                "token",
                "authorization",
                "secret",
                "cookie",
                "command",
                "environment",
            ]
            .iter()
            .any(|v| key.to_ascii_lowercase().contains(v))
        })
        .take(32)
        .map(|(key, value)| (truncate(&key, 64), value))
        .collect()
}
fn parameter_from_json(value: Value) -> Option<OperationParameterValue> {
    match value {
        Value::String(v) => Some(OperationParameterValue::String(truncate(&v, 256))),
        Value::Number(v) => v.as_f64().map(OperationParameterValue::Number),
        Value::Bool(v) => Some(OperationParameterValue::Boolean(v)),
        _ => None,
    }
}

fn normalize_operation_target(
    event: &mut StoredOperationEvent,
    parameters: &BTreeMap<String, OperationParameterValue>,
) {
    if event.task_id.is_none()
        && let Some(OperationParameterValue::String(task_id)) =
            parameters.get("taskId").or_else(|| parameters.get("runId"))
    {
        event.task_id = non_empty(task_id, 128);
    }
    if matches!(
        event.target_kind.as_deref(),
        Some("fileTask" | "fileTransfer" | "disk_operation" | "imagePullTask")
    ) {
        let resource_kind = match event.target_kind.as_deref() {
            Some("fileTask" | "fileTransfer") => Some("file"),
            Some("disk_operation") => Some("disk"),
            _ => None,
        };
        if event.task_id.is_none() {
            event.task_id = event.target_id.take();
        }
        event.target_kind = None;
        event.target_id = None;
        event.target_display_name = None;
        event.target_ownership = None;
        if let (Some(kind), Some(display_name)) =
            (resource_kind, parameter_display_name(parameters))
        {
            event.target_kind = Some(kind.to_string());
            event.target_id = Some(display_name.clone());
            event.target_display_name = Some(display_name);
        }
        return;
    }
    if event.target_display_name.is_none() {
        event.target_display_name = parameter_display_name(parameters);
    }
}

fn parameter_display_name(
    parameters: &BTreeMap<String, OperationParameterValue>,
) -> Option<String> {
    [
        "targetName",
        "nodeName",
        "scriptName",
        "taskName",
        "processName",
        "suiteName",
        "projectName",
        "containerName",
        "volumeName",
        "networkName",
        "targetPath",
        "mountPath",
        "deviceName",
        "imageRef",
        "image",
        "name",
        "version",
    ]
    .into_iter()
    .find_map(|key| match parameters.get(key) {
        Some(OperationParameterValue::String(value)) if !is_opaque_identifier(value) => {
            non_empty(value, 256)
        }
        _ => None,
    })
}

async fn enrich_target_display_name(pool: &DbPool, event: &mut StoredOperationEvent) {
    if event.target_display_name.is_some() {
        return;
    }
    let (Some(kind), Some(id)) = (event.target_kind.as_deref(), event.target_id.as_deref()) else {
        return;
    };
    let resolved = match kind {
        "node" if id == "local" => Some("Local Node".to_string()),
        "node" => {
            query_display_name(pool, "SELECT name FROM nodes WHERE node_id = ?", id).await
        }
        "agent" => {
            query_display_name(
                pool,
                "SELECT n.name FROM node_identities i JOIN nodes n ON n.node_id = i.node_id WHERE i.agent_id = ?",
                id,
            )
            .await
        }
        "script" => {
            query_display_name(pool, "SELECT name FROM scripts WHERE script_id = ?", id).await
        }
        "scheduled_task" => {
            query_display_name(
                pool,
                "SELECT name FROM scheduled_tasks WHERE task_id = ?",
                id,
            )
            .await
        }
        "upgrade_plan" => {
            query_display_name(
                pool,
                "SELECT target_version FROM upgrade_plans WHERE plan_id = ?",
                id,
            )
            .await
        }
        "file" | "docker_image" | "image" | "container" | "network" | "volume"
        | "upgrade_release" | "seclab"
            if !is_opaque_identifier(id) =>
        {
            Some(id.to_string())
        }
        _ => None,
    };
    event.target_display_name = resolved.map(|value| truncate(&value, 256));
}

async fn query_display_name(pool: &DbPool, sql: &str, id: &str) -> Option<String> {
    match sqlx::query_scalar::<_, String>(sql)
        .bind(id)
        .fetch_optional(pool)
        .await
    {
        Ok(value) => value.and_then(|value| non_empty(&value, 256)),
        Err(error) => {
            warn!(%error, "Operation audit target name resolution failed");
            None
        }
    }
}

fn is_opaque_identifier(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok()
        || (value.len() >= 24 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn actor_kind_value(value: OperationActorKind) -> &'static str {
    match value {
        OperationActorKind::User => "user",
        OperationActorKind::Anonymous => "anonymous",
        OperationActorKind::System => "system",
        OperationActorKind::Agent => "agent",
    }
}
fn parse_module(value: &str) -> Result<OperationModule, ApiError> {
    match value {
        "auth" => Ok(OperationModule::Auth),
        "nodes" => Ok(OperationModule::Nodes),
        "suites" => Ok(OperationModule::Suites),
        "docker" => Ok(OperationModule::Docker),
        "files" => Ok(OperationModule::Files),
        "processes" => Ok(OperationModule::Processes),
        "disks" => Ok(OperationModule::Disks),
        "monitoring" => Ok(OperationModule::Monitoring),
        "scripts" => Ok(OperationModule::Scripts),
        "scheduledTasks" => Ok(OperationModule::ScheduledTasks),
        "upgrades" => Ok(OperationModule::Upgrades),
        "terminal" => Ok(OperationModule::Terminal),
        "settings" => Ok(OperationModule::Settings),
        _ => Err(ApiError::Internal("invalid operation module".to_string())),
    }
}
fn parse_outcome(value: &str) -> Result<OperationOutcome, ApiError> {
    match value {
        "success" => Ok(OperationOutcome::Success),
        "failure" => Ok(OperationOutcome::Failure),
        "partial" => Ok(OperationOutcome::Partial),
        "canceled" => Ok(OperationOutcome::Canceled),
        "timedOut" => Ok(OperationOutcome::TimedOut),
        _ => Err(ApiError::Internal("invalid operation outcome".to_string())),
    }
}
fn parse_impact(value: &str) -> Result<OperationImpact, ApiError> {
    match value {
        "info" => Ok(OperationImpact::Info),
        "warning" => Ok(OperationImpact::Warning),
        "error" => Ok(OperationImpact::Error),
        _ => Err(ApiError::Internal("invalid operation impact".to_string())),
    }
}
fn parse_actor_kind(value: &str) -> Result<OperationActorKind, ApiError> {
    match value {
        "user" => Ok(OperationActorKind::User),
        "anonymous" => Ok(OperationActorKind::Anonymous),
        "system" => Ok(OperationActorKind::System),
        "agent" => Ok(OperationActorKind::Agent),
        _ => Err(ApiError::Internal("invalid operation actor".to_string())),
    }
}
fn parse_origin_kind(value: &str) -> Result<OperationOriginKind, ApiError> {
    match value {
        "master" => Ok(OperationOriginKind::Master),
        "agent" => Ok(OperationOriginKind::Agent),
        _ => Err(ApiError::Internal("invalid operation origin".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::setup_test_db;
    #[test]
    fn sanitizer_drops_sensitive_and_complex_values() {
        let (parameters, code, summary) = sanitize_parameters(Some(
            serde_json::json!({"nodeId":"n1","password":"bad","items":[1],"errorCode":"FAILED","error":"safe summary"}),
        ));
        assert_eq!(parameters.len(), 1);
        assert_eq!(code.as_deref(), Some("FAILED"));
        assert_eq!(summary.as_deref(), Some("safe summary"));
    }
    #[test]
    fn registry_computes_module_and_high_impact() {
        assert_eq!(
            module_for_event("script_run_completed"),
            OperationModule::Scripts
        );
        assert!(is_high_impact("node_remove"));
    }

    #[test]
    fn anonymous_actor_is_not_classified_as_user() {
        let event =
            OperationEventBuilder::new("anonymous", "user_login", "127.0.0.1".parse().unwrap());
        assert_eq!(event.event.actor_kind, "anonymous");
        assert!(event.event.actor_user_id.is_none());
        assert!(event.event.target_kind.is_none());
        assert!(event.event.target_id.is_none());
    }

    #[test]
    fn reported_client_ip_accepts_only_ip_addresses() {
        assert_eq!(
            sanitize_reported_client_ip(Some(" ::ffff:10.121.7.7 ")).as_deref(),
            Some("::ffff:10.121.7.7")
        );
        assert_eq!(sanitize_reported_client_ip(Some("forwarded-client")), None);
        assert_eq!(sanitize_reported_client_ip(None), None);
    }

    #[tokio::test]
    async fn target_resolution_uses_domain_name_and_moves_operation_id() {
        let pool = setup_test_db().await;
        sqlx::query(
            "INSERT INTO nodes (node_id, name, normalized_name) VALUES ('node-id', 'Edge Node', 'edge node')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut node_event =
            OperationEventBuilder::new("admin", "node_update", "127.0.0.1".parse().unwrap())
                .target_type("node")
                .target_id("node-id")
                .event;
        enrich_target_display_name(&pool, &mut node_event).await;
        assert_eq!(node_event.target_display_name.as_deref(), Some("Edge Node"));

        let mut operation_event = OperationEventBuilder::new(
            "admin",
            "disk_operation_finished",
            "127.0.0.1".parse().unwrap(),
        )
        .target_type("disk_operation")
        .target_id("019f6f64-cc8d-7a30-9812-845e0f56f185")
        .event;
        let parameters = BTreeMap::from([(
            "targetName".to_string(),
            OperationParameterValue::String("/dev/sdb".to_string()),
        )]);
        normalize_operation_target(&mut operation_event, &parameters);
        assert_eq!(
            operation_event.task_id.as_deref(),
            Some("019f6f64-cc8d-7a30-9812-845e0f56f185")
        );
        assert_eq!(operation_event.target_kind.as_deref(), Some("disk"));
        assert_eq!(
            operation_event.target_display_name.as_deref(),
            Some("/dev/sdb")
        );
    }

    #[tokio::test]
    async fn query_validates_pagination_and_uses_stable_order() {
        let pool = setup_test_db().await;
        for event_id in [
            "018f0000-0000-7000-8000-000000000001",
            "018f0000-0000-7000-8000-000000000002",
        ] {
            insert_event(
                &pool,
                &StoredOperationEvent {
                    event_id: event_id.to_string(),
                    occurred_at: "2025-01-01T00:00:00Z".to_string(),
                    module: OperationModule::Nodes,
                    event_code: "node_update".to_string(),
                    actor_kind: "user".to_string(),
                    actor_user_id: None,
                    actor_display_name: "admin".to_string(),
                    origin_kind: "master".to_string(),
                    origin_node_id: None,
                    origin_node_name: None,
                    target_kind: Some("node".to_string()),
                    target_id: Some("node-1".to_string()),
                    target_display_name: None,
                    target_ownership: None,
                    outcome: OperationOutcome::Success,
                    impact: OperationImpact::Info,
                    trace_id: "trace".to_string(),
                    task_id: None,
                    client_ip: "127.0.0.1".to_string(),
                    request_method: Some("PATCH".to_string()),
                    route_template: Some("/api/v1/node/{node_id}".to_string()),
                    parameters_json: "{}".to_string(),
                    error_code: None,
                    error_summary: None,
                },
            )
            .await
            .unwrap();
        }
        let page = query_operation_logs(
            &pool,
            OperationLogQuery {
                page: 1,
                page_size: 20,
                modules: Some(vec![OperationModule::Nodes]),
                event_codes: None,
                outcomes: None,
                impacts: None,
                user_ids: None,
                node_ids: None,
                occurred_from: None,
                occurred_to: None,
                keyword: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(page.total, 2);
        assert!(page.items[0].event_id > page.items[1].event_id);
        assert_eq!(page.items[0].client_ip.as_deref(), Some("127.0.0.1"));
        let invalid = query_operation_logs(
            &pool,
            OperationLogQuery {
                page: 0,
                page_size: 10,
                modules: None,
                event_codes: None,
                outcomes: None,
                impacts: None,
                user_ids: None,
                node_ids: None,
                occurred_from: None,
                occurred_to: None,
                keyword: None,
            },
        )
        .await;
        assert!(invalid.is_err());
    }

    #[tokio::test]
    async fn registered_terminal_event_projects_once_with_safe_parameters() {
        let pool = setup_test_db().await;
        sqlx::query("INSERT INTO users (id, username, password_hash) VALUES (1, 'admin', 'hash')")
            .execute(&pool)
            .await
            .unwrap();
        let mut event = OperationEventBuilder::new(
            "admin",
            "script_run_completed",
            "127.0.0.1".parse().unwrap(),
        )
        .user_id(1)
        .target_type("script")
        .target_id("script-1")
        .task_id("run-1")
        .set_success()
        .event;
        event.parameters_json = serde_json::json!({
            "scriptName": "Safe Script",
            "token": "must-not-project",
            "unregistered": "must-not-project",
        })
        .to_string();

        insert_event(&pool, &event).await.unwrap();
        insert_event(&pool, &event).await.unwrap();

        let operation_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM operation_logs")
            .fetch_one(&pool)
            .await
            .unwrap();
        let notification_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_notifications")
            .fetch_one(&pool)
            .await
            .unwrap();
        let (recipient, code, attention_level, outcome, parameters): (
            i64,
            String,
            String,
            Option<String>,
            String,
        ) = sqlx::query_as(
            "SELECT recipient_user_id, code, attention_level, outcome, parameters_json FROM user_notifications",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(operation_count, 1);
        assert_eq!(notification_count, 1);
        assert_eq!(recipient, 1);
        assert_eq!(code, "scriptRunFinished");
        assert_eq!(attention_level, "info");
        assert_eq!(outcome.as_deref(), Some("success"));
        assert!(parameters.contains("Safe Script"));
        assert!(!parameters.contains("token"));
        assert!(!parameters.contains("unregistered"));
    }

    #[test]
    fn notification_attention_is_independent_from_operation_outcome() {
        assert_eq!(
            notification_attention_level(
                NotificationAttentionPolicy::OutcomeBased,
                OperationOutcome::Success,
            ),
            NotificationAttentionLevel::Info
        );
        assert_eq!(
            notification_attention_level(
                NotificationAttentionPolicy::OutcomeBased,
                OperationOutcome::Failure,
            ),
            NotificationAttentionLevel::Warning
        );
        assert_eq!(
            notification_attention_level(
                NotificationAttentionPolicy::Fixed(NotificationAttentionLevel::Critical),
                OperationOutcome::Success,
            ),
            NotificationAttentionLevel::Critical
        );
    }

    #[tokio::test]
    async fn system_transition_notifies_only_active_admins() {
        let pool = setup_test_db().await;
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, status) VALUES \
             (1, 'active-1', 'hash', 'active'), \
             (2, 'active-2', 'hash', 'active'), \
             (3, 'disabled', 'hash', 'disabled')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let event =
            OperationEventBuilder::new("system", "node_offline", "127.0.0.1".parse().unwrap())
                .target_type("node")
                .target_id("node-1")
                .set_success()
                .event;
        insert_event(&pool, &event).await.unwrap();

        let recipients = sqlx::query_scalar::<_, i64>(
            "SELECT recipient_user_id FROM user_notifications ORDER BY recipient_user_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(recipients, vec![1, 2]);
        let projections = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT attention_level, outcome FROM user_notifications ORDER BY recipient_user_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            projections,
            vec![("warning".to_string(), None), ("warning".to_string(), None)]
        );
    }

    #[tokio::test]
    async fn unregistered_operation_event_does_not_create_notification() {
        let pool = setup_test_db().await;
        let event =
            OperationEventBuilder::new("system", "runtime_heartbeat", "127.0.0.1".parse().unwrap())
                .set_success()
                .event;
        insert_event(&pool, &event).await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_notifications")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}
