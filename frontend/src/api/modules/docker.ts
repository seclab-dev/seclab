import http from '@/api'
import * as docker from '../interface/docker'

const buildDockerPath = (path: string, nodeId?: string) => {
  if (!nodeId || nodeId === 'local') {
    return path
  }
  return `/node/${encodeURIComponent(nodeId)}${path}`
}

const createScopedDockerApi = (nodeId?: string) => ({
  fetchStatus: () => {
    return http.get<{ dockerAvailable?: boolean; dockerStatus?: string }>(
      buildDockerPath('/agent/docker/status', nodeId),
    )
  },
  fetchInfo: () => {
    return http.get(buildDockerPath('/agent/docker/info', nodeId))
  },

  /** 获取概览实时数据，合并了容器和镜像统计 */
  fetchOverviewRealtime: () => {
    return http.post<docker.OverviewRealtimeResponse>(
      buildDockerPath('/agent/docker/overview/realtime', nodeId),
    )
  },
  listContainers: () => {
    return http.get<docker.ContainerSummary[]>(buildDockerPath('/agent/docker/containers', nodeId))
  },
  createContainer: (payload: docker.ContainerCreateRequest) => {
    return http.post(buildDockerPath('/agent/docker/containers', nodeId), payload)
  },
  inspectContainer: (id: string) => {
    return http.get<docker.ContainerInspect>(
      buildDockerPath(`/agent/docker/containers/${encodeURIComponent(id)}`, nodeId),
    )
  },
  renameContainer: (id: string, payload: docker.ContainerRenameRequest) => {
    return http.post(
      buildDockerPath(`/agent/docker/containers/${encodeURIComponent(id)}/rename`, nodeId),
      payload,
    )
  },
  pauseContainer: (id: string) => {
    return http.post(
      buildDockerPath(`/agent/docker/containers/${encodeURIComponent(id)}/pause`, nodeId),
    )
  },
  unpauseContainer: (id: string) => {
    return http.post(
      buildDockerPath(`/agent/docker/containers/${encodeURIComponent(id)}/unpause`, nodeId),
    )
  },
  killContainer: (id: string) => {
    return http.post(
      buildDockerPath(`/agent/docker/containers/${encodeURIComponent(id)}/kill`, nodeId),
    )
  },
  execContainer: (id: string, payload: docker.ContainerExecRequest) => {
    return http.post<docker.ContainerExecResult>(
      buildDockerPath(`/agent/docker/containers/${encodeURIComponent(id)}/exec`, nodeId),
      payload,
    )
  },
  fetchContainerTop: (id: string) => {
    return http.get<docker.ContainerTopResponse>(
      buildDockerPath(`/agent/docker/containers/${encodeURIComponent(id)}/top`, nodeId),
    )
  },
  listProjectContainers: () => {
    return http.get<docker.ContainerSummary[]>(
      buildDockerPath('/agent/docker/compose/containers', nodeId),
    )
  },
  listComposeProjects: () => {
    return http.get<docker.ComposeProjectSummary[]>(
      buildDockerPath('/agent/docker/compose/projects', nodeId),
    )
  },
  fetchComposeRoot: () => {
    return http.get<string>(buildDockerPath('/agent/docker/compose/root', nodeId))
  },
  createComposeProject: (payload: docker.ComposeProjectCreateRequest) => {
    return http.post(buildDockerPath('/agent/docker/compose/projects', nodeId), payload)
  },
  startComposeProject: (name: string) => {
    return http.post(
      buildDockerPath(`/agent/docker/compose/projects/${encodeURIComponent(name)}/start`, nodeId),
    )
  },
  stopComposeProject: (name: string) => {
    return http.post(
      buildDockerPath(`/agent/docker/compose/projects/${encodeURIComponent(name)}/stop`, nodeId),
    )
  },
  restartComposeProject: (name: string) => {
    return http.post(
      buildDockerPath(`/agent/docker/compose/projects/${encodeURIComponent(name)}/restart`, nodeId),
    )
  },
  updateComposeProject: (name: string) => {
    return http.post(
      buildDockerPath(`/agent/docker/compose/projects/${encodeURIComponent(name)}/update`, nodeId),
    )
  },
  scaleComposeProject: (name: string, payload: { service: string; replicas: number }) => {
    return http.post(
      buildDockerPath(`/agent/docker/compose/projects/${encodeURIComponent(name)}/scale`, nodeId),
      payload,
    )
  },
  validateComposeYaml: (payload: { compose: string }) => {
    return http.post(buildDockerPath('/agent/docker/compose/validate', nodeId), payload)
  },
  deleteComposeProject: (name: string, params?: docker.ComposeProjectDeleteQuery) => {
    return http.delete(
      buildDockerPath(`/agent/docker/compose/projects/${encodeURIComponent(name)}`, nodeId),
      params,
    )
  },
  fetchComposeProjectLogs: (
    name: string,
    params?: { tail?: number; since?: string; until?: string },
  ) => {
    return http.get<docker.ComposeProjectLogs>(
      buildDockerPath(`/agent/docker/compose/projects/${encodeURIComponent(name)}/logs`, nodeId),
      params,
    )
  },
  listImages: () => {
    return http.get<docker.ImageSummary[]>(buildDockerPath('/agent/docker/images', nodeId))
  },
  fetchDaemonSettings: () => {
    return http.get<docker.DockerDaemonSettings>(
      buildDockerPath('/agent/docker/daemon/settings', nodeId),
    )
  },
  updateDaemonSettings: (payload: docker.DockerDaemonSettings) => {
    return http.put<docker.DockerDaemonSettings>(
      buildDockerPath('/agent/docker/daemon/settings', nodeId),
      payload,
      { timeout: 120000 },
    )
  },
  listVolumes: () => {
    return http.get(buildDockerPath('/agent/docker/volumes', nodeId))
  },
  createVolume: (payload: {
    name: string
    driver?: string
    driverOpts?: Record<string, string>
    labels?: Record<string, string>
  }) => {
    return http.post(buildDockerPath('/agent/docker/volumes', nodeId), payload)
  },
  removeVolume: (name: string) => {
    return http.delete(buildDockerPath(`/agent/docker/volumes/${encodeURIComponent(name)}`, nodeId))
  },
  inspectVolume: (name: string) => {
    return http.get(buildDockerPath(`/agent/docker/volumes/${encodeURIComponent(name)}`, nodeId))
  },
  dfSystem: () => {
    return http.get<docker.DockerDiskUsageSummary>(
      buildDockerPath('/agent/docker/system/df', nodeId),
    )
  },
  pruneSystem: () => {
    return http.post(buildDockerPath('/agent/docker/system/prune', nodeId), undefined, {
      timeout: 600000,
    })
  },
  /** 获取 Docker 网络摘要列表。 */
  listNetworks: () => {
    return http.get<docker.DockerNetworkSummary[]>(
      buildDockerPath('/agent/docker/networks', nodeId),
    )
  },
  createNetwork: (payload: docker.DockerNetworkCreateRequest) => {
    return http.post<docker.DockerNetworkCreateResult>(
      buildDockerPath('/agent/docker/networks', nodeId),
      payload,
    )
  },
  inspectNetwork: (id: string) => {
    return http.get<docker.DockerNetworkDetail>(
      buildDockerPath(`/agent/docker/networks/${encodeURIComponent(id)}`, nodeId),
    )
  },
  removeNetwork: (id: string) => {
    return http.delete(buildDockerPath(`/agent/docker/networks/${encodeURIComponent(id)}`, nodeId))
  },
  connectNetwork: (id: string, payload: docker.NetworkConnectRequest) => {
    return http.post(
      buildDockerPath(`/agent/docker/networks/${encodeURIComponent(id)}/connect`, nodeId),
      payload,
    )
  },
  disconnectNetwork: (id: string, payload: docker.NetworkDisconnectRequest) => {
    return http.post(
      buildDockerPath(`/agent/docker/networks/${encodeURIComponent(id)}/disconnect`, nodeId),
      payload,
    )
  },
  performAction: (data: docker.ActionRequest) => {
    return http.post<null>(buildDockerPath('/agent/docker/action', nodeId), data)
  },
  fetchDockerActivityLogs: (payload: docker.DockerActivityLogQuery) => {
    return http.post<docker.DockerActivityLogPage>(
      buildDockerPath('/agent/docker/activity-logs/query', nodeId),
      payload,
    )
  },
  fetchResourceUsageHistory: (hours?: number) => {
    return http.post(
      buildDockerPath('/agent/docker/stats/history', nodeId),
      hours ? { hours } : undefined,
    )
  },
  fetchContainerResourceUsageSummary: (id: string) => {
    return http.get<docker.ContainerResourceUsageSummary>(
      buildDockerPath(`/agent/docker/containers/${encodeURIComponent(id)}/stats/summary`, nodeId),
    )
  },
  fetchContainerResourceUsageHistory: (id: string, hours?: number) => {
    return http.post<docker.ContainerStatsHistoryAllResponse>(
      buildDockerPath(`/agent/docker/containers/${encodeURIComponent(id)}/stats/history`, nodeId),
      hours ? { hours } : undefined,
    )
  },
  fetchContainerResourceUsageHistoryAll: (payload: docker.ContainerStatsHistoryQuery) => {
    return http.post<docker.ContainerStatsHistoryAllResponse>(
      buildDockerPath('/agent/docker/containers/stats/history', nodeId),
      payload,
    )
  },
  fetchContainerResourceUsageSummaries: (payload: docker.ContainerStatsBatchRequest) => {
    return http.post<docker.ContainerStatsBatchResponse>(
      buildDockerPath('/agent/docker/containers/stats/summary', nodeId),
      payload,
    )
  },
  fetchContainerLogs: (containerId: string, tail = 100) => {
    return http.get<string[]>(
      buildDockerPath(`/agent/docker/containers/${encodeURIComponent(containerId)}/logs`, nodeId),
      { tail },
    )
  },
  installDocker: (timeoutSecs: number = 600) => {
    return http.post<{
      exitCode: number
      stdout: string
      stderr: string
      timedOut: boolean
      startedAt: number
      finishedAt: number
    }>(
      buildDockerPath('/agent/docker/install', nodeId),
      {
        mirror: 'official',
        timeoutSecs,
      },
      {
        timeout: timeoutSecs * 1000,
      },
    )
  },
  removeImage: (payload: docker.ImageRef) => {
    return http.delete<docker.ImageDeleteResponseItem[]>(
      buildDockerPath('/agent/docker/image/remove', nodeId),
      payload,
    )
  },
  resolveImage: (imageName: string) => {
    return http.post<ImageResolveResponse>(
      buildDockerPath('/agent/docker/images/resolve', nodeId),
      {
        imageName,
      },
    )
  },
  searchImages: (payload: { keyword: string; page: number; pageSize: number }) => {
    return http.post<ImageSearchResponse>(
      buildDockerPath('/agent/docker/images/search', nodeId),
      payload,
    )
  },
  fetchImageTags: (payload: { repository: string; page: number; pageSize: number }) => {
    return http.post<ImageTagsResponse>(
      buildDockerPath('/agent/docker/images/tags', nodeId),
      payload,
    )
  },
})

