//! 节点控制面 API：以 `node_id` 作为唯一资源标识。

use crate::api::auth::AuthenticatedAdmin;
use crate::api::node_proxy::{proxy_handler_for_node, websocket_proxy_handler_for_node};
use crate::models::logging::LogModule;
use crate::models::node_api_types::{NodeCreatePayload, NodeUpdatePayload};
use crate::models::nodes::NodeStatus;
use crate::services::logging::{self, OperationEventBuilder};
use crate::services::node_check::{NodeCheckResponse, check_node_health};
use crate::services::node_deploy::{
    NodeDeployError, NodeDeployInput, NodeDeployPayload, deploy_node, deploy_node_with,
    generate_node_id, repair_node, retire_node, uninstall_node,
};
use crate::services::node_precheck::{NodePrecheckInput, NodePrecheckResponse, precheck_node};
use crate::services::node_state_machine::{
    transition_to_awaiting_registration, transition_to_deploy_failed,
};
use crate::services::node_target_guard::{
    NodeTargetGuardInput, assert_target_not_current_host, assert_target_not_node_host,
};
use crate::services::runtime_metrics;
use crate::services::{node_enrollment, node_inventory, node_provisioning, node_read_model};
use crate::state::AppState;
use crate::types::{ApiError, ApiResponse, ApiResult, new_uuid_v7};
use axum::{
    Json, Router,
    extract::{Path, Query, State, connect_info::ConnectInfo},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::{any, delete, get, post, put},
};
use seclab_contracts::api::ErrorCode;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDeployCreateResult {
    pub logs: Vec<String>,
    pub node: Option<node_read_model::NodeSummaryView>,
}

/// 一步创建并部署节点所需的字段。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDeployCreatePayload {
    pub name: Option<String>,
    pub group_id: Option<String>,
    pub description: Option<String>,
    pub addr: Option<String>,
    pub port: Option<String>,
    pub user: Option<String>,
    pub pwd: Option<String>,
    pub private_key: Option<String>,
    pub private_key_passphrase: Option<String>,
    pub auth_mode: Option<String>,
    pub tags: Option<Vec<String>>,
    pub metadata: Option<serde_json::Value>,
    pub install_dir: Option<String>,
    #[serde(rename = "servicePort", alias = "listenPort")]
    pub service_port: Option<String>,
    pub sync_enabled: Option<bool>,
    pub listen_addr: Option<String>,
    pub seclab_url: Option<String>,
}

/// 远程预检所需的连接与认证信息。
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodePrecheckPayload {
    pub name: Option<String>,
    pub addr: Option<String>,
    pub port: Option<String>,
    pub user: Option<String>,
    pub pwd: Option<String>,
    pub private_key: Option<String>,
    pub private_key_passphrase: Option<String>,
    pub auth_mode: Option<String>,
    #[serde(rename = "servicePort", alias = "listenPort")]
    pub service_port: Option<String>,
    pub install_dir: Option<String>,
    pub seclab_url: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeHistoryQuery {
    pub limit: Option<i64>,
}

fn normalize_history_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(20).clamp(1, 100)
}

pub async fn list(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    let mut nodes = node_read_model::list_node_summaries(&state.metadata_db).await?;

    let local_exists = crate::types::agent_socket_path().exists();
    let local_status = if local_exists {
        "online".to_string()
    } else {
        "offline".to_string()
    };

    let local_resource = {
        let cache = state.local_node_resource.lock().await;
        cache.clone()
    };

    let mut metadata = serde_json::json!({});
    if let Some(res) = local_resource
        && let Some(obj) = metadata.as_object_mut()
    {
        obj.insert("resource".to_string(), res);
    }

    let local_node = node_read_model::NodeSummaryView {
        node_id: "local".to_string(),
        name: "Local Node".to_string(),
        group_name: "default".to_string(),
        description: Some("Local Controller".to_string()),
        address: Some("127.0.0.1".to_string()),
        service_port: None,
        status: local_status.clone(),
        lifecycle_status: local_status,
        runtime_status: Some("active".to_string()),
        health_status: Some("online".to_string()),
        tags: vec!["local".to_string()],
        metadata: Some(metadata),
        last_seen_at: None,
    };

    nodes.insert(0, local_node);

    Ok(ApiResponse::success_with_raw("Nodes loaded", Some(nodes)).into_response())
}

