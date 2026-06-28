import http from '@/api'
import type { ApiResponse } from '@/api/interface'
import type { TaskItem, TaskRun, UpsertTaskPayload } from '@/api/interface/task'

interface RawTaskItem extends Omit<TaskItem, 'nodeId'> {
  nodeId?: string
}

interface RawTaskRun extends Omit<TaskRun, 'nodeId'> {
  nodeId?: string
}

const normalizeTaskItem = (item: RawTaskItem): TaskItem => ({
  ...item,
  nodeId: item.nodeId ?? item.agentId,
})

const normalizeTaskRun = (item: RawTaskRun): TaskRun => ({
  ...item,
  nodeId: item.nodeId ?? item.agentId,
})

const normalizeResponse = <TRaw, TData>(
  response: ApiResponse<TRaw>,
  transform: (data: TRaw) => TData,
): ApiResponse<TData> => {
  if (response.data === undefined || response.data === null) {
    return {
      ...response,
      data: undefined,
    }
  }
  return {
    ...response,
    data: transform(response.data),
  }
}

export const taskApi = {
  list: async (nodeId?: string) => {
    const response = await http.get<RawTaskItem[]>('/tasks', nodeId ? { nodeId } : undefined)
    return normalizeResponse(response, (data) => data.map(normalizeTaskItem))
  },
  create: async (payload: UpsertTaskPayload) => {
    const response = await http.post<RawTaskItem>('/tasks', payload)
    return normalizeResponse(response, normalizeTaskItem)
  },
  update: async (id: number, payload: UpsertTaskPayload) => {
    const response = await http.put<RawTaskItem>(`/tasks/${id}`, payload)
    return normalizeResponse(response, normalizeTaskItem)
  },
  remove: (id: number) => {
    return http.delete<null>(`/tasks/${id}`)
  },
  toggle: async (id: number, enabled: boolean) => {
    const response = await http.post<RawTaskItem>(`/tasks/${id}/toggle`, { enabled })
    return normalizeResponse(response, normalizeTaskItem)
  },
  run: (id: number) => {
    return http.post<null>(`/tasks/${id}/run`)
  },
  sync: (id: number, force = false) => {
    return http.post<null>(`/tasks/${id}/sync`, undefined, { params: { force } })
  },
  runs: async (id: number, limit = 50) => {
    const response = await http.get<RawTaskRun[]>(`/tasks/${id}/runs`, { limit })
    return normalizeResponse(response, (data) => data.map(normalizeTaskRun))
  },
}
