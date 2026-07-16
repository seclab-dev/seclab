import { defineComponent } from 'vue'
import { createI18n } from 'vue-i18n'
import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ScriptListPage, ScriptRun, ScriptSummary } from '@/api/generated/scripts'
import { nodesApi } from '@/api/modules/nodes'
import { scriptsApi } from '@/api/modules/scripts'
import { useScriptLibrary } from '@/composables/useScriptLibrary'
import en from '@/locales/en'
import zh from '@/locales/zh'

vi.mock('@/api/modules/nodes', () => ({ nodesApi: { list: vi.fn() } }))
vi.mock('@/api/modules/scripts', () => ({
  scriptsApi: {
    list: vi.fn(),
    detail: vi.fn(),
    create: vi.fn(),
    update: vi.fn(),
    remove: vi.fn(),
    startRun: vi.fn(),
    runs: vi.fn(),
    run: vi.fn(),
    output: vi.fn(),
    cancel: vi.fn(),
  },
}))

const api = vi.mocked(scriptsApi)
const response = <T>(data: T) => ({ success: true, code: 200, message: '', data })
const summary = (scriptId: string): ScriptSummary => ({
  scriptId,
  name: scriptId,
  language: 'shell',
  revision: 1,
  ownership: { kind: 'custom' },
  capabilities: { canUpdate: true, canRemove: true, canClone: true, canRun: true },
  createdAt: '2026-07-16T00:00:00Z',
  updatedAt: '2026-07-16T00:00:00Z',
  updatedBy: 'admin',
})
const page = (items: ScriptSummary[]): ScriptListPage => ({
  items,
  page: 1,
  pageSize: 50,
  total: items.length,
  loadedAt: '2026-07-16T00:00:00Z',
})
const run = (): ScriptRun => ({
  runId: 'run-1',
  scriptId: 'script-1',
  scriptName: 'Demo',
  scriptRevision: 1,
  sourceSha256: 'a'.repeat(64),
  nodeId: 'local',
  nodeName: 'Local',
  status: 'queued',
  queuedAt: '2026-07-16T00:00:00Z',
  output: { available: false, truncated: false, sizeBytes: 0 },
  capabilities: { canCancel: true },
})
const deferred = <T>() => {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => (resolve = done))
  return { promise, resolve }
}
const Harness = defineComponent({ setup: useScriptLibrary, template: '<div />' })
const mountHarness = () =>
  mount(Harness, {
    global: { plugins: [createI18n({ legacy: false, locale: 'zh', messages: { zh, en } })] },
  })

describe('useScriptLibrary', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    localStorage.clear()
    vi.mocked(nodesApi.list).mockResolvedValue(response([]))
    api.list.mockResolvedValue(response(page([])))
  })
  afterEach(() => {
    vi.useRealTimers()
    vi.clearAllMocks()
  })

  it('旧搜索响应不能覆盖新结果', async () => {
    const oldRequest = deferred<ReturnType<typeof response<ScriptListPage>>>()
    const newRequest = deferred<ReturnType<typeof response<ScriptListPage>>>()
    api.list
      .mockImplementationOnce(() => oldRequest.promise)
      .mockImplementationOnce(() => newRequest.promise)
    const wrapper = mountHarness()
    wrapper.vm.filters.keyword = 'new'
    await vi.advanceTimersByTimeAsync(300)
    oldRequest.resolve(response(page([summary('old')])))
    newRequest.resolve(response(page([summary('new')])))
    await flushPromises()
    expect(wrapper.vm.scripts.map((item) => item.scriptId)).toEqual(['new'])
    wrapper.unmount()
  })

  it('刷新失败保留旧列表并显示警告', async () => {
    api.list.mockResolvedValueOnce(response(page([summary('script-1')])))
    const wrapper = mountHarness()
    await flushPromises()
    api.list.mockRejectedValueOnce(new Error('temporary failure'))
    await wrapper.vm.refreshScripts()
    expect(wrapper.vm.scripts).toHaveLength(1)
    expect(wrapper.vm.listState.warning).toBe('temporary failure')
    expect(wrapper.vm.listState.error).toBe('')
    wrapper.unmount()
  })

  it('重复执行只提交一个幂等请求', async () => {
    const pending = deferred<ReturnType<typeof response<ScriptRun>>>()
    api.startRun.mockImplementation(() => pending.promise)
    const wrapper = mountHarness()
    await flushPromises()
    const first = wrapper.vm.startRun('script-1', 'local', 30)
    await expect(wrapper.vm.startRun('script-1', 'local', 30)).rejects.toThrow()
    expect(api.startRun).toHaveBeenCalledTimes(1)
    pending.resolve(response(run()))
    await first
    wrapper.unmount()
  })
})
