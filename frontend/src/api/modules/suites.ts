import http from '@/api'
import type * as suites from '../interface/suites'

export const suitesApi = {
  fetchSuites: () => {
    return http.get<suites.SuiteListResponse>('/suites/list')
  },
  importSuite: (file: File) => {
    const formData = new FormData()
    formData.append('file', file)
    return http.upload<unknown>('/suites/import', formData, {
      headers: { 'Content-Type': 'multipart/form-data' },
      timeout: 300000,
    })
  },
  installSuite: (suiteId: string, nodeId = 'local') => {
    return http.post<suites.SuiteInstallTaskResponse>(
      `/suites/${encodeURIComponent(suiteId)}/install`,
      { nodeId },
    )
  },
  fetchInstallProgress: (taskId: string) => {
    return http.get<suites.SuiteInstallProgress>('/suites/install-progress', { taskId })
  },
  cancelInstall: (taskId: string) => {
    return http.post<suites.SuiteInstallProgress>(
      `/suites/install-progress/${encodeURIComponent(taskId)}/cancel`,
    )
  },
  deleteSuite: (suiteId: string) => {
    return http.post<unknown>(`/suites/${encodeURIComponent(suiteId)}/delete`)
  },
  enableInstance: (instanceId: string) => {
    return http.post<suites.SuiteInstanceSummary>(
      `/suites/instance/${encodeURIComponent(instanceId)}/enable`,
      undefined,
      { timeout: 300000 },
    )
  },
  disableInstance: (instanceId: string) => {
    return http.post<suites.SuiteInstanceSummary>(
      `/suites/instance/${encodeURIComponent(instanceId)}/disable`,
      undefined,
      { timeout: 300000 },
    )
  },
  uninstallInstance: (instanceId: string, payload: suites.SuiteUninstallRequest) => {
    return http.post<unknown>(
      `/suites/instance/${encodeURIComponent(instanceId)}/uninstall`,
      payload,
      {
        timeout: 300000,
      },
    )
  },
}
