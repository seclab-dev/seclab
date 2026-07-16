import { createPinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { nodesApi } from '@/api/modules/nodes'
import { scriptsApi } from '@/api/modules/scripts'
import ScriptManagerView from '@/apps/views/ScriptManagerView.vue'
import en from '@/locales/en'
import zh from '@/locales/zh'

vi.mock('@/components/editor/MonacoEditor.vue', () => ({
  default: {
    name: 'MonacoEditor',
    props: ['id', 'name', 'modelValue'],
    template: '<textarea :id="id" :name="name" />',
  },
}))
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

const response = <T>(data: T) => ({ success: true, code: 200, message: '', data })
const script = {
  scriptId: 'script-readonly',
  name: 'Readonly',
  language: 'shell' as const,
  revision: 1,
  ownership: { kind: 'custom' as const },
  capabilities: { canUpdate: false, canRemove: false, canClone: false, canRun: false },
  createdAt: '2026-07-16T00:00:00Z',
  updatedAt: '2026-07-16T00:00:00Z',
  updatedBy: 'admin',
}

describe('ScriptManagerView', () => {
  beforeEach(() => {
    localStorage.clear()
    vi.mocked(nodesApi.list).mockResolvedValue(response([]))
    vi.mocked(scriptsApi.list).mockResolvedValue(
      response({
        items: [script],
        page: 1,
        pageSize: 50,
        total: 1,
        loadedAt: '2026-07-16T00:00:00Z',
      }),
    )
    vi.mocked(scriptsApi.detail).mockResolvedValue(
      response({
        ...script,
        source: { content: 'echo ok', sizeBytes: 7, sha256: 'a'.repeat(64) },
        executionDefaults: { timeoutSeconds: 300 },
      }),
    )
  })

  it('提供稳定标记和真实搜索控件标识', async () => {
    const wrapper = mount(ScriptManagerView, {
      global: {
        plugins: [createPinia(), createI18n({ legacy: false, locale: 'zh', messages: { zh, en } })],
      },
    })
    await flushPromises()
    expect(wrapper.attributes('data-page')).toBe('script-library')
    expect(wrapper.find('[data-ui="toolbar"]').exists()).toBe(true)
    expect(wrapper.find('[data-ui="script-list"]').exists()).toBe(true)
    expect(wrapper.find('#script-library-search').attributes('name')).toBe('scriptLibrarySearch')
    wrapper.unmount()
  })

  it('能力为 false 时不渲染编辑、执行和删除按钮', async () => {
    const wrapper = mount(ScriptManagerView, {
      global: {
        plugins: [createPinia(), createI18n({ legacy: false, locale: 'zh', messages: { zh, en } })],
      },
    })
    await flushPromises()
    const text = wrapper.text()
    expect(text).not.toContain('执行脚本')
    expect(wrapper.find('.editor-actions').text()).not.toContain('编辑')
    expect(wrapper.find('.editor-actions').text()).not.toContain('删除')
    wrapper.unmount()
  })
})
