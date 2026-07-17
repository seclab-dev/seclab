import { createI18n } from 'vue-i18n'
import { flushPromises, mount } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'
import { operationLogApi } from '@/api/modules/operation-log'
import OperationLogView from '@/apps/views/OperationLogView.vue'
import en from '@/locales/en'
import zh from '@/locales/zh'

vi.mock('@/api/modules/operation-log', () => ({
  operationLogApi: { query: vi.fn(), detail: vi.fn() },
}))

const response = (eventId: string) => ({
  success: true,
  code: 200,
  message: '',
  data: {
    total: 1,
    page: 1,
    pageSize: 20,
    items: [
      {
        eventId,
        occurredAt: '2026-07-17T00:00:00Z',
        module: 'nodes',
        eventCode: 'node_update',
        actor: { kind: 'user', displayName: eventId },
        clientIp: '::ffff:10.121.7.7',
        origin: { kind: 'master' },
        target: { kind: 'node', id: eventId },
        outcome: 'success',
        impact: 'info',
        traceId: 'trace',
        capabilities: { canViewDetails: true },
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

describe('OperationLogView', () => {
  it('只允许最新请求更新数据并提供稳定控件标识', async () => {
    const first = deferred<ReturnType<typeof response>>()
    const second = deferred<ReturnType<typeof response>>()
    vi.mocked(operationLogApi.query)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise)
    const wrapper = mount(OperationLogView, {
      global: { plugins: [createI18n({ legacy: false, locale: 'zh', messages: { zh, en } })] },
    })
    const vm = wrapper.vm as unknown as { load: () => Promise<void> }
    const latest = vm.load()
    second.resolve(response('latest'))
    await latest
    first.resolve(response('stale'))
    await flushPromises()
    expect(wrapper.text()).toContain('latest (::ffff:10.121.7.7)')
    expect(wrapper.text()).not.toContain('stale')
    expect(wrapper.text()).not.toContain('Master')
    expect(wrapper.text()).not.toContain('年')
    expect(wrapper.find('[data-ui="table"]').text()).toMatch(/2026-\d{2}-\d{2} \d{2}:\d{2}:\d{2}/)
    expect(wrapper.attributes('data-page')).toBe('operation-log')
    expect(wrapper.find('#operation-log-keyword').attributes('name')).toBe('operationLogKeyword')
    expect(wrapper.findAll('.filter-select')).toHaveLength(2)
    expect(wrapper.find('[data-ui="toolbar-actions"]').exists()).toBe(true)
    expect(vi.mocked(operationLogApi.query).mock.calls[0]?.[1]?.aborted).toBe(true)
    wrapper.unmount()
  })

  it('刷新失败时保留已经加载的数据', async () => {
    vi.mocked(operationLogApi.query)
      .mockReset()
      .mockResolvedValueOnce(response('kept'))
      .mockResolvedValueOnce({ success: false, code: 500, message: 'refresh failed' })
    const wrapper = mount(OperationLogView, {
      global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { zh, en } })] },
    })
    await flushPromises()
    await (wrapper.vm as unknown as { load: () => Promise<void> }).load()
    await flushPromises()
    expect(wrapper.text()).toContain('kept')
    expect(wrapper.find('[data-ui="refresh-warning"]').exists()).toBe(true)
    wrapper.unmount()
  })

  it('匿名认证事件使用可信操作者名称且不显示目标', async () => {
    const result = response('auth-failure')
    result.data.items[0] = {
      ...result.data.items[0],
      module: 'auth',
      eventCode: 'user_login',
      actor: { kind: 'anonymous', displayName: 'anonymous' },
      target: undefined,
      outcome: 'failure',
      impact: 'error',
    }
    vi.mocked(operationLogApi.query).mockReset().mockResolvedValue(result)
    const wrapper = mount(OperationLogView, {
      global: { plugins: [createI18n({ legacy: false, locale: 'zh', messages: { zh, en } })] },
    })
    await flushPromises()
    expect(wrapper.text()).toContain('匿名访问者')
    wrapper.unmount()
  })

  it('表格隐藏目标，详情抽屉展示领域名称与目标 ID', async () => {
    const result = response('named-target')
    result.data.items[0].target = {
      kind: 'node',
      id: '019f6f64-cc8d-7a30-9812-845e0f56f185',
      displayName: '边缘节点',
    }
    vi.mocked(operationLogApi.query).mockReset().mockResolvedValue(result)
    vi.mocked(operationLogApi.detail)
      .mockReset()
      .mockResolvedValue({
        success: true,
        code: 200,
        message: '',
        data: {
          ...result.data.items[0],
          parameters: {},
        },
      })
    const wrapper = mount(OperationLogView, {
      global: { plugins: [createI18n({ legacy: false, locale: 'zh', messages: { zh, en } })] },
    })
    await flushPromises()
    expect(wrapper.find('[data-ui="table"]').text()).not.toContain('边缘节点')
    expect(wrapper.text()).not.toContain('019f6f64-cc8d-7a30-9812-845e0f56f185')
    await wrapper.find('[data-ui="table"] button').trigger('click')
    await flushPromises()
    expect(document.body.textContent).toContain('边缘节点')
    expect(document.body.textContent).toContain('019f6f64-cc8d-7a30-9812-845e0f56f185')
    wrapper.unmount()
  })
})
