//! 路由注册：挂载所有 API 子路由并拼装主路由器。

use super::api::{
    disks, docker, firewall, fs, host_terminal, operation_events, process, runtime_logs,
    scheduled_tasks, script_runs, suite_workloads, system, system_monitoring, upgrade, websocket,
};
use crate::db;
use crate::state::DbPool;

use crate::services::{
    docker_project_tasks, docker_stats, system_monitoring as monitoring_service, task_scheduler,
    websocket::create_channel,
};
use crate::state::AppState;
use anyhow::Result;
use axum::extract::connect_info::ConnectInfo;
use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{Method, Request},
};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;
use tracing::info_span;

/// 需要 state 的路由
pub fn state_api_router() -> Router<Arc<AppState>> {
    Router::new()
        .nest("/docker", docker::docker_router())
        .nest("/disks", disks::disk_router())
        .nest("/files", fs::fs_router())
        .nest("/firewall", firewall::firewall_router())
        .nest("/terminal", host_terminal::host_terminal_router())
        .nest("/runtime-logs", runtime_logs::runtime_log_router())
        .nest(
            "/operation-events",
            operation_events::operation_event_router(),
        )
        .nest("/system", system::system_router())
        .nest(
            "/system-monitoring",
            system_monitoring::system_monitoring_router(),
        )
        .nest("/scheduled-tasks", scheduled_tasks::scheduled_task_router())
        .nest("/script-runs", script_runs::script_run_router())
        .nest("/upgrade", upgrade::upgrade_router())
        .nest("/websocket", websocket::websocket_router())
        .merge(process::process_router())
        .nest(
            "/suite-workloads",
            suite_workloads::suite_workloads_router(),
        )
}

/// 创建路由
pub async fn create_router() -> Result<(Router, DbPool)> {
    // 建立数据库连接池。
    let pool = db::establish_connection().await?;
    let pool_for_shutdown = pool.clone();
    tracing::info!("Database connection pool established successfully.");
    let system_monitoring =
        Arc::new(monitoring_service::SystemMonitoringRuntime::load(&pool).await?);
    let process_manager = Arc::new(crate::services::process_manager::ProcessManagerRuntime::new());
    docker_project_tasks::initialize(&pool).await?;
    crate::services::file_tasks::initialize(&pool).await?;
    crate::services::process_manager::initialize_signal_operations(&pool).await?;

    // 初始化 Docker 客户端，连接失败时仅记录日志
    let (docker, docker_status) = AppState::init_docker_state().await;

    let websocket_sender = create_channel();

    // 创建共享的应用状态
    let app_state = Arc::new(AppState {
        server_name: "SecLab".to_string(),
        docker: tokio::sync::RwLock::new(docker),
        docker_status: tokio::sync::RwLock::new(docker_status),
        system_monitoring: Arc::clone(&system_monitoring),
        process_manager: Arc::clone(&process_manager),
        metadata_db: pool,
        websocket_sender,
        running_task_ids: tokio::sync::Mutex::new(std::collections::HashSet::new()),
    });

    docker_stats::spawn_stats_collector(Arc::clone(&app_state));
    crate::services::database_maintenance::spawn_worker(app_state.metadata_db.clone());
    docker_project_tasks::spawn_retention_worker(app_state.metadata_db.clone());
    crate::services::file_transfers::spawn_retention_worker(app_state.metadata_db.clone());
    monitoring_service::spawn_sampler(app_state.metadata_db.clone(), system_monitoring);
    process_manager
        .refresh_process_snapshot(&app_state)
        .await
        .map_err(anyhow::Error::msg)?;
    process_manager
        .refresh_network_snapshot()
        .await
        .map_err(anyhow::Error::msg)?;
    process_manager.spawn_samplers(Arc::clone(&app_state));
    task_scheduler::spawn_scheduler(Arc::clone(&app_state));
    crate::services::script_runs::recover(&app_state)
        .await
        .map_err(|error| anyhow::anyhow!("failed to recover script runs: {error:?}"))?;
    crate::services::disk_operations::recover(&app_state)
        .await
        .map_err(|error| anyhow::anyhow!("failed to recover disk operations: {error:?}"))?;

    // 配置 CORS：允许所有来源、常用方法
    let cors = CorsLayer::new()
        .allow_origin(Any) // 你也可以用 .allow_origin("https://example.com".parse().unwrap())
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers(Any);

    // 创建一个需要状态的路由实例，并立即为其提供状态
    let state_routes = state_api_router().with_state(Arc::clone(&app_state));

    let router = Router::new()
        .nest("/api/v1/agent", state_routes)
        .layer(cors)
        .layer(axum::middleware::from_fn(
            seclab_api::logging::http_log_middleware,
        ))
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &Request<_>| {
                // 尝试从 request 的 extensions 中获取 ConnectInfo
                let client_ip = request
                    .extensions()
                    .get::<ConnectInfo<SocketAddr>>()
                    .map(|ConnectInfo(addr)| addr.to_string())
                    .unwrap_or_else(|| "unix_socket".to_string());

                info_span!(
                    env!("CARGO_CRATE_NAME"),
                    client_ip,
                    method = ?request.method(),
                    path = ?request.uri().path(),
                    some_other_field = tracing::field::Empty,
                )
            }),
        )
        .layer(DefaultBodyLimit::disable())
        .layer(RequestBodyLimitLayer::new(
            10 * 1024 * 1024 * 1024, /* 10GB */
        ));

    Ok((router, pool_for_shutdown))
}
