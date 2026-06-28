export interface TaskItem {
  id: number
  name: string
  nodeId: string
  agentId: string
  command: string
  cronExpr: string
  enabled: boolean
  timeoutSecs: number
  noOverlap: boolean
  lastRunAt?: number
  nextRunAt?: number
  createdAt: string
  updatedAt: string
  syncStatus: string
  syncError?: string
  syncedAt?: string
  revision: number
  lastStatus?: string
}

export interface TaskRun {
  id: number
  taskId: number
  nodeId: string
  agentId: string
  triggeredAt: number
  startedAt?: number
  finishedAt?: number
  status: string
  exitCode?: number
  logExcerpt?: string
  errorMessage?: string
  createdAt: string
}

export interface UpsertTaskPayload {
  name: string
  nodeId: string
  command: string
  cronExpr: string
  enabled: boolean
  timeoutSecs: number
  noOverlap: boolean
}
