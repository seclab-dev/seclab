import http from '@/api'
import * as logging from '../interface/logging'
import type { RuntimeLogFile, RuntimeLogQuery, RuntimeLogQueryResult } from '@/api/generated'

export const platformLogApi = {
  fetchLogs: (payload: logging.PlatformLogQuery) => {
    return http.post<logging.PlatformLogList>('/platform/logs', payload)
  },
  fetchFilteredLogs: (payload: logging.PlatformLogQuery) => {
    return http.post<logging.PlatformLogList>('/platform/logs', payload)
  },
  fetchRuntimeLogFiles: (params?: { service?: string; nodeId?: string }) => {
    return http.get<RuntimeLogFile[]>('/platform/runtime-logs/files', params)
  },
  queryRuntimeLogs: (payload: RuntimeLogQuery) => {
    return http.post<RuntimeLogQueryResult>('/platform/runtime-logs/query', payload)
  },
}
