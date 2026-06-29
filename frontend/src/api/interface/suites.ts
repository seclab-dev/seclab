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

export interface SuiteInstallTaskResponse {
  taskId: string
  instanceId: string
}

export interface SuiteInstallProgress {
  taskId: string
  instanceId: string
  nodeId: string
  progressPercent: number
  status: string
  currentStep: string
  currentImage?: string
  isFinished: boolean
  error?: string
  cancelRequested: boolean
}

export interface SuiteUninstallRequest {
  removeData: boolean
}
