import http from '@/api'

export interface ScriptItem {
  scriptId: string
  title: string
  description?: string
  content: string
  createdAt: string
  updatedAt: string
}

export interface ScriptPayload {
  title: string
  description?: string
  content: string
}

export interface ExecuteScriptResult {
  exitCode: number
  stdout: string
  stderr: string
  timedOut: boolean
  startedAt: number
  finishedAt: number
}

export const scriptsApi = {
  fetchScripts: () => {
    return http.get<ScriptItem[]>('/scripts')
  },
  fetchScriptDetail: (scriptId: string) => {
    return http.get<ScriptItem>(`/scripts/${scriptId}/detail`)
  },
  createScript: (payload: ScriptPayload) => {
    return http.post<ScriptItem>('/scripts', payload)
  },
  updateScript: (scriptId: string, payload: ScriptPayload) => {
    return http.put<ScriptItem>(`/scripts/${scriptId}/update`, payload)
  },
  deleteScript: (scriptId: string) => {
    return http.delete(`/scripts/${scriptId}/remove`)
  },
  /** 高危脚本执行能力，仅供脚本库和计划任务使用；业务动作应使用专用 API。 */
  executeScript: (nodeId: string, command: string, timeoutSecs: number = 300) => {
    return http.post<ExecuteScriptResult>(
      `/node/${nodeId}/agent/tasks/execute`,
      {
        command,
        timeoutSecs,
      },
      {
        timeout: 300000,
      },
    )
  },
}
