//! Docker 容器 API：容器列表、操作与状态查询。

use crate::api::docker::context::DockerOperationContext;
use crate::models::docker;
use crate::state::AppState;
use crate::types::{ApiError, ApiResponse, ApiResult};

use axum::{
    extract::{Json, Path, Query, State},
    response::{IntoResponse, Response},
};

use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::models::{
    ContainerCreateBody, ContainerSummary, ContainerTopResponse, HostConfig, PortBinding,
    RestartPolicy, RestartPolicyNameEnum,
};
use bollard::query_parameters::{self, LogsOptions};
use chrono::{DateTime, Local};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info};

/// 获取所有 Docker 容器（包括已停止的）的摘要信息列表。
pub async fn list_containers(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    info!("Requesting all docker containers");
    let docker = state.docker_client().await?;

    let options = query_parameters::ListContainersOptionsBuilder::new()
        .all(true)
        .build();
    let containers: Vec<ContainerSummary> = docker.list_containers(Some(options)).await?;

    Ok(ApiResponse::success_with_raw("Container list loaded", Some(containers)).into_response())
}

/// 获取所有由 `docker-compose` 管理的容器列表。
///
/// 通过标签 `com.docker.compose.project` 进行过滤。
pub async fn list_project_containers(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    info!("Requesting docker compose project containers");
    let docker = state.docker_client().await?;

    let mut filters = HashMap::new();
    filters.insert(
        "label".to_string(),
        vec!["com.docker.compose.project".to_string()],
    );
    let project = docker
        .list_containers(Some(
            bollard::query_parameters::ListContainersOptionsBuilder::new()
                .all(true)
                .filters(&filters)
                .build(),
        ))
        .await?;
    Ok(
        ApiResponse::success_with_raw("Project container list loaded", Some(project))
            .into_response(),
    )
}

/// 创建容器并按需启动。
pub async fn create_container(
    State(state): State<Arc<AppState>>,
    context: DockerOperationContext,
    Json(payload): Json<docker::ContainerCreateRequest>,
) -> ApiResult<Response> {
    info!("Requesting container create: {}", payload.name);
    let container_name = payload.name.clone();
    let image_name = payload.image.clone();
    let result: ApiResult<Response> = async {
        let docker = state.docker_client().await?;

        let env = split_nonempty_lines(payload.env.as_deref());
        let binds = split_nonempty_lines(payload.volumes.as_deref());
        let (exposed_ports, port_bindings) = parse_port_bindings(payload.ports.as_deref())?;

        let host_config = HostConfig {
            port_bindings: if port_bindings.is_empty() {
                None
            } else {
                Some(port_bindings)
            },
            binds: if binds.is_empty() { None } else { Some(binds) },
            restart_policy: payload
                .restart_policy
                .as_deref()
                .and_then(parse_restart_policy)
                .map(|policy| RestartPolicy {
                    name: Some(policy),
                    maximum_retry_count: None,
                }),
            auto_remove: payload.auto_remove,
            network_mode: payload.network.clone(),
            ..Default::default()
        };

        let cmd = payload
            .command
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|value| vec!["/bin/sh".to_string(), "-c".to_string(), value.to_string()]);

        let config = ContainerCreateBody {
            image: Some(payload.image.clone()),
            env: if env.is_empty() { None } else { Some(env) },
            cmd,
            exposed_ports: if exposed_ports.is_empty() {
                None
            } else {
                Some(exposed_ports)
            },
            host_config: Some(host_config),
            ..Default::default()
        };

        let options = query_parameters::CreateContainerOptionsBuilder::new()
            .name(&payload.name)
            .build();
        let created = docker.create_container(Some(options), config).await?;

        if payload.auto_start.unwrap_or(true) {
            docker
                .start_container(
                    &payload.name,
                    None::<query_parameters::StartContainerOptions>,
                )
                .await?;
        }

        Ok(ApiResponse::success_with_raw("Container created", Some(created)).into_response())
    }
    .await;
    context
        .finish(
            &state.metadata_db,
            "container.create",
            Some(("container", &container_name)),
            json!({ "name": container_name, "image": image_name }),
            false,
            result,
        )
        .await
}

