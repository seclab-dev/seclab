//! Docker Compose API：项目列表、详情与操作。
//! 基于数据库 docker_compose_projects 进行干净的项目登记与路径维护。

use crate::api::docker::containers::format_log_output;
use crate::config;
use crate::models::docker;
use crate::state::AppState;
use crate::types::{ApiError, ApiResponse, ApiResult};

use axum::extract::{Json, Path, Query, State};
use axum::response::{IntoResponse, Response};
use bollard::models::ContainerSummaryStateEnum;
use bollard::query_parameters::{self, LogsOptions};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;
use tokio::process::Command;
use tracing::info;

const COMPOSE_FILE_NAME: &str = "compose.yaml";

/// 解析项目的存储目录与形态类型。
/// 仅从数据库获取项目路径与类型，若记录不存在则直接返回错误。
async fn resolve_project_meta(
    state: &Arc<AppState>,
    name: &str,
) -> Result<(PathBuf, String), ApiError> {
    let project = normalize_project_name(name)?;
    let row = sqlx::query(
        "SELECT compose_dir, project_type FROM docker_compose_projects WHERE name = ?1",
    )
    .bind(&project)
    .fetch_optional(&state.metadata_db)
    .await?;

    if let Some(row) = row {
        use sqlx::Row;
        let dir: String = row.try_get("compose_dir")?;
        let ptype: String = row.try_get("project_type")?;
        Ok((PathBuf::from(dir), ptype))
    } else {
        Err(ApiError::BadRequest(format!(
            "Compose project not registered in database: {}",
            project
        )))
    }
}

/// 确保 Compose 文件存在并返回其路径。
async fn ensure_compose_file(state: &Arc<AppState>, project: &str) -> Result<PathBuf, ApiError> {
    let (dir, _) = resolve_project_meta(state, project).await?;
    let compose_file = dir.join(COMPOSE_FILE_NAME);
    if tokio::fs::metadata(&compose_file).await.is_err() {
        return Err(ApiError::BadRequest(format!(
            "Compose file does not exist: {}",
            compose_file.display()
        )));
    }
    Ok(compose_file)
}

/// 写入 compose 文件并启动项目。
pub async fn create_project(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<docker::ComposeProjectCreateRequest>,
) -> ApiResult<Response> {
    let project = normalize_project_name(&payload.name)?;
    let ptype = payload
        .project_type
        .clone()
        .unwrap_or_else(|| "docker".to_string());

    let dir = if let Some(custom_dir) = payload.dir.filter(|d| !d.trim().is_empty()) {
        let p = PathBuf::from(custom_dir);
        if p.is_absolute() {
            p
        } else {
            config::compose_root_dir().join(p)
        }
    } else {
        let sub_dir = if ptype == "docker" { "docker" } else { &ptype };
        config::compose_root_dir().join(sub_dir).join(&project)
    };

    tokio::fs::create_dir_all(&dir).await?;
    let compose_file = dir.join(COMPOSE_FILE_NAME);
    tokio::fs::write(&compose_file, payload.compose).await?;

    let dir_str = dir.to_string_lossy().to_string();
    sqlx::query(
        "INSERT OR REPLACE INTO docker_compose_projects (name, compose_dir, project_type) VALUES (?1, ?2, ?3)",
    )
    .bind(&project)
    .bind(&dir_str)
    .bind(&ptype)
    .execute(&state.metadata_db)
    .await?;

    run_compose_command(&project, &compose_file, &["up", "-d"]).await?;

    info!("Compose project created: {} in dir {}", project, dir_str);
    Ok(ApiResponse::ok("Compose project created").into_response())
}

