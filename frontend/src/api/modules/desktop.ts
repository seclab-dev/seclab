import http from '@/api'
import * as desktop from '../interface/desktop'

export const desktopApi = {
  fetchDesktopApps: (nodeId?: string) => {
    return http.get<desktop.DesktopAppsResponse>('/desktop/shortcuts', {
      params: nodeId ? { nodeId } : undefined,
    })
  },
  saveDesktopApps: (payload: desktop.DesktopAppsPayload, nodeId?: string) => {
    return http.put<unknown>('/desktop/shortcuts', payload, {
      params: nodeId ? { nodeId } : undefined,
    })
  },
}
