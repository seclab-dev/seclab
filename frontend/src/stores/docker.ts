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
  data: dockerType.ResourceUsageSummary
  fetchedAt: number
}

/** 概览数据缓存 */
interface OverviewCacheEntry {
  timestamp: number
  overview: dockerType.OverviewStatus
}

/** 容器资源统计缓存 TTL (毫秒) */
const CONTAINER_STATS_TTL = 10_000
/** 概览缓存 TTL (毫秒) */
const OVERVIEW_CACHE_TTL = 30_000
/** 概览最大可选容器数 */
const MAX_OVERVIEW_CONTAINERS = 7

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
  const totalContainerCount = ref<number>(0)
  const runningContainerCount = ref<number>(0)
  const totalImageCount = ref<number>(0)
  const projectRunningCount = ref<number>(0)
  const projectTotalCount = ref<number>(0)
  const resourceUsage = ref<dockerType.ResourceUsageSummary | null>(null)

  // ─── 概览图表数据 ───
  const overviewContainers = ref<Array<{ id: string; name: string }>>([])
  const overviewSelectedContainerIds = ref<string[]>([])
  const overviewHistoryMap = ref<Record<string, dockerType.ResourceUsageHistory>>({})
  const overviewHistoryLatestMap = ref<Record<string, number | null>>({})

  // ─── 容器列表 ───
  const containers = ref<dockerType.ContainerSummary[]>([])
  const containerResourceStats = ref<Record<string, ContainerStatsEntry>>({})
  const isContainerDetailActive = ref(false)

  // ─── Compose 项目 ───
  const composeProjects = ref<dockerType.ComposeProjectSummary[]>([])

  // ─── 镜像 / 网络 / 卷 / 系统 ───
  const imagesList = ref<dockerType.ImageSummary[]>([])
  const networks = ref<dockerType.Network[]>([])
  const volumes = ref<dockerType.VolumeSummary[]>([])
  const systemInfo = ref<dockerType.DockerSystemInfo | null>(null)
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

  // ─── 网络创建/查看 ───
  const isNetworkCreateActive = ref(false)
  const networkForm = ref({
    name: '',
    driver: 'bridge',
    subnet: '',
    gateway: '',
    labels: '',
  })
  const selectedNetworkDetail = ref<dockerType.Network | null>(null)
  const isNetworkModalVisible = ref(false)

  // ─── 缓存与防并发 ───
  const overviewCache = ref<OverviewCacheEntry | null>(null)
  const overviewRefreshInProgress = ref(false)

  // ─── 轮询定时器 ───
  let resourceUsageTimer: number | null = null
  let resourceHistoryTimer: number | null = null
  let containerStatsPollingTimer: number | null = null

  // ─── 容器统计批量队列 ───
  let containerStatsTimer: number | null = null
  const pendingContainerStatIds = new Set<string>()

  // ─── 选中镜像标签 (计算属性) ───
  const selectedImageLabel = computed(() => {
    const img = imagesList.value.find((i) => i.Id === selectedImageId.value)
    if (!img) return ''
    return img.RepoTags?.[0] || img.Id?.substring(7, 19) || img.Id || ''
  })

  // ========================================
  // 数据重置
  // ========================================

  /** 重置所有 Docker 数据到初始状态 */
  const resetAll = () => {
    dockerStatus.value = false
    totalContainerCount.value = 0
    runningContainerCount.value = 0
    totalImageCount.value = 0
    resourceUsage.value = null
    overviewContainers.value = []
    overviewSelectedContainerIds.value = []
    overviewHistoryMap.value = {}
    overviewHistoryLatestMap.value = {}
    containers.value = []
    containerResourceStats.value = {}
    composeProjects.value = []
    projectRunningCount.value = 0
    projectTotalCount.value = 0
    imagesList.value = []
    networks.value = []
    volumes.value = []
    systemInfo.value = null
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
  const fetchOverviewData = async (): Promise<dockerType.OverviewStatus | null> => {
    if (!dockerAvailable.value) return null
    const res = await dockerClient.value.fetchOverviewRealtime()
    if (!res.success || !res.data) {
      notificationStore.error(
        t('app.docker.messages.overviewFailed', {
          message: res.message || t('common.unknownError'),
        }),
      )
      dockerStatus.value = false
      return null
    }
    const payload = res.data
    dockerStatus.value = payload.overview.status
    totalContainerCount.value = payload.overview.totalContainerCount
    runningContainerCount.value = payload.overview.runningContainerCount
    totalImageCount.value = payload.overview.totalImageCount
    projectRunningCount.value = payload.overview.projectRunningCount
    projectTotalCount.value = payload.overview.projectTotalCount
    resourceUsage.value = payload.resourceUsage
    const running = (payload.overviewContainers || []).map((item) => ({
      id: item.id,
      name: item.name,
    }))
    updateOverviewContainerState(running)
    return payload.overview
  }

  /** 获取全部容器的历史资源数据 */
  const fetchOverviewHistoryAll = async (hours = 12) => {
    if (!dockerAvailable.value) return
    const res = await dockerClient.value.fetchContainerResourceUsageHistoryAll(hours)
    if (!res.success || !res.data) return
    const nextItems: Record<string, dockerType.ResourceUsageHistory> = {}
    const nextLatestMap: Record<string, number | null> = {}
    for (const item of res.data.containers || []) {
      nextItems[item.id] = { points: item.points }
      const points = item.points || []
      nextLatestMap[item.id] = points.length ? points[points.length - 1]!.timestamp : null
    }
    let changed = false
    for (const id of overviewSelectedContainerIds.value) {
      if (overviewHistoryLatestMap.value[id] !== nextLatestMap[id]) {
        changed = true
        break
      }
    }
    if (Object.keys(nextLatestMap).length !== Object.keys(overviewHistoryLatestMap.value).length) {
      changed = true
    }
    if (changed) {
      overviewHistoryLatestMap.value = nextLatestMap
      overviewHistoryMap.value = nextItems
    }
  }

  /** 更新概览容器选择状态 */
  const updateOverviewContainerState = (running: Array<{ id: string; name: string }>) => {
    overviewContainers.value = running
    const runningIds = new Set(running.map((item) => item.id))
    const filteredSelected = overviewSelectedContainerIds.value.filter((id) => runningIds.has(id))
    const nextSelected =
      filteredSelected.length > 0
        ? filteredSelected.slice(0, MAX_OVERVIEW_CONTAINERS)
        : running.slice(0, MAX_OVERVIEW_CONTAINERS).map((item) => item.id)
    overviewSelectedContainerIds.value = nextSelected
    void fetchOverviewHistoryAll()
  }

  /** 更新概览选中的容器列表 */
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

  /** 将概览数据写入缓存 */
  const cacheOverviewState = (overview: dockerType.OverviewStatus) => {
    overviewCache.value = { timestamp: Date.now(), overview }
  }

  /** 统一加载概览数据 */
  const loadOverviewData = async ({ showLoading }: { showLoading: boolean }) => {
    if (overviewRefreshInProgress.value) return
    if (showLoading) isLoading.value = true
    overviewRefreshInProgress.value = true
    try {
      const overviewResult = await fetchOverviewData()
      if (overviewResult) cacheOverviewState(overviewResult)
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

  /** 获取镜像列表 */
  const fetchImagesList = async () => {
    if (!dockerAvailable.value) return
    const res = await dockerClient.value.listImages()
    if (res.success) {
      imagesList.value = res.data || []
    } else {
      notificationStore.error(
        t('app.docker.messages.listImagesFailed', {
          message: res.message || t('common.unknownError'),
        }),
      )
      imagesList.value = []
    }
  }

  /** 获取网络列表 */
  const fetchNetworks = async () => {
    if (!dockerAvailable.value) return
    const res = await dockerClient.value.listNetworks()
    if (res.success) {
      networks.value = res.data || []
    } else {
      notificationStore.error(
        t('app.docker.messages.listNetworksFailed', {
          message: res.message || t('common.unknownError'),
        }),
      )
      networks.value = []
    }
  }

  /** 获取卷列表 */
  const fetchVolumes = async () => {
    if (!dockerAvailable.value) return
    const res = await dockerClient.value.listVolumes()
    if (res.success) {
      volumes.value = (res.data as { Volumes?: dockerType.VolumeSummary[] })?.Volumes || []
    } else {
      volumes.value = []
    }
  }

  /** 获取 Docker 系统信息 */
  const fetchDockerInfo = async () => {
    if (!dockerAvailable.value) return
    const res = await dockerClient.value.fetchInfo()
    if (res.success && res.data) {
      systemInfo.value = res.data as dockerType.DockerSystemInfo
    } else {
      systemInfo.value = null
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
    resourceUsageTimer = window.setInterval(fetchOverviewData, 10_000)
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
  const handleDeleteImage = async ({ id, containers }: { id: string; containers: number }) => {
    if (!id) {
      notificationStore.error(t('app.docker.messages.invalidImageId'))
      return
    }
    const image = imagesList.value.find((i) => i.Id === id)
    const displayName =
      image?.RepoTags?.[0] || image?.Id?.substring(7, 19) || id.substring(7, 19) || id
    if (containers > 0) {
      notificationStore.error(t('app.docker.messages.imageInUse', { name: displayName }))
      return
    }
    const confirmed = await modalStore.showConfirmation(
      t('app.docker.messages.deleteImageConfirm', { name: displayName }),
      t('app.docker.messages.deleteConfirmTitle'),
      t('app.docker.messages.deleteAction'),
      t('confirmation.cancel'),
    )
    if (!confirmed) return
    const res = await dockerClient.value.removeImage({ id, name: displayName })
    if (res.success) {
      notificationStore.success(t('app.docker.messages.deleteImageSuccess', { name: displayName }))
      await Promise.all([fetchImagesList(), fetchOverviewData()])
    } else {
      notificationStore.error(
        t('app.docker.messages.deleteImageFailed', {
          name: displayName,
          message: res.message || t('common.unknownError'),
        }),
      )
    }
  }

  /** 删除网络 */
  const handleDeleteNetwork = async (id: string) => {
    const confirmed = await modalStore.showConfirmation(
      t('app.docker.messages.deleteNetworkConfirm', { id: id.substring(0, 12) }),
      t('app.docker.messages.deleteConfirmTitle'),
      t('app.docker.messages.deleteAction'),
      t('confirmation.cancel'),
    )
    if (!confirmed) return
    const res = await dockerClient.value.removeNetwork(id)
    if (res.success) {
      notificationStore.success(t('app.docker.messages.deleteNetworkSuccess'))
      await fetchNetworks()
    } else {
      notificationStore.error(
        t('app.docker.messages.deleteNetworkFailed', {
          message: res.message || t('common.unknownError'),
        }),
      )
    }
  }

  /** 查看网络详情 (调用 inspectNetwork 获取详细数据) */
  const handleViewNetwork = async (id: string) => {
    const res = await dockerClient.value.inspectNetwork(id)
    if (res.success && res.data) {
      selectedNetworkDetail.value = res.data
      isNetworkModalVisible.value = true
    } else {
      notificationStore.error(
        t('app.docker.messages.inspectNetworkFailed', {
          message: res.message || t('common.unknownError'),
        }),
      )
    }
  }

  /** 连接容器到网络 */
  const handleConnectNetwork = async (networkId: string, containerId: string) => {
    if (!networkId || !containerId) return
    const res = await dockerClient.value.connectNetwork(networkId, { container: containerId })
    if (res.success) {
      notificationStore.success(t('app.docker.messages.connectNetworkSuccess'))
      // 刷新网络详情
      const inspectRes = await dockerClient.value.inspectNetwork(networkId)
      if (inspectRes.success && inspectRes.data) {
        selectedNetworkDetail.value = inspectRes.data
      }
      // 刷新网络列表
      void fetchNetworks()
    } else {
      notificationStore.error(
        t('app.docker.messages.connectNetworkFailed', {
          message: res.message || t('common.unknownError'),
        }),
      )
    }
  }

  /** 从网络断开容器 */
  const handleDisconnectNetwork = async (networkId: string, containerId: string) => {
    if (!networkId || !containerId) return
    const res = await dockerClient.value.disconnectNetwork(networkId, {
      container: containerId,
      force: true,
    })
    if (res.success) {
      notificationStore.success(t('app.docker.messages.disconnectNetworkSuccess'))
      // 刷新网络详情
      const inspectRes = await dockerClient.value.inspectNetwork(networkId)
      if (inspectRes.success && inspectRes.data) {
        selectedNetworkDetail.value = inspectRes.data
      }
      // 刷新网络列表
      void fetchNetworks()
    } else {
      notificationStore.error(
        t('app.docker.messages.disconnectNetworkFailed', {
          message: res.message || t('common.unknownError'),
        }),
      )
    }
  }

  /** 关闭网络详情弹窗 */
  const closeNetworkModal = () => {
    isNetworkModalVisible.value = false
    selectedNetworkDetail.value = null
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

  // ─── 网络创建流程 ───

  /** 开始创建网络 */
  const handleCreateNetwork = () => {
    isNetworkCreateActive.value = true
  }

  /** 取消网络创建 */
  const cancelNetworkCreate = () => {
    isNetworkCreateActive.value = false
    networkForm.value = {
      name: '',
      driver: 'bridge',
      subnet: '',
      gateway: '',
      labels: '',
    }
  }

  /** 提交网络创建 */
  const submitNetworkForm = async () => {
    if (!networkForm.value.name.trim()) {
      notificationStore.error(t('app.docker.messages.networkNameRequired'))
      return
    }
    const { parseLabels } = await import('@/utils/docker-format')
    const labels = parseLabels(networkForm.value.labels)
    if (labels === null) {
      notificationStore.error(t('app.docker.messages.invalidLabels'))
      return
    }
    const res = await dockerClient.value.createNetwork({
      name: networkForm.value.name.trim(),
      driver: networkForm.value.driver || undefined,
      subnet: networkForm.value.subnet || undefined,
      gateway: networkForm.value.gateway || undefined,
      labels: labels || undefined,
    })
    if (res.success) {
      notificationStore.success(
        t('app.docker.messages.createNetworkSuccess', { name: networkForm.value.name }),
      )
      cancelNetworkCreate()
      await fetchNetworks()
    } else {
      notificationStore.error(
        t('app.docker.messages.createNetworkFailed', {
          message: res.message || t('common.unknownError'),
        }),
      )
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

  /** 创建数据卷 */
  const handleCreateVolume = async (payload: {
    name: string
    driver?: string
    labels?: string
  }) => {
    const { parseLabels } = await import('@/utils/docker-format')
    const parsedLabels = payload.labels ? parseLabels(payload.labels) : null
    if (payload.labels && parsedLabels === null) {
      notificationStore.error(t('app.docker.messages.invalidLabels'))
      return
    }
    const res = await dockerClient.value.createVolume({
      name: payload.name,
      driver: payload.driver || 'local',
      labels: parsedLabels || undefined,
    })
    if (res.success) {
      notificationStore.success(
        t('app.docker.messages.createVolumeSuccess', { name: payload.name }),
      )
      await fetchVolumes()
    } else {
      notificationStore.error(
        t('app.docker.messages.createVolumeFailed', {
          message: res.message || t('common.unknownError'),
        }),
      )
    }
  }

  /** 删除数据卷 */
  const handleDeleteVolume = async (name: string) => {
    const confirmed = await modalStore.showConfirmation(
      t('app.docker.messages.deleteVolumeConfirm', { name }),
      t('app.docker.messages.deleteConfirmTitle'),
      t('app.docker.messages.deleteAction'),
      t('confirmation.cancel'),
    )
    if (!confirmed) return
    const res = await dockerClient.value.removeVolume(name)
    if (res.success) {
      notificationStore.success(t('app.docker.messages.deleteVolumeSuccess'))
      await fetchVolumes()
    } else {
      notificationStore.error(
        t('app.docker.messages.deleteVolumeFailed', {
          message: res.message || t('common.unknownError'),
        }),
      )
    }
  }

  /** 清理 Docker 系统垃圾 */
  const handlePruneSystem = async () => {
    const confirmed = await modalStore.showConfirmation(
      t('app.docker.messages.pruneConfirm'),
      t('app.docker.messages.pruneConfirmTitle'),
      t('app.docker.messages.pruneAction'),
      t('confirmation.cancel'),
    )
    if (!confirmed) return
    isLoading.value = true
    const res = await dockerClient.value.pruneSystem()
    isLoading.value = false
    if (res.success) {
      notificationStore.success(t('app.docker.messages.pruneSuccess'))
      await Promise.all([
        fetchContainers(),
        fetchImagesList(),
        fetchNetworks(),
        fetchVolumes(),
        fetchOverviewData(),
      ])
    } else {
      notificationStore.error(
        t('app.docker.messages.pruneFailed', {
          message: res.message || t('common.unknownError'),
        }),
      )
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
    totalContainerCount,
    runningContainerCount,
    totalImageCount,
    projectRunningCount,
    projectTotalCount,
    resourceUsage,

    // 概览图表
    overviewContainers,
    overviewSelectedContainerIds,
    overviewHistoryMap,

    // 容器
    containers,
    containerResourceStats,
    isContainerDetailActive,

    // Compose 项目
    composeProjects,

    // 镜像 / 网络 / 卷 / 系统
    imagesList,
    networks,
    volumes,
    systemInfo,
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

    // 网络创建/查看
    isNetworkCreateActive,
    networkForm,
    selectedNetworkDetail,
    isNetworkModalVisible,

    // 数据获取
    fetchDockerAvailability,
    fetchOverviewData,
    fetchOverviewHistoryAll,
    fetchContainers,
    fetchComposeProjects,
    fetchImagesList,
    fetchNetworks,
    fetchVolumes,
    fetchDockerInfo,
    fetchComposeContainers,
    fetchProjectLogs,
    fetchContainerResourceStats,
    loadOverviewData,
    refreshOverviewDataIfNeeded,
    updateOverviewSelectedContainers,
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
    handleDeleteNetwork,
    handleViewNetwork,
    handleConnectNetwork,
    handleDisconnectNetwork,
    closeNetworkModal,
    handleCreateNetwork,

    // 容器创建
    startContainerCreateFlow,
    cancelContainerCreate,
    submitContainerConfig,
    resetContainerForm,

    // 项目创建
    cancelProjectCreate,
    submitProjectForm,
    validateComposeYaml,

    // 网络创建
    cancelNetworkCreate,
    submitNetworkForm,

    // 编排与系统操作
    handleScaleComposeProject,
    handleCreateVolume,
    handleDeleteVolume,
    handlePruneSystem,

    // 工具
    resetAll,
  }
})
