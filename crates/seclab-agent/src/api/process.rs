//! 进程管理 WebSocket API：实时推送进程与网络连接快照。

use crate::services::process_manager;
use axum::{
    extract::{
        WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use seclab_contracts::process::{
    NetworkSnapshot, ProcessManagerActiveView, ProcessManagerClientMessage, ProcessManagerError,
    ProcessManagerServerMessage, ProcessSnapshot, SignalResult,
};
use tokio::{
    sync::mpsc,
    task::JoinHandle,
    time::{self, Duration, MissedTickBehavior},
};
use tracing::{debug, info, warn};

const PROCESS_INTERVAL: Duration = Duration::from_secs(3);
const NETWORK_INTERVAL: Duration = Duration::from_secs(3);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

/// 进程管理 WebSocket 升级入口。
pub async fn process_manager_websocket_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(socket: WebSocket) {
    info!("New process manager WebSocket client connected.");

    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::channel::<ProcessManagerServerMessage>(64);

    let mut send_task = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if let Ok(payload) = serde_json::to_string(&message)
                && sender.send(Message::Text(payload.into())).await.is_err()
            {
                warn!("Failed to send process manager WebSocket message.");
                break;
            }
        }
    });

    let mut active_view = ProcessManagerActiveView::Process;
    send_active_snapshot(tx.clone(), active_view).await;

    let mut process_ticker = time::interval(PROCESS_INTERVAL);
    let mut network_ticker = time::interval(NETWORK_INTERVAL);
    let mut heartbeat_ticker = time::interval(HEARTBEAT_INTERVAL);
    process_ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    network_ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    process_ticker.tick().await;
    network_ticker.tick().await;
    let mut worker_tasks: Vec<JoinHandle<()>> = Vec::new();

    loop {
        tokio::select! {
            _ = process_ticker.tick(), if active_view == ProcessManagerActiveView::Process => {
                send_process_snapshot(tx.clone()).await;
            }
            _ = network_ticker.tick(), if active_view == ProcessManagerActiveView::Network => {
                send_network_snapshot(tx.clone()).await;
            }
            _ = heartbeat_ticker.tick() => {
                if tx.send(ProcessManagerServerMessage::Heartbeat).await.is_err() {
                    break;
                }
            }
            message = receiver.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        if let Some(next_view) =
                            handle_client_message(&text, tx.clone(), &mut worker_tasks).await
                        {
                            active_view = next_view;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        debug!("Process manager WebSocket client disconnected.");
                        break;
                    }
                    Some(Err(err)) => {
                        warn!("Process manager WebSocket receive error: {}", err);
                        break;
                    }
                    _ => {}
                }
            }
            _ = &mut send_task => {
                break;
            }
        }

        worker_tasks.retain(|task| !task.is_finished());
    }

    for task in worker_tasks {
        task.abort();
    }
    send_task.abort();
    info!("Process manager WebSocket client connection closed.");
}

async fn handle_client_message(
    text: &str,
    tx: mpsc::Sender<ProcessManagerServerMessage>,
    worker_tasks: &mut Vec<JoinHandle<()>>,
) -> Option<ProcessManagerActiveView> {
    match serde_json::from_str::<ProcessManagerClientMessage>(text) {
        Ok(ProcessManagerClientMessage::SetActiveView(view)) => {
            send_active_snapshot(tx, view).await;
            Some(view)
        }
        Ok(ProcessManagerClientMessage::SendSignal(payload)) => {
            let task = tokio::spawn(async move {
                let request_id = payload.request_id;
                let pid = payload.pid;
                let signal = payload.signal;
                let sampled_at = now_timestamp();
                let signal_for_task = signal.clone();
                let result = tokio::task::spawn_blocking(move || {
                    process_manager::send_signal(pid, &signal_for_task)
                })
                .await;

                match result {
                    Ok(Ok(execution)) => {
                        let response = SignalResult {
                            request_id,
                            pid: execution.pid,
                            signal: execution.signal,
                            success: execution.success,
                            process_existed: execution.process_existed,
                            message: execution.message,
                            sampled_at,
                        };
                        let _ = tx
                            .send(ProcessManagerServerMessage::SignalResult(response))
                            .await;
                        send_process_snapshot(tx).await;
                    }
                    Ok(Err(message)) => {
                        let _ = tx
                            .send(ProcessManagerServerMessage::Error(ProcessManagerError {
                                request_id: Some(request_id),
                                message,
                            }))
                            .await;
                    }
                    Err(err) => {
                        let _ = tx
                            .send(ProcessManagerServerMessage::Error(ProcessManagerError {
                                request_id: Some(request_id),
                                message: err.to_string(),
                            }))
                            .await;
                    }
                }
            });
            worker_tasks.push(task);
            None
        }
        Err(err) => {
            let _ = tx
                .send(ProcessManagerServerMessage::Error(ProcessManagerError {
                    request_id: None,
                    message: format!("failed to parse client message: {err}"),
                }))
                .await;
            None
        }
    }
}

async fn send_active_snapshot(
    tx: mpsc::Sender<ProcessManagerServerMessage>,
    active_view: ProcessManagerActiveView,
) {
    match active_view {
        ProcessManagerActiveView::Process => send_process_snapshot(tx).await,
        ProcessManagerActiveView::Network => send_network_snapshot(tx).await,
    }
}

async fn send_process_snapshot(tx: mpsc::Sender<ProcessManagerServerMessage>) {
    let result = tokio::task::spawn_blocking(process_manager::collect_processes).await;
    match result {
        Ok(Ok(processes)) => {
            let _ = tx
                .send(ProcessManagerServerMessage::ProcessSnapshot(
                    ProcessSnapshot {
                        processes,
                        sampled_at: now_timestamp(),
                    },
                ))
                .await;
        }
        Ok(Err(message)) => {
            let _ = tx
                .send(ProcessManagerServerMessage::Error(ProcessManagerError {
                    request_id: None,
                    message,
                }))
                .await;
        }
        Err(err) => {
            let _ = tx
                .send(ProcessManagerServerMessage::Error(ProcessManagerError {
                    request_id: None,
                    message: err.to_string(),
                }))
                .await;
        }
    }
}

async fn send_network_snapshot(tx: mpsc::Sender<ProcessManagerServerMessage>) {
    let result = tokio::task::spawn_blocking(|| {
        let connections = process_manager::collect_network_connections()?;
        let summary = process_manager::summarize_network_connections(&connections);
        Ok::<_, String>((connections, summary))
    })
    .await;

    match result {
        Ok(Ok((connections, summary))) => {
            let _ = tx
                .send(ProcessManagerServerMessage::NetworkSnapshot(
                    NetworkSnapshot {
                        connections,
                        summary,
                        sampled_at: now_timestamp(),
                    },
                ))
                .await;
        }
        Ok(Err(message)) => {
            let _ = tx
                .send(ProcessManagerServerMessage::Error(ProcessManagerError {
                    request_id: None,
                    message,
                }))
                .await;
        }
        Err(err) => {
            let _ = tx
                .send(ProcessManagerServerMessage::Error(ProcessManagerError {
                    request_id: None,
                    message: err.to_string(),
                }))
                .await;
        }
    }
}

fn now_timestamp() -> i64 {
    chrono::Utc::now().timestamp()
}
