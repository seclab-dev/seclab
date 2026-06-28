import http from '@/api'
import type {
  AlertThresholds,
  DiskInfo,
  HostSystemSummary,
  SystemAboutInfo,
  SystemCollectorStatus,
  SystemHistoryPoint,
  FirewallStatusInfo,
} from '../interface/system'

const buildSystemPath = (path: string, nodeId?: string) => {
  if (!nodeId || nodeId === 'local') {
    return path
  }
  return `/node/${encodeURIComponent(nodeId)}${path}`
}

const createScopedSystemApi = (nodeId?: string) => ({
  fetchAbout: () => {
    return http.get<SystemAboutInfo>(buildSystemPath('/agent/system/about', nodeId))
  },
  fetchSummary: () => {
    return http.get<HostSystemSummary>(buildSystemPath('/agent/system/summary', nodeId))
  },
  fetchDisks: () => {
    return http.get<DiskInfo[]>(buildSystemPath('/agent/system/disks', nodeId))
  },
  mountPartition: (diskName: string, partitionName: string, mountpoint?: string) => {
    return http.post<null>(
      buildSystemPath(
        `/agent/system/disks/${encodeURIComponent(diskName)}/partitions/${encodeURIComponent(partitionName)}/mount`,
        nodeId,
      ),
      mountpoint ? { mountpoint } : {},
    )
  },
  unmountPartition: (diskName: string, partitionName: string) => {
    return http.post<null>(
      buildSystemPath(
        `/agent/system/disks/${encodeURIComponent(diskName)}/partitions/${encodeURIComponent(partitionName)}/unmount`,
        nodeId,
      ),
      {},
    )
  },
  formatPartition: (diskName: string, partitionName: string, filesystem: 'ext4' = 'ext4') => {
    return http.post<null>(
      buildSystemPath(
        `/agent/system/disks/${encodeURIComponent(diskName)}/partitions/${encodeURIComponent(partitionName)}/format`,
        nodeId,
      ),
      { filesystem },
    )
  },
  initializeDisk: (diskName: string, filesystem: 'ext4' = 'ext4', mountpoint?: string) => {
    return http.post<null>(
      buildSystemPath(`/agent/system/disks/${encodeURIComponent(diskName)}/initialize`, nodeId),
      {
        filesystem,
        ...(mountpoint ? { mountpoint } : {}),
      },
    )
  },
  fetchHistory: (payload?: { hours?: number }) => {
    return http.post<SystemHistoryPoint[]>(
      buildSystemPath('/agent/system/history', nodeId),
      payload,
    )
  },
  clearHistory: () => {
    return http.delete<null>(buildSystemPath('/agent/system/history', nodeId))
  },
  fetchCollectorStatus: () => {
    return http.get<SystemCollectorStatus>(
      buildSystemPath('/agent/system/collector/status', nodeId),
    )
  },
  setCollectorStatus: (enabled: boolean) => {
    return http.put<SystemCollectorStatus>(
      buildSystemPath('/agent/system/collector/status', nodeId),
      { enabled },
    )
  },
  detectFirewall: () => {
    return http.get<FirewallStatusInfo>(buildSystemPath('/agent/system/firewall/detect', nodeId))
  },
  fetchAlertThresholds: () => {
    return http.get<AlertThresholds>(buildSystemPath('/agent/system/alert/thresholds', nodeId))
  },
  setAlertThresholds: (thresholds: AlertThresholds) => {
    return http.put<AlertThresholds>(
      buildSystemPath('/agent/system/alert/thresholds', nodeId),
      thresholds,
    )
  },
})

export const systemApi = Object.assign(createScopedSystemApi(), {
  forNode: createScopedSystemApi,
  getTerminalWsPath: (nodeId?: string): string => {
    return nodeId && nodeId !== 'local'
      ? `/node/${encodeURIComponent(nodeId)}/agent/websocket/host-terminal/ws`
      : '/agent/websocket/host-terminal/ws'
  },
})
