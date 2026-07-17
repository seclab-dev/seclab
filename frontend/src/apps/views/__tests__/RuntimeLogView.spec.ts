import { createI18n } from 'vue-i18n'
import { flushPromises, mount } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'
import { nodesApi } from '@/api/modules/nodes'
import { runtimeLogApi } from '@/api/modules/runtime-log'
import RuntimeLogView from '@/apps/views/RuntimeLogView.vue'
import en from '@/locales/en'
import zh from '@/locales/zh'

vi.mock('@/api/modules/nodes', () => ({ nodesApi: { list: vi.fn() } }))
vi.mock('@/api/modules/runtime-log', () => ({
  runtimeLogApi: { files: vi.fn(), query: vi.fn() },
}))

function mountView() {
  return mount(RuntimeLogView, {
    global: { plugins: [createI18n({ legacy: false, locale: 'zh', messages: { zh, en } })] },
  })
}

describe('RuntimeLogView', () => {
  it('默认折叠低频筛选并将三个功能按钮收敛到独立动作区', async () => {
    vi.mocked(nodesApi.list).mockResolvedValue({
      success: true,
      code: 200,
      message: '',
      data: [],
    })
    vi.mocked(runtimeLogApi.files).mockResolvedValue({
      success: true,
      code: 200,
      message: '',
      data: { availability: 'available', files: [] },
    })

    const wrapper = mountView()
    await flushPromises()

    const actions = wrapper.find('[data-ui="toolbar-actions"]')
    expect(actions.findAll('button')).toHaveLength(3)
    expect(wrapper.find('[data-ui="filters"]').exists()).toBe(false)
    expect(wrapper.find('#runtime-log-level').exists()).toBe(false)
    expect(wrapper.find('#runtime-log-target').exists()).toBe(false)
    expect(wrapper.find('#runtime-log-keyword').exists()).toBe(false)

    await actions.find('button').trigger('click')

    expect(wrapper.find('[data-ui="filters"]').exists()).toBe(true)
    expect(wrapper.find('#runtime-log-level').exists()).toBe(true)
    expect(wrapper.find('#runtime-log-target').attributes('name')).toBe('runtimeLogTarget')
    expect(wrapper.find('#runtime-log-keyword').attributes('name')).toBe('runtimeLogKeyword')
    expect(actions.find('button').attributes('aria-expanded')).toBe('true')
    wrapper.unmount()
  })
})
