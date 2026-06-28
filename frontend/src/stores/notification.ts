import { defineStore } from 'pinia'
import { ref } from 'vue'
import { notificationsApi } from '@/api/modules/notifications'
import type { NotificationCreatePayload } from '@/api/generated'

// 定义通知类型
export type NotificationType = 'success' | 'error' | 'warning' | 'info'

// 定义单个通知实例的数据结构
export interface Notification {
  id: number // 唯一标识符
  type: NotificationType
  title?: string // 可选自定义标题，供套件等业务通知保留原始语义
  message: string // 显示的消息内容
  timestamp: number // 记录时间戳，用于历史记录
  duration: number // 持续时间 (毫秒)
}

// 默认持续时间
const DEFAULT_DURATION = 4000 // 4秒

export const useNotificationStore = defineStore('notification', () => {
  // 存储所有活动的通知实例
  const activeNotifications = ref<Notification[]>([])
  const nextId = ref(1)

  /**
   * @description 添加一个新的通知到队列中。
   * @param type 通知类型 ('success', 'error', 'warning', 'info')。
   * @param message 显示给用户的消息内容。
   * @param duration 可选：通知在屏幕上停留的时间 (毫秒)。
   */
  function addNotification(
    type: NotificationType,
    message: string,
    duration: number = DEFAULT_DURATION,
    persist: boolean = true,
    title?: string,
  ) {
    const newNotification: Notification = {
      id: nextId.value,
      type,
      title,
      message,
      timestamp: Date.now(),
      duration,
    }
    nextId.value += 1

    activeNotifications.value.push(newNotification)
    if (persist) {
      void persistNotification({
        level: type,
        message,
      })
    }

    // 自动在 duration 后移除活动通知
    setTimeout(() => {
      removeNotification(newNotification.id)
    }, duration)
  }

  /**
   * @description 根据 ID 从队列中移除一个通知。
   * @param id 要移除的通知的唯一 ID。
   */
  function removeNotification(id: number) {
    activeNotifications.value = activeNotifications.value.filter((n) => n.id !== id)
  }

  async function persistNotification(payload: NotificationCreatePayload) {
    try {
      await notificationsApi.create(payload)
    } catch (error) {
      console.error('Failed to persist notification:', error)
    }
  }

  // 快捷方法
  const success = (message: string, duration?: number) =>
    addNotification('success', message, duration)
  const error = (message: string, duration?: number) => addNotification('error', message, duration)
  const warning = (message: string, duration?: number) =>
    addNotification('warning', message, duration)
  const info = (message: string, duration?: number) => addNotification('info', message, duration)
  const showToast = (type: NotificationType, message: string, duration?: number, title?: string) =>
    addNotification(type, message, duration, false, title)

  return {
    activeNotifications,
    addNotification,
    removeNotification,
    success,
    error,
    warning,
    info,
    showToast,
  }
})
