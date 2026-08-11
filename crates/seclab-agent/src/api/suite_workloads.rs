//! 套件工作负载 API：允许已安装套件请求 Agent 创建和回收受控容器。

use crate::api::docker::suites;
use crate::services::pcap::{CaptureEndpoint, CaptureTransport, PcapMuxHub};
use crate::state::AppState;
use crate::types::{ApiError, ApiResult};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, header},
    response::IntoResponse,
    routing::{get, post},
};
use bollard::models::{ContainerCreateBody, HostConfig, PortBinding, RestartPolicy};
use bollard::query_parameters;
use seclab_contracts::api::ErrorCode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use tokio::net::{TcpListener, UdpSocket};
use uuid::Uuid;

const SUITE_NETWORK_NAME: &str = "seclab-suite-network";
const WORKLOAD_LABEL: &str = "seclab.workload_type";
const WORKLOAD_LABEL_VALUE: &str = "suite-workload";
const MAX_WORKLOAD_CONTAINER_NAME_LEN: usize = 96;
type ExposedPorts = Vec<String>;
type PortBindings = HashMap<String, Option<Vec<PortBinding>>>;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartWorkloadRequest {
    pub workload_kind: String,
    pub workload_name: String,
    pub image: String,
    #[serde(default)]
    pub ports: Vec<WorkloadPort>,
    #[serde(default)]
    pub env: serde_json::Value,
    #[serde(default)]
    pub config_json: serde_json::Value,
    #[serde(default)]
    pub resources: WorkloadResources,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadPort {
    pub endpoint_id: String,
    pub host_port: u16,
    pub container_port: u16,
    pub protocol: WorkloadTransport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkloadTransport {
    Tcp,
    Udp,
}

impl WorkloadTransport {
    fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadResources {
    pub memory_mb: Option<u32>,
    pub cpu_shares: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartWorkloadResponse {
    pub workload_id: String,
    pub container_id: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartPcapResponse {
    pub capture_id: String,
    pub status: String,
    pub endpoints: Vec<WorkloadCaptureEndpoint>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadCaptureEndpoint {
    pub endpoint_id: String,
    pub host_port: u16,
    pub protocol: WorkloadTransport,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadSummary {
    pub workload_id: String,
    pub suite_id: String,
    pub suite_instance_id: String,
    pub workload_kind: String,
    pub name: String,
    pub image: String,
    pub status: String,
    pub container_id: String,
}

#[derive(Debug, Deserialize)]
pub struct WorkloadPath {
    workload_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CapturePath {
    workload_id: String,
    capture_id: String,
}

/// 组装套件运行时工作负载路由。
pub fn suite_workloads_router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/suite-runtime/workloads",
            post(start_workload).get(list_workloads),
        )
        .route(
            "/suite-runtime/workloads/{workload_id}",
            get(detail_workload).delete(delete_workload),
        )
        .route(
            "/suite-runtime/workloads/{workload_id}/captures",
            post(start_pcap),
        )
        .route(
            "/suite-runtime/workloads/{workload_id}/captures/{capture_id}/finish",
            post(stop_pcap),
        )
}

async fn start_workload(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<StartWorkloadRequest>,
) -> ApiResult<impl IntoResponse> {
    let principal = require_suite_capability(&headers, "workloads.manage").await?;
    validate_workload_request(&payload, &principal.runtime_images)?;
    ensure_workload_ports_available(&payload.ports).await?;

    let docker = state.docker_client().await?;
    let workload_id = format!("workload-{}", Uuid::now_v7());
    let container_name = workload_container_name(&payload.workload_name, &workload_id);
    let labels = workload_labels(
        &payload,
        &workload_id,
        &principal.suite_id,
        &principal.instance_id,
    );
    let (exposed_ports, port_bindings) = port_maps(&payload.ports);
    let mut env = env_map_to_vec(&payload.env)?;
    env.push(format!(
        "SECLAB_WORKLOAD_CONFIG_JSON={}",
        serde_json::to_string(&payload.config_json).unwrap_or_else(|_| "{}".to_string())
    ));

    let host_config = HostConfig {
        port_bindings: if port_bindings.is_empty() {
            None
        } else {
            Some(port_bindings)
        },
        network_mode: Some(SUITE_NETWORK_NAME.to_string()),
        restart_policy: Some(RestartPolicy {
            name: Some(bollard::models::RestartPolicyNameEnum::UNLESS_STOPPED),
            maximum_retry_count: None,
        }),
        memory: payload
            .resources
            .memory_mb
            .map(|value| i64::from(value) * 1024 * 1024),
        cpu_shares: payload.resources.cpu_shares.map(i64::from),
        privileged: Some(false),
        ..Default::default()
    };

    let config = ContainerCreateBody {
        image: Some(payload.image.clone()),
        env: Some(env),
        labels: Some(labels),
        exposed_ports: if exposed_ports.is_empty() {
            None
        } else {
            Some(exposed_ports)
        },
        host_config: Some(host_config),
        ..Default::default()
    };

    let options = query_parameters::CreateContainerOptionsBuilder::new()
        .name(&container_name)
        .build();
    let created = docker.create_container(Some(options), config).await?;
    if let Err(start_error) = docker
        .start_container(
            &container_name,
            None::<query_parameters::StartContainerOptions>,
        )
        .await
    {
        if let Err(cleanup_error) = docker
            .remove_container(
                &created.id,
                Some(query_parameters::RemoveContainerOptions {
                    force: true,
                    v: false,
                    link: false,
                }),
            )
            .await
        {
            tracing::error!(
                container_id = %created.id,
                container_name,
                error = %cleanup_error,
                "failed to remove suite workload container after start failure"
            );
        }
        return Err(ApiError::from(start_error));
    }

    Ok(Json(StartWorkloadResponse {
        workload_id,
        container_id: created.id,
        status: "running".to_string(),
    }))
}

async fn start_pcap(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(path): Path<WorkloadPath>,
) -> ApiResult<impl IntoResponse> {
    let principal = require_suite_capability(&headers, "captures.manage").await?;
    validate_id("workload_id", &path.workload_id)?;
    let docker = state.docker_client().await?;
    let container_id = find_owned_container(&docker, &principal.instance_id, &path.workload_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let endpoints = workload_capture_endpoints(&docker, &container_id).await?;
    if endpoints.is_empty() {
        return Err(ApiError::BadRequest(
            "suite workload has no published endpoints to capture".to_string(),
        ));
    }

    let capture_id = format!("pcap-{}", Uuid::now_v7());
    PcapMuxHub::global()
        .start_capture_for_workload(
            capture_id.clone(),
            endpoints
                .iter()
                .map(|endpoint| CaptureEndpoint {
                    endpoint_id: endpoint.endpoint_id.clone(),
                    host_port: endpoint.host_port,
                    transport: match endpoint.protocol {
                        WorkloadTransport::Tcp => CaptureTransport::Tcp,
                        WorkloadTransport::Udp => CaptureTransport::Udp,
                    },
                })
                .collect(),
            Some(principal.instance_id.clone()),
            Some(path.workload_id.clone()),
        )
        .await
        .map_err(|err| ApiError::BadRequest(format!("failed to start pcap capture: {err:#}")))?;

    Ok(Json(StartPcapResponse {
        capture_id,
        status: "capturing".to_string(),
        endpoints,
    }))
}

/// 返回 Agent 在创建工作负载时记录的全部命名端点。
async fn workload_capture_endpoints(
    docker: &bollard::Docker,
    container_id: &str,
) -> ApiResult<Vec<WorkloadCaptureEndpoint>> {
    let detail = docker
        .inspect_container(
            container_id,
            None::<query_parameters::InspectContainerOptions>,
        )
        .await?;
    let encoded = detail
        .config
        .and_then(|config| config.labels)
        .and_then(|labels| labels.get("seclab.workload_ports").cloned())
        .ok_or_else(|| ApiError::BadRequest("workload endpoint metadata is missing".to_string()))?;
    serde_json::from_str::<Vec<WorkloadPort>>(&encoded)
        .map(|ports| {
            ports
                .into_iter()
                .map(|port| WorkloadCaptureEndpoint {
                    endpoint_id: port.endpoint_id,
                    host_port: port.host_port,
                    protocol: port.protocol,
                })
                .collect()
        })
        .map_err(|err| ApiError::BadRequest(format!("invalid workload endpoint metadata: {err}")))
}

async fn stop_pcap(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(path): Path<CapturePath>,
) -> ApiResult<impl IntoResponse> {
    let principal = require_suite_capability(&headers, "captures.manage").await?;
    validate_id("workload_id", &path.workload_id)?;
    validate_id("capture_id", &path.capture_id)?;
    let docker = state.docker_client().await?;
    find_owned_container(&docker, &principal.instance_id, &path.workload_id)
        .await?
        .ok_or(ApiError::NotFound)?;

    let bytes = PcapMuxHub::global()
        .stop_capture_owned(&path.capture_id, &principal.instance_id, &path.workload_id)
        .await
        .map_err(|err| ApiError::BadRequest(format!("failed to stop pcap capture: {err:#}")))?;
    Ok((
        [(header::CONTENT_TYPE, "application/vnd.tcpdump.pcap")],
        bytes,
    ))
}

async fn list_workloads(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    let principal = require_suite_capability(&headers, "workloads.manage").await?;
    let docker = state.docker_client().await?;
    let mut filters = HashMap::new();
    filters.insert(
        "label".to_string(),
        vec![format!("{WORKLOAD_LABEL}={WORKLOAD_LABEL_VALUE}")],
    );
    let containers = docker
        .list_containers(Some(
            query_parameters::ListContainersOptionsBuilder::new()
                .all(true)
                .filters(&filters)
                .build(),
        ))
        .await?;
    let items = containers
        .into_iter()
        .filter_map(|container| {
            let labels = container.labels?;
            if labels.get("seclab.suite_instance_id") != Some(&principal.instance_id) {
                return None;
            }
            Some(WorkloadSummary {
                workload_id: labels.get("seclab.workload_id")?.clone(),
                suite_id: labels.get("seclab.suite_id")?.clone(),
                suite_instance_id: labels.get("seclab.suite_instance_id")?.clone(),
                workload_kind: labels.get("seclab.workload_kind")?.clone(),
                name: labels
                    .get("seclab.workload_name")
                    .cloned()
                    .unwrap_or_default(),
                image: container.image.unwrap_or_default(),
                status: container.status.unwrap_or_default(),
                container_id: container.id.unwrap_or_default(),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(items))
}

async fn detail_workload(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(path): Path<WorkloadPath>,
) -> ApiResult<impl IntoResponse> {
    let principal = require_suite_capability(&headers, "workloads.manage").await?;
    let docker = state.docker_client().await?;
    let container_id = find_owned_container(&docker, &principal.instance_id, &path.workload_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let detail = docker
        .inspect_container(
            &container_id,
            None::<query_parameters::InspectContainerOptions>,
        )
        .await?;
    Ok(Json(detail))
}

async fn delete_workload(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(path): Path<WorkloadPath>,
) -> ApiResult<impl IntoResponse> {
    let principal = require_suite_capability(&headers, "workloads.manage").await?;
    let docker = state.docker_client().await?;
    let container_id = find_owned_container(&docker, &principal.instance_id, &path.workload_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    docker
        .remove_container(
            &container_id,
            Some(query_parameters::RemoveContainerOptions {
                force: true,
                v: false,
                link: false,
            }),
        )
        .await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// 清理指定套件实例下由 suite workload API 创建的所有容器和抓包任务。
pub(crate) async fn cleanup_suite_workloads_by_instance(
    docker: &bollard::Docker,
    suite_instance_id: &str,
) -> ApiResult<usize> {
    validate_id("suite_instance_id", suite_instance_id)?;
    let stopped_captures = PcapMuxHub::global()
        .stop_captures_for_suite_instance(suite_instance_id)
        .await;
    if !stopped_captures.is_empty() {
        tracing::info!(
            suite_instance_id,
            stopped_count = stopped_captures.len(),
            "stopped suite workload pcap captures before cleanup"
        );
    }

    let filters = suite_workload_cleanup_filters(suite_instance_id);
    let containers = docker
        .list_containers(Some(
            query_parameters::ListContainersOptionsBuilder::new()
                .all(true)
                .filters(&filters)
                .build(),
        ))
        .await?;

    let mut removed = 0usize;
    for container in containers {
        let Some(container_id) = container.id else {
            continue;
        };
        if let Err(err) = docker
            .stop_container(
                &container_id,
                Some(query_parameters::StopContainerOptions {
                    signal: None,
                    t: Some(5),
                }),
            )
            .await
        {
            tracing::debug!(
                suite_instance_id,
                container_id,
                error = %err,
                "suite workload cleanup stop returned an error before removal"
            );
        }
        docker
            .remove_container(
                &container_id,
                Some(query_parameters::RemoveContainerOptions {
                    force: true,
                    v: false,
                    link: false,
                }),
            )
            .await?;
        removed += 1;
    }

    if removed > 0 {
        tracing::info!(
            suite_instance_id,
            removed_count = removed,
            "removed suite workload containers"
        );
    }
    Ok(removed)
}

fn suite_workload_cleanup_filters(suite_instance_id: &str) -> HashMap<String, Vec<String>> {
    let mut filters = HashMap::new();
    filters.insert(
        "label".to_string(),
        vec![
            format!("{WORKLOAD_LABEL}={WORKLOAD_LABEL_VALUE}"),
            format!("seclab.suite_instance_id={suite_instance_id}"),
        ],
    );
    filters
}

async fn require_suite_capability(
    headers: &HeaderMap,
    capability: &str,
) -> ApiResult<suites::SuiteRuntimePrincipal> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApiError::forbidden(ErrorCode::AuthForbidden, "suite runtime token is required")
        })?;
    let principal = suites::authenticate_suite_runtime(token).await?;
    if !principal
        .capabilities
        .iter()
        .any(|value| value == capability)
    {
        return Err(ApiError::forbidden(
            ErrorCode::AuthForbidden,
            "suite runtime capability is not granted",
        ));
    }
    Ok(principal)
}

fn validate_workload_request(
    payload: &StartWorkloadRequest,
    runtime_images: &[String],
) -> ApiResult<()> {
    validate_id("workload_kind", &payload.workload_kind)?;
    if payload.workload_name.trim().is_empty() {
        return Err(ApiError::BadRequest("workloadName is required".to_string()));
    }
    if !runtime_images.iter().any(|image| image == &payload.image) {
        return Err(ApiError::BadRequest(
            "suite workload image is not declared by the suite manifest".to_string(),
        ));
    }
    let mut endpoint_ids = std::collections::HashSet::new();
    let mut bindings = std::collections::HashSet::new();
    let mut container_bindings = std::collections::HashSet::new();
    for port in &payload.ports {
        validate_id("endpoint_id", &port.endpoint_id)?;
        if !endpoint_ids.insert(port.endpoint_id.as_str()) {
            return Err(ApiError::BadRequest(format!(
                "duplicate workload endpoint id: {}",
                port.endpoint_id
            )));
        }
        if !bindings.insert((port.protocol, port.host_port)) {
            return Err(ApiError::BadRequest(format!(
                "duplicate workload endpoint binding: {}/{}",
                port.host_port,
                port.protocol.as_str()
            )));
        }
        if port.host_port == 0 || port.container_port == 0 {
            return Err(ApiError::BadRequest(
                "workload endpoint ports must be between 1 and 65535".to_string(),
            ));
        }
        if !container_bindings.insert((port.protocol, port.container_port)) {
            return Err(ApiError::BadRequest(format!(
                "duplicate workload container binding: {}/{}",
                port.container_port,
                port.protocol.as_str()
            )));
        }
    }
    Ok(())
}

async fn ensure_workload_ports_available(ports: &[WorkloadPort]) -> ApiResult<()> {
    for port in ports {
        let address = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port.host_port);
        let result = match port.protocol {
            WorkloadTransport::Tcp => TcpListener::bind(address).await.map(drop),
            WorkloadTransport::Udp => UdpSocket::bind(address).await.map(drop),
        };
        if let Err(err) = result {
            return Err(ApiError::conflict(
                ErrorCode::SuiteWorkloadPortUnavailable,
                "suite workload host port is unavailable",
            )
            .with_detail(format!(
                "protocol={} hostPort={} error={err}",
                port.protocol.as_str(),
                port.host_port
            )));
        }
    }
    Ok(())
}

fn workload_container_name(workload_name: &str, workload_id: &str) -> String {
    let suffix = compact_container_id(workload_id);
    let max_readable_len = MAX_WORKLOAD_CONTAINER_NAME_LEN
        .saturating_sub("seclab".len())
        .saturating_sub(suffix.len())
        .saturating_sub(2);
    let readable =
        truncate_container_segment(&sanitize_container_segment(workload_name), max_readable_len)
            .unwrap_or_else(|| "workload".to_string());
    format!("seclab-{readable}-{suffix}")
}

fn sanitize_container_segment(value: &str) -> String {
    let mut output = String::new();
    let mut previous_dash = false;
    for ch in value.trim().chars().flat_map(char::to_lowercase) {
        let normalized = if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_') {
            ch
        } else {
            '-'
        };
        if normalized == '-' {
            if previous_dash {
                continue;
            }
            previous_dash = true;
        } else {
            previous_dash = false;
        }
        output.push(normalized);
    }
    output
        .trim_matches(|ch| matches!(ch, '-' | '.' | '_'))
        .to_string()
}

fn truncate_container_segment(value: &str, max_len: usize) -> Option<String> {
    if value.is_empty() || max_len == 0 {
        return None;
    }
    let truncated = if value.len() > max_len {
        &value[..max_len]
    } else {
        value
    }
    .trim_matches(|ch| matches!(ch, '-' | '.' | '_'))
    .to_string();
    if truncated.is_empty() {
        None
    } else {
        Some(truncated)
    }
}

fn compact_container_id(value: &str) -> String {
    let normalized = value
        .chars()
        .filter(|value| value.is_ascii_alphanumeric())
        .collect::<String>();
    let compact = if normalized.len() > 12 {
        &normalized[normalized.len() - 12..]
    } else {
        normalized.as_str()
    };
    compact
        .chars()
        .map(|value| {
            if value.is_ascii_alphanumeric() {
                value
            } else {
                '-'
            }
        })
        .collect()
}

fn workload_labels(
    payload: &StartWorkloadRequest,
    workload_id: &str,
    suite_id: &str,
    suite_instance_id: &str,
) -> HashMap<String, String> {
    HashMap::from([
        ("seclab.managed_by".to_string(), "seclab-agent".to_string()),
        (WORKLOAD_LABEL.to_string(), WORKLOAD_LABEL_VALUE.to_string()),
        ("seclab.suite_id".to_string(), suite_id.to_string()),
        (
            "seclab.suite_instance_id".to_string(),
            suite_instance_id.to_string(),
        ),
        ("seclab.workload_id".to_string(), workload_id.to_string()),
        (
            "seclab.workload_kind".to_string(),
            payload.workload_kind.clone(),
        ),
        (
            "seclab.workload_name".to_string(),
            payload.workload_name.clone(),
        ),
        (
            "seclab.workload_ports".to_string(),
            serde_json::to_string(&payload.ports).unwrap_or_else(|_| "[]".to_string()),
        ),
    ])
}

fn port_maps(ports: &[WorkloadPort]) -> (ExposedPorts, PortBindings) {
    let mut exposed = Vec::new();
    let mut bindings = HashMap::new();
    for port in ports {
        let key = format!("{}/{}", port.container_port, port.protocol.as_str());
        exposed.push(key.clone());
        bindings.insert(
            key,
            Some(vec![PortBinding {
                host_ip: Some("0.0.0.0".to_string()),
                host_port: Some(port.host_port.to_string()),
            }]),
        );
    }
    (exposed, bindings)
}

fn env_map_to_vec(value: &serde_json::Value) -> ApiResult<Vec<String>> {
    let Some(object) = value.as_object() else {
        return Ok(Vec::new());
    };
    let mut env = Vec::with_capacity(object.len());
    for (key, value) in object {
        validate_env_key(key)?;
        let value = value
            .as_str()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| value.to_string());
        env.push(format!("{key}={value}"));
    }
    Ok(env)
}

fn validate_env_key(key: &str) -> ApiResult<()> {
    if key.trim().is_empty()
        || !key
            .chars()
            .all(|value| value.is_ascii_uppercase() || value.is_ascii_digit() || value == '_')
    {
        return Err(ApiError::BadRequest(format!(
            "invalid workload env key: {key}"
        )));
    }
    Ok(())
}

async fn find_owned_container(
    docker: &bollard::Docker,
    suite_instance_id: &str,
    workload_id: &str,
) -> ApiResult<Option<String>> {
    let mut filters = HashMap::new();
    filters.insert(
        "label".to_string(),
        vec![
            format!("{WORKLOAD_LABEL}={WORKLOAD_LABEL_VALUE}"),
            format!("seclab.suite_instance_id={suite_instance_id}"),
            format!("seclab.workload_id={workload_id}"),
        ],
    );
    find_container_with_filters(docker, filters).await
}

async fn find_container_with_filters(
    docker: &bollard::Docker,
    filters: HashMap<String, Vec<String>>,
) -> ApiResult<Option<String>> {
    let containers = docker
        .list_containers(Some(
            query_parameters::ListContainersOptionsBuilder::new()
                .all(true)
                .filters(&filters)
                .build(),
        ))
        .await?;
    Ok(containers
        .first()
        .and_then(|container| container.id.as_ref())
        .cloned())
}

fn validate_id(label: &str, value: &str) -> ApiResult<()> {
    if value.trim().is_empty()
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(ApiError::BadRequest(format!(
            "{label} may only contain letters, digits, hyphen, underscore, and dot"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_WORKLOAD_CONTAINER_NAME_LEN, StartWorkloadRequest, WorkloadPort, WorkloadResources,
        WorkloadTransport, ensure_workload_ports_available, sanitize_container_segment,
        suite_workload_cleanup_filters, validate_workload_request, workload_container_name,
    };
    use seclab_contracts::api::ErrorCode;
    use std::net::{Ipv4Addr, SocketAddrV4};
    use tokio::net::{TcpListener, UdpSocket};

    #[test]
    fn workload_container_name_uses_rule_id_and_short_workload_id() {
        assert_eq!(
            workload_container_name(
                "sim-rule-427001",
                "workload-019f409d-8e1d-73e0-f3dc45e75209"
            ),
            "seclab-sim-rule-427001-f3dc45e75209"
        );
    }

    #[test]
    fn workload_container_name_sanitizes_readable_segment() {
        assert_eq!(
            workload_container_name(
                "  Sim Rule/邮件:427001  ",
                "workload-019f409d-8e1d-73e0-f3dc45e75209"
            ),
            "seclab-sim-rule-427001-f3dc45e75209"
        );
        assert_eq!(sanitize_container_segment("...___---"), "");
    }

    #[test]
    fn workload_container_name_truncates_long_readable_segment() {
        let name = workload_container_name(
            "sim-rule-very-long-name-that-keeps-going-until-it-would-make-the-docker-container-name-too-long",
            "workload-019f409d-8e1d-73e0-f3dc45e75209",
        );
        assert!(name.starts_with("seclab-sim-rule-very-long-name"));
        assert!(name.ends_with("-f3dc45e75209"));
        assert!(name.len() <= MAX_WORKLOAD_CONTAINER_NAME_LEN);
    }

    #[test]
    fn workload_container_name_falls_back_when_name_has_no_safe_chars() {
        assert_eq!(
            workload_container_name("邮件规则", "workload-019f409d-8e1d-73e0-f3dc45e75209"),
            "seclab-workload-f3dc45e75209"
        );
    }

    #[test]
    fn suite_workload_cleanup_filters_scope_to_suite_instance() {
        let filters = suite_workload_cleanup_filters("suite-instance-1");
        assert_eq!(
            filters.get("label"),
            Some(&vec![
                "seclab.workload_type=suite-workload".to_string(),
                "seclab.suite_instance_id=suite-instance-1".to_string()
            ])
        );
    }

    #[tokio::test]
    async fn workload_port_check_rejects_bound_tcp_port() {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let err = ensure_workload_ports_available(&[WorkloadPort {
            endpoint_id: "tcp".to_string(),
            host_port: port,
            container_port: port,
            protocol: WorkloadTransport::Tcp,
        }])
        .await
        .unwrap_err();

        assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
        assert_eq!(err.code, ErrorCode::SuiteWorkloadPortUnavailable);
    }

    #[tokio::test]
    async fn workload_port_check_rejects_bound_udp_port() {
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
            .await
            .unwrap();
        let port = socket.local_addr().unwrap().port();
        let err = ensure_workload_ports_available(&[WorkloadPort {
            endpoint_id: "udp".to_string(),
            host_port: port,
            container_port: port,
            protocol: WorkloadTransport::Udp,
        }])
        .await
        .unwrap_err();

        assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
        assert_eq!(err.code, ErrorCode::SuiteWorkloadPortUnavailable);
    }

    fn request(ports: Vec<WorkloadPort>) -> StartWorkloadRequest {
        StartWorkloadRequest {
            workload_kind: "simulation-rule".to_string(),
            workload_name: "rule-1".to_string(),
            image: "example/engine:alpha".to_string(),
            ports,
            env: serde_json::json!({}),
            config_json: serde_json::json!({}),
            resources: WorkloadResources::default(),
        }
    }

    #[test]
    fn workload_request_requires_manifest_declared_image() {
        let payload = request(Vec::new());
        assert!(validate_workload_request(&payload, &[]).is_err());
        assert!(validate_workload_request(&payload, &["example/engine:alpha".to_string()]).is_ok());
    }

    #[test]
    fn workload_request_rejects_duplicate_or_zero_endpoint_bindings() {
        let declared = ["example/engine:alpha".to_string()];
        let endpoint = WorkloadPort {
            endpoint_id: "main".to_string(),
            host_port: 8080,
            container_port: 80,
            protocol: WorkloadTransport::Tcp,
        };
        assert!(
            validate_workload_request(
                &request(vec![endpoint.clone(), endpoint.clone()]),
                &declared,
            )
            .is_err()
        );
        assert!(
            validate_workload_request(
                &request(vec![WorkloadPort {
                    host_port: 0,
                    ..endpoint
                }]),
                &declared,
            )
            .is_err()
        );
    }
}
