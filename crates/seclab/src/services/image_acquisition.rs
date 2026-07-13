//! 节点 Docker 镜像获取编排：目标复用、主控分发与仓库回退。

use crate::models::NodeRuntimeClient;
use crate::models::logging::LogModule;
use crate::services::logging::PlatformLogEntry;
use crate::state::AppState;
use anyhow::{Context, anyhow};
use bollard::Docker;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex as AsyncMutex, Notify};
use uuid::Uuid;

const AGENT_AVAILABILITY_PATH: &str = "/api/v1/agent/docker/images/availability";
const AGENT_LOAD_PATH: &str = "/api/v1/agent/docker/images/load";
const AGENT_PULL_TASKS_PATH: &str = "/api/v1/agent/docker/image-pull-tasks";
const TASK_TTL: Duration = Duration::from_secs(30 * 60);

/// 镜像任务状态。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ImageTaskStatus {
    Pending,
    Running,
    Success,
    Failed,
    Cancelled,
}

/// 镜像最终来源。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ImageSource {
    Target,
    Controller,
    Registry,
}

/// 镜像任务当前阶段。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImageStage {
    Checking,
    Exporting,
    Uploading,
    Loading,
    Pulling,
}

/// 主控统一镜像任务快照。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageTask {
    pub task_id: String,
    pub node_id: String,
    pub image_ref: String,
    pub status: ImageTaskStatus,
    pub source: Option<ImageSource>,
    pub stage: ImageStage,
    pub progress_percent: u8,
    pub status_text: String,
    pub controller_error: Option<String>,
    pub registry_error: Option<String>,
    #[serde(skip)]
    cancel: Arc<AtomicBool>,
    #[serde(skip)]
    finished_at: Option<Instant>,
}

#[derive(Default)]
struct AcquisitionGate {
    running: bool,
    notify: Arc<Notify>,
}

/// 可克隆的镜像获取服务。
#[derive(Clone, Default)]
pub struct ImageAcquisitionService {
    tasks: Arc<Mutex<HashMap<String, ImageTask>>>,
    gates: Arc<AsyncMutex<HashMap<String, AcquisitionGate>>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentAvailability {
    image_ref: String,
    available: bool,
}

impl ImageAcquisitionService {
    pub fn new() -> Self {
        Self::default()
    }

    /// 创建后台镜像获取任务。
    pub fn start(&self, state: Arc<AppState>, node_id: String, image_ref: String) -> ImageTask {
        self.cleanup();
        let task = ImageTask {
            task_id: Uuid::new_v4().to_string(),
            node_id,
            image_ref,
            status: ImageTaskStatus::Pending,
            source: None,
            stage: ImageStage::Checking,
            progress_percent: 0,
            status_text: "Waiting to acquire image".to_string(),
            controller_error: None,
            registry_error: None,
            cancel: Arc::new(AtomicBool::new(false)),
            finished_at: None,
        };
        self.tasks
            .lock()
            .expect("image task lock poisoned")
            .insert(task.task_id.clone(), task.clone());
        let service = self.clone();
        let task_id = task.task_id.clone();
        tokio::spawn(async move {
            service.run_task(state, &task_id).await;
        });
        task
    }

    pub fn get(&self, task_id: &str) -> Option<ImageTask> {
        self.tasks
            .lock()
            .expect("image task lock poisoned")
            .get(task_id)
            .cloned()
    }

    pub fn cancel(&self, task_id: &str) -> Option<ImageTask> {
        let mut tasks = self.tasks.lock().expect("image task lock poisoned");
        let task = tasks.get_mut(task_id)?;
        if matches!(
            task.status,
            ImageTaskStatus::Pending | ImageTaskStatus::Running
        ) {
            task.cancel.store(true, Ordering::Relaxed);
            task.status_text = "Cancelling image acquisition".to_string();
        }
        Some(task.clone())
    }

    /// 同步确保镜像在节点可用，供套件安装流程复用。
    pub async fn acquire(
        &self,
        state: Arc<AppState>,
        node_id: &str,
        image_ref: &str,
    ) -> anyhow::Result<ImageSource> {
        self.acquire_inner(
            state,
            node_id,
            image_ref,
            Arc::new(AtomicBool::new(false)),
            None,
        )
        .await
    }

