//! 协议仿真 API 路由处理器：接收控制端仿真下发与销毁动作。

use crate::services::simulation::{
    SimFtpConfig, SimHttpConfig, SimImapConfig, SimPop3Config, SimRdpConfig, SimRedisConfig,
    SimSmtpConfig, SimSshConfig, start_ftp_simulation, start_http_simulation,
    start_imap_simulation, start_pop3_simulation, start_rdp_simulation, start_redis_simulation,
    start_smtp_simulation, start_ssh_simulation,
};
use crate::state::AppState;
use crate::types::{ApiError, ApiResult};
use axum::{
    Router,
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// 启动仿真请求负载。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartSimulationRequest {
    pub rule_id: String,
    pub rule_name: Option<String>,
    pub protocol: String,
    pub port: u16,
    pub config_yaml: String, // 实际存储为高内聚的 JSON 格式配置字符串
    pub seclab_callback_url: String,
    pub node_id: String,
}

/// 已按协议解析完成的仿真配置。
enum ParsedSimulationConfig {
    Http(SimHttpConfig),
    Redis(SimRedisConfig),
    Smtp(SimSmtpConfig),
    Pop3(SimPop3Config),
    Imap(SimImapConfig),
    Ssh(SimSshConfig),
    Ftp(SimFtpConfig),
    Rdp(SimRdpConfig),
}

/// 启动仿真监听器所需的共享上下文。
struct SimulationStartContext {
    rule_id: String,
    rule_name: Option<String>,
    port: u16,
    callback_url: String,
    node_id: String,
    tcp_listener: tokio::net::TcpListener,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
}

impl ParsedSimulationConfig {
    /// 按协议解析仿真配置。
    fn parse(protocol: &str, config_yaml: &str) -> ApiResult<Self> {
        match protocol {
            "http" => Ok(Self::Http(serde_json::from_str(config_yaml).map_err(
                |err| ApiError::BadRequest(format!("Invalid HTTP simulation config: {}", err)),
            )?)),
            "redis" => Ok(Self::Redis(serde_json::from_str(config_yaml).map_err(
                |err| ApiError::BadRequest(format!("Invalid Redis simulation config: {}", err)),
            )?)),
            "smtp" => Ok(Self::Smtp(serde_json::from_str(config_yaml).map_err(
                |err| ApiError::BadRequest(format!("Invalid SMTP simulation config: {}", err)),
            )?)),
            "pop3" => Ok(Self::Pop3(serde_json::from_str(config_yaml).map_err(
                |err| ApiError::BadRequest(format!("Invalid POP3 simulation config: {}", err)),
            )?)),
            "imap" => Ok(Self::Imap(serde_json::from_str(config_yaml).map_err(
                |err| ApiError::BadRequest(format!("Invalid IMAP simulation config: {}", err)),
            )?)),
            "ssh" => Ok(Self::Ssh(serde_json::from_str(config_yaml).map_err(
                |err| ApiError::BadRequest(format!("Invalid SSH simulation config: {}", err)),
            )?)),
            "ftp" => Ok(Self::Ftp(serde_json::from_str(config_yaml).map_err(
                |err| ApiError::BadRequest(format!("Invalid FTP simulation config: {}", err)),
            )?)),
            "rdp" => Ok(Self::Rdp(serde_json::from_str(config_yaml).map_err(
                |err| ApiError::BadRequest(format!("Invalid RDP simulation config: {}", err)),
            )?)),
            _ => Err(ApiError::BadRequest(format!(
                "Unsupported simulation protocol: {}",
                protocol
            ))),
        }
    }
}

/// 停止仿真请求负载。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopSimulationRequest {
    pub port: u16,
}

/// 活跃仿真列表项载荷。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveSimulation {
    pub port: u16,
    pub rule_id: String,
}

