/**
 * Docker 状态管理 Store
 *
 * 统一管理 Docker 全部领域数据、数据获取、轮询定时器和业务操作。
 * 替代原 DockerManagerView.vue 中散落的 30+ ref 状态和 20+ 业务函数。
 */

import { ref, computed } from 'vue'
import { defineStore } from 'pinia'
import { useI18n } from 'vue-i18n'
import { dockerApi } from '@/api/modules/docker'
import type * as dockerType from '@/api/interface/docker'
import { useNotificationStore } from '@/stores/notification'
import { useConfirmationModalStore } from '@/stores/confirmation-modal'
import { useNodeStore } from '@/stores/node'
import { useWindowManagerStore } from '@/stores/window-manager'
import { load as parseYaml } from 'js-yaml'

/** 容器资源统计缓存条目 */
export interface ContainerStatsEntry {
  data: dockerType.ContainerResourceUsageSummary
  fetchedAt: number
}

/** 概览数据缓存 */
interface OverviewCacheEntry {
  timestamp: number
}

/** 容器资源统计缓存 TTL (毫秒) */
const CONTAINER_STATS_TTL = 60_000
/** 概览缓存 TTL (毫秒) */
const OVERVIEW_CACHE_TTL = 30_000
/** 概览最大可选容器数 */
const MAX_OVERVIEW_CONTAINERS = 5

const emptyContainerStates = (): dockerType.ContainerStateCounts => ({
  total: 0,
  running: 0,
  paused: 0,
  restarting: 0,
  exited: 0,
  other: 0,
})

const emptyProjectStates = (): dockerType.ProjectStateCounts => ({
  total: 0,
  healthy: 0,
  partial: 0,
  stopped: 0,
  unknown: 0,
})

