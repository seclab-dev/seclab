import http from '@/api'
import * as desktop from '../interface/desktop'

export const desktopApi = {
  fetchDesktopApps: () => {
    return http.get<desktop.DesktopAppsResponse>('/desktop/shortcuts')
  },
  saveDesktopApps: (payload: desktop.DesktopAppsPayload) => {
    return http.put<unknown>('/desktop/shortcuts', payload)
  },
}