/// 启动仿真接口路由处理器。
pub async fn start_simulation_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<StartSimulationRequest>,
) -> ApiResult<impl IntoResponse> {
    // 1. 校验协议类型，并在返回成功前完成协议配置 schema 解析
    let parsed_config = ParsedSimulationConfig::parse(&payload.protocol, &payload.config_yaml)?;

    // 2. 避免端口冲突，若该端口已在监听，优雅将其关闭
    let mut listeners = state.simulation_listeners.lock().await;
    if let Some((_, shutdown_tx)) = listeners.remove(&payload.port) {
        let _ = shutdown_tx.send(());
        // 休眠 100ms 以给操作系统内核腾出端口释放的时间
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    // 2.5 同步进行端口绑定检查，防范外部程序占用或权限不足导致拉起失败
    let listen_addr = format!("0.0.0.0:{}", payload.port);
    let tcp_listener = tokio::net::TcpListener::bind(&listen_addr).await.map_err(|err| {
        let msg = format!(
            "Failed to bind simulation port {}: {} (possibly already in use or permission denied)",
            payload.port,
            err
        );
        ApiError::bad_request(
            seclab_contracts::api::ErrorCode::SimulationPortUnavailable,
            msg.clone(),
        )
        .with_detail(msg)
    })?;

    // 3. 构建优雅停机信道，动态启动服务协程
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let rule_id = payload.rule_id.clone();
    let port = payload.port;
    let callback_url = payload.seclab_callback_url.clone();
    let node_id = payload.node_id.clone();

    // 在后台协程中启动服务监听，独立于当前 API 请求的上下文生命周期
    let protocol = payload.protocol.clone();
    let rule_name = payload.rule_name.clone();
    let start_context = SimulationStartContext {
        rule_id,
        rule_name,
        port,
        callback_url,
        node_id,
        tcp_listener,
        shutdown_rx,
    };
    tokio::spawn(async move {
        let port = start_context.port;
        if let Err(err) = run_simulation_listener(parsed_config, start_context).await {
            tracing::error!(
                "Error in {} simulation listener on port {}: {:?}",
                protocol,
                port,
                err
            );
        }
    });

    // 4. 将运行句柄存入内存中维护
    listeners.insert(port, (payload.rule_id, shutdown_tx));

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({ "status": "success", "message": "Simulation started" })),
    ))
}

/// 按已解析的协议配置启动对应的仿真监听器。
async fn run_simulation_listener(
    parsed_config: ParsedSimulationConfig,
    context: SimulationStartContext,
) -> anyhow::Result<()> {
    match parsed_config {
        ParsedSimulationConfig::Http(config) => {
            start_http_simulation(
                context.rule_id,
                context.rule_name,
                context.port,
                context.callback_url,
                context.node_id,
                config,
                context.tcp_listener,
                context.shutdown_rx,
            )
            .await
        }
        ParsedSimulationConfig::Redis(config) => {
            start_redis_simulation(
                context.rule_id,
                context.rule_name,
                context.port,
                context.callback_url,
                context.node_id,
                config,
                context.tcp_listener,
                context.shutdown_rx,
            )
            .await
        }
        ParsedSimulationConfig::Smtp(config) => {
            start_smtp_simulation(
                context.rule_id,
                context.rule_name,
                context.port,
                context.callback_url,
                context.node_id,
                config,
                context.tcp_listener,
                context.shutdown_rx,
            )
            .await
        }
        ParsedSimulationConfig::Pop3(config) => {
            start_pop3_simulation(
                context.rule_id,
                context.rule_name,
                context.port,
                context.callback_url,
                context.node_id,
                config,
                context.tcp_listener,
                context.shutdown_rx,
            )
            .await
        }
        ParsedSimulationConfig::Imap(config) => {
            start_imap_simulation(
                context.rule_id,
                context.rule_name,
                context.port,
                context.callback_url,
                context.node_id,
                config,
                context.tcp_listener,
                context.shutdown_rx,
            )
            .await
        }
        ParsedSimulationConfig::Ssh(config) => {
            start_ssh_simulation(
                context.rule_id,
                context.rule_name,
                context.port,
                context.callback_url,
                context.node_id,
                config,
                context.tcp_listener,
                context.shutdown_rx,
            )
            .await
        }
        ParsedSimulationConfig::Ftp(config) => {
            start_ftp_simulation(
                context.rule_id,
                context.rule_name,
                context.port,
                context.callback_url,
                context.node_id,
                config,
                context.tcp_listener,
                context.shutdown_rx,
            )
            .await
        }
        ParsedSimulationConfig::Rdp(config) => {
            start_rdp_simulation(
                context.rule_id,
                context.rule_name,
                context.port,
                context.callback_url,
                context.node_id,
                config,
                context.tcp_listener,
                context.shutdown_rx,
            )
            .await
        }
    }
}