export const useDockerStore = defineStore('docker', () => {
  const { t } = useI18n()
  const notificationStore = useNotificationStore()
  const modalStore = useConfirmationModalStore()
  const nodeStore = useNodeStore()
  const windowStore = useWindowManagerStore()

  // ─── Docker 客户端 (响应节点切换) ───
  const dockerClient = computed(() => dockerApi.forNode(nodeStore.currentNodeId))

  // ─── 核心状态 ───
  const dockerAvailable = ref<boolean>(true)
  const dockerStatus = ref<boolean>(false)
  const dockerStatusCode = ref<string>('unknown')
  const isLoading = ref<boolean>(true)

  // ─── 概览统计 ───
  const overviewCollectedAt = ref<number | null>(null)
  const overviewError = ref<string | null>(null)
  const containerStates = ref<dockerType.ContainerStateCounts>(emptyContainerStates())
  const projectStates = ref<dockerType.ProjectStateCounts>(emptyProjectStates())
  const imageCounts = ref<dockerType.ImageCounts>({ total: 0, dangling: 0 })
  const resourceUsage = ref<dockerType.HostResourceUsageSummary | null>(null)

  // ─── 概览图表数据 ───
  const overviewContainers = ref<dockerType.TrendContainerItem[]>([])
  const overviewSelectedContainerIds = ref<string[]>([])
  const overviewHistoryMap = ref<Record<string, dockerType.ContainerResourceUsageHistory>>({})
  const overviewHistoryLatestMap = ref<Record<string, number | null>>({})
  const overviewHistoryHours = ref<1 | 6 | 12>(1)
  const overviewHistoryError = ref<string | null>(null)

  // ─── 容器列表 ───
  const containers = ref<dockerType.ContainerSummary[]>([])
  const containerResourceStats = ref<Record<string, ContainerStatsEntry>>({})
  const isContainerDetailActive = ref(false)

  // ─── Compose 项目 ───
  const composeProjects = ref<dockerType.ComposeProjectSummary[]>([])

  // ─── 镜像 / 网络 / 卷 / 系统 ───
  const imagesList = ref<dockerType.DockerImageSummary[]>([])
  const imageListLoading = ref(false)
  const imageListError = ref<string | null>(null)
  const imageListLoadedAt = ref<number | null>(null)
  const imageDeleteLoadingId = ref<string | null>(null)
  const controllerImages = ref<dockerType.DockerImageSummary[]>([])
  const controllerImageLoading = ref(false)
  const controllerImageError = ref<string | null>(null)
  const controllerImageLoadedAt = ref<number | null>(null)
  const imageDistributionTask = ref<dockerType.DockerImageDistributionTask | null>(null)
  const imageDistributionLoading = ref(false)
  const imageDistributionError = ref<string | null>(null)
  const imageDistributionStarting = ref(false)
  const imageDistributionCanceling = ref(false)
  const networks = ref<dockerType.DockerNetworkSummary[]>([])
  const networkListLoading = ref(false)
  const networkListError = ref<string | null>(null)
  const networkListLoadedAt = ref<number | null>(null)
  const networkDetail = ref<dockerType.DockerNetworkDetail | null>(null)
  const networkDetailLoading = ref(false)
  const networkDetailError = ref<string | null>(null)
  const networkCreateLoading = ref(false)
  const networkDeleteLoadingId = ref<string | null>(null)
  const networkConnectLoading = ref(false)
  const networkDisconnectLoading = ref<Record<string, boolean>>({})
  const volumes = ref<dockerType.DockerVolumeSummary[]>([])
  const volumeWarnings = ref<string[]>([])
  const volumeListLoading = ref(false)
  const volumeListError = ref<string | null>(null)
  const volumeDetail = ref<dockerType.DockerVolumeDetail | null>(null)
  const volumeDetailLoading = ref(false)
  const volumeDetailError = ref<string | null>(null)
  const volumeCreateLoading = ref(false)
  const volumeCreateError = ref<string | null>(null)
  const volumeDeleteLoadingName = ref<string | null>(null)
  const systemInfo = ref<dockerType.DockerSystemInfo | null>(null)
  const systemInfoLoading = ref(false)
  const systemInfoError = ref<string | null>(null)
  const systemInfoLoadedAt = ref<number | null>(null)
  const diskUsage = ref<dockerType.DockerDiskUsageSummary | null>(null)
  const diskUsageLoading = ref(false)
  const diskUsageError = ref<string | null>(null)
  const pruneLoading = ref(false)
  const composeContainers = ref<dockerType.ContainerSummary[]>([])
  const projectLogs = ref<string[]>([])

  // ─── 容器创建表单 ───
  const isContainerCreateActive = ref(false)
  const containerStep = ref<'selectImage' | 'config'>('selectImage')
  const selectedImageId = ref<string | null>(null)
  const containerForm = ref({
    name: '',
    command: '',
    env: '',
    ports: '',
    volumes: '',
    restartPolicy: 'no',
    network: '',
    autoRemove: false,
  })

  // ─── 项目创建表单 ───
  const isProjectCreateActive = ref(false)
  const projectFormName = ref('')
  const projectFormDir = ref('')
  const projectFormCompose = ref(
    `services:\n  nginx:\n    image:  nginx:latest\n    container_name: nginx\n    ports:\n      - "8080:80"\n`,
  )
  const composeYamlError = ref('')

  // ─── 缓存与防并发 ───
  const overviewCache = ref<OverviewCacheEntry | null>(null)
  const overviewRefreshInProgress = ref(false)
  const historyRefreshInProgress = ref(false)
  let historyRefreshQueued = false
  let imageListRequestSequence = 0
  let imageNodeId: string | null = null
  let controllerImageRequestSequence = 0
  let imageDistributionRequestSequence = 0
  let networkListRequestSequence = 0
  let networkDetailRequestSequence = 0
  let networkNodeId: string | null = null
  let volumeListRequestSequence = 0
  let volumeDetailRequestSequence = 0
  let volumeNodeId: string | null = null

  // ─── 轮询定时器 ───
  let resourceUsageTimer: number | null = null
  let resourceHistoryTimer: number | null = null
  let containerStatsPollingTimer: number | null = null

  // ─── 容器统计批量队列 ───
  let containerStatsTimer: number | null = null
  const pendingContainerStatIds = new Set<string>()

  // ─── 选中镜像标签 (计算属性) ───
  const selectedImageLabel = computed(() => {
    const img = imagesList.value.find((i) => i.id === selectedImageId.value)
    if (!img) return ''
    return img.tags[0] || img.id.replace(/^sha256:/, '').substring(0, 12)
  })

  // ========================================
  // 数据重置
  // ========================================

  /** 重置所有 Docker 数据到初始状态 */
  const resetAll = () => {
    dockerStatus.value = false
    overviewCollectedAt.value = null
    overviewError.value = null
    containerStates.value = emptyContainerStates()
    projectStates.value = emptyProjectStates()
    imageCounts.value = { total: 0, dangling: 0 }
    resourceUsage.value = null
    overviewContainers.value = []
    overviewSelectedContainerIds.value = []
    overviewHistoryMap.value = {}
    overviewHistoryLatestMap.value = {}
    overviewHistoryHours.value = 1
    overviewHistoryError.value = null
    containers.value = []
    containerResourceStats.value = {}
    composeProjects.value = []
    imagesList.value = []
    imageListLoading.value = false
    imageListError.value = null
    imageListLoadedAt.value = null
    imageDeleteLoadingId.value = null
    imageNodeId = null
    imageListRequestSequence += 1
    controllerImages.value = []
    controllerImageLoading.value = false
    controllerImageError.value = null
    controllerImageLoadedAt.value = null
    controllerImageRequestSequence += 1
    imageDistributionTask.value = null
    imageDistributionLoading.value = false
    imageDistributionError.value = null
    imageDistributionStarting.value = false
    imageDistributionCanceling.value = false
    imageDistributionRequestSequence += 1
    networks.value = []
    networkListLoading.value = false
    networkListError.value = null
    networkListLoadedAt.value = null
    networkDetail.value = null
    networkDetailLoading.value = false
    networkDetailError.value = null
    networkCreateLoading.value = false
    networkDeleteLoadingId.value = null
    networkConnectLoading.value = false
    networkDisconnectLoading.value = {}
    networkNodeId = null
    networkListRequestSequence += 1
    networkDetailRequestSequence += 1
    volumes.value = []
    volumeWarnings.value = []
    volumeListLoading.value = false
    volumeListError.value = null
    volumeDetail.value = null
    volumeDetailLoading.value = false
    volumeDetailError.value = null
    volumeCreateLoading.value = false
    volumeCreateError.value = null
    volumeDeleteLoadingName.value = null
    volumeNodeId = null
    volumeListRequestSequence += 1
    volumeDetailRequestSequence += 1
    systemInfo.value = null
    systemInfoLoading.value = false
    systemInfoError.value = null
    systemInfoLoadedAt.value = null
    diskUsage.value = null
    diskUsageLoading.value = false
    diskUsageError.value = null
    pruneLoading.value = false
    composeContainers.value = []
    projectLogs.value = []
    overviewCache.value = null
  }

  // ========================================
  // 数据获取
  // ========================================

  /** 检测 Docker 可用性 */
  const fetchDockerAvailability = async (): Promise<boolean> => {
    const res = await dockerClient.value.fetchStatus()
    if (res.success && res.data) {
      const payload = res.data as { dockerAvailable?: boolean; dockerStatus?: string }
      dockerAvailable.value = Boolean(payload.dockerAvailable)
      dockerStatus.value = dockerAvailable.value
      dockerStatusCode.value = payload.dockerStatus || 'unknown'
      return dockerAvailable.value
    }
    dockerAvailable.value = false
    dockerStatus.value = false
    dockerStatusCode.value = 'unknown'
    return false
  }

  /** 获取概览数据并更新状态 */
  const fetchOverviewData = async (): Promise<dockerType.OverviewRealtimeResponse | null> => {
    if (!dockerAvailable.value) return null
    const res = await dockerClient.value.fetchOverviewRealtime()
    if (!res.success || !res.data) {
      overviewError.value = t('app.docker.messages.overviewFailed', {
        message: res.message || t('common.unknownError'),
      })
      resourceUsage.value = resourceUsage.value
        ? { ...resourceUsage.value, status: 'unavailable' }
        : null
      return null
    }
    const payload = res.data
    overviewError.value = null
    overviewCollectedAt.value = payload.collectedAt
    containerStates.value = payload.containerStates
    projectStates.value = payload.projectStates
    imageCounts.value = payload.images
    resourceUsage.value = payload.resourceUsage
    updateOverviewContainerState(payload.trendContainers || [])
    return payload
  }

  /** 获取当前选中容器的历史资源数据。 */
  const fetchOverviewHistoryAll = async (hours = overviewHistoryHours.value) => {
    if (!dockerAvailable.value) return
    if (historyRefreshInProgress.value) {
      historyRefreshQueued = true
      return
    }
    const ids = overviewSelectedContainerIds.value.slice(0, MAX_OVERVIEW_CONTAINERS)
    const requestKey = `${hours}:${ids.join(',')}`
    if (ids.length === 0) {
      overviewHistoryMap.value = {}
      overviewHistoryLatestMap.value = {}
      overviewHistoryError.value = null
      return
    }
    historyRefreshInProgress.value = true
    try {
      const res = await dockerClient.value.fetchContainerResourceUsageHistoryAll({ ids, hours })
      if (!res.success || !res.data) {
        overviewHistoryError.value = res.message || t('common.unknownError')
        return
      }
      overviewHistoryError.value = null
      const nextItems: Record<string, dockerType.ContainerResourceUsageHistory> = {}
      const nextLatestMap: Record<string, number | null> = {}
      for (const item of res.data.containers || []) {
        nextItems[item.id] = { points: item.points }
        const points = item.points || []
        nextLatestMap[item.id] = points.length ? points[points.length - 1]!.timestamp : null
      }
      const currentKey = `${overviewHistoryHours.value}:${overviewSelectedContainerIds.value
        .slice(0, MAX_OVERVIEW_CONTAINERS)
        .join(',')}`
      if (requestKey !== currentKey) {
        historyRefreshQueued = true
        return
      }
      overviewHistoryLatestMap.value = nextLatestMap
      overviewHistoryMap.value = nextItems
    } finally {
      historyRefreshInProgress.value = false
      if (historyRefreshQueued) {
        historyRefreshQueued = false
        void fetchOverviewHistoryAll()
      }
    }
  }

  /** 更新概览趋势容器，并保留仍存在但已停止的选择。 */
  const updateOverviewContainerState = (items: dockerType.TrendContainerItem[]) => {
    overviewContainers.value = items
    const availableIds = new Set(items.map((item) => item.id))
    const retained = overviewSelectedContainerIds.value.filter((id) => availableIds.has(id))
    const preferred = [
      ...items.filter((item) => item.state.toLowerCase() === 'running'),
      ...items.filter((item) => item.state.toLowerCase() !== 'running'),
    ]
    const nextSelected =
      retained.length > 0
        ? retained.slice(0, MAX_OVERVIEW_CONTAINERS)
        : preferred.slice(0, MAX_OVERVIEW_CONTAINERS).map((item) => item.id)
    const selectionChanged = nextSelected.join(',') !== overviewSelectedContainerIds.value.join(',')
    overviewSelectedContainerIds.value = nextSelected
    if (selectionChanged) void fetchOverviewHistoryAll()
  }

  /** 更新概览选中的容器列表。 */
  const updateOverviewSelectedContainers = (ids: string[]) => {
    const unique = Array.from(new Set(ids))
    let nextIds = unique
    if (unique.length > MAX_OVERVIEW_CONTAINERS) {
      notificationStore.error(
        t('app.docker.messages.maxOverviewContainers', { count: MAX_OVERVIEW_CONTAINERS }),
      )
      nextIds = unique.slice(0, MAX_OVERVIEW_CONTAINERS)
    }
    overviewSelectedContainerIds.value = nextIds
    void fetchOverviewHistoryAll()
  }

  /** 更新资源趋势时间范围。 */
  const setOverviewHistoryHours = (hours: 1 | 6 | 12) => {
    if (overviewHistoryHours.value === hours) return
    overviewHistoryHours.value = hours
    void fetchOverviewHistoryAll(hours)
  }

  /** 将概览数据写入缓存 */
  const cacheOverviewState = () => {
    overviewCache.value = { timestamp: Date.now() }
  }

  /** 统一加载概览数据 */
  const loadOverviewData = async ({ showLoading }: { showLoading: boolean }) => {
    if (overviewRefreshInProgress.value) return
    if (showLoading) isLoading.value = true
    overviewRefreshInProgress.value = true
    try {
      const overviewResult = await fetchOverviewData()
      if (overviewResult) cacheOverviewState()
    } finally {
      overviewRefreshInProgress.value = false
      if (showLoading) isLoading.value = false
    }
  }

  /** 按需刷新概览数据 (缓存未过期则跳过) */
  const refreshOverviewDataIfNeeded = async (): Promise<'loaded' | 'cached' | 'skipped'> => {
    if (!dockerAvailable.value || overviewRefreshInProgress.value) return 'skipped'
    const cacheValid =
      overviewCache.value && Date.now() - overviewCache.value.timestamp < OVERVIEW_CACHE_TTL
    if (cacheValid) return 'cached'
    await loadOverviewData({ showLoading: true })
    return 'loaded'
  }

  /** 获取所有容器列表 */
  const fetchContainers = async () => {
    if (!dockerAvailable.value) return
    const res = await dockerClient.value.listContainers()
    if (!res.success) {
      notificationStore.error(
        t('app.docker.messages.listContainersFailed', {
          message: res.message || t('common.unknownError'),
        }),
      )
      containers.value = []
      return
    }
    containers.value = res.data || []
    const runningIds = (res.data || [])
      .filter((c) => c.State === 'running' && c.Id)
      .map((c) => c.Id!)
    if (runningIds.length > 0 && !isContainerDetailActive.value) {
      void queueContainerStatsFetch(runningIds)
    }
  }

  /** 获取 Compose 项目列表 */
  const fetchComposeProjects = async () => {
    if (!dockerAvailable.value) return
    const res = await dockerClient.value.listComposeProjects()
    if (res.success) {
      composeProjects.value = res.data || []
    } else {
      notificationStore.error(
        t('app.docker.messages.listProjectsFailed', {
          message: res.message || t('common.unknownError'),
        }),
      )
      composeProjects.value = []
    }
  }

  /** 获取当前节点镜像列表，并阻止旧节点响应覆盖当前数据。 */
  const fetchImagesList = async (): Promise<boolean> => {
    if (!dockerAvailable.value) return false
    const nodeId = nodeStore.currentNodeId || 'local'
    if (imageListLoading.value && imageNodeId === nodeId) return false
    if (imageNodeId !== nodeId) {
      imagesList.value = []
      imageListLoadedAt.value = null
      imageNodeId = nodeId
    }
    const sequence = ++imageListRequestSequence
    imageListLoading.value = true
    imageListError.value = null
    try {
      const res = await dockerClient.value.listImages()
      if (
        sequence !== imageListRequestSequence ||
        nodeId !== (nodeStore.currentNodeId || 'local')
      ) {
        return false
      }
      if (!res.success) {
        imageListError.value = res.message || t('common.unknownError')
        return false
      }
      imagesList.value = res.data || []
      imageListLoadedAt.value = Date.now()
      return true
    } catch (error) {
      if (sequence === imageListRequestSequence) {
        imageListError.value = error instanceof Error ? error.message : String(error)
      }
      return false
    } finally {
      if (sequence === imageListRequestSequence) imageListLoading.value = false
    }
  }

  /** 获取主控镜像库。 */
  const fetchControllerImages = async (): Promise<boolean> => {
    if (controllerImageLoading.value) return false
    const sequence = ++controllerImageRequestSequence
    controllerImageLoading.value = true
    controllerImageError.value = null
    try {
      const res = await dockerApi.fetchControllerImages()
      if (sequence !== controllerImageRequestSequence) return false
      if (!res.success) {
        controllerImageError.value = res.message || t('common.unknownError')
        return false
      }
      controllerImages.value = res.data || []
      controllerImageLoadedAt.value = Date.now()
      return true
    } catch (error) {
      if (sequence === controllerImageRequestSequence) {
        controllerImageError.value = error instanceof Error ? error.message : String(error)
      }
      return false
    } finally {
      if (sequence === controllerImageRequestSequence) controllerImageLoading.value = false
    }
  }

  /** 恢复保留期内最近的未结束分发任务。 */
  const fetchRecentImageDistributionTask = async (): Promise<boolean> => {
    if (imageDistributionLoading.value) return false
    const sequence = ++imageDistributionRequestSequence
    imageDistributionLoading.value = true
    imageDistributionError.value = null
    try {
      const res = await dockerApi.fetchRecentImageDistributionTasks()
      if (sequence !== imageDistributionRequestSequence) return false
      if (!res.success) {
        imageDistributionError.value = res.message || t('common.unknownError')
        return false
      }
      const tasks = res.data || []
      imageDistributionTask.value =
        tasks.find((task) => ['pending', 'running'].includes(task.status)) || null
      return true
    } catch (error) {
      if (sequence === imageDistributionRequestSequence) {
        imageDistributionError.value = error instanceof Error ? error.message : String(error)
      }
      return false
    } finally {
      if (sequence === imageDistributionRequestSequence) imageDistributionLoading.value = false
    }
  }

  /** 创建主控镜像批量分发任务。 */
  const startImageDistribution = async (
    payload: dockerType.DockerImageDistributionCreateRequest,
  ): Promise<boolean> => {
    if (imageDistributionStarting.value) return false
    const sequence = ++imageDistributionRequestSequence
    imageDistributionLoading.value = false
    imageDistributionStarting.value = true
    imageDistributionError.value = null
    try {
      const res = await dockerApi.startImageDistributionTask(payload)
      if (sequence !== imageDistributionRequestSequence) return false
      if (!res.success || !res.data) {
        imageDistributionError.value = res.message || t('common.unknownError')
        return false
      }
      imageDistributionTask.value = res.data
      return true
    } catch (error) {
      if (sequence === imageDistributionRequestSequence) {
        imageDistributionError.value = error instanceof Error ? error.message : String(error)
      }
      return false
    } finally {
      if (sequence === imageDistributionRequestSequence) imageDistributionStarting.value = false
    }
  }

  /** 刷新当前批量分发任务，禁止轮询请求重叠。 */
  const fetchImageDistributionTask = async (): Promise<boolean> => {
    const taskId = imageDistributionTask.value?.taskId
    if (
      !taskId ||
      imageDistributionLoading.value ||
      imageDistributionStarting.value ||
      imageDistributionCanceling.value
    ) {
      return false
    }
    const sequence = ++imageDistributionRequestSequence
    imageDistributionLoading.value = true
    try {
      const res = await dockerApi.fetchImageDistributionTask(taskId)
      if (sequence !== imageDistributionRequestSequence) return false
      if (!res.success || !res.data) {
        imageDistributionError.value = res.message || t('common.unknownError')
        return false
      }
      imageDistributionTask.value = res.data
      imageDistributionError.value = null
      return true
    } catch (error) {
      if (sequence === imageDistributionRequestSequence) {
        imageDistributionError.value = error instanceof Error ? error.message : String(error)
      }
      return false
    } finally {
      if (sequence === imageDistributionRequestSequence) imageDistributionLoading.value = false
    }
  }

  /** 取消当前批量分发任务。 */
  const cancelImageDistribution = async (): Promise<boolean> => {
    const taskId = imageDistributionTask.value?.taskId
    if (!taskId || imageDistributionCanceling.value) return false
    const sequence = ++imageDistributionRequestSequence
    imageDistributionLoading.value = false
    imageDistributionCanceling.value = true
    try {
      const res = await dockerApi.cancelImageDistributionTask(taskId)
      if (sequence !== imageDistributionRequestSequence) return false
      if (!res.success || !res.data) {
        imageDistributionError.value = res.message || t('common.unknownError')
        return false
      }
      imageDistributionTask.value = res.data
      return true
    } catch (error) {
      if (sequence === imageDistributionRequestSequence) {
        imageDistributionError.value = error instanceof Error ? error.message : String(error)
      }
      return false
    } finally {
      if (sequence === imageDistributionRequestSequence) imageDistributionCanceling.value = false
    }
  }

  /** 离开镜像分发页面时废弃在途请求，并清除页面级终态与错误。 */
  const leaveImageDistributionView = () => {
    imageDistributionRequestSequence += 1
    imageDistributionLoading.value = false
    imageDistributionError.value = null
    imageDistributionStarting.value = false
    imageDistributionCanceling.value = false
    if (
      imageDistributionTask.value &&
      ['success', 'failed', 'cancelled'].includes(imageDistributionTask.value.status)
    ) {
      imageDistributionTask.value = null
    }
  }

  /** 获取网络摘要列表，并阻止旧节点响应覆盖当前状态。 */
  const fetchNetworks = async (): Promise<boolean> => {
    if (!dockerAvailable.value) return false
    const nodeId = nodeStore.currentNodeId || 'local'
    if (networkNodeId !== nodeId) {
      networks.value = []
      networkListLoadedAt.value = null
      networkDetail.value = null
      networkDetailLoading.value = false
      networkDetailError.value = null
      networkDetailRequestSequence += 1
      networkNodeId = nodeId
    }
    const sequence = ++networkListRequestSequence
    networkListLoading.value = true
    networkListError.value = null
    const res = await dockerClient.value.listNetworks()
    if (
      sequence !== networkListRequestSequence ||
      nodeId !== (nodeStore.currentNodeId || 'local')
    ) {
      return false
    }
    networkListLoading.value = false
    if (res.success && res.data) {
      networks.value = res.data
      networkListLoadedAt.value = Date.now()
      return true
    }
    networkListError.value = t('app.docker.messages.listNetworksFailed', {
      message: res.message || t('common.unknownError'),
    })
    return false
  }

  /** 获取卷摘要列表，并阻止旧节点响应覆盖当前状态。 */
  const fetchVolumes = async (): Promise<boolean> => {
    if (!dockerAvailable.value) return false
    const nodeId = nodeStore.currentNodeId || 'local'
    if (volumeNodeId !== nodeId) {
      volumes.value = []
      volumeWarnings.value = []
      volumeDetail.value = null
      volumeDetailLoading.value = false
      volumeDetailError.value = null
      volumeDetailRequestSequence += 1
      volumeNodeId = nodeId
    }
    const sequence = ++volumeListRequestSequence
    volumeListLoading.value = true
    volumeListError.value = null
    const res = await dockerClient.value.listVolumes()
    if (sequence !== volumeListRequestSequence || nodeId !== (nodeStore.currentNodeId || 'local')) {
      return false
    }
    volumeListLoading.value = false
    if (res.success && res.data) {
      volumes.value = res.data.items
      volumeWarnings.value = res.data.warnings
      return true
    }
    volumeListError.value = t('app.docker.messages.listVolumesFailed', {
      message: res.message || t('common.unknownError'),
    })
    return false
  }

  /** 按需获取卷详情和引用容器。 */
  const fetchVolumeDetail = async (name: string): Promise<boolean> => {
    const nodeId = nodeStore.currentNodeId || 'local'
    const sequence = ++volumeDetailRequestSequence
    volumeDetailLoading.value = true
    volumeDetailError.value = null
    const res = await dockerClient.value.inspectVolume(name)
    if (
      sequence !== volumeDetailRequestSequence ||
      nodeId !== (nodeStore.currentNodeId || 'local')
    ) {
      return false
    }
    volumeDetailLoading.value = false
    if (res.success && res.data) {
      volumeDetail.value = res.data
      return true
    }
    volumeDetailError.value = res.message || t('common.unknownError')
    return false
  }

  /** 清除卷详情并使尚未返回的详情请求失效。 */
  const clearVolumeDetail = (): void => {
    volumeDetailRequestSequence += 1
    volumeDetail.value = null
    volumeDetailLoading.value = false
    volumeDetailError.value = null
  }

  /** 获取 Docker 系统信息 */
  const fetchDockerInfo = async () => {
    if (!dockerAvailable.value) return
    systemInfoLoading.value = true
    systemInfoError.value = null
    try {
      const res = await dockerClient.value.fetchInfo()
      if (res.success && res.data) {
        systemInfo.value = res.data as dockerType.DockerSystemInfo
        systemInfoLoadedAt.value = Math.floor(Date.now() / 1000)
      } else {
        systemInfo.value = null
        systemInfoLoadedAt.value = null
        systemInfoError.value = res.message || t('common.unknownError')
      }
    } finally {
      systemInfoLoading.value = false
    }
  }

  /** 获取 Docker 磁盘使用与可回收空间。 */
  const fetchDockerDiskUsage = async () => {
    if (!dockerAvailable.value) return
    diskUsageLoading.value = true
    diskUsageError.value = null
    try {
      const res = await dockerClient.value.dfSystem()
      if (res.success && res.data) {
        diskUsage.value = res.data
      } else {
        diskUsage.value = null
        diskUsageError.value = res.message || t('common.unknownError')
      }
    } finally {
      diskUsageLoading.value = false
    }
  }

  /** 获取 Compose 关联容器列表 */
  const fetchComposeContainers = async () => {
    if (!dockerAvailable.value) return
    const res = await dockerClient.value.listProjectContainers()
    if (res.success) {
      composeContainers.value = res.data || []
    } else {
      composeContainers.value = []
    }
  }

  /** 获取 Compose 项目日志 */
  const fetchProjectLogs = async (name: string) => {
    if (!dockerAvailable.value) return
    const res = await dockerClient.value.fetchComposeProjectLogs(name)
    if (res.success && res.data) {
      projectLogs.value = res.data
    } else {
      projectLogs.value = []
    }
  }

  /** 单容器资源统计 (带 TTL 缓存) */
  const fetchContainerResourceStats = async (id: string) => {
    if (!dockerAvailable.value) return
    const now = Date.now()
    const existing = containerResourceStats.value[id]
    if (existing && now - existing.fetchedAt < CONTAINER_STATS_TTL) return
    void queueContainerStatsFetch([id])
  }

  /** 批量容器资源统计 (防抖队列) */
  const queueContainerStatsFetch = async (ids: string[]) => {
    if (!dockerAvailable.value) return
    ids.forEach((id) => pendingContainerStatIds.add(id))
    if (containerStatsTimer !== null) return
    containerStatsTimer = window.setTimeout(async () => {
      const batchIds = Array.from(pendingContainerStatIds)
      pendingContainerStatIds.clear()
      containerStatsTimer = null
      if (batchIds.length === 0) return
      const res = await dockerClient.value.fetchContainerResourceUsageSummaries({ ids: batchIds })
      if (res.success && res.data) {
        const now = Date.now()
        const updates = { ...containerResourceStats.value }
        for (const [id, summary] of Object.entries(res.data.summaries || {})) {
          updates[id] = { data: summary, fetchedAt: now }
        }
        containerResourceStats.value = updates
      }
    }, 150)
  }

  // ========================================
  // 轮询管理
  // ========================================

  /** 启动概览数据轮询 */
  const startOverviewPolling = () => {
    if (!dockerAvailable.value || resourceUsageTimer !== null) return
    resourceUsageTimer = window.setInterval(() => {
      void loadOverviewData({ showLoading: false })
    }, 30_000)
  }

  /** 停止概览数据轮询 */
  const stopOverviewPolling = () => {
    if (resourceUsageTimer === null) return
    window.clearInterval(resourceUsageTimer)
    resourceUsageTimer = null
  }

  /** 启动历史数据轮询 */
  const startHistoryPolling = () => {
    if (!dockerAvailable.value || resourceHistoryTimer !== null) return
    resourceHistoryTimer = window.setInterval(() => {
      void fetchOverviewHistoryAll()
    }, 60_000)
  }

  /** 停止历史数据轮询 */
  const stopHistoryPolling = () => {
    if (resourceHistoryTimer === null) return
    window.clearInterval(resourceHistoryTimer)
    resourceHistoryTimer = null
  }

  /** 启动容器统计轮询 */
  const startContainerStatsPolling = (activeMenuGetter: () => string) => {
    if (!dockerAvailable.value || containerStatsPollingTimer !== null) return
    containerStatsPollingTimer = window.setInterval(() => {
      if (activeMenuGetter() !== 'containers') return
      if (isContainerDetailActive.value) return
      const runningIds = containers.value
        .filter((c) => c.State === 'running' && c.Id)
        .map((c) => c.Id!)
      if (runningIds.length > 0) void queueContainerStatsFetch(runningIds)
    }, CONTAINER_STATS_TTL)
  }

  /** 停止容器统计轮询 */
  const stopContainerStatsPolling = () => {
    if (containerStatsPollingTimer === null) return
    window.clearInterval(containerStatsPollingTimer)
    containerStatsPollingTimer = null
  }

  /** 停止所有轮询 */
  const stopAllPolling = () => {
    stopOverviewPolling()
    stopHistoryPolling()
    stopContainerStatsPolling()
  }

  // ========================================
  // 初始加载
  // ========================================

  /** 初始加载流程 */
  const initialLoad = async () => {
    const available = await fetchDockerAvailability()
    if (!available) {
      resetAll()
      isLoading.value = false
      return
    }
    await loadOverviewData({ showLoading: true })
  }

  // ========================================
  // 业务操作
  // ========================================

  /** 容器操作 (支持批量) */
  const handleContainerAction = async (
    idOrIds: string | null | (string | null)[],
    nameOrNames: string | null | (string | null)[],
    action: 'start' | 'stop' | 'restart' | 'remove' | 'pause' | 'unpause' | 'kill',
  ) => {
    const ids = Array.isArray(idOrIds) ? idOrIds : [idOrIds]
    const names = Array.isArray(nameOrNames) ? nameOrNames : [nameOrNames]
    const validTargets = ids
      .map((id, index) => ({ id, name: names[index] }))
      .filter((item) => item.id && item.name)
    if (validTargets.length === 0) {
      notificationStore.error(t('app.docker.messages.invalidContainerNameOrId'))
      return
    }
    let confirmed = true
    if (action === 'remove') {
      const targetNames = validTargets.map((item) => item.name).join(', ')
      const promptName =
        validTargets.length > 1
          ? t('app.docker.messages.containerCount', { count: validTargets.length })
          : `"${targetNames}"`
      confirmed = await modalStore.showConfirmation(
        t('app.docker.messages.deleteContainersConfirm', { name: promptName }),
        t('app.docker.messages.deleteConfirmTitle'),
        t('app.docker.messages.deleteAction'),
        t('confirmation.cancel'),
      )
    }
    if (!confirmed) return

    // pause / unpause / kill 使用专用 API 端点
    if (action === 'pause' || action === 'unpause' || action === 'kill') {
      let hasError = false
      await Promise.all(
        validTargets.map(async ({ id, name }) => {
          let res
          if (action === 'pause') res = await dockerClient.value.pauseContainer(id as string)
          else if (action === 'unpause')
            res = await dockerClient.value.unpauseContainer(id as string)
          else res = await dockerClient.value.killContainer(id as string)
          if (!res.success) {
            notificationStore.error(
              t('app.docker.messages.containerActionFailed', {
                name,
                action,
                message: res.message || t('common.unknownError'),
              }),
            )
            hasError = true
          }
        }),
      )
      if (!hasError) {
        notificationStore.success(
          t('app.docker.messages.containerActionSuccess', {
            count: validTargets.length,
            action,
          }),
        )
      }
    } else {
      let hasError = false
      await Promise.all(
        validTargets.map(async ({ id, name }) => {
          const res = await dockerClient.value.performAction({
            id: id as string,
            name: name as string,
            action,
          })
          if (!res.success) {
            notificationStore.error(
              t('app.docker.messages.containerActionFailed', {
                name,
                action,
                message: res.message || t('common.unknownError'),
              }),
            )
            hasError = true
          }
        }),
      )
      if (!hasError) {
        notificationStore.success(
          t('app.docker.messages.containerActionSuccess', {
            count: validTargets.length,
            action,
          }),
        )
      }
    }
    await Promise.all([fetchContainers(), fetchComposeProjects(), fetchOverviewData()])
  }

  /** Compose 项目操作 */
  const handleComposeProjectAction = async (
    name: string,
    action: 'start' | 'stop' | 'restart' | 'delete' | 'deleteFiles' | 'update',
  ) => {
    if (!name) {
      notificationStore.error(t('app.docker.messages.invalidProjectName'))
      return
    }
    // 更新操作
    if (action === 'update') {
      const confirmed = await modalStore.showConfirmation(
        t('app.docker.messages.updateProjectConfirm', { name }),
        t('app.docker.messages.updateConfirmTitle', { name }),
        t('app.docker.messages.updateAction'),
        t('confirmation.cancel'),
      )
      if (!confirmed) return
      const res = await dockerClient.value.updateComposeProject(name)
      if (res.success) {
        notificationStore.success(
          t('app.docker.messages.projectActionSuccess', {
            name,
            action: t('app.docker.messages.actionLabels.update'),
          }),
        )
        await Promise.all([fetchComposeProjects(), fetchOverviewData()])
      } else {
        notificationStore.error(
          t('app.docker.messages.projectActionFailed', {
            name,
            action: t('app.docker.messages.actionLabels.update'),
            message: res.message || t('common.unknownError'),
          }),
        )
      }
      return
    }
    // 删除操作
    if (action === 'delete' || action === 'deleteFiles') {
      const deleteFiles = action === 'deleteFiles'
      const deletePath =
        composeProjects.value.find((project) => project.name === name)?.composeDir ?? null
      const message = deleteFiles
        ? t('app.docker.messages.deleteProjectWithFileConfirm', {
            name,
            file: deletePath ?? t('app.docker.messages.composeConfigFile'),
          })
        : t('app.docker.messages.deleteProjectConfirm', { name })
      const confirmed = await modalStore.showConfirmation(
        message,
        t('app.docker.messages.deleteConfirmTitle'),
        t('app.docker.messages.deleteAction'),
        t('confirmation.cancel'),
      )
      if (!confirmed) return
      const res = await dockerClient.value.deleteComposeProject(
        name,
        deleteFiles ? { deleteFiles } : undefined,
      )
      if (res.success) {
        notificationStore.success(t('app.docker.messages.deleteProjectSuccess', { name }))
        await Promise.all([fetchComposeProjects(), fetchOverviewData()])
      } else {
        notificationStore.error(
          t('app.docker.messages.deleteProjectFailed', {
            name,
            message: res.message || t('common.unknownError'),
          }),
        )
      }
      return
    }
    // 启动/停止/重启
    const actionLabelMap: Record<string, string> = {
      start: t('app.docker.messages.actionLabels.start'),
      stop: t('app.docker.messages.actionLabels.stop'),
      restart: t('app.docker.messages.actionLabels.restart'),
    }
    const actionLabel = actionLabelMap[action] || action
    const res = await (action === 'start'
      ? dockerClient.value.startComposeProject(name)
      : action === 'stop'
        ? dockerClient.value.stopComposeProject(name)
        : dockerClient.value.restartComposeProject(name))
    if (res.success) {
      notificationStore.success(
        t('app.docker.messages.projectActionSuccess', { name, action: actionLabel }),
      )
      await Promise.all([fetchComposeProjects(), fetchOverviewData()])
    } else {
      notificationStore.error(
        t('app.docker.messages.projectActionFailed', {
          name,
          action: actionLabel,
          message: res.message || t('common.unknownError'),
        }),
      )
    }
  }

  /** 打开 Compose 配置文件编辑器 */
  const handleEditComposeConfig = (name: string) => {
    if (!name) {
      notificationStore.error(t('app.docker.messages.invalidProjectName'))
      return
    }
    const composeDir = composeProjects.value.find((project) => project.name === name)?.composeDir
    if (!composeDir) {
      notificationStore.error(t('app.docker.messages.composeDirMissing'))
      return
    }
    const path = `${composeDir.replace(/\/$/, '')}/compose.yaml`
    windowStore.openWindowWithPayload('file-editor', { path }, { title: name })
  }

  /** 删除镜像 */
  const handleDeleteImage = async (id: string): Promise<boolean> => {
    if (!id) {
      notificationStore.error(t('app.docker.messages.invalidImageId'))
      return false
    }
    if (imageDeleteLoadingId.value) return false
    const image = imagesList.value.find((item) => item.id === id)
    const displayName = image?.tags[0] || id.replace(/^sha256:/, '').substring(0, 12) || id
    const confirmed = await modalStore.showConfirmation(
      t('app.docker.messages.deleteImageConfirmAllTags', {
        name: displayName,
        count: image?.tags.length || 0,
      }),
      t('app.docker.messages.deleteConfirmTitle'),
      t('app.docker.messages.deleteAction'),
      t('confirmation.cancel'),
    )
    if (!confirmed) return false
    imageDeleteLoadingId.value = id
    try {
      const res = await dockerClient.value.removeImage(id)
      if (!res.success) {
        notificationStore.error(
          t('app.docker.messages.deleteImageFailed', {
            name: displayName,
            message: res.message || t('common.unknownError'),
          }),
        )
        return false
      }
      notificationStore.success(t('app.docker.messages.deleteImageSuccess', { name: displayName }))
      await Promise.all([fetchImagesList(), fetchOverviewData()])
      return true
    } finally {
      imageDeleteLoadingId.value = null
    }
  }

  /** 按需获取网络详情，并阻止过期详情覆盖当前选择。 */
  const fetchNetworkDetail = async (id: string): Promise<dockerType.DockerNetworkDetail | null> => {
    if (!id) return null
    const nodeId = nodeStore.currentNodeId || 'local'
    const sequence = ++networkDetailRequestSequence
    if (networkDetail.value?.summary.id !== id) networkDetail.value = null
    networkDetailLoading.value = true
    networkDetailError.value = null
    const res = await dockerClient.value.inspectNetwork(id)
    if (
      sequence !== networkDetailRequestSequence ||
      nodeId !== (nodeStore.currentNodeId || 'local')
    ) {
      return null
    }
    networkDetailLoading.value = false
    if (res.success && res.data) {
      networkDetail.value = res.data
      return res.data
    }
    networkDetailError.value = t('app.docker.messages.inspectNetworkFailed', {
      message: res.message || t('common.unknownError'),
    })
    return null
  }

  /** 清理当前网络详情并使在途请求失效。 */
  const clearNetworkDetail = () => {
    networkDetailRequestSequence += 1
    networkDetail.value = null
    networkDetailLoading.value = false
    networkDetailError.value = null
  }

  /** 创建 Bridge 网络。 */
  const createNetwork = async (
    payload: dockerType.DockerNetworkCreateRequest,
  ): Promise<boolean> => {
    if (networkCreateLoading.value) return false
    networkCreateLoading.value = true
    try {
      const res = await dockerClient.value.createNetwork(payload)
      if (!res.success) {
        notificationStore.error(
          t('app.docker.messages.createNetworkFailed', {
            message: res.message || t('common.unknownError'),
          }),
        )
        return false
      }
      notificationStore.success(
        t('app.docker.messages.createNetworkSuccess', { name: payload.name }),
      )
      if (res.data?.warning) notificationStore.info(res.data.warning)
      await fetchNetworks()
      return true
    } finally {
      networkCreateLoading.value = false
    }
  }

  /** 删除自定义网络。 */
  const removeNetwork = async (network: dockerType.DockerNetworkSummary): Promise<boolean> => {
    if (networkDeleteLoadingId.value) return false
    networkDeleteLoadingId.value = network.id
    try {
      const res = await dockerClient.value.removeNetwork(network.id)
      if (!res.success) {
        notificationStore.error(
          t('app.docker.messages.deleteNetworkFailed', {
            message: res.message || t('common.unknownError'),
          }),
        )
        return false
      }
      notificationStore.success(t('app.docker.messages.deleteNetworkSuccess'))
      if (networkDetail.value?.summary.id === network.id) clearNetworkDetail()
      await fetchNetworks()
      return true
    } finally {
      networkDeleteLoadingId.value = null
    }
  }

  /** 将运行中的容器连接到自定义网络。 */
  const connectNetwork = async (networkId: string, containerId: string): Promise<boolean> => {
    if (!networkId || !containerId || networkConnectLoading.value) return false
    networkConnectLoading.value = true
    try {
      const res = await dockerClient.value.connectNetwork(networkId, { container: containerId })
      if (!res.success) {
        notificationStore.error(
          t('app.docker.messages.connectNetworkFailed', {
            message: res.message || t('common.unknownError'),
          }),
        )
        return false
      }
      notificationStore.success(t('app.docker.messages.connectNetworkSuccess'))
      await fetchNetworkDetail(networkId)
      return true
    } finally {
      networkConnectLoading.value = false
    }
  }

  /** 从自定义网络断开容器。 */
  const disconnectNetwork = async (
    networkId: string,
    containerId: string,
    force: boolean,
  ): Promise<boolean> => {
    if (!networkId || !containerId || networkDisconnectLoading.value[containerId]) return false
    networkDisconnectLoading.value[containerId] = true
    try {
      const res = await dockerClient.value.disconnectNetwork(networkId, {
        container: containerId,
        force,
      })
      if (!res.success) {
        notificationStore.error(
          t('app.docker.messages.disconnectNetworkFailed', {
            message: res.message || t('common.unknownError'),
          }),
        )
        return false
      }
      notificationStore.success(t('app.docker.messages.disconnectNetworkSuccess'))
      await fetchNetworkDetail(networkId)
      return true
    } finally {
      networkDisconnectLoading.value[containerId] = false
    }
  }

  // ─── 容器创建流程 ───

  const resetContainerForm = () => {
    containerForm.value = {
      name: '',
      command: '',
      env: '',
      ports: '',
      volumes: '',
      restartPolicy: 'no',
      network: '',
      autoRemove: false,
    }
  }

  /** 启动容器创建流程 */
  const startContainerCreateFlow = async () => {
    await fetchImagesList()
    if (networks.value.length === 0) await fetchNetworks()
    selectedImageId.value = null
    containerStep.value = 'selectImage'
    resetContainerForm()
    isContainerCreateActive.value = true
  }

  /** 取消容器创建 */
  const cancelContainerCreate = () => {
    isContainerCreateActive.value = false
    selectedImageId.value = null
    containerStep.value = 'selectImage'
    resetContainerForm()
  }

  /** 提交容器创建 */
  const submitContainerConfig = async () => {
    if (!selectedImageId.value) {
      notificationStore.error(t('app.docker.messages.imageRequired'))
      return
    }
    if (!containerForm.value.name.trim()) {
      notificationStore.error(t('app.docker.messages.containerNameRequired'))
      return
    }
    const res = await dockerClient.value.createContainer({
      name: containerForm.value.name.trim(),
      image: selectedImageId.value,
      command: containerForm.value.command || undefined,
      env: containerForm.value.env || undefined,
      ports: containerForm.value.ports || undefined,
      volumes: containerForm.value.volumes || undefined,
      restartPolicy: containerForm.value.restartPolicy || undefined,
      network: containerForm.value.network || undefined,
      autoRemove: containerForm.value.autoRemove,
      autoStart: true,
    })
    if (res.success) {
      notificationStore.success(
        t('app.docker.messages.createContainerSuccess', { name: containerForm.value.name }),
      )
      cancelContainerCreate()
      await Promise.all([fetchContainers(), fetchComposeProjects(), fetchOverviewData()])
    } else {
      notificationStore.error(
        t('app.docker.messages.createContainerFailed', {
          message: res.message || t('common.unknownError'),
        }),
      )
    }
  }

  // ─── 项目创建流程 ───

  /** 取消项目创建 */
  const cancelProjectCreate = () => {
    isProjectCreateActive.value = false
    projectFormName.value = ''
    projectFormDir.value = ''
    composeYamlError.value = ''
  }

  /** 提交项目创建 */
  const submitProjectForm = async () => {
    if (!projectFormName.value.trim()) {
      notificationStore.error(t('app.docker.messages.projectNameRequired'))
      return
    }
    if (!projectFormCompose.value.trim()) {
      notificationStore.error(t('app.docker.messages.composeRequired'))
      return
    }
    if (composeYamlError.value) {
      notificationStore.error(
        t('app.docker.messages.yamlValidationFailed', { message: composeYamlError.value }),
      )
      return
    }
    const res = await dockerClient.value.createComposeProject({
      name: projectFormName.value.trim(),
      compose: projectFormCompose.value,
      dir: projectFormDir.value.trim() || undefined,
      projectType: 'docker',
    })
    if (res.success) {
      notificationStore.success(
        t('app.docker.messages.createProjectSuccess', { name: projectFormName.value }),
      )
      cancelProjectCreate()
      await Promise.all([fetchComposeProjects(), fetchOverviewData()])
    } else {
      notificationStore.error(
        t('app.docker.messages.createProjectFailed', {
          message: res.message || t('common.unknownError'),
        }),
      )
    }
  }

  /** 校验 Compose YAML */
  const validateComposeYaml = async (value: string) => {
    const trimmed = value.trim()
    if (!trimmed) {
      composeYamlError.value = ''
      return
    }
    const res = await dockerClient.value.validateComposeYaml({ compose: trimmed })
    if (res.success && res.data) {
      const data = res.data as { valid: boolean; error?: string }
      if (data.valid) {
        composeYamlError.value = ''
      } else {
        composeYamlError.value = data.error || t('app.docker.messages.yamlParseFailed')
      }
    } else {
      // 降级使用本地 js-yaml 校验
      try {
        parseYaml(trimmed)
        composeYamlError.value = ''
      } catch (error) {
        composeYamlError.value =
          error instanceof Error ? error.message : t('app.docker.messages.yamlParseFailed')
      }
    }
  }

  /** 伸缩 Compose 项目中的指定服务 */
  const handleScaleComposeProject = async (name: string, service: string, replicas: number) => {
    if (!name || !service) return
    const res = await dockerClient.value.scaleComposeProject(name, { service, replicas })
    if (res.success) {
      notificationStore.success(
        t('app.docker.messages.scaleProjectSuccess', { name, service, count: replicas }),
      )
      await fetchComposeContainers()
    } else {
      notificationStore.error(
        t('app.docker.messages.scaleProjectFailed', {
          message: res.message || t('common.unknownError'),
        }),
      )
    }
  }

  /** 创建本地数据卷，仅成功时刷新列表。 */
  const createVolume = async (payload: dockerType.DockerVolumeCreateRequest): Promise<boolean> => {
    if (volumeCreateLoading.value) return false
    volumeCreateLoading.value = true
    volumeCreateError.value = null
    try {
      const res = await dockerClient.value.createVolume(payload)
      if (!res.success) {
        volumeCreateError.value = t('app.docker.messages.createVolumeFailed', {
          message: res.message || t('common.unknownError'),
        })
        return false
      }
      notificationStore.success(
        t('app.docker.messages.createVolumeSuccess', { name: payload.name }),
      )
      await fetchVolumes()
      return true
    } finally {
      volumeCreateLoading.value = false
    }
  }

  /** 删除允许管理的数据卷，并保留失败后的现有列表。 */
  const removeVolume = async (volume: dockerType.DockerVolumeSummary): Promise<boolean> => {
    if (!volume.capabilities.canRemove || volumeDeleteLoadingName.value) return false
    const confirmed = await modalStore.showConfirmation(
      t('app.docker.messages.deleteVolumeConfirm', { name: volume.name }),
      t('app.docker.messages.deleteConfirmTitle'),
      t('app.docker.messages.deleteAction'),
      t('confirmation.cancel'),
    )
    if (!confirmed) return false
    volumeDeleteLoadingName.value = volume.name
    try {
      const res = await dockerClient.value.removeVolume(volume.name)
      if (!res.success) {
        notificationStore.error(
          t('app.docker.messages.deleteVolumeFailed', {
            message: res.message || t('common.unknownError'),
          }),
        )
        return false
      }
      notificationStore.success(t('app.docker.messages.deleteVolumeSuccess'))
      await fetchVolumes()
      return true
    } finally {
      volumeDeleteLoadingName.value = null
    }
  }

  /** 清理 Docker 系统垃圾 */
  const handlePruneSystem = async () => {
    if (pruneLoading.value) return
    const confirmed = await modalStore.showConfirmation(
      t('app.docker.messages.pruneConfirm'),
      t('app.docker.messages.pruneConfirmTitle'),
      t('app.docker.messages.pruneAction'),
      t('confirmation.cancel'),
    )
    if (!confirmed) return
    pruneLoading.value = true
    notificationStore.info(t('app.docker.messages.pruneStarted'))
    try {
      const res = await dockerClient.value.pruneSystem()
      if (res.success) {
        notificationStore.success(t('app.docker.messages.pruneSuccess'))
        await Promise.all([
          fetchContainers(),
          fetchImagesList(),
          fetchNetworks(),
          fetchVolumes(),
          fetchOverviewData(),
          fetchDockerDiskUsage(),
        ])
      } else {
        notificationStore.error(
          t('app.docker.messages.pruneFailed', {
            message: res.message || t('common.unknownError'),
          }),
        )
      }
    } catch (error) {
      notificationStore.error(
        t('app.docker.messages.pruneFailed', {
          message: error instanceof Error ? error.message : t('common.unknownError'),
        }),
      )
    } finally {
      pruneLoading.value = false
    }
  }

  // ========================================
  // 导出
  // ========================================

  return {
    // 核心状态
    dockerAvailable,
    dockerStatus,
    dockerStatusCode,
    isLoading,

    // 概览统计
    overviewCollectedAt,
    overviewError,
    containerStates,
    projectStates,
    imageCounts,
    resourceUsage,

    // 概览图表
    overviewContainers,
    overviewSelectedContainerIds,
    overviewHistoryMap,
    overviewHistoryHours,
    overviewHistoryError,

    // 容器
    containers,
    containerResourceStats,
    isContainerDetailActive,

    // Compose 项目
    composeProjects,

    // 镜像 / 网络 / 卷 / 系统
    imagesList,
    imageListLoading,
    imageListError,
    imageListLoadedAt,
    imageDeleteLoadingId,
    controllerImages,
    controllerImageLoading,
    controllerImageError,
    controllerImageLoadedAt,
    imageDistributionTask,
    imageDistributionLoading,
    imageDistributionError,
    imageDistributionStarting,
    imageDistributionCanceling,
    networks,
    networkListLoading,
    networkListError,
    networkListLoadedAt,
    networkDetail,
    networkDetailLoading,
    networkDetailError,
    networkCreateLoading,
    networkDeleteLoadingId,
    networkConnectLoading,
    networkDisconnectLoading,
    volumes,
    volumeWarnings,
    volumeListLoading,
    volumeListError,
    volumeDetail,
    volumeDetailLoading,
    volumeDetailError,
    volumeCreateLoading,
    volumeCreateError,
    volumeDeleteLoadingName,
    systemInfo,
    systemInfoLoading,
    systemInfoError,
    systemInfoLoadedAt,
    diskUsage,
    diskUsageLoading,
    diskUsageError,
    pruneLoading,
    composeContainers,
    projectLogs,

    // 容器创建
    isContainerCreateActive,
    containerStep,
    selectedImageId,
    selectedImageLabel,
    containerForm,

    // 项目创建
    isProjectCreateActive,
    projectFormName,
    projectFormDir,
    projectFormCompose,
    composeYamlError,

    // 数据获取
    fetchDockerAvailability,
    fetchOverviewData,
    fetchOverviewHistoryAll,
    fetchContainers,
    fetchComposeProjects,
    fetchImagesList,
    fetchControllerImages,
    fetchRecentImageDistributionTask,
    startImageDistribution,
    fetchImageDistributionTask,
    cancelImageDistribution,
    leaveImageDistributionView,
    fetchNetworks,
    fetchVolumes,
    fetchVolumeDetail,
    clearVolumeDetail,
    fetchDockerInfo,
    fetchDockerDiskUsage,
    fetchComposeContainers,
    fetchProjectLogs,
    fetchContainerResourceStats,
    loadOverviewData,
    refreshOverviewDataIfNeeded,
    updateOverviewSelectedContainers,
    setOverviewHistoryHours,
    initialLoad,

    // 轮询
    startOverviewPolling,
    stopOverviewPolling,
    startHistoryPolling,
    stopHistoryPolling,
    startContainerStatsPolling,
    stopContainerStatsPolling,
    stopAllPolling,

    // 业务操作
    handleContainerAction,
    handleComposeProjectAction,
    handleEditComposeConfig,
    handleDeleteImage,
    fetchNetworkDetail,
    clearNetworkDetail,
    createNetwork,
    removeNetwork,
    connectNetwork,
    disconnectNetwork,

    // 容器创建
    startContainerCreateFlow,
    cancelContainerCreate,
    submitContainerConfig,
    resetContainerForm,

    // 项目创建
    cancelProjectCreate,
    submitProjectForm,
    validateComposeYaml,

    // 编排与系统操作
    handleScaleComposeProject,
    createVolume,
    removeVolume,
    handlePruneSystem,

    // 工具
    resetAll,
  }
})
