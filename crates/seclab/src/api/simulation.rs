//! 协议仿真 API 控制器：提供仿真管理 CRUD、服务动态部署与免授权审计日志主动上报路由。

use crate::api::auth::AuthenticatedAdmin;
use crate::models::simulation::{
    BUILTIN_SIM_RULE_MAX_ID, SimLogRecord, SimRuleRecord, count_all_sim_logs,
    count_sim_logs_by_node, delete_sim_rule, get_sim_instance_by_id, get_sim_rule_by_id,
    insert_custom_sim_rule_deduplicated, insert_sim_log, list_all_sim_instances,
    list_all_sim_logs_paginated, list_sim_instances_by_node, list_sim_logs_by_node_paginated,
    list_sim_rules, update_sim_log_pcap,
};
use crate::services::logging::PlatformLogEntry;
use crate::services::simulation::{deploy_simulation_service, undeploy_simulation_service};
use crate::services::simulation_protocols::{
    custom_rule_protocols_label, is_custom_rule_protocol, list_simulation_protocols,
};
use crate::state::AppState;
use crate::types::{ApiError, ApiResponse, ApiResult};
use axum::{
    Router,
    extract::{Json, Multipart, Path, Query, State, connect_info::ConnectInfo},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
};
use chrono::Utc;
use seclab_contracts::simulation::{
    SimulationEventType, SimulationProtocol, SimulationProtocolCapability, parse_simulation_config,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;

const SIM_RULE_PACKAGE_EXTENSION: &str = ".slrp";

// --- 请求与响应载荷结构定义 ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimLogQuery {
    pub page: Option<i32>,
    pub page_size: Option<i32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimLogListResponse {
    pub total: i64,
    pub page: i32,
    pub page_size: i32,
    pub records: Vec<SimLogRecord>,
}

/// 将协议能力注册表转换为 API 响应载荷。
fn simulation_protocol_responses() -> Vec<SimulationProtocolCapability> {
    list_simulation_protocols()
        .iter()
        .map(|item| SimulationProtocolCapability {
            protocol: SimulationProtocol::from_str(item.protocol)
                .expect("simulation protocol registry must use contract protocol identifiers"),
            label: item.label.to_string(),
            default_port: item.default_port,
            deployable: item.deployable,
            custom_rule_creatable: item.custom_rule_creatable,
            event_types: item
                .event_types
                .iter()
                .map(|event_type| {
                    SimulationEventType::from_str(event_type)
                        .expect("simulation protocol registry must use contract event identifiers")
                })
                .collect(),
        })
        .collect::<Vec<_>>()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRuleRequest {
    pub id: Option<i64>,
    pub name: String,
    pub name_en: Option<String>,
    pub cve: Option<String>,
    pub category: Option<String>,
    pub description_zh: Option<String>,
    pub description_en: Option<String>,
    pub protocol: String,
    pub default_port: Option<i64>,
    pub config_yaml: String, // 实际用作 JSON 格式配置
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploySimulationRequest {
    pub node_id: String,
    pub port: u16,
    pub rule_id: i64,
    pub seclab_callback_url: String, // 传入控制端的完整回调端点，例如 "http://127.0.0.1:7310/api/v1/simulation-public/log"
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UndeploySimulationRequest {
    pub instance_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportSimLogRequest {
    pub rule_id: String,
    pub node_id: String,
    pub client_ip: String,
    pub client_port: u16,
    pub server_port: u16,
    pub event_type: String,
    pub detail_summary: String,
    pub payload_hex: Option<String>,
}

/// 校验自定义仿真规则配置必须是可被 Agent 解析的协议 JSON 对象。
fn validate_custom_rule_config(protocol: &str, config_yaml: &str) -> ApiResult<()> {
    let value: serde_json::Value = serde_json::from_str(config_yaml).map_err(|err| {
        ApiError::BadRequest(format!(
            "Invalid {} simulation config JSON: {}",
            protocol.to_uppercase(),
            err
        ))
    })?;

    if !value.is_object() {
        return Err(ApiError::BadRequest(format!(
            "{} simulation config must be a JSON object.",
            protocol.to_uppercase()
        )));
    }

    let protocol_kind = SimulationProtocol::from_str(protocol)
        .map_err(|err| ApiError::BadRequest(err.to_string()))?;

    parse_simulation_config(protocol_kind, value).map_err(|err| {
        ApiError::BadRequest(format!(
            "Invalid {} simulation config: {}",
            protocol.to_uppercase(),
            err
        ))
    })?;

    Ok(())
}

// --- 控制端管理接口处理器 ---

/// 列出主控当前认识的协议仿真能力。
pub async fn list_protocols_handler() -> ApiResult<impl IntoResponse> {
    Ok(ApiResponse::success_with_raw(
        "Simulation protocols loaded",
        simulation_protocol_responses(),
    )
    .into_response())
}

/// 创建仿真规则。
pub async fn create_rule_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateRuleRequest>,
) -> ApiResult<impl IntoResponse> {
    if !is_custom_rule_protocol(&payload.protocol) {
        return Err(ApiError::BadRequest(format!(
            "Only {} protocol is supported for custom simulation rules at present stage.",
            custom_rule_protocols_label()
        )));
    }
    validate_custom_rule_config(&payload.protocol, &payload.config_yaml)?;

    if payload.id.is_some_and(|id| id <= BUILTIN_SIM_RULE_MAX_ID) {
        return Err(ApiError::BadRequest(format!(
            "Custom simulation rule id must be greater than {}.",
            BUILTIN_SIM_RULE_MAX_ID
        )));
    }

    let now = Utc::now().to_rfc3339();
    let record = SimRuleRecord {
        id: payload.id.unwrap_or(BUILTIN_SIM_RULE_MAX_ID + 1),
        name: payload.name.clone(),
        name_en: payload.name_en.unwrap_or_else(|| payload.name.clone()),
        cve: payload.cve,
        category: payload.category.unwrap_or_else(|| "custom".to_string()),
        description_zh: payload
            .description_zh
            .unwrap_or_else(|| payload.name.clone()),
        description_en: payload.description_en.unwrap_or_else(|| "".to_string()),
        protocol: payload.protocol,
        default_port: payload.default_port,
        config_yaml: payload.config_yaml,
        source_type: "custom".to_string(),
        source_package_id: None,
        rule_status: "active".to_string(),
        created_at: now.clone(),
        updated_at: now,
    };

    let record = insert_custom_sim_rule_deduplicated(&state.metadata_db, record, payload.id)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    Ok(ApiResponse::success_with_raw("Simulation rule created", record).into_response())
}

/// 列出所有仿真规则。
pub async fn list_rules_handler(
    State(state): State<Arc<AppState>>,
) -> ApiResult<impl IntoResponse> {
    let rules = list_sim_rules(&state.metadata_db)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;
    Ok(ApiResponse::success_with_raw("Simulation rules loaded", rules).into_response())
}

/// 删除仿真规则。
pub async fn delete_rule_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<impl IntoResponse> {
    // 检查规则是否存在
    let existing = get_sim_rule_by_id(&state.metadata_db, id)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;
    let Some(existing_record) = existing else {
        return Err(ApiError::not_found(
            seclab_contracts::api::ErrorCode::SimulationRuleNotFound,
            format!("Rule {} not found", id),
        ));
    };

    if existing_record.source_type == "package" || id <= BUILTIN_SIM_RULE_MAX_ID {
        return Err(ApiError::BadRequest(
            "Official package simulation rules are not allowed to be deleted. Please upgrade or disable the rules package instead.".to_string(),
        ));
    }

    delete_sim_rule(&state.metadata_db, id)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    Ok(ApiResponse::success_with_raw("Simulation rule deleted", ()).into_response())
}

/// 导入规则包。
pub async fn import_rule_package_handler(
    State(state): State<Arc<AppState>>,
    admin: AuthenticatedAdmin,
    ConnectInfo(conn): ConnectInfo<SocketAddr>,
    mut multipart: Multipart,
) -> ApiResult<impl IntoResponse> {
    let mut file_bytes = Vec::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "archive" {
            let file_name = field.file_name().unwrap_or_default().to_string();
            if !file_name.ends_with(SIM_RULE_PACKAGE_EXTENSION) {
                return Err(ApiError::BadRequest(
                    "simulation rule package must be .slrp".to_string(),
                ));
            }
            file_bytes = field.bytes().await.unwrap_or_default().to_vec();
        }
    }

    if file_bytes.is_empty() {
        return Err(ApiError::BadRequest(
            "Upload file 'archive' is missing or empty.".to_string(),
        ));
    }

    let import_res =
        crate::services::rule_package::import_rule_package(&state.metadata_db, &file_bytes).await;

    match &import_res {
        Ok((record, skipped)) => {
            PlatformLogEntry::new(&admin.username, "simulation_rule_package_import", conn.ip())
                .module(crate::models::logging::LogModule::System)
                .target_type("simulation_rule_package")
                .target_id(&record.version)
                .set_success()
                .metadata(serde_json::json!({
                    "package_id": record.package_id,
                    "version": record.version,
                    "rule_count": record.rule_count,
                    "skipped": *skipped,
                }))
                .finish(&state.metadata_db);
        }
        Err(err) => {
            PlatformLogEntry::new(&admin.username, "simulation_rule_package_import", conn.ip())
                .module(crate::models::logging::LogModule::System)
                .target_type("simulation_rule_package")
                .status(crate::models::logging::LogStatus::Failed)
                .metadata(serde_json::json!({
                    "error": err.to_string(),
                }))
                .finish(&state.metadata_db);
        }
    }

    let (record, skipped) = import_res.map_err(ApiError::BadRequest)?;

    let message = if skipped {
        "Rules package is already up to date"
    } else {
        "Rules package imported successfully"
    };

    let mut response = ApiResponse::success_with_raw(message, record);
    if skipped {
        response.message_key =
            Some("app.simulation.rules.messages.packageImportAlreadyLatest".to_string());
    } else {
        response.message_key =
            Some("app.simulation.rules.messages.packageImportSuccess".to_string());
    }

    Ok(response.into_response())
}

/// 获取已导入的规则包历史列表。
pub async fn list_rule_packages_handler(
    State(state): State<Arc<AppState>>,
    _admin: AuthenticatedAdmin,
) -> ApiResult<impl IntoResponse> {
    let list = crate::models::simulation::list_rule_packages(&state.metadata_db)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;
    Ok(
        ApiResponse::success_with_raw("Rule packages historical records loaded", list)
            .into_response(),
    )
}

/// 获取当前激活的规则包。
pub async fn get_current_rule_package_handler(
    State(state): State<Arc<AppState>>,
    _admin: AuthenticatedAdmin,
) -> ApiResult<impl IntoResponse> {
    let record =
        crate::models::simulation::get_active_rule_package(&state.metadata_db, "seclab-sim-rules")
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?;
    Ok(
        ApiResponse::success_with_raw("Current active rules package loaded", record)
            .into_response(),
    )
}

/// 部署仿真到节点。
pub async fn deploy_simulation_handler(
    State(state): State<Arc<AppState>>,
    admin: AuthenticatedAdmin,
    ConnectInfo(conn): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<DeploySimulationRequest>,
) -> ApiResult<impl IntoResponse> {
    let trace_id = crate::services::logging::resolve_trace_id(&headers);

    let result = deploy_simulation_service(
        &state.metadata_db,
        &payload.node_id,
        payload.port,
        payload.rule_id,
        &payload.seclab_callback_url,
    )
    .await;

    let mut platform_log = PlatformLogEntry::new(&admin.username, "simulation_deploy", conn.ip())
        .module(crate::models::logging::LogModule::System)
        .target_type("simulation_instance")
        .trace_id(&trace_id)
        .source("seclab_api")
        .request("POST", "/api/v1/simulation/deploy");

    match &result {
        Ok(instance) => {
            platform_log = platform_log
                .target_id(&instance.instance_id)
                .metadata(serde_json::json!({
                    "node_id": payload.node_id,
                    "port": payload.port,
                    "rule_id": payload.rule_id,
                    "instance_id": instance.instance_id,
                }))
                .set_success();
        }
        Err(err) => {
            platform_log = platform_log
                .target_id(&format!("node:{}/port:{}", payload.node_id, payload.port))
                .metadata(serde_json::json!({
                    "node_id": payload.node_id,
                    "port": payload.port,
                    "rule_id": payload.rule_id,
                    "error": err.to_string(),
                }));
        }
    }
    platform_log.finish(&state.metadata_db);

    let instance = result?;
    Ok(ApiResponse::success_with_raw("Simulation service deployed", instance).into_response())
}

/// 从节点注销并停止仿真实例。
pub async fn undeploy_simulation_handler(
    State(state): State<Arc<AppState>>,
    admin: AuthenticatedAdmin,
    ConnectInfo(conn): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<UndeploySimulationRequest>,
) -> ApiResult<impl IntoResponse> {
    let trace_id = crate::services::logging::resolve_trace_id(&headers);

    // 在下线前，先查询该 instance 记录以获取更多元数据（如 node_id, rule_id, listen_port）
    let instance_opt = get_sim_instance_by_id(&state.metadata_db, &payload.instance_id)
        .await
        .ok()
        .flatten();

    let result = undeploy_simulation_service(&state.metadata_db, &payload.instance_id).await;

    let mut platform_log = PlatformLogEntry::new(&admin.username, "simulation_undeploy", conn.ip())
        .module(crate::models::logging::LogModule::System)
        .target_type("simulation_instance")
        .target_id(&payload.instance_id)
        .trace_id(&trace_id)
        .source("seclab_api")
        .request("POST", "/api/v1/simulation/undeploy");

    match &result {
        Ok(_) => {
            let metadata = if let Some(instance) = &instance_opt {
                serde_json::json!({
                    "instance_id": payload.instance_id,
                    "node_id": instance.node_id,
                    "rule_id": instance.rule_id,
                    "port": instance.listen_port,
                })
            } else {
                serde_json::json!({
                    "instance_id": payload.instance_id,
                })
            };

            platform_log = platform_log.metadata(metadata).set_success();
        }
        Err(err) => {
            let metadata = if let Some(instance) = &instance_opt {
                serde_json::json!({
                    "instance_id": payload.instance_id,
                    "node_id": instance.node_id,
                    "rule_id": instance.rule_id,
                    "port": instance.listen_port,
                    "error": err.to_string(),
                })
            } else {
                serde_json::json!({
                    "instance_id": payload.instance_id,
                    "error": err.to_string(),
                })
            };

            platform_log = platform_log.metadata(metadata);
        }
    }
    platform_log.finish(&state.metadata_db);

    result?;
    Ok(ApiResponse::success_with_raw("Simulation instance stopped", ()).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureControlRequest {
    pub instance_id: String,
}

/// 启动实例的常驻实时网络流量取证嗅探。
pub async fn start_capture_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CaptureControlRequest>,
) -> ApiResult<impl IntoResponse> {
    // 1. 获取活动实例
    let Some(instance) = get_sim_instance_by_id(&state.metadata_db, &payload.instance_id)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?
    else {
        return Err(ApiError::not_found(
            seclab_contracts::api::ErrorCode::NodeNotFound,
            format!("Simulation instance {} not found", payload.instance_id),
        ));
    };

    // 2. 构建 mTLS 客户端，向边缘 Agent 下发开启指令
    let client = crate::models::node_runtime_client::NodeRuntimeClient::from_node_route(
        &state.metadata_db,
        Some(&instance.node_id),
    )
    .await?;
    let callback_url = {
        let base_seclab_url = crate::services::node_deploy::resolve_seclab_url();
        let trimmed_base = base_seclab_url.trim_end_matches('/');
        format!("{}/api/v1/simulation-public/log", trimmed_base)
    };

    let agent_payload = serde_json::json!({
        "port": instance.listen_port as u16,
        "instanceId": instance.instance_id.clone(),
        "ruleId": instance.rule_id.to_string(),
        "nodeId": instance.node_id.clone(),
        "callbackUrl": callback_url,
    });

    let _agent_resp: serde_json::Value = client
        .post_json("/api/v1/agent/simulation/pcap/start", &agent_payload)
        .await
        .map_err(|err| {
            ApiError::Internal(format!(
                "Failed to dispatch start capture command to agent: {}",
                err
            ))
        })?;

    // 3. 更新控制端数据库中实例的 pcap 状态与时间戳
    let now_ts = Utc::now().timestamp();
    crate::models::simulation::update_sim_instance_pcap(
        &state.metadata_db,
        &instance.instance_id,
        "capturing",
        Some(now_ts),
        None,
    )
    .await
    .map_err(|err| ApiError::Internal(err.to_string()))?;

    Ok(ApiResponse::success_with_raw("Forensic capture started", ()).into_response())
}

/// 停止实例的常驻实时网络流量取证嗅探，触发回传。
pub async fn stop_capture_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CaptureControlRequest>,
) -> ApiResult<impl IntoResponse> {
    // 1. 获取活动实例
    let Some(instance) = get_sim_instance_by_id(&state.metadata_db, &payload.instance_id)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?
    else {
        return Err(ApiError::not_found(
            seclab_contracts::api::ErrorCode::NodeNotFound,
            format!("Simulation instance {} not found", payload.instance_id),
        ));
    };

    // 2. 构建 mTLS 客户端，向边缘 Agent 下发关闭指令
    let client = crate::models::node_runtime_client::NodeRuntimeClient::from_node_route(
        &state.metadata_db,
        Some(&instance.node_id),
    )
    .await?;
    let agent_payload = serde_json::json!({
        "port": instance.listen_port as u16,
    });

    let _agent_resp: serde_json::Value = client
        .post_json("/api/v1/agent/simulation/pcap/stop", &agent_payload)
        .await
        .map_err(|err| {
            ApiError::Internal(format!(
                "Failed to dispatch stop capture command to agent: {}",
                err
            ))
        })?;

    // 3. 停止指令下发成功。数据库状态无需在此处立即设置为 ready，
    // 我们将保持为 capturing，直到边缘 Agent 的物理 PCAP 异步上传到 report_sim_pcap_handler 时，
    // 控制端才会自动将其优雅变更为 ready 并绑定正确的物理文件名。

    Ok(ApiResponse::success_with_raw("Forensic capture stopped", ()).into_response())
}

/// 重置并物理擦除实例的流量取证包。
pub async fn reset_capture_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CaptureControlRequest>,
) -> ApiResult<impl IntoResponse> {
    // 1. 获取活动实例
    let Some(instance) = get_sim_instance_by_id(&state.metadata_db, &payload.instance_id)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?
    else {
        return Err(ApiError::not_found(
            seclab_contracts::api::ErrorCode::NodeNotFound,
            format!("Simulation instance {} not found", payload.instance_id),
        ));
    };

    // 2. 如果存在关联的物理 PCAP 文件，从服务器磁盘彻底擦除，防止信息泄露
    if let Some(filename) = &instance.pcap_file_path {
        let pcap_path = crate::config::pcap_dir().join(filename);
        if pcap_path.exists() {
            let _ = tokio::fs::remove_file(pcap_path.clone()).await;
            tracing::info!(
                "[Control Plane] Deleted physical PCAP file '{:?}' on reset request.",
                pcap_path
            );
        }
    }

    // 3. 将状态重置为 idle，物理文件路径归零
    crate::models::simulation::update_sim_instance_pcap(
        &state.metadata_db,
        &instance.instance_id,
        "idle",
        None,
        None,
    )
    .await
    .map_err(|err| ApiError::Internal(err.to_string()))?;

    Ok(ApiResponse::success_with_raw("Forensic capture data reset", ()).into_response())
}

/// 获取特定节点的全部仿真运行实例。
pub async fn list_instances_by_node_handler(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let instances = if node_id == "all" {
        list_all_sim_instances(&state.metadata_db)
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?
    } else {
        list_sim_instances_by_node(&state.metadata_db, &node_id)
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?
    };
    Ok(ApiResponse::success_with_raw("Simulation instances loaded", instances).into_response())
}

/// 获取特定节点的仿真审计日志列表。
pub async fn list_sim_logs_handler(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
    Query(query): Query<SimLogQuery>,
) -> ApiResult<impl IntoResponse> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(50).max(1);
    let offset = ((page - 1) * page_size) as i64;
    let limit = page_size as i64;

    let (total, logs) = if node_id == "all" {
        let total = count_all_sim_logs(&state.metadata_db)
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?;
        let records = list_all_sim_logs_paginated(&state.metadata_db, limit, offset)
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?;
        (total, records)
    } else {
        let total = count_sim_logs_by_node(&state.metadata_db, &node_id)
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?;
        let records = list_sim_logs_by_node_paginated(&state.metadata_db, &node_id, limit, offset)
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?;
        (total, records)
    };

    Ok(ApiResponse::success_with_raw(
        "Simulation logs loaded",
        SimLogListResponse {
            total,
            page,
            page_size,
            records: logs,
        },
    )
    .into_response())
}

/// 安全地提供 PCAP 数据包文件下载，防范路径穿越风险。
pub async fn download_pcap_handler(
    State(_state): State<Arc<AppState>>,
    Path(filename): Path<String>,
) -> impl IntoResponse {
    let sanitized_filename = std::path::Path::new(&filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    if sanitized_filename.is_empty() {
        return (StatusCode::BAD_REQUEST, "Invalid pcap filename").into_response();
    }

    let pcap_path = crate::config::pcap_dir().join(sanitized_filename);

    match tokio::fs::read(&pcap_path).await {
        Ok(bytes) => {
            let body = axum::body::Body::from(bytes);
            let mut headers = axum::http::HeaderMap::new();
            headers.insert(
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("application/vnd.tcpdump.pcap"),
            );
            headers.insert(
                axum::http::header::CONTENT_DISPOSITION,
                axum::http::HeaderValue::from_str(&format!(
                    "attachment; filename=\"{}\"",
                    sanitized_filename
                ))
                .unwrap(),
            );
            (StatusCode::OK, headers, body).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "PCAP file not found").into_response(),
    }
}

// --- 免授权公开数据上报处理器 ---

/// 接收来自边缘 Agent 异步上传的协议仿真审计日志。
pub async fn report_sim_log_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ReportSimLogRequest>,
) -> impl IntoResponse {
    // 根据 node_id 查寻可能匹配的活动 instance_id，以确保日志表引用关系建立
    let now = Utc::now().to_rfc3339();

    // 查询该节点的活动运行实例，若无法匹配特定 instance，则留空字符串以增强兼容性
    let instance_id = match crate::models::simulation::get_sim_instance_by_node_port(
        &state.metadata_db,
        &payload.node_id,
        payload.server_port as i64,
    )
    .await
    {
        Ok(Some(inst)) => inst.instance_id,
        _ => "".to_string(), // 若端口无法完美匹配，仍作审计归档
    };

    let record = SimLogRecord {
        log_id: None,
        instance_id: if instance_id.is_empty() {
            payload.rule_id.clone()
        } else {
            instance_id
        },
        node_id: payload.node_id,
        client_ip: payload.client_ip,
        client_port: payload.client_port as i64,
        event_type: payload.event_type,
        detail_summary: payload.detail_summary,
        payload_hex: payload.payload_hex,
        pcap_file_path: None,
        timestamp: now,
    };

    if let Err(err) = insert_sim_log(&state.metadata_db, &record).await {
        tracing::error!(
            "Failed to save incoming simulation log from agent: {:?}",
            err
        );
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to persist simulation log",
        );
    }

    (StatusCode::OK, "Log reported successfully")
}