/// 停止仿真接口路由处理器。
pub async fn stop_simulation_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<StopSimulationRequest>,
) -> ApiResult<impl IntoResponse> {
    let mut listeners = state.simulation_listeners.lock().await;
    if let Some((_, shutdown_tx)) = listeners.remove(&payload.port) {
        let _ = shutdown_tx.send(());
        Ok((
            StatusCode::OK,
            Json(serde_json::json!({ "status": "success", "message": "Simulation stopped" })),
        ))
    } else {
        Err(ApiError::not_found(
            seclab_contracts::api::ErrorCode::NodeNotFound,
            format!("No active simulation running on port {}", payload.port),
        ))
    }
}

/// 获取当前活跃仿真端口一览的处理器。
pub async fn active_simulations_handler(
    State(state): State<Arc<AppState>>,
) -> ApiResult<impl IntoResponse> {
    let listeners = state.simulation_listeners.lock().await;
    let active_list: Vec<ActiveSimulation> = listeners
        .iter()
        .map(|(&port, (rule_id, _))| ActiveSimulation {
            port,
            rule_id: rule_id.clone(),
        })
        .collect();

    Ok((StatusCode::OK, Json(active_list)))
}

/// 启动抓包请求负载。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartPcapRequest {
    pub port: u16,
    pub instance_id: String,
    pub rule_id: String,
    pub node_id: String,
    pub callback_url: String,
}

/// 停止抓包请求负载。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopPcapRequest {
    pub port: u16,
}

/// 开启常驻流量取证接口路由处理器。
pub async fn start_pcap_handler(
    Json(payload): Json<StartPcapRequest>,
) -> ApiResult<impl IntoResponse> {
    match crate::services::simulation::PcapMuxHub::global()
        .start_capture(
            payload.port,
            &payload.instance_id,
            &payload.rule_id,
            &payload.node_id,
            &payload.callback_url,
        )
        .await
    {
        Ok(filename) => Ok((
            StatusCode::OK,
            Json(
                serde_json::json!({ "status": "success", "message": "Forensic capture started", "filename": filename }),
            ),
        )),
        Err(err) => Err(ApiError::Internal(format!(
            "Failed to start forensic capture: {}",
            err
        ))),
    }
}

/// 停止常驻流量取证接口路由处理器。
pub async fn stop_pcap_handler(
    Json(payload): Json<StopPcapRequest>,
) -> ApiResult<impl IntoResponse> {
    let success = crate::services::simulation::PcapMuxHub::global()
        .stop_capture(payload.port)
        .await;
    if success {
        Ok((
            StatusCode::OK,
            Json(serde_json::json!({ "status": "success", "message": "Forensic capture stopped" })),
        ))
    } else {
        Err(ApiError::not_found(
            seclab_contracts::api::ErrorCode::NodeNotFound,
            format!(
                "No active forensic capture running on port {}",
                payload.port
            ),
        ))
    }
}

/// 组装仿真路由。
pub fn simulation_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/start", post(start_simulation_handler))
        .route("/stop", post(stop_simulation_handler))
        .route("/active", get(active_simulations_handler))
        .route("/pcap/start", post(start_pcap_handler))
        .route("/pcap/stop", post(stop_pcap_handler))
}