/// 获取容器详情（inspect）。
pub async fn inspect_container(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    info!("Requesting container inspect: {}", id);
    let docker = state.docker_client().await?;
    let detail = docker
        .inspect_container(&id, None::<query_parameters::InspectContainerOptions>)
        .await?;
    Ok(ApiResponse::success_with_raw("Container detail loaded", Some(detail)).into_response())
}

/// 获取容器进程列表 (docker top)。
pub async fn top_container(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    info!("Requesting container top: {}", id);
    let docker = state.docker_client().await?;
    let options = query_parameters::TopOptionsBuilder::default()
        .ps_args("aux")
        .build();
    let response: ContainerTopResponse = docker.top_processes(&id, Some(options)).await?;
    Ok(
        ApiResponse::success_with_raw("Container process list loaded", Some(response))
            .into_response(),
    )
}

/// 重命名容器。
pub async fn rename_container(
    State(state): State<Arc<AppState>>,
    context: DockerOperationContext,
    Path(id): Path<String>,
    Json(payload): Json<docker::ContainerRenameRequest>,
) -> ApiResult<Response> {
    info!("Requesting container rename: {} -> {}", id, payload.name);
    let new_name = payload.name.clone();
    let result: ApiResult<Response> = async {
        let docker = state.docker_client().await?;
        let options = query_parameters::RenameContainerOptionsBuilder::new()
            .name(&payload.name)
            .build();
        docker.rename_container(&id, options).await?;
        Ok(ApiResponse::ok("Container renamed").into_response())
    }
    .await;
    context
        .finish(
            &state.metadata_db,
            "container.rename",
            Some(("container", &id)),
            json!({ "name": id, "newName": new_name }),
            false,
            result,
        )
        .await
}

/// 暂停容器。
pub async fn pause_container(
    State(state): State<Arc<AppState>>,
    context: DockerOperationContext,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    info!("Requesting container pause: {}", id);
    let result: ApiResult<Response> = async {
        let docker = state.docker_client().await?;
        docker.pause_container(&id).await?;
        Ok(ApiResponse::ok("Container paused").into_response())
    }
    .await;
    context
        .finish(
            &state.metadata_db,
            "container.pause",
            Some(("container", &id)),
            json!({ "name": id }),
            false,
            result,
        )
        .await
}

/// 恢复容器。
pub async fn unpause_container(
    State(state): State<Arc<AppState>>,
    context: DockerOperationContext,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    info!("Requesting container unpause: {}", id);
    let result: ApiResult<Response> = async {
        let docker = state.docker_client().await?;
        docker.unpause_container(&id).await?;
        Ok(ApiResponse::ok("Container unpaused").into_response())
    }
    .await;
    context
        .finish(
            &state.metadata_db,
            "container.unpause",
            Some(("container", &id)),
            json!({ "name": id }),
            false,
            result,
        )
        .await
}

/// 强制停止容器。
pub async fn kill_container(
    State(state): State<Arc<AppState>>,
    context: DockerOperationContext,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    info!("Requesting container kill: {}", id);
    let result: ApiResult<Response> = async {
        let docker = state.docker_client().await?;
        docker
            .kill_container(&id, None::<query_parameters::KillContainerOptions>)
            .await?;
        Ok(ApiResponse::ok("Container killed").into_response())
    }
    .await;
    context
        .finish(
            &state.metadata_db,
            "container.kill",
            Some(("container", &id)),
            json!({ "name": id }),
            true,
            result,
        )
        .await
}

