import http from '@/api'
import type {
  NotificationCreatePayload,
  NotificationList,
  NotificationQuery,
  NotificationBatchDeletePayload,
} from '@/api/generated'

export const notificationsApi = {
  create: (payload: NotificationCreatePayload) => {
    return http.post<null>('/notifications', payload)
  },
  list: (params?: NotificationQuery) => {
    return http.get<NotificationList>('/notifications', params)
  },
  clear: () => {
    return http.delete<null>('/notifications')
  },
  batchRemove: (payload: NotificationBatchDeletePayload) => {
    return http.delete<null>('/notifications/batch', payload)
  },
}
