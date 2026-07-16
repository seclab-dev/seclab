/**
 * @file scripts.ts
 * @description 脚本库稳定领域 API；正文按需加载，运行使用幂等异步接口。
 */

import type { AxiosRequestConfig } from 'axios'
import http from '@/api'
import type {
  CreateScriptRequest,
  CreateScriptRunRequest,
  ScriptDetail,
  ScriptListPage,
  ScriptRun,
  ScriptRunOutputPage,
  ScriptRunPage,
  ScriptRunStatus,
  UpdateScriptRequest,
} from '@/api/generated/scripts'

export type ScriptListQuery = {
  keyword?: string
  page?: number
  pageSize?: number
  sortBy?: 'name' | 'updatedAt'
  sortOrder?: 'asc' | 'desc'
}

export type ScriptRunListQuery = {
  scriptId?: string
  nodeId?: string
  status?: ScriptRunStatus
  page?: number
  pageSize?: number
}

const config = (signal?: AbortSignal): AxiosRequestConfig => ({ signal })

export const scriptsApi = {
  list: (query: ScriptListQuery = {}, signal?: AbortSignal) =>
    http.get<ScriptListPage>('/scripts', query, config(signal)),
  detail: (scriptId: string, signal?: AbortSignal) =>
    http.get<ScriptDetail>(`/scripts/${scriptId}`, undefined, config(signal)),
  create: (payload: CreateScriptRequest) => http.post<ScriptDetail>('/scripts', payload),
  update: (scriptId: string, payload: UpdateScriptRequest) =>
    http.patch<ScriptDetail>(`/scripts/${scriptId}`, payload),
  remove: (scriptId: string) => http.delete<void>(`/scripts/${scriptId}`),
  startRun: (scriptId: string, payload: CreateScriptRunRequest, idempotencyKey: string) =>
    http.post<ScriptRun>(`/scripts/${scriptId}/runs`, payload, {
      headers: { 'Idempotency-Key': idempotencyKey },
    }),
  runs: (query: ScriptRunListQuery = {}, signal?: AbortSignal) =>
    http.get<ScriptRunPage>('/script-runs', query, config(signal)),
  run: (runId: string, signal?: AbortSignal) =>
    http.get<ScriptRun>(`/script-runs/${runId}`, undefined, config(signal)),
  output: (runId: string, cursor = 0, limit = 100, signal?: AbortSignal) =>
    http.get<ScriptRunOutputPage>(
      `/script-runs/${runId}/output`,
      { cursor, limit },
      config(signal),
    ),
  cancel: (runId: string) => http.post<ScriptRun>(`/script-runs/${runId}/cancel`),
}

export type {
  CreateScriptRequest,
  CreateScriptRunRequest,
  ScriptDetail,
  ScriptListPage,
  ScriptRun,
  ScriptRunOutputPage,
  ScriptRunPage,
  ScriptRunStatus,
  ScriptSummary,
  UpdateScriptRequest,
} from '@/api/generated/scripts'