/// 汇总登记在数据库中的 Compose 项目的运行状态与容器数量。
pub async fn list_projects(State(state): State<Arc<AppState>>) -> ApiResult<Response> {
    let docker = state.docker_client().await?;
    let options = query_parameters::ListContainersOptionsBuilder::new()
        .all(true)
        .build();
    let containers = docker.list_containers(Some(options)).await?;

    // 从数据库中查询所有的项目
    let mut projects = HashMap::new();
    let rows = sqlx::query("SELECT name, compose_dir, project_type FROM docker_compose_projects")
        .fetch_all(&state.metadata_db)
        .await?;
    for row in rows {
        use sqlx::Row;
        let name: String = row.try_get("name")?;
        let dir: String = row.try_get("compose_dir")?;
        let ptype: String = row.try_get("project_type")?;

        let has_compose_file = PathBuf::from(&dir).join(COMPOSE_FILE_NAME).exists();

        projects.insert(
            name.clone(),
            docker::ComposeProjectSummary {
                name,
                status: "stopped".to_string(),
                total_containers: 0,
                running_containers: 0,
                exited_containers: 0,
                paused_containers: 0,
                restarting_containers: 0,
                has_compose_file,
                compose_dir: Some(dir),
                project_type: Some(ptype),
            },
        );
    }

    // 根据当前运行容器的 labels 进行状态和容器计数累加
    for container in containers {
        let labels = match &container.labels {
            Some(labels) => labels,
            None => continue,
        };
        let Some(project) = labels.get("com.docker.compose.project") else {
            continue;
        };

        // 干净重构：如果容器的项目不在数据库登记列表中，直接忽略
        let entry = match projects.get_mut(project) {
            Some(entry) => entry,
            None => continue,
        };

        entry.total_containers += 1;
        match container.state {
            Some(ContainerSummaryStateEnum::RUNNING) => entry.running_containers += 1,
            Some(ContainerSummaryStateEnum::EXITED) => entry.exited_containers += 1,
            Some(ContainerSummaryStateEnum::PAUSED) => entry.paused_containers += 1,
            Some(ContainerSummaryStateEnum::RESTARTING) => entry.restarting_containers += 1,
            _ => {}
        }
    }

    // 计算汇总状态
    for summary in projects.values_mut() {
        summary.status = if summary.running_containers > 0 {
            "running".to_string()
        } else if summary.paused_containers > 0 {
            "paused".to_string()
        } else if summary.restarting_containers > 0 {
            "restarting".to_string()
        } else if summary.exited_containers > 0 || summary.total_containers == 0 {
            "stopped".to_string()
        } else {
            "unknown".to_string()
        };
    }

    let mut list: Vec<docker::ComposeProjectSummary> = projects.into_values().collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(ApiResponse::success_with_raw("Compose project list loaded", Some(list)).into_response())
}