/// 删除容器（含参数）
pub async fn remove_container(
    State(state): State<Arc<AppState>>,
    context: DockerOperationContext,
    Path(id): Path<String>,
    Query(query): Query<docker::ContainerRemoveQuery>,
) -> ApiResult<Response> {
    info!("Requesting container remove: {}", id);
    let result: ApiResult<Response> = async {
        let docker = state.docker_client().await?;

        // 不能删除运行中的容器
        let container_info = docker
            .inspect_container(&id, None::<query_parameters::InspectContainerOptions>)
            .await?;
        if let Some(true) = container_info.state.and_then(|s| s.running) {
            return Err(ApiError::BadRequest(
                "Cannot delete a running container. Please stop it first.".to_string(),
            ));
        }

        let options = query_parameters::RemoveContainerOptionsBuilder::new()
            .force(query.force.unwrap_or(false))
            .v(query.volumes.unwrap_or(false))
            .link(query.link.unwrap_or(false))
            .build();
        docker.remove_container(&id, Some(options)).await?;
        Ok(ApiResponse::ok("Container removed").into_response())
    }
    .await;
    context
        .finish(
            &state.metadata_db,
            "container.remove",
            Some(("container", &id)),
            json!({ "name": id }),
            true,
            result,
        )
        .await
}

/// 容器 exec 执行命令。
pub async fn exec_container(
    State(state): State<Arc<AppState>>,
    context: DockerOperationContext,
    Path(id): Path<String>,
    Json(payload): Json<docker::ContainerExecRequest>,
) -> ApiResult<Response> {
    info!("Requesting container exec: {}", id);
    let result: ApiResult<Response> = async {
        let docker = state.docker_client().await?;
        let exec = docker
            .create_exec(
                &id,
                CreateExecOptions {
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    cmd: Some(vec![
                        "/bin/sh".to_string(),
                        "-c".to_string(),
                        payload.command,
                    ]),
                    ..Default::default()
                },
            )
            .await?;

        let mut output = String::new();
        if let StartExecResults::Attached {
            output: mut stream, ..
        } = docker.start_exec(&exec.id, None).await?
        {
            while let Some(Ok(log)) = stream.next().await {
                if let Some(line) = format_log_output(log) {
                    output.push_str(&line);
                    output.push('\n');
                }
            }
        }

        let inspect = docker.inspect_exec(&exec.id).await?;
        let result = docker::ContainerExecResult {
            exit_code: inspect.exit_code,
            output,
        };

        Ok(ApiResponse::success_with_raw("Command executed", Some(result)).into_response())
    }
    .await;
    context
        .finish(
            &state.metadata_db,
            "container.exec",
            Some(("container", &id)),
            json!({ "name": id }),
            false,
            result,
        )
        .await
}