export interface ImageTagInfo {
  name: string
  fullSize?: number
  lastUpdated?: string
}

export interface ImageSearchResult {
  repository: string
  displayName: string
  description?: string
  starCount?: number
  pullCount?: number
  isOfficial: boolean
  isAutomated: boolean
}

export interface ImageSearchResponse {
  page: number
  pageSize: number
  hasMore: boolean
  results: ImageSearchResult[]
}

export interface ImageTagsResponse {
  repository: string
  page: number
  pageSize: number
  hasMore: boolean
  tags: ImageTagInfo[]
}

export interface ImageResolveResponse {
  repository: string
  displayName: string
  requestedTag?: string
  defaultTag?: string
  tags: ImageTagInfo[]
}

export type ImagePullStatus = 'pending' | 'running' | 'success' | 'failed' | 'cancelled'

export interface ImagePullProgress {
  taskId: string
  nodeId: string
  imageRef: string
  status: ImagePullStatus
  source?: 'target' | 'controller' | 'registry'
  stage: 'checking' | 'exporting' | 'uploading' | 'loading' | 'pulling'
  progressPercent: number
  statusText: string
  controllerError?: string
  registryError?: string
}

export const dockerApi = Object.assign(createScopedDockerApi(), {
  forNode: createScopedDockerApi,
  startImageTask: (payload: { nodeId: string; imageRef: string; sourceMode: 'controller-first' }) =>
    http.post<ImagePullProgress>('/docker/image-tasks', payload),
  fetchImageTaskProgress: (taskId: string) =>
    http.get<ImagePullProgress>(`/docker/image-tasks/${encodeURIComponent(taskId)}/progress`),
  cancelImageTask: (taskId: string) =>
    http.delete<ImagePullProgress>(`/docker/image-tasks/${encodeURIComponent(taskId)}`),
  fetchLocalImages: () => {
    return http.get<docker.ImageSummary[]>('/docker/local-images')
  },
})