/// 接收来自边缘 Agent 异步上传的协议仿真 PCAP 抓包文件并与审计日志自动绑定。
pub async fn report_sim_pcap_handler(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut node_id = String::new();
    let mut _rule_id = String::new();
    let mut client_ip = String::new();
    let mut client_port_str = String::new();
    let mut file_bytes = Vec::new();
    let mut filename = String::new();
    let mut is_empty = false;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "nodeId" => {
                if let Ok(val) = field.text().await {
                    node_id = val;
                }
            }
            "ruleId" => {
                if let Ok(val) = field.text().await {
                    _rule_id = val;
                }
            }
            "clientIp" => {
                if let Ok(val) = field.text().await {
                    client_ip = val;
                }
            }
            "clientPort" => {
                if let Ok(val) = field.text().await {
                    client_port_str = val;
                }
            }
            "isEmpty" => {
                if let Ok(val) = field.text().await {
                    is_empty = val == "true";
                }
            }
            "pcapFile" => {
                if let Some(fname) = field.file_name() {
                    let sanitized = std::path::Path::new(fname)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();
                    if !sanitized.is_empty() {
                        filename = sanitized;
                    }
                }
                if let Ok(bytes) = field.bytes().await {
                    file_bytes = bytes.to_vec();
                }
            }
            _ => {}
        }
    }

    if node_id.is_empty() || client_ip.is_empty() || client_port_str.is_empty() {
        return (StatusCode::BAD_REQUEST, "Missing required multipart fields").into_response();
    }

    if !is_empty && file_bytes.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "Missing file bytes for non-empty pcap",
        )
            .into_response();
    }

    let client_port = match client_port_str.parse::<i64>() {
        Ok(port) => port,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid client port").into_response(),
    };

    if is_empty {
        if let Ok(Some(instance)) = crate::models::simulation::get_sim_instance_by_node_port(
            &state.metadata_db,
            &node_id,
            client_port,
        )
        .await
        {
            if let Err(err) = crate::models::simulation::update_sim_instance_pcap(
                &state.metadata_db,
                &instance.instance_id,
                "idle",
                None,
                None,
            )
            .await
            {
                tracing::error!("Failed to update sim instance to idle: {:?}", err);
            } else {
                tracing::info!(
                    "Successfully reset empty capture PCAP to idle for instance '{}' (port {})",
                    instance.instance_id,
                    client_port
                );
            }
        }
        return (
            StatusCode::OK,
            "Empty PCAP handled and instance reset to idle",
        )
            .into_response();
    }

    let pcap_dir = crate::config::pcap_dir();
    if let Err(err) = tokio::fs::create_dir_all(&pcap_dir).await {
        tracing::error!("Failed to create pcap directory: {:?}", err);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to initialize storage",
        )
            .into_response();
    }

    if filename.is_empty() {
        let timestamp = Utc::now().timestamp_millis();
        filename = format!("{}_{}_{}.pcap", node_id, client_port, timestamp);
    }

    let file_path = pcap_dir.join(&filename);
    let file_path_str = file_path.to_string_lossy().to_string();

    if let Err(err) = tokio::fs::write(&file_path, file_bytes).await {
        tracing::error!("Failed to save pcap file: {:?}", err);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to save pcap file",
        )
            .into_response();
    }

    if let Err(err) = update_sim_log_pcap(
        &state.metadata_db,
        &node_id,
        client_port,
        &client_ip,
        &file_path_str,
    )
    .await
    {
        tracing::error!("Failed to update sim log with pcap path: {:?}", err);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to bind pcap to log",
        )
            .into_response();
    }

    // 自动反查并关联仿真运行实例的状态
    if let Ok(Some(instance)) = crate::models::simulation::get_sim_instance_by_node_port(
        &state.metadata_db,
        &node_id,
        client_port,
    )
    .await
    {
        if let Err(err) = crate::models::simulation::update_sim_instance_pcap(
            &state.metadata_db,
            &instance.instance_id,
            "ready",
            None,
            Some(&filename),
        )
        .await
        {
            tracing::error!("Failed to update sim instance pcap status: {:?}", err);
        } else {
            tracing::info!(
                "Successfully associated PCAP '{}' with instance '{}' (port {}) and set status to ready.",
                filename,
                instance.instance_id,
                client_port
            );
        }
    }

    (StatusCode::OK, "PCAP uploaded and bound successfully").into_response()
}