/// 启动指定的 Compose 项目。
pub async fn start_project(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<Response> {
    let project = normalize_project_name(&name)?;
    let compose_file = ensure_compose_file(&state, &project).await?;
    run_compose_command(&project, &compose_file, &["start"]).await?;
    Ok(ApiResponse::ok("Compose project started").into_response())
}

/// 停止指定的 Compose 项目。
pub async fn stop_project(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<Response> {
    let project = normalize_project_name(&name)?;
    let compose_file = ensure_compose_file(&state, &project).await?;
    run_compose_command(&project, &compose_file, &["stop"]).await?;
    Ok(ApiResponse::ok("Compose project stopped").into_response())
}

/// 重启指定的 Compose 项目。
pub async fn restart_project(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<Response> {
    let project = normalize_project_name(&name)?;
    let compose_file = ensure_compose_file(&state, &project).await?;
    run_compose_command(&project, &compose_file, &["restart"]).await?;
    Ok(ApiResponse::ok("Compose project restarted").into_response())
}

/// 删除项目时的可选参数（干净重构下已不再强依赖此参数）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteProjectQuery {
    _delete_files: Option<bool>,
}

/// 停止并删除 Compose 项目，依据形态类型自动决定是否保留文件。
pub async fn delete_project(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Query(_query): Query<DeleteProjectQuery>,
) -> ApiResult<Response> {
    let project = normalize_project_name(&name)?;

    // 不能删除运行中的 Compose 项目
    let docker = state.docker_client().await?;
    let mut filters = HashMap::new();
    filters.insert(
        "label".to_string(),
        vec![format!("com.docker.compose.project={}", project)],
    );
    filters.insert("status".to_string(), vec!["running".to_string()]);
    let options = query_parameters::ListContainersOptionsBuilder::new()
        .all(true)
        .filters(&filters)
        .build();
    let running_containers = docker.list_containers(Some(options)).await?;
    if !running_containers.is_empty() {
        return Err(ApiError::BadRequest(
            "Cannot delete a running compose project. Please stop it first.".to_string(),
        ));
    }

    let (dir, ptype) = resolve_project_meta(&state, &project).await?;
    let compose_file = dir.join(COMPOSE_FILE_NAME);
    if tokio::fs::metadata(&compose_file).await.is_ok() {
        run_compose_command(&project, &compose_file, &["down"]).await?;
    } else {
        remove_project_containers(&state, &project).await?;
    }

    // 从数据库中移除登记
    sqlx::query("DELETE FROM docker_compose_projects WHERE name = ?1")
        .bind(&project)
        .execute(&state.metadata_db)
        .await?;

    // 形态控制删除：
    // - "docker" 形态：坚决不清除本地文件目录以防止挂载或关键数据丢失
    // - 其他一键部署形态：彻底清理该项目的整个目录
    if ptype != "docker" {
        if !is_safe_project_name(&project) {
            return Err(ApiError::BadRequest("unsafe project name".to_string()));
        }
        if tokio::fs::metadata(&dir).await.is_ok() {
            tokio::fs::remove_dir_all(&dir).await?;
        }
    }

    Ok(ApiResponse::ok("Compose project deleted").into_response())
}

/// 拉取最新镜像并重建 Compose 项目。
pub async fn update_project(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<Response> {
    let project = normalize_project_name(&name)?;
    let compose_file = ensure_compose_file(&state, &project).await?;
    run_compose_command(&project, &compose_file, &["pull"]).await?;
    run_compose_command(&project, &compose_file, &["up", "-d", "--force-recreate"]).await?;
    Ok(ApiResponse::ok("Compose project updated").into_response())
}

/// 返回 Compose 项目根目录路径。
pub async fn compose_root() -> ApiResult<Response> {
    let root = config::compose_root_dir();
    Ok(ApiResponse::success_with_raw(
        "Compose root loaded",
        Some(root.to_string_lossy().to_string()),
    )
    .into_response())
}

/// 聚合指定 Compose 项目内所有容器的日志。
pub async fn project_logs(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Query(query): Query<docker::ComposeProjectLogsQuery>,
) -> ApiResult<Response> {
    let project = normalize_project_name(&name)?;
    let tail = query.tail.unwrap_or(200).clamp(1, 500);
    let since = parse_timestamp(query.since.as_deref())?;
    let until = parse_timestamp(query.until.as_deref())?;
    let docker = state.docker_client().await?;

    let mut filters = HashMap::new();
    filters.insert(
        "label".to_string(),
        vec![format!("com.docker.compose.project={}", project)],
    );
    let options = query_parameters::ListContainersOptionsBuilder::new()
        .all(true)
        .filters(&filters)
        .build();
    let containers = docker.list_containers(Some(options)).await?;

    let mut lines = Vec::new();
    for container in containers {
        let id = match &container.id {
            Some(id) => id,
            None => continue,
        };
        let name = container
            .names
            .as_ref()
            .and_then(|names| names.first())
            .map(|value| value.trim_start_matches('/').to_string())
            .unwrap_or_else(|| id.chars().take(12).collect());

        let options = LogsOptions {
            stdout: true,
            stderr: true,
            timestamps: true,
            tail: tail.to_string(),
            since: since.unwrap_or_default(),
            until: until.unwrap_or_default(),
            ..Default::default()
        };

        let mut stream = docker.logs(id, Some(options));
        while let Some(item) = stream.next().await {
            match item {
                Ok(log) => {
                    if let Some(line) = format_log_output(log) {
                        lines.push(format!("[{}] {}", name, line));
                    }
                }
                Err(err) => {
                    return Err(ApiError::DockerApi(err));
                }
            }
        }
    }

    Ok(ApiResponse::success_with_raw("Compose project logs loaded", Some(lines)).into_response())
}

fn normalize_project_name(name: &str) -> Result<String, ApiError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest(
            "project name must not be empty".to_string(),
        ));
    }
    if trimmed == "." || trimmed == ".." {
        return Err(ApiError::BadRequest(
            "invalid project name: must not be '.' or '..'".to_string(),
        ));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(ApiError::BadRequest(
            "project name may only contain letters, digits, hyphen, underscore, and dot"
                .to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

fn is_safe_project_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty()
        && trimmed != "."
        && trimmed != ".."
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

async fn run_compose_command(
    project: &str,
    compose_file: &FsPath,
    args: &[&str],
) -> Result<(), ApiError> {
    let output = Command::new("docker")
        .args(["compose", "-f"])
        .arg(compose_file)
        .args(["-p", project])
        .args(args)
        .output()
        .await?;

    if !output.status.success() {
        let detail = command_error_detail(&output);
        tracing::error!(
            project,
            compose_file = %compose_file.display(),
            args = ?args,
            status = ?output.status.code(),
            stdout = %String::from_utf8_lossy(&output.stdout).trim(),
            stderr = %String::from_utf8_lossy(&output.stderr).trim(),
            "docker compose command failed"
        );
        return Err(ApiError::BadRequest(format!(
            "docker compose command failed: {detail}"
        )));
    }

    Ok(())
}

/// 提取 Docker Compose 命令最有价值的错误文本。
fn command_error_detail(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim();
    if !detail.is_empty() {
        return detail.to_string();
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = stdout.trim();
    if !detail.is_empty() {
        return detail.to_string();
    }
    match output.status.code() {
        Some(code) => format!("docker command exited with code {code}"),
        None => "docker command terminated without an exit code".to_string(),
    }
}

async fn remove_project_containers(state: &Arc<AppState>, project: &str) -> Result<(), ApiError> {
    let docker = state.docker_client().await?;
    let mut filters = HashMap::new();
    filters.insert(
        "label".to_string(),
        vec![format!("com.docker.compose.project={}", project)],
    );
    let options = query_parameters::ListContainersOptionsBuilder::new()
        .all(true)
        .filters(&filters)
        .build();
    let containers = docker.list_containers(Some(options)).await?;
    for container in containers {
        if let Some(id) = container.id {
            let remove_options = query_parameters::RemoveContainerOptionsBuilder::new()
                .force(true)
                .build();
            docker.remove_container(&id, Some(remove_options)).await?;
        }
    }
    Ok(())
}

fn parse_timestamp(input: Option<&str>) -> Result<Option<i32>, ApiError> {
    let Some(value) = input.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };

    if let Ok(seconds) = value.parse::<i64>() {
        return Ok(Some(clamp_i64_to_i32(seconds)));
    }

    let parsed = DateTime::parse_from_rfc3339(value).map_err(|_| {
        ApiError::BadRequest("timestamp must be RFC3339 or Unix seconds".to_string())
    })?;
    Ok(Some(clamp_i64_to_i32(
        parsed.with_timezone(&Utc).timestamp(),
    )))
}

fn clamp_i64_to_i32(value: i64) -> i32 {
    if value > i64::from(i32::MAX) {
        i32::MAX
    } else if value < i64::from(i32::MIN) {
        i32::MIN
    } else {
        value as i32
    }
}

/// 伸缩 Compose 项目中指定服务的容器副本数量。
pub async fn scale_project(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(payload): Json<docker::ComposeProjectScaleRequest>,
) -> ApiResult<Response> {
    let project = normalize_project_name(&name)?;
    let compose_file = ensure_compose_file(&state, &project).await?;
    let scale_arg = format!("{}={}", payload.service, payload.replicas);

    run_compose_command(
        &project,
        &compose_file,
        &["up", "-d", "--scale", &scale_arg],
    )
    .await?;

    info!("Compose project scaled: {}/{}", project, scale_arg);
    Ok(ApiResponse::ok("Compose project scaled").into_response())
}

/// 校验 Compose YAML 配置的合法性。
pub async fn validate_compose(
    Json(payload): Json<docker::ComposeProjectValidateRequest>,
) -> ApiResult<Response> {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;

    info!("Validating compose YAML configuration");
    let mut child = Command::new("docker")
        .args(["compose", "-f", "-", "config"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            ApiError::Internal(format!("Failed to spawn docker compose process: {}", e))
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(payload.compose.as_bytes())
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to write compose payload: {}", e)))?;
    }

    let output = child.wait_with_output().await?;

    let response = if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        docker::ComposeProjectValidateResponse {
            valid: true,
            error: None,
            config: Some(stdout),
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        docker::ComposeProjectValidateResponse {
            valid: false,
            error: Some(stderr),
            config: None,
        }
    };

    Ok(
        ApiResponse::success_with_raw("Compose validation finished", Some(response))
            .into_response(),
    )
}

#[cfg(test)]
mod tests {
    use super::command_error_detail;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};

    #[test]
    fn compose_error_detail_prefers_stderr() {
        let output = Output {
            status: ExitStatus::from_raw(1 << 8),
            stdout: b"pulling image".to_vec(),
            stderr: b"manifest unknown".to_vec(),
        };

        assert_eq!(command_error_detail(&output), "manifest unknown");
    }

    #[test]
    fn compose_error_detail_falls_back_to_stdout() {
        let output = Output {
            status: ExitStatus::from_raw(1 << 8),
            stdout: b"network timeout".to_vec(),
            stderr: Vec::new(),
        };

        assert_eq!(command_error_detail(&output), "network timeout");
    }
}
