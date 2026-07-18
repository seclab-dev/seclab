import { defineStore } from 'pinia'
import { ref } from 'vue'
import { notificationsApi } from '@/api/modules/notifications'

const POLL_INTERVAL_MS = 30_000

/** Shell 与通知中心共享的个人未读摘要状态。 */
export const useNotificationCenterStore = defineStore('notification-center', () => {
  const unreadCount = ref(0)
  const latestCreatedAt = ref<string>()
  const version = ref('')
  const loadedAt = ref<string>()
  const error = ref<string>()
  const polling = ref(false)

  let timer: number | undefined
  let controller: AbortController | undefined
  let requestSequence = 0
  let started = false

  /** 查询未读摘要；请求重叠时复用当前状态，失败不会清空旧计数。 */
  async function refreshUnreadSummary() {
    if (polling.value) return false
    polling.value = true
    const sequence = ++requestSequence
    const requestController = new AbortController()
    controller = requestController
    try {
      const response = await notificationsApi.unreadSummary(requestController.signal)
      if (sequence !== requestSequence || !response.success || !response.data) return false
      const changed = version.value !== '' && version.value !== response.data.version
      unreadCount.value = response.data.unreadCount
      latestCreatedAt.value = response.data.latestCreatedAt
      version.value = response.data.version
      loadedAt.value = new Date().toISOString()
      error.value = undefined
      return changed
    } catch (cause) {
      if (sequence === requestSequence && !requestController.signal.aborted) {
        error.value = cause instanceof Error ? cause.message : 'Failed to load unread notifications'
      }
      return false
    } finally {
      if (sequence === requestSequence) {
        polling.value = false
        controller = undefined
      }
    }
  }

  /** 登录桌面后启动可见性轮询。 */
  function startPolling() {
    if (started) return
    started = true
    void refreshUnreadSummary()
    timer = window.setInterval(() => {
      if (document.visibilityState === 'visible') void refreshUnreadSummary()
    }, POLL_INTERVAL_MS)
    document.addEventListener('visibilitychange', handleVisibilityChange)
  }

  function handleVisibilityChange() {
    if (document.visibilityState === 'visible') void refreshUnreadSummary()
  }

  /** 登出或 Shell 卸载时停止轮询并清除当前用户状态。 */
  function stopPolling(resetState = false) {
    started = false
    if (timer !== undefined) window.clearInterval(timer)
    timer = undefined
    document.removeEventListener('visibilitychange', handleVisibilityChange)
    requestSequence += 1
    controller?.abort()
    controller = undefined
    polling.value = false
    if (resetState) {
      unreadCount.value = 0
      latestCreatedAt.value = undefined
      version.value = ''
      loadedAt.value = undefined
      error.value = undefined
    }
  }

  return {
    unreadCount,
    latestCreatedAt,
    version,
    loadedAt,
    error,
    polling,
    refreshUnreadSummary,
    startPolling,
    stopPolling,
  }
})