// --- 路由组装挂载器 ---

pub fn simulation_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/protocols", get(list_protocols_handler))
        .route("/rule", post(create_rule_handler))
        .route("/rules", get(list_rules_handler))
        .route("/rule/{id}", delete(delete_rule_handler))
        .route(
            "/rule-package/import",
            post(import_rule_package_handler)
                .layer(axum::extract::DefaultBodyLimit::max(50 * 1024 * 1024)),
        )
        .route("/rule-packages/list", get(list_rule_packages_handler))
        .route(
            "/rule-package/current",
            get(get_current_rule_package_handler),
        )
        .route("/deploy", post(deploy_simulation_handler))
        .route("/undeploy", post(undeploy_simulation_handler))
        .route("/capture/start", post(start_capture_handler))
        .route("/capture/stop", post(stop_capture_handler))
        .route("/capture/reset", post(reset_capture_handler))
        .route(
            "/node/{node_id}/instances",
            get(list_instances_by_node_handler),
        )
        .route("/node/{node_id}/logs", get(list_sim_logs_handler))
        .route("/pcap/download/{filename}", get(download_pcap_handler))
}

/// 控制端免授权公开上报路由器。
pub fn simulation_public_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/log", post(report_sim_log_handler))
        .route("/pcap", post(report_sim_pcap_handler))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulation_protocol_responses_match_registered_capabilities() {
        let responses = simulation_protocol_responses();

        assert_eq!(responses.len(), list_simulation_protocols().len());
        assert!(
            responses
                .iter()
                .any(|item| item.protocol == SimulationProtocol::Redis)
        );
        assert!(responses.iter().all(|item| item.custom_rule_creatable));

        let first = serde_json::to_value(&responses[0]).unwrap();
        assert!(first.get("defaultPort").is_some());
        assert!(first.get("customRuleCreatable").is_some());
        assert!(first.get("eventTypes").is_some());
    }

    #[test]
    fn validate_custom_rule_config_accepts_minimal_protocol_configs() {
        let cases = [
            ("http", r#"{"server_header":"nginx","exploit_paths":[]}"#),
            (
                "redis",
                r#"{"require_auth":true,"password":"redis123","keys":{"session":"admin"}}"#,
            ),
            (
                "smtp",
                r#"{"hostname":"mail.seclab.local","credentials":[{"username":"admin","password":"password"}]}"#,
            ),
            (
                "pop3",
                r#"{"messages":[{"from":"alerts@seclab.local","to":["admin@seclab.local"],"subject":"Alert","body":"Body"}]}"#,
            ),
            (
                "imap",
                r#"{"mailboxes":{"INBOX":[{"from":"alerts@seclab.local","to":["admin@seclab.local"],"subject":"Alert","body":"Body"}]}}"#,
            ),
            (
                "ssh",
                r#"{"banner":"SSH-2.0-OpenSSH_8.9","credentials":[{"username":"root","password":"toor"}]}"#,
            ),
            (
                "ftp",
                r#"{"server_name":"UNIX Type: L8","allow_anonymous":false,"credentials":[{"username":"admin","password":"password"}]}"#,
            ),
            (
                "rdp",
                r#"{"flags":1,"credentials":[{"username":"administrator","password":"Password123"}]}"#,
            ),
        ];

        for (protocol, config) in cases {
            validate_custom_rule_config(protocol, config)
                .unwrap_or_else(|err| panic!("{} config should be valid: {}", protocol, err));
        }
    }

    #[test]
    fn validate_custom_rule_config_rejects_invalid_json_shape() {
        assert!(validate_custom_rule_config("http", "[]").is_err());
        assert!(validate_custom_rule_config("redis", r#"{"keys":[]}"#).is_err());
        assert!(validate_custom_rule_config("rdp", r#"{"flags":"tls"}"#).is_err());
    }
}
