/**
 * 类型定义参考自 Rust 后端 bollard 库：
 *
 * 注意：TS 类型是手动映射的，可能和 Rust 类型存在微小差异。
 */

interface OciPlatform {
  /** The CPU architecture, for example `amd64` or `ppc64`. */
  architecture?: string

  /** The operating system, for example `linux` or `windows`. */
  os?: string

  /** Optional field specifying the operating system version, for example on Windows `10.0.19041.1165`. */
  os_version?: string

  /** Optional field specifying an array of strings, each listing a required OS feature (for example on Windows `win32k`). */
  os_features?: string[]

  /** Optional field specifying a variant of the CPU, for example `v7` to specify ARMv7 when architecture is `arm`. */
  variant?: string
}

interface OciDescriptor {
  media_type?: string

  digest?: string

  size?: number

  urls?: string[]

  annotations?: Record<string, string>

  data?: string

  platform?: OciPlatform

  /** ArtifactType is the IANA media type of this artifact. */
  artifact_type?: string
}

enum PortTypeEnum {
  EMPTY = '',
  TCP = 'tcp',
  UDP = 'udp',
  SCTP = 'sctp',
}

enum ContainerSummaryStateEnum {
  EMPTY = '',
  CREATED = 'created',
  RUNNING = 'running',
  PAUSED = 'paused',
  RESTARTING = 'restarting',
  EXITED = 'exited',
  REMOVING = 'removing',
  DEAD = 'dead',
}

export interface Port {
  IP?: string
  PrivatePort: number
  PublicPort: number
  Type?: PortTypeEnum
}

interface ContainerSummaryHostConfig {
  /** Networking mode (`host`, `none`, `container:<id>`) or name of the primary
   *  network the container is using. This field is primarily for backward
   *  compatibility. The container can be connected to multiple networks for
   *  which information can be found in the `NetworkSettings.Networks`
   *  field, which enumerates settings per network. */
  network_mode?: string

  /** Arbitrary key-value metadata attached to the container. */
  annotations?: Record<string, string>
}

export interface EndpointIpamConfig {
  /** IPv4 地址 */
  IPv4Address?: string

  /** IPv6 地址 */
  IPv6Address?: string

  /** 本地链路 IP 列表 */
  LinkLocalIPs?: string[]
}

export interface EndpointSettings {
  /** IPAM 配置 */
  IPAMConfig?: EndpointIpamConfig

  /** 链接列表 */
  Links?: string[]

  /** MAC 地址 */
  MacAddress?: string

  /** 别名列表 */
  Aliases?: string[]

  /** 驱动参数映射 */
  DriverOpts?: Record<string, string>

  /** 网关优先级 */
  GwPriority?: number

  /** 网络唯一 ID */
  NetworkID?: string

  /** 端点唯一 ID */
  EndpointID?: string

  /** 网关地址 */
  Gateway?: string

  /** IPv4 地址 */
  IPAddress?: string

  /** IPv4 掩码长度 */
  IPPrefixLen?: number

  /** IPv6 网关地址 */
  IPv6Gateway?: string

  /** 全局 IPv6 地址 */
  GlobalIPv6Address?: string

  /** 全局 IPv6 掩码长度 */
  GlobalIPv6PrefixLen?: number

  /** DNS 名称列表（非 FQDN） */
  DNSNames?: string[]
}

export interface ContainerSummaryNetworkSettings {
  /** Summary of network-settings for each network the container is attached to. */
  Networks?: Record<string, EndpointSettings>
}

/** https://docs.rs/bollard/0.19.4/bollard/models/struct.MountPoint.html */
export interface MountPoint {
  Type?: string
  Name?: string
  Source?: string
  Destination?: string
  Driver?: string
  Mode?: string
  RW?: boolean
  Propagation?: string
}

export interface ContainerSummary {
  /** The ID of this container as a 128-bit (64-character) hexadecimal string (32 bytes). */
  Id?: string

  /** The names associated with this container. */
  Names?: string[]

  /** The name or ID of the image used to create the container. */
  Image?: string

  /** The ID (digest) of the image that this container was created from. */
  ImageID?: string

  /** OCI descriptor of the platform-specific manifest of the image the container was created from. */
  ImageManifestDescriptor?: OciDescriptor

  /** Command to run when starting the container */
  Command?: string

