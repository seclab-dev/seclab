import http from '@/api'
import * as docker from '../interface/docker'
import * as logging from '../interface/logging'

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
    return http.get(buildDockerPath('/agent/docker/system/df', nodeId))
  },
  pruneSystem: () => {
    return http.post(buildDockerPath('/agent/docker/system/prune', nodeId))
  },
  /** 获取所有网络的详细信息。 */
  listNetworks: () => {
    return http.get<docker.Network[]>(buildDockerPath('/agent/docker/networks', nodeId))
  },
  createNetwork: (payload: docker.NetworkCreateRequest) => {
    return http.post(buildDockerPath('/agent/docker/networks', nodeId), payload)
  },
  inspectNetwork: (id: string) => {
    return http.get<docker.Network>(
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
  fetchDockerLogs: (payload: logging.PlatformLogQuery) => {
    return http.post<logging.PlatformLogList>(
      buildDockerPath('/agent/docker/logs', nodeId),
      payload,
    )
  },
  fetchResourceUsageHistory: (hours?: number) => {
    return http.post<docker.ResourceUsageHistory>(
      buildDockerPath('/agent/docker/stats/history', nodeId),
      hours ? { hours } : undefined,
    )
  },
  fetchContainerResourceUsageSummary: (id: string) => {
    return http.get<docker.ResourceUsageSummary>(
      buildDockerPath(`/agent/docker/containers/${encodeURIComponent(id)}/stats/summary`, nodeId),
    )
  },
  fetchContainerResourceUsageHistory: (id: string, hours?: number) => {
    return http.post<docker.ResourceUsageHistory>(
      buildDockerPath(`/agent/docker/containers/${encodeURIComponent(id)}/stats/history`, nodeId),
      hours ? { hours } : undefined,
    )
  },
  fetchContainerResourceUsageHistoryAll: (hours?: number) => {
    return http.post<docker.ContainerStatsHistoryAllResponse>(
      buildDockerPath('/agent/docker/containers/stats/history', nodeId),
      hours ? { hours } : undefined,
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
})

export interface DistributeNodeStatus {
  progressPercent: number
  status: 'waiting' | 'exporting' | 'uploading' | 'loading' | 'success' | 'failed'
  error?: string
}

export interface DistributeSession {
  taskId: string
  createdAt: number
  nodeStatuses: Record<string, DistributeNodeStatus>
}

export const dockerApi = Object.assign(createScopedDockerApi(), {
  forNode: createScopedDockerApi,
  distributeImage: (formData: FormData) => {
    return http.post<string>('/docker/images/distribute', formData, {
      headers: {
        'Content-Type': 'multipart/form-data',
      },
    })
  },
  fetchDistributeStatus: (taskId: string) => {
    return http.get<DistributeSession>('/docker/images/distribute/status', { taskId })
  },
  fetchLocalImages: () => {
    return http.get<docker.ImageSummary[]>('/docker/local-images')
  },
  pullLocalImage: (imageName: string) => {
    return http.post<string>('/docker/local-images/pull', { imageName })
  },
  distributeLocalImage: (payload: { imageName: string; nodeIds: string[] }) => {
    return http.post<string>('/docker/images/distribute/local', payload)
  },
})