/// 对指定容器执行操作，如启动、停止、重启或删除。
///
/// # 参数
/// - `state`: 共享的应用状态，包含 `bollard` Docker 客户端。
/// - `payload`: 包含容器名称和要执行的操作 (`ContainerAction`) 的 JSON 请求体。
///
/// # 处理流程
/// 1. 根据 `payload.action` 的值，调用相应的 `bollard` 函数（如 `start_container`）。
/// 2. 记录操作成功或失败的 Docker 操作日志。
/// 3. 返回一个表示操作结果的 `ApiResponse`。
/// 4. 如果 Docker API 调用失败，将错误转换为 `ApiError::DockerApi` 并返回。
pub async fn handle_action(
    State(state): State<Arc<AppState>>,
    context: DockerOperationContext,
    Json(payload): Json<docker::ActionRequest>,
) -> ApiResult<Response> {
    let container_name = &payload.name;
    let event_code = match payload.action {
        docker::ContainerAction::Start => "container.start",
        docker::ContainerAction::Stop => "container.stop",
        docker::ContainerAction::Restart => "container.restart",
        docker::ContainerAction::Remove => "container.remove",
    };
    let high_impact = matches!(payload.action, docker::ContainerAction::Remove);

    info!(
        "Request to perform action '{:?}' on container '{}'",
        payload.action, container_name
    );

    let result: ApiResult<Response> = async {
        let docker = state.docker_client().await?;
        match payload.action {
            docker::ContainerAction::Start => {
                docker
                    .start_container(
                        container_name,
                        None::<query_parameters::StartContainerOptions>,
                    )
                    .await?;
                info!("Successfully started container '{}'", container_name);
                Ok(ApiResponse::ok("Container started").into_response())
            }

            docker::ContainerAction::Stop => {
                docker
                    .stop_container(
                        container_name,
                        None::<query_parameters::StopContainerOptions>,
                    )
                    .await?;
                info!("Successfully stopped container '{}'", container_name);
                Ok(ApiResponse::ok("Container stopped").into_response())
            }
            docker::ContainerAction::Restart => {
                docker
                    .restart_container(
                        container_name,
                        None::<query_parameters::RestartContainerOptions>,
                    )
                    .await?;
                info!("Successfully restarted container '{}'", container_name);
                Ok(ApiResponse::ok("Container restarted").into_response())
            }
            docker::ContainerAction::Remove => {
                docker
                    .remove_container(
                        container_name,
                        None::<query_parameters::RemoveContainerOptions>,
                    )
                    .await?;
                info!("Successfully removed container '{}'", container_name);
                Ok(ApiResponse::ok("Container removed").into_response())
            }
        }
    }
    .await;
    context
        .finish(
            &state.metadata_db,
            event_code,
            Some(("container", container_name)),
            json!({ "name": container_name }),
            high_impact,
            result,
        )
        .await
}

/// 查询参数，用于获取容器日志。
#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    /// 获取日志的行数上限，默认 100，最大 500。
    #[serde(default = "default_tail")]
    pub tail: u16,
}

/// `tail` 查询参数的默认值。
fn default_tail() -> u16 {
    100
}

/// 获取指定容器最新的 N 行日志。
///
/// 这是一个 RESTful 端点，用于获取一次性的日志快照，通常在“最新100条”模式下被前端调用。
///
/// # 参数
/// - `state`: 共享的应用状态。
/// - `id`: 从 URL 路径中提取的 Docker 容器 ID。
/// - `query`: URL 查询参数，包含 `tail` 字段，用于指定获取的日志行数。
///
/// # 返回
/// 一个包含日志行数组的 `ApiResponse`。
pub async fn latest_container_logs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<LogsQuery>,
) -> ApiResult<Response> {
    let tail = query.tail.clamp(1, 500); // 限制 tail 的最大值，防止滥用
    let logs = fetch_container_logs(&state, &id, tail).await?;
    Ok(ApiResponse::success_with_raw("Container logs loaded", Some(logs)).into_response())
}

/// 从 Docker 守护进程获取指定容器的日志。
///
/// # 参数
/// - `state`: 应用程序的共享状态。
/// - `container_id`: 目标容器的 ID。
/// - `tail`: 要获取的日志行数。
///
/// # 返回
/// 一个包含格式化后日志行 (`String`) 的 `Vec`。
async fn fetch_container_logs(
    state: &AppState,
    container_id: &str,
    tail: u16,
) -> Result<Vec<String>, ApiError> {
    let docker = state.docker_client().await?;
    let options = LogsOptions {
        stdout: true,
        stderr: true,
        timestamps: true,
        tail: tail.to_string(),
        ..Default::default()
    };

    let log_stream = docker.logs(container_id, Some(options));

    let lines: Vec<String> = log_stream
        .filter_map(|item| async {
            match item {
                Ok(log) => format_log_output(log),
                Err(err) => {
                    error!(
                        "Failed to fetch container log for {}: {}",
                        container_id, err
                    );
                    None
                }
            }
        })
        .collect()
        .await;

    Ok(lines)
}