    async fn run_task(&self, state: Arc<AppState>, task_id: &str) {
        let Some(task) = self.get(task_id) else {
            return;
        };
        self.update(task_id, |task| {
            task.status = ImageTaskStatus::Running;
            task.progress_percent = 2;
        });
        if let Some(task) = self.get(task_id) {
            let mut log = PlatformLogEntry::new(
                "system",
                "docker_image_acquisition",
                IpAddr::V4(Ipv4Addr::LOCALHOST),
            )
            .module(LogModule::Docker)
            .target_type("docker_image")
            .target_id(&task.image_ref)
            .metadata(serde_json::json!({
                "node_id": task.node_id,
                "image_ref": task.image_ref,
                "source": task.source,
                "stage": task.stage,
                "controller_error": task.controller_error,
                "registry_error": task.registry_error,
            }));
            if task.status == ImageTaskStatus::Success {
                log = log.set_success();
            }
            log.finish(&state.metadata_db);
        }
        let result = self
            .acquire_inner(
                state,
                &task.node_id,
                &task.image_ref,
                Arc::clone(&task.cancel),
                Some(task_id),
            )
            .await;
        self.update(task_id, |task| {
            task.finished_at = Some(Instant::now());
            match result {
                Ok(source) => {
                    task.status = ImageTaskStatus::Success;
                    task.source = Some(source);
                    task.progress_percent = 100;
                    task.status_text = "Image is ready".to_string();
                }
                Err(_) if task.cancel.load(Ordering::Relaxed) => {
                    task.status = ImageTaskStatus::Cancelled;
                    task.status_text = "Image acquisition cancelled".to_string();
                }
                Err(err) => {
                    task.status = ImageTaskStatus::Failed;
                    task.progress_percent = task.progress_percent.min(99);
                    task.status_text = err.to_string();
                }
            }
        });
    }

    async fn acquire_inner(
        &self,
        state: Arc<AppState>,
        node_id: &str,
        image_ref: &str,
        cancel: Arc<AtomicBool>,
        task_id: Option<&str>,
    ) -> anyhow::Result<ImageSource> {
        let key = format!("{node_id}\0{image_ref}");
        loop {
            let notify = {
                let mut gates = self.gates.lock().await;
                let gate = gates.entry(key.clone()).or_default();
                if gate.running {
                    Some(Arc::clone(&gate.notify))
                } else {
                    gate.running = true;
                    None
                }
            };
            if let Some(notify) = notify {
                notify.notified().await;
                if cancel.load(Ordering::Relaxed) {
                    return Err(anyhow!("image acquisition cancelled"));
                }
                continue;
            }
            break;
        }

        let result = self
            .acquire_exclusive(&state, node_id, image_ref, &cancel, task_id)
            .await;
        let mut gates = self.gates.lock().await;
        if let Some(gate) = gates.remove(&key) {
            gate.notify.notify_waiters();
        }
        result
    }

    async fn acquire_exclusive(
        &self,
        state: &Arc<AppState>,
        node_id: &str,
        image_ref: &str,
        cancel: &Arc<AtomicBool>,
        task_id: Option<&str>,
    ) -> anyhow::Result<ImageSource> {
        self.set_stage(task_id, ImageStage::Checking, 5, "Checking target image");
        let client = NodeRuntimeClient::from_node_route(&state.metadata_db, Some(node_id)).await?;
        if image_available(&client, image_ref).await? {
            return Ok(ImageSource::Target);
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(anyhow!("image acquisition cancelled"));
        }

        let controller = local_docker();
        if let Ok(docker) = controller
            && docker.inspect_image(image_ref).await.is_ok()
        {
            self.set_stage(
                task_id,
                ImageStage::Exporting,
                10,
                "Exporting controller image",
            );
            let service = self.clone();
            let progress_task_id = task_id.map(ToOwned::to_owned);
            match transfer_controller_image(
                docker,
                &client,
                image_ref,
                Arc::clone(cancel),
                move |progress| {
                    service.set_stage(
                        progress_task_id.as_deref(),
                        ImageStage::Uploading,
                        progress,
                        "Transferring controller image",
                    );
                },
            )
            .await
            {
                Ok(()) => {
                    self.set_stage(task_id, ImageStage::Loading, 75, "Image loaded on target");
                    return Ok(ImageSource::Controller);
                }
                Err(err) if cancel.load(Ordering::Relaxed) => return Err(err),
                Err(err) => {
                    tracing::warn!(node_id, image_ref, error = %err, "controller image transfer failed; falling back to registry");
                    self.update_optional(task_id, |task| {
                        task.controller_error = Some(err.to_string())
                    });
                }
            }
        }

        if cancel.load(Ordering::Relaxed) {
            return Err(anyhow!("image acquisition cancelled"));
        }
        self.set_stage(
            task_id,
            ImageStage::Pulling,
            80,
            "Pulling image from registry",
        );
        match pull_from_registry(&client, image_ref, cancel, |progress, text| {
            self.update_optional(task_id, |task| {
                task.progress_percent = (80 + progress / 5).min(99);
                task.status_text = text;
            });
        })
        .await
        {
            Ok(()) => Ok(ImageSource::Registry),
            Err(err) => {
                self.update_optional(task_id, |task| task.registry_error = Some(err.to_string()));
                let controller_error = task_id
                    .and_then(|id| self.get(id))
                    .and_then(|task| task.controller_error);
                Err(match controller_error {
                    Some(first) => {
                        anyhow!("controller transfer failed: {first}; registry pull failed: {err}")
                    }
                    None => anyhow!("registry pull failed: {err}"),
                })
            }
        }
    }

