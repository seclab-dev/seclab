//! runtime API：节点 enrollment、register、heartbeat 与 deregister。

use crate::models::logging::{LogModule, LogStatus, PlatformLogLevel};
use crate::services::logging::PlatformLogEntry;
use crate::services::node_runtime;
use crate::services::runtime_metrics;
use crate::state::AppState;
use crate::types::{ApiError, ApiResponse, ApiResult};
use axum::{
    Json, Router,
    body::Body,
    extract::{ConnectInfo, Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use seclab_contracts::api::ErrorCode;
use seclab_contracts::terminal::{TerminalTicketConsumeRequest, TerminalTicketConsumeResponse};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeNodePayload {
    pub advertise_addr: Option<String>,
    pub listen_port: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CertificateRequestPayload {
    pub public_key_algorithm: Option<String>,
    pub csr_pem: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAgentCompatibilityPayload {
    pub agent_version: Option<String>,
    pub runtime_protocol_version: Option<String>,
    pub min_supported_controller_version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeControllerCompatibilityResponse {
    pub controller_version: String,
    pub runtime_protocol_version: String,
    pub min_supported_agent_version: String,
    pub compatible: bool,
    pub reason: String,
    pub required_action: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCallbackProbeResponse {
    pub controller_version: String,
    pub runtime_protocol_version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrollPayload {
    pub enrollment_token: String,
    pub node: RuntimeNodePayload,
    pub certificate_request: Option<CertificateRequestPayload>,
    pub compatibility: Option<RuntimeAgentCompatibilityPayload>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterPayload {
    pub agent_id: String,
    pub node: RuntimeNodePayload,
    pub compatibility: Option<RuntimeAgentCompatibilityPayload>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatPayload {
    pub agent_id: String,
    pub session_id: String,
    pub lease_id: String,
    pub sequence: i64,
    pub node: Option<RuntimeNodePayload>,
    pub resource: Option<seclab_contracts::types::HostSystemSummary>,
    pub compatibility: Option<RuntimeAgentCompatibilityPayload>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeregisterPayload {
    pub agent_id: String,
    pub session_id: String,
    pub reason: Option<String>,
}

/// 由 Agent 原子消费 Master 签发的一次性终端票据。
async fn consume_terminal_ticket(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<TerminalTicketConsumeRequest>,
) -> ApiResult<Response> {
    let context = state
        .terminal_tickets
        .consume(&payload.ticket, &payload.node_id)
        .ok_or_else(|| {
            ApiError::forbidden(
                ErrorCode::AuthForbidden,
                "terminal ticket is invalid or expired",
            )
        })?;
    Ok(ApiResponse::success_with_raw(
        "Terminal ticket consumed",
        Some(TerminalTicketConsumeResponse {
            actor_name: context.actor_name,
            client_ip: context.client_ip,
            trace_id: context.trace_id,
            node_id: context.node_id,
        }),
    )
    .into_response())
}

/// Runtime 回连探针：用于部署预检验证目标节点能访问主控入口，不产生运行时状态。
pub async fn callback_probe() -> ApiResult<impl IntoResponse> {
    let config = &crate::config::get().agent_version_compatibility;
    Ok(ApiResponse::success_with_raw(
        "Runtime callback probe succeeded",
        Some(RuntimeCallbackProbeResponse {
            controller_version: env!("CARGO_PKG_VERSION").to_string(),
            runtime_protocol_version: config.runtime_protocol_version.clone(),
        }),
    )
    .into_response())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RotateCertificatePayload {
    pub agent_id: String,
    pub session_id: String,
    pub reason: Option<String>,
    pub current_certificate_fingerprint: Option<String>,
    pub certificate_request: Option<CertificateRequestPayload>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactDownloadQuery {
    pub token: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSessionResponse {
    pub node_id: String,
    pub agent_id: String,
    pub session_id: String,
    pub lease_id: String,
    pub lease_ttl_seconds: i64,
    pub heartbeat_interval_seconds: i64,
    pub session_replaced: Option<bool>,
    pub controller_compatibility: RuntimeControllerCompatibilityResponse,
}

fn controller_compatibility(
    agent: Option<&RuntimeAgentCompatibilityPayload>,
) -> RuntimeControllerCompatibilityResponse {
    let config = &crate::config::get().agent_version_compatibility;
    let expected_protocol = config.runtime_protocol_version.clone();
    let controller_version = env!("CARGO_PKG_VERSION").to_string();
    let agent_version = agent.and_then(|value| value.agent_version.as_deref());
    let agent_protocol = agent.and_then(|value| value.runtime_protocol_version.as_deref());
    let agent_min_controller_version =
        agent.and_then(|value| value.min_supported_controller_version.as_deref());

    if agent_protocol != Some(expected_protocol.as_str()) {
        return RuntimeControllerCompatibilityResponse {
            controller_version,
            runtime_protocol_version: expected_protocol,
            min_supported_agent_version: config.min_supported_agent_version.clone(),
            compatible: false,
            reason: "Agent runtime protocol is not compatible with this controller".to_string(),
            required_action: "upgrade_agent".to_string(),
        };
    }

    let Some(agent_version) = agent_version else {
        return RuntimeControllerCompatibilityResponse {
            controller_version,
            runtime_protocol_version: expected_protocol,
            min_supported_agent_version: config.min_supported_agent_version.clone(),
            compatible: false,
            reason: "Agent version is missing".to_string(),
            required_action: "upgrade_agent".to_string(),
        };
    };

    let controller = match Version::parse(env!("CARGO_PKG_VERSION")) {
        Ok(version) => version,
        Err(err) => {
            return RuntimeControllerCompatibilityResponse {
                controller_version,
                runtime_protocol_version: expected_protocol,
                min_supported_agent_version: config.min_supported_agent_version.clone(),
                compatible: false,
                reason: format!("Controller version is not valid SemVer: {err}"),
                required_action: "repair_controller".to_string(),
            };
        }
    };
    let Some(agent_min_controller_version) = agent_min_controller_version else {
        return RuntimeControllerCompatibilityResponse {
            controller_version,
            runtime_protocol_version: expected_protocol,
            min_supported_agent_version: config.min_supported_agent_version.clone(),
            compatible: false,
            reason: "Agent minimum supported controller version is missing".to_string(),
            required_action: "upgrade_agent".to_string(),
        };
    };
    let agent_min_controller =
        match Version::parse(agent_min_controller_version.trim_start_matches('v')) {
            Ok(version) => version,
            Err(err) => {
                return RuntimeControllerCompatibilityResponse {
                    controller_version,
                    runtime_protocol_version: expected_protocol,
                    min_supported_agent_version: config.min_supported_agent_version.clone(),
                    compatible: false,
                    reason: format!(
                        "Agent minimum supported controller version is not valid SemVer: {err}"
                    ),
                    required_action: "upgrade_agent".to_string(),
                };
            }
        };
    if controller < agent_min_controller {
        return RuntimeControllerCompatibilityResponse {
            controller_version,
            runtime_protocol_version: expected_protocol,
            min_supported_agent_version: config.min_supported_agent_version.clone(),
            compatible: false,
            reason: "Controller version is older than agent minimum supported controller version"
                .to_string(),
            required_action: "upgrade_controller".to_string(),
        };
    }
    let agent = match Version::parse(agent_version.trim_start_matches('v')) {
        Ok(version) => version,
        Err(err) => {
            return RuntimeControllerCompatibilityResponse {
                controller_version,
                runtime_protocol_version: expected_protocol,
                min_supported_agent_version: config.min_supported_agent_version.clone(),
                compatible: false,
                reason: format!("Agent version is not valid SemVer: {err}"),
                required_action: "upgrade_agent".to_string(),
            };
        }
    };
    let min_agent = match Version::parse(&config.min_supported_agent_version) {
        Ok(version) => version,
        Err(err) => {
            return RuntimeControllerCompatibilityResponse {
                controller_version,
                runtime_protocol_version: expected_protocol,
                min_supported_agent_version: config.min_supported_agent_version.clone(),
                compatible: false,
                reason: format!("Minimum supported agent version is not valid SemVer: {err}"),
                required_action: "repair_controller".to_string(),
            };
        }
    };

    // 项目初期由于在线与局部升级兼容性需要，放宽了零主版本（0.x.x）的兼容限制。
    // 在 zero_major_requires_exact 为 false 时，仅要求首位主版本号一致；待以后项目文档或兼容矩阵明确后再行调整。
    let compatible = agent >= min_agent
        && if controller.major == 0 {
            if config.zero_major_requires_exact {
                controller.major == agent.major
                    && controller.minor == agent.minor
                    && controller.patch == agent.patch
                    && (!config.zero_major_requires_prerelease_match || controller.pre == agent.pre)
            } else {
                controller.major == agent.major
            }
        } else {
            (!config.stable_requires_same_major || controller.major == agent.major)
                && (!config.stable_disallow_agent_newer_than_controller || agent <= controller)
        };

    RuntimeControllerCompatibilityResponse {
        controller_version,
        runtime_protocol_version: expected_protocol,
        min_supported_agent_version: config.min_supported_agent_version.clone(),
        compatible,
        reason: if compatible {
            "Agent is compatible with this controller".to_string()
        } else {
            "Agent is not compatible with this controller".to_string()
        },
        required_action: if compatible {
            "none".to_string()
        } else {
            "upgrade_agent".to_string()
        },
    }
}

fn ensure_runtime_agent_compatible(
    agent: Option<&RuntimeAgentCompatibilityPayload>,
) -> Result<RuntimeControllerCompatibilityResponse, ApiError> {
    let compatibility = controller_compatibility(agent);
    if compatibility.compatible {
        Ok(compatibility)
    } else {
        Err(ApiError::BadRequest(compatibility.reason.clone()))
    }
}

pub async fn enroll(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<EnrollPayload>,
) -> ApiResult<impl IntoResponse> {
    let controller_compatibility = ensure_runtime_agent_compatible(payload.compatibility.as_ref())?;
    let mut platform_log = PlatformLogEntry::new("runtime-agent", "runtime_enroll", addr.ip())
        .module(LogModule::System)
        .target_type("node")
        .metadata(json!({ "advertise_addr": payload.node.advertise_addr.clone(), "listen_port": payload.node.listen_port }));
    let result = node_runtime::enroll_node(
        &state.metadata_db,
        &payload.enrollment_token,
        payload.node.advertise_addr,
        payload.node.listen_port,
        payload
            .certificate_request
            .as_ref()
            .and_then(|value| value.public_key_algorithm.as_deref())
            .unwrap_or("ed25519"),
        payload
            .certificate_request
            .as_ref()
            .and_then(|value| value.csr_pem.as_deref()),
    )
    .await;
    let response = match result {
        Ok(result) => {
            runtime_metrics::record_enroll(true);
            platform_log = platform_log.target_id(&result.node_id).set_success();

            let state_clone = Arc::clone(&state);
            let agent_id = result.agent_id.clone();
            tokio::spawn(async move {
                if let Err(err) =
                    crate::services::task_sync::reconcile_agent_tasks(&state_clone, &agent_id).await
                {
                    tracing::error!(
                        "Failed to reconcile scheduled tasks for agent {}: {}",
                        agent_id,
                        err
                    );
                }
            });

            Ok(ApiResponse::success_with_raw(
                "Node enrolled",
                Some(RuntimeSessionResponse {
                    node_id: result.node_id,
                    agent_id: result.agent_id,
                    session_id: result.session_id,
                    lease_id: result.lease_id,
                    lease_ttl_seconds: node_runtime::DEFAULT_LEASE_TTL_SECONDS,
                    heartbeat_interval_seconds: node_runtime::DEFAULT_HEARTBEAT_INTERVAL_SECONDS,
                    session_replaced: None,
                    controller_compatibility,
                }),
            ))
        }
        Err(err) => {
            runtime_metrics::record_enroll(false);
            platform_log = platform_log.metadata(json!({ "error": err }));
            Err(err)
        }
    };
    platform_log.finish(&state.metadata_db);
    response
}

pub async fn register(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<RegisterPayload>,
) -> ApiResult<impl IntoResponse> {
    let controller_compatibility = ensure_runtime_agent_compatible(payload.compatibility.as_ref())?;
    let mut platform_log = PlatformLogEntry::new("runtime-agent", "runtime_register", addr.ip())
        .module(LogModule::System)
        .target_type("agent")
        .target_id(&payload.agent_id)
        .metadata(json!({ "advertise_addr": payload.node.advertise_addr.clone(), "listen_port": payload.node.listen_port }));
    let result = node_runtime::register_node(
        &state.metadata_db,
        &payload.agent_id,
        payload.node.advertise_addr,
        payload.node.listen_port,
    )
    .await;
    let response = match result {
        Ok(result) => {
            runtime_metrics::record_register(true);
            platform_log = platform_log.set_success();

            let state_clone = Arc::clone(&state);
            let agent_id = result.agent_id.clone();
            tokio::spawn(async move {
                if let Err(err) =
                    crate::services::task_sync::reconcile_agent_tasks(&state_clone, &agent_id).await
                {
                    tracing::error!(
                        "Failed to reconcile scheduled tasks for agent {}: {}",
                        agent_id,
                        err
                    );
                }
            });

            Ok(ApiResponse::success_with_raw(
                "Node registered",
                Some(RuntimeSessionResponse {
                    node_id: result.node_id,
                    agent_id: result.agent_id,
                    session_id: result.session_id,
                    lease_id: result.lease_id,
                    lease_ttl_seconds: node_runtime::DEFAULT_LEASE_TTL_SECONDS,
                    heartbeat_interval_seconds: node_runtime::DEFAULT_HEARTBEAT_INTERVAL_SECONDS,
                    session_replaced: Some(result.session_replaced),
                    controller_compatibility,
                }),
            ))
        }
        Err(err) => {
            runtime_metrics::record_register(false);
            platform_log = platform_log.metadata(json!({ "error": err }));
            Err(err)
        }
    };
    platform_log.finish(&state.metadata_db);
    response
}

pub async fn heartbeat(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<HeartbeatPayload>,
) -> ApiResult<impl IntoResponse> {
    let controller_compatibility = ensure_runtime_agent_compatible(payload.compatibility.as_ref())?;
    let mut platform_log = PlatformLogEntry::new("runtime-agent", "runtime_heartbeat", addr.ip())
        .module(LogModule::System)
        .target_type("session")
        .target_id(&payload.session_id)
        .metadata(json!({
            "agent_id": payload.agent_id.clone(),
            "sequence": payload.sequence,
            "advertise_addr": payload.node.as_ref().and_then(|node| node.advertise_addr.clone()),
            "listen_port": payload.node.as_ref().and_then(|node| node.listen_port),
        }));
    let start = Instant::now();
    let result = node_runtime::heartbeat(
        &state.metadata_db,
        &payload.agent_id,
        &payload.session_id,
        &payload.lease_id,
        payload.sequence,
        payload
            .node
            .as_ref()
            .and_then(|node| node.advertise_addr.clone()),
        payload.node.as_ref().and_then(|node| node.listen_port),
        payload.resource,
    )
    .await;
    let response = match result {
        Ok(result) => {
            runtime_metrics::record_heartbeat(true, result.sequence_ignored, start.elapsed());
            platform_log = platform_log.set_success().metadata(json!({
                "agent_id": payload.agent_id,
                "sequence": payload.sequence,
                "sequence_ignored": result.sequence_ignored
            }));
            Ok(ApiResponse::success_with_raw(
                "Heartbeat accepted",
                Some(json!({
                    "leaseId": result.lease_id,
                    "leaseTtlSeconds": node_runtime::DEFAULT_LEASE_TTL_SECONDS,
                    "heartbeatIntervalSeconds": node_runtime::DEFAULT_HEARTBEAT_INTERVAL_SECONDS,
                    "requireReRegister": false,
                    "requireCertificateRotation": false,
                    "sequenceIgnored": result.sequence_ignored,
                    "controllerCompatibility": controller_compatibility,
                    "commands": [],
                })),
            ))
        }
        Err(err) => {
            runtime_metrics::record_heartbeat(false, false, start.elapsed());
            platform_log = platform_log.metadata(json!({ "error": err }));
            Err(err)
        }
    };
    platform_log.finish(&state.metadata_db);
    response
}

pub async fn deregister(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<DeregisterPayload>,
) -> ApiResult<impl IntoResponse> {
    let mut platform_log = PlatformLogEntry::new("runtime-agent", "runtime_deregister", addr.ip())
        .module(LogModule::System)
        .target_type("session")
        .target_id(&payload.session_id)
        .metadata(
            json!({ "agent_id": payload.agent_id.clone(), "reason": payload.reason.clone() }),
        );
    let result = node_runtime::deregister(
        &state.metadata_db,
        &payload.session_id,
        payload.reason.as_deref().unwrap_or("shutdown"),
    )
    .await;
    let response = match result {
        Ok(_) => {
            platform_log = platform_log.set_success();
            Ok(ApiResponse::success_with_raw(
                "Node deregistered",
                Some(json!({
                    "sessionClosed": true
                })),
            ))
        }
        Err(err) => {
            platform_log = platform_log.metadata(json!({ "error": err }));
            Err(err)
        }
    };
    platform_log.finish(&state.metadata_db);
    response
}

pub async fn rotate_certificate(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<RotateCertificatePayload>,
) -> ApiResult<impl IntoResponse> {
    let mut platform_log =
        PlatformLogEntry::new("runtime-agent", "runtime_rotate_certificate", addr.ip())
            .module(LogModule::System)
            .target_type("session")
            .target_id(&payload.session_id)
            .metadata(json!({
                "agent_id": payload.agent_id.clone(),
                "reason": payload.reason.clone()
            }));
    let result = node_runtime::rotate_certificate(
        &state.metadata_db,
        &payload.agent_id,
        &payload.session_id,
        payload.current_certificate_fingerprint.as_deref(),
        payload
            .certificate_request
            .as_ref()
            .and_then(|value| value.public_key_algorithm.as_deref())
            .unwrap_or("ed25519"),
        payload
            .certificate_request
            .as_ref()
            .and_then(|value| value.csr_pem.as_deref()),
    )
    .await;
    let response = match result {
        Ok(result) => {
            platform_log = platform_log.set_success();
            Ok(ApiResponse::success_with_raw(
                "Certificate rotated",
                Some(json!({
                    "agentId": result.agent_id,
                    "oldCertificateRetireAfter": result.old_certificate_retire_after,
                    "issuedCertificatePem": "PENDING",
                    "coreTrustBundlePem": "PENDING"
                })),
            ))
        }
        Err(err) => {
            platform_log = platform_log.metadata(json!({ "error": err }));
            Err(err)
        }
    };
    platform_log.finish(&state.metadata_db);
    response
}

/// runtime 制品下载：agent 使用短期 token 从主控下载已缓存或待缓存的升级制品。
pub async fn download_upgrade_artifact(
    State(state): State<Arc<AppState>>,
    Path((version, component, target_triple)): Path<(String, String, String)>,
    Query(query): Query<ArtifactDownloadQuery>,
) -> ApiResult<Response> {
    crate::services::upgrades::validate_artifact_download_token(
        &state.metadata_db,
        &query.token,
        &version,
        &component,
        &target_triple,
    )
    .await?;
    let artifact = crate::services::upgrades::ensure_artifact_cached(
        &state.metadata_db,
        &version,
        &component,
        &target_triple,
    )
    .await?;
    let bytes = tokio::fs::read(&artifact.path)
        .await
        .map_err(ApiError::Io)?;
    let response = Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", artifact.file_name),
        )
        .header("x-seclab-sha256", artifact.sha256)
        .body(Body::from(bytes))?;
    Ok(response)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReportTaskRunsPayload {
    pub agent_id: String,
    pub session_id: String,
    pub runs: Vec<seclab_contracts::scheduled_tasks::AgentScheduledTaskRunReport>,
}

pub async fn report_task_runs(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<ReportTaskRunsPayload>,
) -> ApiResult<impl IntoResponse> {
    let mut platform_log =
        PlatformLogEntry::new("runtime-agent", "runtime_report_task_runs", addr.ip())
            .module(LogModule::System)
            .target_type("session")
            .target_id(&payload.session_id)
            .metadata(json!({
                "agent_id": payload.agent_id.clone(),
                "runs_count": payload.runs.len(),
            }));

    // 校验 session 是否存在且处于活跃状态
    let session =
        crate::models::node_sessions::get_session_by_id(&state.metadata_db, &payload.session_id)
            .await
            .map_err(|err| ApiError::internal(format!("failed to query session: {err}")))?
            .ok_or_else(|| ApiError::BadRequest("runtime session not found".to_string()))?;

    if session.status != "active" {
        return Err(ApiError::BadRequest(
            "runtime session is not active".to_string(),
        ));
    }
    if session.agent_id != payload.agent_id {
        return Err(ApiError::BadRequest(
            "runtime identity mismatch".to_string(),
        ));
    }

    if payload.runs.is_empty() || payload.runs.len() > 100 {
        return Err(ApiError::bad_request(
            seclab_contracts::api::ErrorCode::BadRequest,
            "scheduled task run report must contain 1 to 100 items",
        ));
    }
    for report in &payload.runs {
        crate::models::task_scheduler::save_run_report(
            &state.metadata_db,
            &payload.agent_id,
            report,
        )
        .await?;
        if let Some(audit) = crate::models::task_scheduler::claim_run_terminal_audit(
            &state.metadata_db,
            &report.run.run_id,
        )
        .await?
            && let Ok(client_ip) = audit.client_ip.parse()
        {
            let failed = matches!(
                audit.status,
                seclab_contracts::scheduled_tasks::ScheduledTaskRunStatus::Failed
                    | seclab_contracts::scheduled_tasks::ScheduledTaskRunStatus::TimedOut
            );
            let target_name =
                crate::models::task_scheduler::get_task(&state.metadata_db, &audit.task_id)
                    .await?
                    .map(|task| task.name);
            PlatformLogEntry::new(&audit.actor_name, "scheduled_task_run_completed", client_ip)
                .user_id(audit.actor_user_id)
                .module(LogModule::System)
                .target_type("scheduled_task")
                .target_id(&audit.task_id)
                .trace_id(&audit.trace_id)
                .status(if failed {
                    LogStatus::Failed
                } else {
                    LogStatus::Success
                })
                .level(if failed {
                    PlatformLogLevel::Error
                } else {
                    PlatformLogLevel::Info
                })
                .metadata(json!({
                    "runId": audit.run_id,
                    "targetName": target_name,
                    "result": format!("{:?}", audit.status).to_lowercase(),
                    "errorCode": audit.error_code,
                }))
                .finish(&state.metadata_db);
        }
    }

    platform_log = platform_log.set_success();
    platform_log.finish(&state.metadata_db);

    Ok(ApiResponse::ok("Report received"))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TasksSnapshotParams {
    pub session_id: String,
    pub agent_id: String,
}

pub async fn get_tasks_snapshot(
    State(state): State<Arc<AppState>>,
    Query(params): Query<TasksSnapshotParams>,
) -> ApiResult<impl IntoResponse> {
    let session =
        crate::models::node_sessions::get_session_by_id(&state.metadata_db, &params.session_id)
            .await
            .map_err(|err| ApiError::internal(format!("failed to query session: {err}")))?
            .ok_or_else(|| ApiError::BadRequest("runtime session not found".to_string()))?;

    if session.status != "active" {
        return Err(ApiError::BadRequest(
            "runtime session is not active".to_string(),
        ));
    }
    if session.agent_id != params.agent_id {
        return Err(ApiError::BadRequest(
            "runtime identity mismatch".to_string(),
        ));
    }

    let snapshot =
        crate::models::task_scheduler::snapshot(&state.metadata_db, &params.agent_id).await?;

    Ok(ApiResponse::success_with_raw("Snapshot generated", snapshot).into_response())
}

pub fn runtime_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/callback-probe", get(callback_probe))
        .route("/enroll", post(enroll))
        .route("/register", post(register))
        .route("/heartbeat", post(heartbeat))
        .route("/deregister", post(deregister))
        .route("/terminal-tickets/consume", post(consume_terminal_ticket))
        .route("/rotate-certificate", post(rotate_certificate))
        .route("/scheduled-tasks/runs/report", post(report_task_runs))
        .route("/scheduled-tasks/snapshot", get(get_tasks_snapshot))
        .route(
            "/upgrades/artifacts/{version}/{component}/{target_triple}/download",
            get(download_upgrade_artifact),
        )
}