/// 格式化 Docker 的 `LogOutput` 为可读的字符串。
///
/// # 处理逻辑
/// 1. 从 `LogOutput` 中提取原始日志字节 (`Bytes`)。
/// 2. 将字节转换为 UTF-8 字符串。
/// 3. 如果日志行以 RFC3339 格式的时间戳开头，则解析该时间戳并将其转换为本地时间，
///    格式化为 `[YYYY/MM/DD HH:MM:SS]`，然后与日志正文拼接。
/// 4. 如果没有时间戳，则直接返回裁剪了前后空白的原始日志行。
///
/// # 参数
/// - `log`: 从 `bollard` 获取的 `LogOutput` 枚举实例。
///
/// # 返回
/// `Option<String>`: 成功格式化则返回 `Some(String)`，如果日志类型不受支持则返回 `None`。
pub(crate) fn format_log_output(log: LogOutput) -> Option<String> {
    let message_bytes = match log {
        LogOutput::StdOut { message } => message,
        LogOutput::StdErr { message } => message,
        _ => return None,
    };

    let line = String::from_utf8_lossy(&message_bytes);

    if let Some(space_index) = line.find(' ') {
        let timestamp_part = &line[..space_index];
        if let Ok(datetime_utc) = timestamp_part.parse::<DateTime<chrono::Utc>>() {
            let local_time: DateTime<Local> = datetime_utc.with_timezone(&Local);
            let message_part = line[space_index + 1..].trim();
            return Some(format!(
                "{} {}",
                local_time.format("[%Y/%m/%d %H:%M:%S]"),
                message_part
            ));
        }
    }
    Some(line.trim().to_string())
}

fn split_nonempty_lines(input: Option<&str>) -> Vec<String> {
    input
        .unwrap_or("")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
        .collect()
}

fn parse_port_bindings(input: Option<&str>) -> PortBindingsResult {
    let mut exposed_ports = Vec::new();
    let mut bindings: bollard::models::PortMap = HashMap::new();

    for raw in split_nonempty_lines(input).into_iter() {
        let parts: Vec<&str> = raw.split(':').collect();
        let (host_ip, host_port, container_part) = match parts.as_slice() {
            [host_port, container_part] => (None, *host_port, *container_part),
            [host_ip, host_port, container_part] => (Some(*host_ip), *host_port, *container_part),
            _ => {
                return Err(ApiError::BadRequest(format!(
                    "invalid port mapping: {}",
                    raw
                )));
            }
        };

        let (container_port, proto) = match container_part.split_once('/') {
            Some((port, proto)) => (port, proto),
            None => (container_part, "tcp"),
        };

        if container_port.trim().is_empty() || host_port.trim().is_empty() {
            return Err(ApiError::BadRequest(format!(
                "invalid port mapping: {}",
                raw
            )));
        }

        let key = format!("{}/{}", container_port.trim(), proto.trim());
        exposed_ports.push(key.clone());
        bindings
            .entry(key)
            .or_insert_with(|| Some(Vec::new()))
            .as_mut()
            .ok_or_else(|| ApiError::BadRequest("failed to parse port mapping".to_string()))?
            .push(PortBinding {
                host_ip: host_ip.map(|value| value.to_string()),
                host_port: Some(host_port.trim().to_string()),
            });
    }

    Ok((exposed_ports, bindings))
}

type PortBindingsResult = Result<(Vec<String>, bollard::models::PortMap), ApiError>;

fn parse_restart_policy(policy: &str) -> Option<RestartPolicyNameEnum> {
    match policy.trim().to_ascii_lowercase().as_str() {
        "no" => Some(RestartPolicyNameEnum::NO),
        "always" => Some(RestartPolicyNameEnum::ALWAYS),
        "unless-stopped" => Some(RestartPolicyNameEnum::UNLESS_STOPPED),
        "on-failure" => Some(RestartPolicyNameEnum::ON_FAILURE),
        _ => None,
    }
}
