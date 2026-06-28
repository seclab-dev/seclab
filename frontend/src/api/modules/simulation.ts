import http from '@/api'
import { DEFAULT_CONTROLLER_PORT } from '@/utils/constants'

export interface SimRule {
  id: number
  name: string
  nameEn: string
  cve?: string
  category: string
  descriptionZh: string
  descriptionEn: string
  protocol: string
  defaultPort?: number
  configYaml: string
  createdAt: string
  updatedAt: string
}

export interface SimInstance {
  instanceId: string
  nodeId: string
  ruleId: number
  listenPort: number
  status: 'active' | 'inactive' | 'error'
  errorMessage?: string
  pcapStatus: 'idle' | 'capturing' | 'ready'
  pcapStartTime?: number
  pcapFilePath?: string
  createdAt: string
  updatedAt: string
}

export interface SimLog {
  logId?: number
  instanceId: string
  nodeId: string
  clientIp: string
  clientPort: number
  eventType: string
  detailSummary: string
  payloadHex?: string
  pcapFilePath?: string
  timestamp: string
}

export interface SimRulePackage {
  packageId: string
  version: string
  rulesetFormatVersion: number
  minSeclabVersion: string
  ruleCount: number
  signatureHex: string
  archiveSha256: string
  status: 'active' | 'superseded'
  importedAt: string
}

export interface CreateRuleReq {
  name: string
  nameEn?: string
  cve?: string
  category?: string
  descriptionZh?: string
  descriptionEn?: string
  protocol: string
  defaultPort?: number
  configYaml: string
}

export interface DeploySimReq {
  nodeId: string
  port: number
  ruleId: number
  seclabCallbackUrl: string
}

export const simulationApi = {
  createRule: (data: CreateRuleReq) => {
    return http.post<SimRule>('/simulation/rule', data)
  },
  listRules: () => {
    return http.get<SimRule[]>('/simulation/rules')
  },
  deleteRule: (id: number) => {
    return http.delete<unknown>(`/simulation/rule/${id}`)
  },
  importRulePackage: (file: File) => {
    const formData = new FormData()
    formData.append('archive', file)
    return http.post<SimRulePackage>('/simulation/rule-package/import', formData, {
      headers: {
        'Content-Type': 'multipart/form-data',
      },
    })
  },
  listRulePackages: () => {
    return http.get<SimRulePackage[]>('/simulation/rule-packages/list')
  },
  getCurrentRulePackage: () => {
    return http.get<SimRulePackage | null>('/simulation/rule-package/current')
  },
  deploySimulation: (data: DeploySimReq) => {
    return http.post<SimInstance>('/simulation/deploy', data)
  },
  undeploySimulation: (instanceId: string) => {
    return http.post<unknown>('/simulation/undeploy', { instanceId })
  },
  listInstances: (nodeId: string) => {
    return http.get<SimInstance[]>(`/simulation/node/${nodeId}/instances`)
  },
  listLogs: (nodeId: string) => {
    return http.get<SimLog[]>(`/simulation/node/${nodeId}/logs`)
  },
  startCapture: (instanceId: string) => {
    return http.post<unknown>('/simulation/capture/start', { instanceId })
  },
  stopCapture: (instanceId: string) => {
    return http.post<unknown>('/simulation/capture/stop', { instanceId })
  },
  resetCapture: (instanceId: string) => {
    return http.post<unknown>('/simulation/capture/reset', { instanceId })
  },
  getPcapDownloadUrl: (pcapFilePath?: string) => {
    return pcapFilePath ? `/api/v1/simulation/pcap/download/${pcapFilePath}` : ''
  },
  getCallbackUrl: (origin: string) => {
    const correctedOrigin = origin.includes(':5173')
      ? origin.replace(':5173', `:${DEFAULT_CONTROLLER_PORT}`)
      : origin
    return `${correctedOrigin}/api/v1/simulation-public/log`
  },
}