  /** Date and time at which the container was created as a Unix timestamp */
  Created?: number

  /** Port-mappings for the container. */
  Ports?: Port[]

  /** The size of files that have been created or changed by this container. */
  SizeRw?: number

  /** The total size of all files in the read-only layers from the image that the container uses. */
  SizeRootFs?: number

  /** User-defined key/value metadata. */
  Labels?: Record<string, string>

  /** The state of this container. */
  State?: ContainerSummaryStateEnum

  /** Additional human-readable status of this container (e.g. `Exit 0`) */
  Status?: string

  HostConfig?: ContainerSummaryHostConfig

  NetworkSettings?: ContainerSummaryNetworkSettings

  /** List of mounts used by the container. */
  Mounts?: MountPoint[]
}

/** Docker 镜像列表使用的稳定领域摘要。 */
export interface DockerImageSummary {
  id: string
  tags: string[]
  digests: string[]
  createdAt: number
  sizeBytes: number
  containerCount: number
  dangling: boolean
}

export type DockerImageTaskStatus = 'pending' | 'running' | 'success' | 'failed' | 'cancelled'
export type DockerImageSource = 'target' | 'controller' | 'registry'
export type DockerImageStage = 'checking' | 'exporting' | 'uploading' | 'loading' | 'pulling'

/** 单个目标节点的镜像分发状态。 */
export interface DockerImageDistributionTarget {
  nodeId: string
  nodeName: string
  status: DockerImageTaskStatus
  source?: DockerImageSource
  stage: DockerImageStage
  progressPercent: number
  errorSummary?: string
}

/** 主控镜像批量分发任务。 */
export interface DockerImageDistributionTask {
  taskId: string
  imageRef: string
  status: DockerImageTaskStatus
  createdAt: number
  targets: DockerImageDistributionTarget[]
}

export interface DockerImageDistributionCreateRequest {
  imageRef: string
  targetNodeIds: string[]
}

/** 容器操作请求参数 */
export interface ActionRequest {
  id: string
  name: string
  action: 'start' | 'stop' | 'restart' | 'remove'
}

/** 创建容器请求参数 */
export interface ContainerCreateRequest {
  name: string
  image: string
  command?: string
  env?: string
  ports?: string
  volumes?: string
  restartPolicy?: string
  network?: string
  autoRemove?: boolean
  autoStart?: boolean
}

/** 容器重命名请求 */
export interface ContainerRenameRequest {
  name: string
}

/** 容器 exec 请求 */
export interface ContainerExecRequest {
  command: string
}

/** 容器 exec 返回 */
export interface ContainerExecResult {
  exitCode?: number | null
  output: string
}

export interface ContainerTopResponse {
  titles?: string[]
  processes?: string[][]
}

export interface ContainerStatsBatchRequest {
  ids: string[]
}

export interface ContainerStatsBatchResponse {
  summaries: Record<string, ContainerResourceUsageSummary>
}

/** 容器详情 (inspect) 的简化版本 */
export interface ContainerInspect {
  Id?: string
  Name?: string
  Created?: string
  Path?: string
  Args?: string[]
  State?: {
    Status?: string
    Running?: boolean
    Paused?: boolean
    Restarting?: boolean
    OOMKilled?: boolean
    Dead?: boolean
    Pid?: number
    ExitCode?: number
    Error?: string
    StartedAt?: string
    FinishedAt?: string
  }
  Config?: {
    Hostname?: string
    Image?: string
    Cmd?: string[]
    Env?: string[]
    Labels?: Record<string, string>
    WorkingDir?: string
    User?: string
  }
  HostConfig?: {
    NetworkMode?: string
    RestartPolicy?: {
      Name?: string
      MaximumRetryCount?: number
    }
  }
  NetworkSettings?: {
    Networks?: Record<
      string,
      {
        IPAddress?: string
        Gateway?: string
        MacAddress?: string
        NetworkID?: string
      }
    >
  }
  Mounts?: Array<{
    Type?: string
    Source?: string
    Destination?: string
    Mode?: string
    RW?: boolean
    Propagation?: string
  }>
}

/** Compose 项目创建请求 */
export interface ComposeProjectCreateRequest {
  name: string
  compose: string
  dir?: string
  projectType?: string
}

