import http from '@/api'
import * as apps from '../interface/apps'

export const appsApi = {
  fetchApps: () => {
    return http.get<apps.AppItem[]>('/apps')
  },
}
