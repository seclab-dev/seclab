import http from '@/api'
import type { DiskInfo, SystemAboutInfo, FirewallStatusInfo } from '../interface/system'

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
  detectFirewall: () => {
    return http.get<FirewallStatusInfo>(buildSystemPath('/agent/system/firewall/detect', nodeId))
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