/** Compose 项目删除参数 */
export interface ComposeProjectDeleteQuery {
  deleteFiles?: boolean
}

/** Compose 项目概要 */
export interface ComposeProjectSummary {
  name: string
  status: string
  totalContainers: number
  runningContainers: number
  exitedContainers: number
  pausedContainers: number
  restartingContainers: number
  hasComposeFile: boolean
  composeDir?: string
  projectType?: string
}

/** Docker daemon 镜像与代理设置。 */
export interface DockerDaemonSettings {
  registryMirrors: string[]
  proxy: string
  proxyEnabled: boolean
}

/** Compose 项目日志响应 */
export type ComposeProjectLogs = string[]

/** Docker 网络 IPAM 配置。 */
export interface DockerNetworkIpamConfig {
  subnet?: string
  gateway?: string
  ipRange?: string
}

export type DockerNetworkManagementKind = 'system' | 'compose' | 'suite' | 'custom'

/** Docker 网络的管理归属。 */
export interface DockerNetworkManagement {
  kind: DockerNetworkManagementKind
  ownerName?: string
  readOnly: boolean
}

/** Docker 网络允许执行的操作。 */
export interface DockerNetworkCapabilities {
  canRemove: boolean
  canManageConnections: boolean
}

/** Docker 网络列表项。 */
export interface DockerNetworkSummary {
  id: string
  name: string
  createdAt?: number
  driver: string
  scope: string
  enableIpv4: boolean
  enableIpv6: boolean
  internal: boolean
  attachable: boolean
  ingress: boolean
  configOnly: boolean
  subnets: string[]
  management: DockerNetworkManagement
  capabilities: DockerNetworkCapabilities
}

/** Docker 网络中的容器端点。 */
export interface DockerNetworkContainer {
  id: string
  name: string
  endpointId?: string
  macAddress?: string
  ipv4Address?: string
  ipv6Address?: string
}

/** Docker Overlay 网络的对等节点。 */
export interface DockerNetworkPeer {
  name?: string
  ip?: string
}

/** Docker 网络详情。 */
export interface DockerNetworkDetail {
  summary: DockerNetworkSummary
  ipamConfigs: DockerNetworkIpamConfig[]
  options: Record<string, string>
  labels: Record<string, string>
  containers: DockerNetworkContainer[]
  peers: DockerNetworkPeer[]
}

/** Bridge 网络创建请求。 */
export interface DockerNetworkCreateRequest {
  name: string
  internal: boolean
  enableIpv6: boolean
  ipv4?: DockerNetworkIpamConfig
  ipv6?: DockerNetworkIpamConfig
  options?: Record<string, string>
  labels?: Record<string, string>
}

/** Docker 网络创建结果。 */
export interface DockerNetworkCreateResult {
  id: string
  warning?: string
}

/** 网络连接请求 */
export interface NetworkConnectRequest {
  container: string
}

/** 网络断开请求 */
export interface NetworkDisconnectRequest {
  container: string
  force: boolean
}

/** 容器资源统计快照。 */
export interface ContainerResourceUsageSummary {
  cpuCorePercent: number
  memoryWorkingSetBytes: number
  memoryLimitBytes: number
  memoryPercent: number
  networkRxBytes: number
  networkTxBytes: number
}

export type ResourceSampleStatus = 'fresh' | 'partial' | 'stale' | 'unavailable'

/** 宿主机 Docker 资源汇总。 */
export interface HostResourceUsageSummary {
  status: ResourceSampleStatus
  collectedAt: number | null
  runningContainerCount: number
  sampledContainerCount: number
  cpuHostPercent: number
  cpuCorePercent: number
  memoryWorkingSetBytes: number
  memoryLimitBytes: number
  memoryPercent: number
}

/** 容器资源趋势点。 */
export interface ContainerResourceUsagePoint {
  timestamp: number
  cpuCorePercent: number
  memoryWorkingSetBytes: number
  memoryLimitBytes: number
  memoryPercent: number
  networkRxBytesPerSecond: number | null
  networkTxBytesPerSecond: number | null
}

export interface ContainerResourceUsageHistory {
  points: ContainerResourceUsagePoint[]
}

export interface ContainerStatsHistoryAllItem {
  id: string
  name: string
  points: ContainerResourceUsagePoint[]
}