    fn set_stage(&self, task_id: Option<&str>, stage: ImageStage, progress: u8, text: &str) {
        self.update_optional(task_id, |task| {
            task.stage = stage;
            task.progress_percent = task.progress_percent.max(progress);
            task.status_text = text.to_string();
        });
    }
    fn update_optional(&self, task_id: Option<&str>, update: impl FnOnce(&mut ImageTask)) {
        if let Some(id) = task_id {
            self.update(id, update);
        }
    }
    fn update(&self, task_id: &str, update: impl FnOnce(&mut ImageTask)) {
        if let Some(task) = self
            .tasks
            .lock()
            .expect("image task lock poisoned")
            .get_mut(task_id)
        {
            update(task);
        }
    }
    fn cleanup(&self) {
        self.tasks
            .lock()
            .expect("image task lock poisoned")
            .retain(|_, task| {
                task.finished_at
                    .is_none_or(|time| time.elapsed() < TASK_TTL)
            });
    }
}

fn local_docker() -> anyhow::Result<Docker> {
    Docker::connect_with_local_defaults().context("controller Docker is unavailable")
}

async fn image_available(client: &NodeRuntimeClient, image_ref: &str) -> anyhow::Result<bool> {
    let response: Value = client
        .post_json(
            AGENT_AVAILABILITY_PATH,
            &serde_json::json!({"images": [image_ref]}),
        )
        .await?;
    let items: Vec<AgentAvailability> =
        serde_json::from_value(response.get("data").cloned().unwrap_or(Value::Null))?;
    Ok(items
        .first()
        .is_some_and(|item| item.image_ref == image_ref && item.available))
}

async fn transfer_controller_image(
    docker: Docker,
    client: &NodeRuntimeClient,
    image_ref: &str,
    cancel: Arc<AtomicBool>,
    progress: impl Fn(u8) + Send + Sync + 'static,
) -> anyhow::Result<()> {
    let stream = docker
        .export_image(image_ref)
        .scan(0_u64, move |sent, chunk| {
            let cancel = Arc::clone(&cancel);
            let result = if cancel.load(Ordering::Relaxed) {
                Err(std::io::Error::other("image acquisition cancelled"))
            } else {
                chunk
                    .inspect(|bytes| {
                        *sent += bytes.len() as u64;
                        progress((15 + ((*sent / (1024 * 1024)).min(45)) as u8).min(60));
                    })
                    .map_err(std::io::Error::other)
            };
            futures_util::future::ready(Some(result))
        });
    let part = reqwest::multipart::Part::stream(reqwest::Body::wrap_stream(stream))
        .file_name("image.tar")
        .mime_str("application/x-tar")?;
    let response = client
        .client
        .post(client.build_uri(AGENT_LOAD_PATH))
        .multipart(reqwest::multipart::Form::new().part("file", part))
        .timeout(Duration::from_secs(600))
        .send()
        .await?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(anyhow!(
            "Agent image load failed: {} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        ))
    }
}

async fn pull_from_registry(
    client: &NodeRuntimeClient,
    image_ref: &str,
    cancel: &Arc<AtomicBool>,
    progress: impl Fn(u8, String),
) -> anyhow::Result<()> {
    let response: Value = client
        .post_json(
            AGENT_PULL_TASKS_PATH,
            &serde_json::json!({"imageName": image_ref}),
        )
        .await?;
    let task_id = response
        .pointer("/data/taskId")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Agent did not return image pull task id"))?
        .to_string();
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _: Value = client
                .delete_json(&format!("{AGENT_PULL_TASKS_PATH}/{task_id}"))
                .await?;
            return Err(anyhow!("image acquisition cancelled"));
        }
        let value: Value = client
            .get_json(&format!("{AGENT_PULL_TASKS_PATH}/{task_id}/progress"))
            .await?;
        let data = value
            .get("data")
            .ok_or_else(|| anyhow!("Agent image pull progress missing data"))?;
        let percent = data
            .get("progressPercent")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u8;
        let text = data
            .get("statusText")
            .and_then(Value::as_str)
            .unwrap_or("Pulling image from registry")
            .to_string();
        progress(percent, text);
        match data.get("status").and_then(Value::as_str) {
            Some("success") => return Ok(()),
            Some("failed") => {
                return Err(anyhow!(
                    data.get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("Agent registry pull failed")
                        .to_string()
                ));
            }
            Some("cancelled") => return Err(anyhow!("Agent registry pull cancelled")),
            _ => tokio::time::sleep(Duration::from_millis(300)).await,
        }
    }
}
