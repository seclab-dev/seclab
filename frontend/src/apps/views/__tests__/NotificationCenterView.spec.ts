import { createPinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import { flushPromises, mount } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'
import { notificationsApi } from '@/api/modules/notifications'
import NotificationCenterView from '@/apps/views/NotificationCenterView.vue'
import en from '@/locales/en'
import zh from '@/locales/zh'

vi.mock('@/api/modules/notifications', () => ({
  notificationsApi: {
    query: vi.fn(),
    unreadSummary: vi.fn(),
    detail: vi.fn(),
    updateReadState: vi.fn(),
    readAll: vi.fn(),
    updateArchiveState: vi.fn(),
    updateBatchArchiveState: vi.fn(),
  },
}))

const response = (notificationId: string) => ({
  success: true,
  code: 200,
  message: '',
  data: {
    total: 1,
    page: 1,
    pageSize: 20,
    items: [
      {
        notificationId,
        createdAt: '2026-07-18T00:00:00Z',
        code: 'scriptRunFinished' as const,
        category: 'task' as const,
        attentionLevel: 'info' as const,
        outcome: 'success' as const,
        source: { module: 'scripts' as const, nodeName: 'Node A' },
        subject: { kind: 'script', id: 'script-1', displayName: notificationId },
        operationEventId: `event-${notificationId}`,
        parameters: {},
        capabilities: {
          canViewDetails: true,
          canMarkRead: true,
          canMarkUnread: false,
          canArchive: true,
          canRestore: false,
          canOpenTarget: false,
        },
      },
    ],
  },
})

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

function mountView() {
  return mount(NotificationCenterView, {
    global: {
      plugins: [createPinia(), createI18n({ legacy: false, locale: 'zh', messages: { zh, en } })],
    },
  })
}

describe('NotificationCenterView', () => {
  it('只允许最新列表请求更新状态并提供稳定控件标识', async () => {
    const first = deferred<ReturnType<typeof response>>()
    const second = deferred<ReturnType<typeof response>>()
    vi.mocked(notificationsApi.query)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise)
    const wrapper = mountView()
    const latest = (
      wrapper.vm as unknown as { loadNotifications: () => Promise<void> }
    ).loadNotifications()
    second.resolve(response('latest'))
    await latest
    first.resolve(response('stale'))
    await flushPromises()
    expect(wrapper.text()).toContain('latest')
    expect(wrapper.text()).not.toContain('stale')
    expect(wrapper.attributes('data-page')).toBe('notification-center')
    expect(wrapper.find('#notification-keyword').attributes('name')).toBe('notificationKeyword')
    expect(vi.mocked(notificationsApi.query).mock.calls[0]?.[1]?.aborted).toBe(true)
    wrapper.unmount()
  })

  it('刷新失败时保留已加载通知并展示非阻塞警告', async () => {
    vi.mocked(notificationsApi.query)
      .mockReset()
      .mockResolvedValueOnce(response('kept'))
      .mockResolvedValueOnce({ success: false, code: 500, message: 'refresh failed' })
    const wrapper = mountView()
    await flushPromises()
    await (
      wrapper.vm as unknown as { loadNotifications: (silent: boolean) => Promise<void> }
    ).loadNotifications(true)
    await flushPromises()
    expect(wrapper.text()).toContain('kept')
    expect(wrapper.find('[data-slot="refresh-warning"]').exists()).toBe(true)
    wrapper.unmount()
  })

  it('使用表格当前页全选并通过选择栏清除选择', async () => {
    vi.mocked(notificationsApi.query).mockReset().mockResolvedValue(response('selectable'))
    const wrapper = mountView()
    await flushPromises()
    expect(wrapper.text()).toContain('关注级别')
    expect(wrapper.text()).toContain('提示')
    expect(wrapper.text()).toContain('结果')
    expect(wrapper.text()).toContain('成功')
    expect(wrapper.text()).toContain('目标')
    expect(wrapper.get('[data-ui="notification-table"]').text()).toContain('selectable')
    expect(wrapper.get('[data-ui="notification-table"]').text()).not.toContain('Node A')

    await wrapper.get('[data-ui="table-select-all"] input').setValue(true)
    await flushPromises()
    expect(wrapper.get('[data-ui="selection-bar"] .sl-selection-count').text()).toBe('1')
    expect(wrapper.get('[data-ui="table-row-selection"] input').element.checked).toBe(true)

    await wrapper.get('[data-ui="clear-selection"]').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-ui="selection-bar"]').exists()).toBe(false)
    expect(wrapper.get('[data-ui="table-row-selection"] input').element.checked).toBe(false)
    wrapper.unmount()
  })
})
