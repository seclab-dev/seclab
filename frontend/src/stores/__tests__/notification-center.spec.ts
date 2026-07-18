import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { notificationsApi } from '@/api/modules/notifications'
import { useNotificationCenterStore } from '@/stores/notification-center'

vi.mock('@/api/modules/notifications', () => ({
  notificationsApi: { unreadSummary: vi.fn() },
}))

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

describe('NotificationCenterStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  it('未读摘要请求不重叠，失败时保留旧计数', async () => {
    const pending = deferred<{
      success: boolean
      code: number
      message: string
      data: { unreadCount: number; latestCreatedAt: string; version: string }
    }>()
    vi.mocked(notificationsApi.unreadSummary).mockReturnValueOnce(pending.promise)
    const store = useNotificationCenterStore()
    const first = store.refreshUnreadSummary()
    expect(await store.refreshUnreadSummary()).toBe(false)
    expect(notificationsApi.unreadSummary).toHaveBeenCalledTimes(1)
    pending.resolve({
      success: true,
      code: 200,
      message: '',
      data: {
        unreadCount: 8,
        latestCreatedAt: '2026-07-18T00:00:00Z',
        version: 'v1',
      },
    })
    await first
    expect(store.unreadCount).toBe(8)

    vi.mocked(notificationsApi.unreadSummary).mockRejectedValueOnce(new Error('offline'))
    await store.refreshUnreadSummary()
    expect(store.unreadCount).toBe(8)
    expect(store.error).toBe('offline')
  })
})
