export interface SuiteCatalogItem {
  suiteId: string
  version: string
  name: string
  summary: string
  icon: string
  status: string
  checksum: string
  createdAt: string
  updatedAt: string
  category?: string
}

export interface SuiteInstanceSummary {
  instanceId: string
  suiteId: string
  version: string
  nodeId: string
  composeProjectName: string
  status: string
  lastError?: string
  createdAt: string
  updatedAt: string
}

export interface SuiteListResponse {
  catalog: SuiteCatalogItem[]
  instances: SuiteInstanceSummary[]
}

export interface SuiteUninstallRequest {
  removeData: boolean
}
