//! Docker 客户端模型：与 Docker daemon 通信封装。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 容器动作请求，包含目标与操作类型。
#[derive(Debug, Deserialize)]
pub struct ActionRequest {
    pub id: String,
    /// 为了减少一次查询`name`的请求
    pub name: String,
    pub action: ContainerAction,
}

/// 区分操作类型
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerAction {
    Start,
    Stop,
    Restart,
    Remove,
}

/// Docker 容器状态分布。
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerStateCounts {
    pub total: usize,
    pub running: usize,
    pub paused: usize,
    pub restarting: usize,
    pub exited: usize,
    pub other: usize,
}

/// Docker Compose 项目健康状态分布。
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStateCounts {
    pub total: usize,
    pub healthy: usize,
    pub partial: usize,
    pub stopped: usize,
    pub unknown: usize,
}

/// Docker 镜像数量分布。
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageCounts {
    pub total: usize,
    pub dangling: usize,
}

/// 精简的容器引用信息。
#[derive(Debug, Deserialize)]
pub struct ContainerRef {
    pub id: String,
    pub name: String,
}

/// 精简的镜像引用信息。
#[derive(Debug, Deserialize)]
pub struct ImageRef {
    pub id: String,
    pub name: String,
}

/// 创建容器所需的参数集合。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerCreateRequest {
    pub name: String,
    pub image: String,
    pub command: Option<String>,
    pub env: Option<String>,
    pub ports: Option<String>,
    pub volumes: Option<String>,
    pub restart_policy: Option<String>,
    pub network: Option<String>,
    pub auto_remove: Option<bool>,
    pub auto_start: Option<bool>,
}

/// 容器重命名请求。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerRenameRequest {
    pub name: String,
}

/// 容器内执行命令的请求。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerExecRequest {
    pub command: String,
}

/// 容器内执行命令的结果。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerExecResult {
    pub exit_code: Option<i64>,
    pub output: String,
}

/// 删除容器时的可选参数。
#[derive(Debug, Deserialize)]
pub struct ContainerRemoveQuery {
    pub force: Option<bool>,
    pub volumes: Option<bool>,
    pub link: Option<bool>,
}

/// 创建 Compose 项目的请求体。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeProjectCreateRequest {
    pub name: String,
    pub compose: String,
    pub dir: Option<String>,
    pub project_type: Option<String>,
}

/// Compose 项目状态与容器数量汇总。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeProjectSummary {
    pub name: String,
    pub status: String,
    pub total_containers: usize,
    pub running_containers: usize,
    pub exited_containers: usize,
    pub paused_containers: usize,
    pub restarting_containers: usize,
    pub has_compose_file: bool,
    pub compose_dir: Option<String>,
    pub project_type: Option<String>,
}

/// Compose 项目日志查询参数。
#[derive(Debug, Deserialize)]
pub struct ComposeProjectLogsQuery {
    pub tail: Option<u16>,
    pub since: Option<String>,
    pub until: Option<String>,
}

/// 创建网络的请求参数。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkCreateRequest {
    pub name: String,
    pub driver: Option<String>,
    pub subnet: Option<String>,
    pub gateway: Option<String>,
    pub labels: Option<HashMap<String, String>>,
}

/// 将容器连接到网络的请求参数。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkConnectRequest {
    pub container: String,
}

/// 从网络断开容器的请求参数。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkDisconnectRequest {
    pub container: String,
    pub force: Option<bool>,
}

/// 资源采样数据的新鲜度与完整性状态。
#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ResourceSampleStatus {
    Fresh,
    Partial,
    Stale,
    Unavailable,
}

/// Docker 容器资源统计快照。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerResourceUsageSummary {
    pub cpu_core_percent: f64,
    pub memory_working_set_bytes: u64,
    pub memory_limit_bytes: u64,
    pub memory_percent: f64,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
}

/// Docker 宿主机资源汇总。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostResourceUsageSummary {
    pub status: ResourceSampleStatus,
    pub collected_at: Option<i64>,
    pub running_container_count: usize,
    pub sampled_container_count: usize,
    pub cpu_host_percent: f64,
    pub cpu_core_percent: f64,
    pub memory_working_set_bytes: u64,
    pub memory_limit_bytes: u64,
    pub memory_percent: f64,
}

/// Docker 宿主机单个时间点的资源使用数据。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostResourceUsagePoint {
    pub timestamp: i64,
    pub cpu_host_percent: f64,
    pub cpu_core_percent: f64,
    pub memory_working_set_bytes: u64,
    pub memory_limit_bytes: u64,
    pub memory_percent: f64,
    pub running_container_count: usize,
    pub sampled_container_count: usize,
}

/// Docker 容器单个时间点的资源使用数据。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerResourceUsagePoint {
    pub timestamp: i64,
    pub cpu_core_percent: f64,
    pub memory_working_set_bytes: u64,
    pub memory_limit_bytes: u64,
    pub memory_percent: f64,
    pub network_rx_bytes_per_second: Option<f64>,
    pub network_tx_bytes_per_second: Option<f64>,
}

/// Docker 宿主机资源历史序列。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostResourceUsageHistory {
    pub points: Vec<HostResourceUsagePoint>,
}

/// 批量查询容器统计的请求体。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerStatsBatchRequest {
    pub ids: Vec<String>,
}

/// 批量容器统计的响应数据。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerStatsBatchResponse {
    pub summaries: HashMap<String, ContainerResourceUsageSummary>,
}

/// 概览趋势选择器中的容器简要信息。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrendContainerItem {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub state: String,
}

/// 概览页所需的实时数据汇总。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewRealtimeResponse {
    pub collected_at: i64,
    pub container_states: ContainerStateCounts,
    pub project_states: ProjectStateCounts,
    pub images: ImageCounts,
    pub resource_usage: HostResourceUsageSummary,
    pub trend_containers: Vec<TrendContainerItem>,
}

/// 单个容器的历史趋势数据项。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerStatsHistoryAllItem {
    pub id: String,
    pub name: String,
    pub points: Vec<ContainerResourceUsagePoint>,
}

/// 多容器历史趋势的汇总响应。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerStatsHistoryAllResponse {
    pub containers: Vec<ContainerStatsHistoryAllItem>,
}

/// 批量查询容器资源趋势的请求体。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerStatsHistoryQuery {
    pub ids: Vec<String>,
    pub hours: Option<i64>,
}

/// Docker 磁盘使用分类统计。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerDiskUsageCategory {
    pub total_count: usize,
    pub active_count: usize,
    pub size_bytes: u64,
    pub reclaimable_bytes: u64,
}

/// Docker 系统磁盘使用汇总。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerDiskUsageSummary {
    pub collected_at: i64,
    pub images: DockerDiskUsageCategory,
    pub containers: DockerDiskUsageCategory,
    pub volumes: DockerDiskUsageCategory,
    pub build_cache: DockerDiskUsageCategory,
}

/// Compose 项目服务伸缩请求体
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeProjectScaleRequest {
    pub service: String,
    pub replicas: u32,
}

/// Compose YAML 格式校验请求体
#[derive(Debug, Deserialize)]
pub struct ComposeProjectValidateRequest {
    pub compose: String,
}

/// Compose YAML 格式校验响应体
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeProjectValidateResponse {
    pub valid: bool,
    pub error: Option<String>,
    pub config: Option<String>,
}

/// 卷创建请求体
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeCreateRequest {
    pub name: String,
    pub driver: Option<String>,
    pub driver_opts: Option<HashMap<String, String>>,
    pub labels: Option<HashMap<String, String>>,
}