export interface ContainerStatsHistoryAllResponse {
  containers: ContainerStatsHistoryAllItem[]
}

export interface ContainerStateCounts {
  total: number
  running: number
  paused: number
  restarting: number
  exited: number
  other: number
}

export interface ProjectStateCounts {
  total: number
  healthy: number
  partial: number
  stopped: number
  unknown: number
}

export interface ImageCounts {
  total: number
  dangling: number
}

export interface TrendContainerItem {
  id: string
  name: string
  createdAt: number
  state: string
}

export interface OverviewRealtimeResponse {
  collectedAt: number
  containerStates: ContainerStateCounts
  projectStates: ProjectStateCounts
  images: ImageCounts
  resourceUsage: HostResourceUsageSummary
  trendContainers: TrendContainerItem[]
}

export interface ContainerStatsHistoryQuery {
  ids: string[]
  hours?: number
}

export interface DockerDiskUsageCategory {
  totalCount: number
  activeCount: number
  sizeBytes: number
  reclaimableBytes: number
}

export interface DockerDiskUsageSummary {
  collectedAt: number
  images: DockerDiskUsageCategory
  containers: DockerDiskUsageCategory
  volumes: DockerDiskUsageCategory
  buildCache: DockerDiskUsageCategory
}

/** 容器引用 */
export interface ContainerRef {
  id: string
  name: string
}

/** Docker 系统信息 */
export interface DockerSystemInfo {
  ServerVersion?: string
  Driver?: string
  KernelVersion?: string
  OperatingSystem?: string
  OSType?: string
  Architecture?: string
  NCPU?: number
  MemTotal?: number
  Images?: number
  Containers?: number
  ContainersRunning?: number
  ContainersPaused?: number
  ContainersStopped?: number
}

export type DockerVolumeManagementKind = 'suite' | 'compose' | 'custom'

/** Docker 卷归属信息。 */
export interface DockerVolumeManagement {
  kind: DockerVolumeManagementKind
  ownerName?: string
  readOnly: boolean
}

/** Docker 卷在当前模块允许执行的操作。 */
export interface DockerVolumeCapabilities {
  canRemove: boolean
}

/** Docker 卷列表摘要。 */
export interface DockerVolumeSummary {
  name: string
  createdAt?: number
  management: DockerVolumeManagement
  capabilities: DockerVolumeCapabilities
}

/** Docker 卷列表响应。 */
export interface DockerVolumeListResponse {
  items: DockerVolumeSummary[]
  warnings: string[]
}

/** 引用 Docker 卷的容器挂载信息。 */
export interface DockerVolumeContainerReference {
  id: string
  name: string
  state: string
  destination?: string
  readOnly: boolean
}

/** Docker 卷详情。 */
export interface DockerVolumeDetail {
  summary: DockerVolumeSummary
  mountpoint: string
  scope: string
  options: Record<string, string>
  labels: Record<string, string>
  referencedContainers: DockerVolumeContainerReference[]
}

/** Docker 本地卷创建请求。 */
export interface DockerVolumeCreateRequest {
  name: string
  options?: Record<string, string>
  labels?: Record<string, string>
}

/** Compose 服务容器 (通过标签筛选) */
export interface ComposeContainerSummary extends ContainerSummary {
  projectName: string
  serviceName: string
}

export type DockerActivityActorKind = 'user' | 'system'
export type DockerActivityLevel = 'info' | 'warning' | 'error'
export type DockerActivityOutcome = 'success' | 'failure'

export interface DockerActivityActor {
  kind: DockerActivityActorKind
  name: string
  clientIp?: string
}

export interface DockerActivityTarget {
  kind: string
  id: string
}

export interface DockerActivityLogItem {
  id: number
  occurredAt: number
  actor: DockerActivityActor
  level: DockerActivityLevel
  outcome: DockerActivityOutcome
  eventCode: string
  target?: DockerActivityTarget
  messageParams: Record<string, unknown>
  errorMessage?: string
  traceId?: string
}

export interface DockerActivityLogQuery {
  page: number
  pageSize: number
  levels?: DockerActivityLevel[]
  actorKinds?: DockerActivityActorKind[]
  startAt?: number
  endAt?: number
  keyword?: string
}

export interface DockerActivityLogPage {
  total: number
  page: number
  pageSize: number
  items: DockerActivityLogItem[]
}