pub async fn detail(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> ApiResult<Response> {
    let detail = node_read_model::get_node_detail(&state.metadata_db, &node_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(ApiResponse::success_with_raw("Node detail loaded", Some(detail)).into_response())
}

pub async fn observations(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
    Query(query): Query<NodeHistoryQuery>,
) -> ApiResult<Response> {
    let items = node_read_model::list_node_observation_views(
        &state.metadata_db,
        &node_id,
        normalize_history_limit(query.limit),
    )
    .await?;
    Ok(
        ApiResponse::success_with_raw("Node observation history loaded", Some(items))
            .into_response(),
    )
}

pub async fn sessions(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
    Query(query): Query<NodeHistoryQuery>,
) -> ApiResult<Response> {
    let items = node_read_model::list_node_session_views(
        &state.metadata_db,
        &node_id,
        normalize_history_limit(query.limit),
    )
    .await?;
    Ok(ApiResponse::success_with_raw("Node session history loaded", Some(items)).into_response())
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<NodeCreatePayload>,
) -> ApiResult<Response> {
    if let Some(ref name) = payload.name
        && !name.trim().is_empty()
    {
        let is_conflict =
            node_inventory::check_name_conflict(&state.metadata_db, name, None).await?;
        if is_conflict {
            return Err(ApiError::conflict(
                ErrorCode::NodeAlreadyExists,
                "node name already exists",
            ));
        }
    }
    if should_guard_create_payload(&payload) {
        let guard_input = NodeTargetGuardInput {
            addr: payload.addr.clone().ok_or_else(|| {
                ApiError::BadRequest("node address must not be empty".to_string())
            })?,
            port: payload.port.clone(),
            user: payload
                .user
                .clone()
                .ok_or_else(|| ApiError::BadRequest("SSH user must not be empty".to_string()))?,
            auth_mode: payload.auth_mode.clone(),
            password: payload.pwd.clone(),
            private_key: payload.private_key.clone(),
            private_key_passphrase: payload.private_key_passphrase.clone(),
        };
        assert_target_not_node_host(guard_input).await?;
    }

    let node_id = payload
        .agent_id
        .clone()
        .unwrap_or_else(|| generate_node_id().unwrap_or_else(|_| new_uuid_v7()));
    let status = if payload.status.as_deref() == Some("ONLINE") {
        NodeStatus::AwaitingRegistration
    } else {
        NodeStatus::Draft
    };
    node_inventory::create_node_from_payload(&state.metadata_db, &node_id, &payload, status)
        .await?;
    node_provisioning::create_provisioning_from_payload(&state.metadata_db, &node_id, &payload)
        .await?;

    let node = node_read_model::get_node_summary(&state.metadata_db, &node_id).await?;
    Ok(ApiResponse::success_with_raw("Node created", node).into_response())
}

fn should_guard_create_payload(payload: &NodeCreatePayload) -> bool {
    let has_connection = payload.addr.is_some() && payload.user.is_some();
    let has_auth = payload.pwd.is_some() || payload.private_key.is_some();
    let looks_like_reuse_register = payload.status.as_deref() == Some("ONLINE");
    has_connection && has_auth && looks_like_reuse_register
}

pub async fn precheck(
    State(state): State<Arc<AppState>>,
    ConnectInfo(_conn): ConnectInfo<SocketAddr>,
    _admin: AuthenticatedAdmin,
    _headers: HeaderMap,
    Json(payload): Json<NodePrecheckPayload>,
) -> ApiResult<Response> {
    let addr = payload
        .addr
        .clone()
        .ok_or_else(|| ApiError::BadRequest("node address must not be empty".to_string()))?;
    assert_target_not_current_host(&addr).await?;
    let user = payload
        .user
        .clone()
        .ok_or_else(|| ApiError::BadRequest("SSH user must not be empty".to_string()))?;

    let input = NodePrecheckInput {
        addr,
        port: payload.port,
        user,
        auth_mode: payload.auth_mode,
        password: payload.pwd,
        private_key: payload.private_key,
        private_key_passphrase: payload.private_key_passphrase,
        service_port: payload.service_port,
        install_dir: payload.install_dir,
        seclab_url: payload.seclab_url,
    };

    let result: Result<NodePrecheckResponse, ApiError> = async {
        if let Some(ref name) = payload.name
            && !name.trim().is_empty()
        {
            let is_conflict =
                node_inventory::check_name_conflict(&state.metadata_db, name, None).await?;
            if is_conflict {
                return Err(ApiError::conflict(
                    ErrorCode::NodeAlreadyExists,
                    "node name already exists",
                ));
            }
        }
        precheck_node(&state.metadata_db, input).await
    }
    .await;
    Ok(ApiResponse::success_with_raw("Node precheck completed", Some(result?)).into_response())
}

pub async fn deploy_create(
    State(state): State<Arc<AppState>>,
    ConnectInfo(conn): ConnectInfo<SocketAddr>,
    admin: AuthenticatedAdmin,
    headers: HeaderMap,
    Json(payload): Json<NodeDeployCreatePayload>,
) -> ApiResult<Response> {
    let node_id = generate_node_id()?;
    if let Some(ref name) = payload.name
        && !name.trim().is_empty()
    {
        let is_conflict =
            node_inventory::check_name_conflict(&state.metadata_db, name, None).await?;
        if is_conflict {
            return Err(ApiError::conflict(
                ErrorCode::NodeAlreadyExists,
                "node name already exists",
            ));
        }
    }
    let addr = payload
        .addr
        .clone()
        .ok_or_else(|| ApiError::BadRequest("node address must not be empty".to_string()))?;
    assert_target_not_current_host(&addr).await?;
    let user = payload
        .user
        .clone()
        .ok_or_else(|| ApiError::BadRequest("SSH user must not be empty".to_string()))?;
    let ssh_port = payload
        .port
        .clone()
        .unwrap_or_else(|| crate::config::DEFAULT_SSH_PORT.to_string());
    let service_port = payload
        .service_port
        .clone()
        .unwrap_or_else(|| crate::config::DEFAULT_AGENT_PORT.to_string());
    let install_dir = {
        let parent = payload
            .install_dir
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or("/opt");
        let parent_trimmed = parent.trim_end_matches('/');
        if parent_trimmed.is_empty() {
            "/seclab".to_string()
        } else {
            format!("{}/seclab", parent_trimmed)
        }
    };
    let auth_mode = payload
        .auth_mode
        .clone()
        .unwrap_or_else(|| "password".to_string());
    let listen_addr = payload
        .listen_addr
        .clone()
        .unwrap_or_else(|| format!("{}:{service_port}", crate::config::DEFAULT_AGENT_LISTEN_IP));
    let precheck_result = precheck_node(
        &state.metadata_db,
        NodePrecheckInput {
            addr: addr.clone(),
            port: payload.port.clone(),
            user: user.clone(),
            auth_mode: payload.auth_mode.clone(),
            password: payload.pwd.clone(),
            private_key: payload.private_key.clone(),
            private_key_passphrase: payload.private_key_passphrase.clone(),
            service_port: payload.service_port.clone(),
            install_dir: payload.install_dir.clone(),
            seclab_url: payload.seclab_url.clone(),
        },
    )
    .await?;
    if !precheck_result.passed {
        return Err(ApiError::BadRequest(
            precheck_result.agent_status.message.clone(),
        ));
    }

    let create_payload = NodeCreatePayload {
        agent_id: Some(node_id.clone()),
        name: payload.name.clone(),
        group_id: payload.group_id.clone(),
        description: payload.description.clone(),
        addr: Some(addr.clone()),
        port: payload.port.clone(),
        user: Some(user.clone()),
        pwd: payload.pwd.clone(),
        private_key: payload.private_key.clone(),
        private_key_passphrase: payload.private_key_passphrase.clone(),
        auth_mode: payload.auth_mode.clone(),
        status: Some("OFFLINE".to_string()),
        version: None,
        tags: payload.tags.clone(),
        metadata: payload.metadata.clone(),
        install_dir: payload.install_dir.clone(),
        service_port: payload.service_port.clone(),
        sync_enabled: payload.sync_enabled,
    };
    node_inventory::create_node_from_payload(
        &state.metadata_db,
        &node_id,
        &create_payload,
        NodeStatus::Deploying,
    )
    .await?;
    node_provisioning::create_provisioning_from_payload(
        &state.metadata_db,
        &node_id,
        &create_payload,
    )
    .await?;

    let issued = node_enrollment::issue_enrollment_token(&state.metadata_db, &node_id).await?;

    let deploy_input = NodeDeployInput {
        agent_id: node_id.clone(),
        enrollment_id: issued.enrollment_id.clone(),
        addr: addr.clone(),
        port: ssh_port,
        user: user.clone(),
        auth_mode,
        password: payload.pwd.clone(),
        private_key: payload.private_key.clone(),
        private_key_passphrase: payload.private_key_passphrase.clone(),
        install_dir,
        service_port: service_port.clone(),
        listen_addr,
        seclab_url: crate::services::node_deploy::resolve_seclab_url_override(
            payload.seclab_url.as_deref(),
        )?,
        enrollment_token: issued.token.clone(),
        allow_existing_agent: false,
    };
    let deploy_sync = NodeDeployInput {
        agent_id: deploy_input.agent_id.clone(),
        enrollment_id: deploy_input.enrollment_id.clone(),
        addr: deploy_input.addr.clone(),
        port: deploy_input.port.clone(),
        user: deploy_input.user.clone(),
        auth_mode: deploy_input.auth_mode.clone(),
        password: deploy_input.password.clone(),
        private_key: deploy_input.private_key.clone(),
        private_key_passphrase: deploy_input.private_key_passphrase.clone(),
        install_dir: deploy_input.install_dir.clone(),
        service_port: deploy_input.service_port.clone(),
        listen_addr: deploy_input.listen_addr.clone(),
        seclab_url: deploy_input.seclab_url.clone(),
        enrollment_token: deploy_input.enrollment_token.clone(),
        allow_existing_agent: deploy_input.allow_existing_agent,
    };
    // 初始化 deploy_sessions 状态
    {
        let mut map = state.deploy_sessions.lock().unwrap();
        map.insert(
            node_id.clone(),
            crate::state::DeploySession {
                progress_percent: 0,
                logs: vec!["Deploy task created".to_string()],
                is_finished: false,
                error: None,
            },
        );
    }

    let state_clone = Arc::clone(&state);
    let node_id_clone = node_id.clone();
    let claims_username = admin.username.clone();
    let actor_user_id = admin.id;
    let client_ip = conn.ip();
    let trace_id = logging::resolve_trace_id(&headers);

    tokio::spawn(async move {
        let result =
            deploy_node_with(deploy_input, Some(Arc::clone(&state_clone.deploy_sessions))).await;

        let mut platform_log =
            OperationEventBuilder::new(&claims_username, "node_deploy_create", client_ip)
                .user_id(actor_user_id)
                .module(LogModule::System)
                .target_type("node")
                .target_id(&node_id_clone)
                .trace_id(&trace_id)
                .source("seclab_api")
                .request("POST", "/api/v1/nodes/deploy");

        if let Err(err) = &result {
            platform_log = platform_log.metadata(json!({ "error": err.error }));
        } else {
            platform_log = platform_log.set_success();
        }
        platform_log.finish(&state_clone.metadata_db);

        match result {
            Ok(_) => {
                runtime_metrics::record_deploy_result(true);
                let _ =
                    transition_to_awaiting_registration(&state_clone.metadata_db, &node_id_clone)
                        .await;
                let _ = node_provisioning::mark_deployed(
                    &state_clone.metadata_db,
                    &node_provisioning::MarkDeployedInput {
                        node_id: node_id_clone.clone(),
                        deploy_method: "ssh_push".to_string(),
                        result_status: "prepared".to_string(),
                        ssh_addr: deploy_sync.addr.clone(),
                        ssh_port: deploy_sync.port.clone(),
                        ssh_user: deploy_sync.user.clone(),
                        auth_mode: deploy_sync.auth_mode.clone(),
                        install_dir: deploy_sync.install_dir.clone(),
                        service_port: deploy_sync.service_port.clone(),
                        seclab_url: deploy_sync.seclab_url.clone(),
                    },
                )
                .await;
            }
            Err(NodeDeployError { error, .. }) => {
                runtime_metrics::record_deploy_result(false);
                let _ = transition_to_deploy_failed(&state_clone.metadata_db, &node_id_clone).await;
                let _ = node_provisioning::record_operation_result(
                    &state_clone.metadata_db,
                    &node_id_clone,
                    "ssh_push",
                    "failed",
                    Some(format!("{error:?}")),
                )
                .await;
            }
        }
    });

    let node = node_read_model::get_node_summary(&state.metadata_db, &node_id).await?;
    let response = NodeDeployCreateResult {
        node,
        logs: vec!["Deployment started in background".to_string()],
    };
    Ok(ApiResponse::success_with_raw("Node deployed and created", Some(response)).into_response())
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    ConnectInfo(conn): ConnectInfo<SocketAddr>,
    admin: AuthenticatedAdmin,
    headers: HeaderMap,
    Path(node_id): Path<String>,
    Json(mut payload): Json<NodeUpdatePayload>,
) -> ApiResult<Response> {
    let trace_id = logging::resolve_trace_id(&headers);
    let result = async {
        if let Some(ref name) = payload.name
            && !name.trim().is_empty()
        {
            let is_conflict =
                node_inventory::check_name_conflict(&state.metadata_db, name, Some(&node_id))
                    .await?;
            if is_conflict {
                return Err(ApiError::conflict(
                    ErrorCode::NodeAlreadyExists,
                    "node name already exists",
                ));
            }
        }
        if let Some(seclab_url) = payload.seclab_url.as_ref().filter(|_| node_id != "local") {
            // 1. 校验节点在线状态
            let node_sum = node_read_model::get_node_summary(&state.metadata_db, &node_id)
                .await?
                .ok_or(ApiError::NotFound)?;
            if node_sum.status != "online" {
                return Err(ApiError::BadRequest(
                    "Node is offline. Cannot update remote seclabUrl when node is not online."
                        .to_string(),
                ));
            }

            // 2. 校验新的 seclab_url 格式
            let trimmed_url = seclab_url.trim_end_matches('/').to_string();
            let parsed = reqwest::Url::parse(&trimmed_url).map_err(|err| {
                ApiError::BadRequest(format!("seclabUrl must be a valid HTTPS URL: {}", err))
            })?;

            if parsed.scheme() != "https" {
                return Err(ApiError::BadRequest("seclabUrl must use HTTPS".to_string()));
            }

            if parsed.host_str().is_none() {
                return Err(ApiError::BadRequest(
                    "seclabUrl must include a host".to_string(),
                ));
            }

            if (parsed.path() != "" && parsed.path() != "/")
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                return Err(ApiError::BadRequest(
                    "seclabUrl must not include path, query, or fragment".to_string(),
                ));
            }

            // 3. 远程调用子节点更新接口
            let client = crate::models::node_runtime_client::NodeRuntimeClient::from_node_route(
                &state.metadata_db,
                Some(&node_id),
            )
            .await?;

            #[derive(serde::Serialize)]
            #[serde(rename_all = "camelCase")]
            struct SeclabUrlPayload {
                seclab_url: String,
            }

            let _resp: serde_json::Value = client
                .put_json(
                    "/api/v1/agent/system/seclab-url",
                    &SeclabUrlPayload {
                        seclab_url: trimmed_url.clone(),
                    },
                )
                .await
                .map_err(|err| {
                    ApiError::Internal(format!(
                        "Failed to update seclabUrl on remote node: {}",
                        err
                    ))
                })?;
            payload.seclab_url = Some(trimmed_url);
        }

        node_inventory::update_node_from_payload(&state.metadata_db, &node_id, &payload).await?;
        node_provisioning::update_provisioning_from_payload(&state.metadata_db, &node_id, &payload)
            .await?;
        node_read_model::get_node_summary(&state.metadata_db, &node_id)
            .await?
            .ok_or(ApiError::NotFound)
    }
    .await;
    let mut platform_log = OperationEventBuilder::new(&admin.username, "node_update", conn.ip())
        .module(LogModule::System)
        .target_type("node")
        .target_id(&node_id)
        .trace_id(&trace_id)
        .source("seclab_api")
        .request("PUT", "/api/v1/node/{node_id}/update");

    match &result {
        Ok(_) => platform_log = platform_log.set_success(),
        Err(err) => platform_log = platform_log.metadata(json!({ "error": err })),
    }
    platform_log.finish(&state.metadata_db);

    Ok(ApiResponse::success_with_raw("Node updated", Some(result?)).into_response())
}

pub async fn remove(
    State(state): State<Arc<AppState>>,
    ConnectInfo(conn): ConnectInfo<SocketAddr>,
    admin: AuthenticatedAdmin,
    headers: HeaderMap,
    Path(node_id): Path<String>,
) -> ApiResult<Response> {
    let trace_id = logging::resolve_trace_id(&headers);
    let target_name = node_read_model::get_node_summary(&state.metadata_db, &node_id)
        .await?
        .map(|node| node.name);
    let result = node_inventory::delete_node(&state.metadata_db, &node_id).await;
    let mut platform_log = OperationEventBuilder::new(&admin.username, "node_remove", conn.ip())
        .module(LogModule::System)
        .target_type("node")
        .target_id(&node_id)
        .trace_id(&trace_id)
        .source("seclab_api")
        .request("DELETE", "/api/v1/node/{node_id}/remove");
    if let Some(target_name) = target_name.as_deref() {
        platform_log = platform_log.target_display_name(target_name);
    }

    match &result {
        Ok(_) => platform_log = platform_log.set_success(),
        Err(err) => {
            platform_log =
                platform_log.metadata(json!({ "error": ApiError::database(err.to_string()) }))
        }
    }
    platform_log.finish(&state.metadata_db);

    result?;
    Ok(ApiResponse::success_with_raw("Node removed", Some(true)).into_response())
}

pub async fn deploy(
    State(state): State<Arc<AppState>>,
    ConnectInfo(conn): ConnectInfo<SocketAddr>,
    admin: AuthenticatedAdmin,
    headers: HeaderMap,
    Path(node_id): Path<String>,
    Json(payload): Json<NodeDeployPayload>,
) -> ApiResult<Response> {
    let client_ip = conn.ip();
    let trace_id = logging::resolve_trace_id(&headers);

    // 初始化 deploy_sessions 状态
    {
        let mut map = state.deploy_sessions.lock().unwrap();
        map.insert(
            node_id.clone(),
            crate::state::DeploySession {
                progress_percent: 0,
                logs: vec!["Deploy task created".to_string()],
                is_finished: false,
                error: None,
            },
        );
    }

    let state_clone = Arc::clone(&state);
    let node_id_clone = node_id.clone();
    let claims_username = admin.username.clone();
    let actor_user_id = admin.id;

    tokio::spawn(async move {
        let result = deploy_node(
            &state_clone.metadata_db,
            &node_id_clone,
            payload,
            Some(Arc::clone(&state_clone.deploy_sessions)),
        )
        .await;

        let mut platform_log =
            OperationEventBuilder::new(&claims_username, "node_deploy", client_ip)
                .user_id(actor_user_id)
                .module(LogModule::System)
                .target_type("node")
                .target_id(&node_id_clone)
                .trace_id(&trace_id)
                .source("seclab_api")
                .request("POST", "/api/v1/node/{node_id}/deploy");

        if let Err(err) = &result {
            platform_log = platform_log.metadata(json!({ "error": err }));
        } else {
            platform_log = platform_log.set_success();
        }
        platform_log.finish(&state_clone.metadata_db);
    });

    Ok(ApiResponse::success_with_raw("Node deploy submitted", Some(true)).into_response())
}

pub async fn deploy_progress(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> ApiResult<Response> {
    let map = state.deploy_sessions.lock().unwrap();
    if let Some(session) = map.get(&node_id) {
        Ok(
            ApiResponse::success_with_raw("Deploy progress fetched", Some(session.clone()))
                .into_response(),
        )
    } else {
        Err(ApiError::NotFound)
    }
}

pub async fn check(
    State(state): State<Arc<AppState>>,
    ConnectInfo(_conn): ConnectInfo<SocketAddr>,
    _admin: AuthenticatedAdmin,
    _headers: HeaderMap,
    Path(node_id): Path<String>,
) -> ApiResult<Response> {
    let result: Result<NodeCheckResponse, ApiError> =
        check_node_health(&state.metadata_db, &node_id).await;

    Ok(ApiResponse::success_with_raw("Node check completed", Some(result?)).into_response())
}

pub async fn repair(
    State(state): State<Arc<AppState>>,
    ConnectInfo(conn): ConnectInfo<SocketAddr>,
    admin: AuthenticatedAdmin,
    headers: HeaderMap,
    Path(node_id): Path<String>,
    Json(payload): Json<NodeDeployPayload>,
) -> ApiResult<Response> {
    let trace_id = logging::resolve_trace_id(&headers);
    let result = repair_node(&state.metadata_db, &node_id, payload).await;
    let mut platform_log = OperationEventBuilder::new(&admin.username, "node_repair", conn.ip())
        .module(LogModule::System)
        .target_type("node")
        .target_id(&node_id)
        .trace_id(&trace_id)
        .source("seclab_api")
        .request("POST", "/api/v1/node/{node_id}/repair");
    match &result {
        Ok(_) => platform_log = platform_log.set_success(),
        Err(err) => platform_log = platform_log.metadata(json!({ "error": err })),
    }
    platform_log.finish(&state.metadata_db);
    result?;
    Ok(ApiResponse::success_with_raw("Node repair submitted", Some(true)).into_response())
}

pub async fn retire(
    State(state): State<Arc<AppState>>,
    ConnectInfo(conn): ConnectInfo<SocketAddr>,
    admin: AuthenticatedAdmin,
    headers: HeaderMap,
    Path(node_id): Path<String>,
) -> ApiResult<Response> {
    let trace_id = logging::resolve_trace_id(&headers);
    let result = retire_node(&state.metadata_db, &node_id).await;
    let mut platform_log = OperationEventBuilder::new(&admin.username, "node_retire", conn.ip())
        .module(LogModule::System)
        .target_type("node")
        .target_id(&node_id)
        .trace_id(&trace_id)
        .source("seclab_api")
        .request("POST", "/api/v1/node/{node_id}/retire");
    match &result {
        Ok(_) => platform_log = platform_log.set_success(),
        Err(err) => platform_log = platform_log.metadata(json!({ "error": err })),
    }
    platform_log.finish(&state.metadata_db);
    result?;
    Ok(ApiResponse::success_with_raw("Node retired", Some(true)).into_response())
}

pub async fn uninstall(
    State(state): State<Arc<AppState>>,
    ConnectInfo(conn): ConnectInfo<SocketAddr>,
    admin: AuthenticatedAdmin,
    headers: HeaderMap,
    Path(node_id): Path<String>,
) -> ApiResult<Response> {
    let trace_id = logging::resolve_trace_id(&headers);
    let result = uninstall_node(&state.metadata_db, &node_id).await;
    let mut platform_log = OperationEventBuilder::new(&admin.username, "node_uninstall", conn.ip())
        .module(LogModule::System)
        .target_type("node")
        .target_id(&node_id)
        .trace_id(&trace_id)
        .source("seclab_api")
        .request("POST", "/api/v1/node/{node_id}/uninstall");
    match &result {
        Ok(_) => platform_log = platform_log.set_success(),
        Err(err) => platform_log = platform_log.metadata(json!({ "error": err })),
    }
    platform_log.finish(&state.metadata_db);
    result?;
    Ok(ApiResponse::success_with_raw("Node uninstalled", Some(true)).into_response())
}

pub fn nodes_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/list", get(list))
        .route("/create", post(create))
        .route("/precheck", post(precheck))
        .route("/deploy", post(deploy_create))
}

pub fn single_node_router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/{node_id}/agent/websocket/{*path}",
            get(websocket_proxy_handler_for_node),
        )
        .route("/{node_id}/agent/{*path}", any(proxy_handler_for_node))
        .route("/{node_id}/detail", get(detail))
        .route("/{node_id}/update", put(update))
        .route("/{node_id}/remove", delete(remove))
        .route("/{node_id}/observations", get(observations))
        .route("/{node_id}/sessions", get(sessions))
        .route("/{node_id}/deploy", post(deploy))
        .route("/{node_id}/deploy-progress", get(deploy_progress))
        .route("/{node_id}/check", post(check))
        .route("/{node_id}/repair", post(repair))
        .route("/{node_id}/retire", post(retire))
        .route("/{node_id}/uninstall", post(uninstall))
}
