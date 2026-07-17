import http from '@/api'
import type { SystemAboutInfo, FirewallStatusInfo } from '../interface/system'

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
  detectFirewall: () => {
    return http.get<FirewallStatusInfo>(buildSystemPath('/agent/system/firewall/detect', nodeId))
  },
})

export const systemApi = Object.assign(createScopedSystemApi(), {
  forNode: createScopedSystemApi,
})
