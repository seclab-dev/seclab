import { defineComponent, nextTick } from 'vue'
import { createPinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import { mount } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'
import { useNodeStore } from '@/stores/node'
import { useWindowManagerStore } from '@/stores/window-manager'
import zh from '@/locales/zh'
import en from '@/locales/en'

vi.mock('@/api/modules/apps', () => ({
  appsApi: {
    fetchApps: vi.fn().mockResolvedValue({ success: false, data: null }),
  },
}))

vi.mock('@/api/modules/desktop', () => ({
  desktopApi: {
    fetchDesktopApps: vi.fn().mockResolvedValue({ success: false, data: null }),
    saveDesktopApps: vi.fn().mockResolvedValue({ success: true, data: null }),
  },
}))

const Harness = defineComponent({
  setup() {
    return { windowStore: useWindowManagerStore() }
  },
  template: '<div />',
})

describe('终端窗口节点绑定', () => {
  it('固定节点 payload，标题只显示可国际化的应用名', async () => {
    const pinia = createPinia()
    const i18n = createI18n({ legacy: false, locale: 'zh', messages: { zh, en } })
    const wrapper = mount(Harness, {
      global: {
        plugins: [pinia, i18n],
      },
    })
    const nodeStore = useNodeStore(pinia)
    nodeStore.setNodes([
      { id: 'node-a', name: '节点 A', address: '', status: 'online', tags: [] },
      { id: 'node-b', name: '节点 B', address: '', status: 'online', tags: [] },
    ])
    nodeStore.setCurrentNode('node-a')

    wrapper.vm.windowStore.openWindow('terminal')
    const terminalWindow = wrapper.vm.windowStore.openWindows[0]
    expect(terminalWindow?.payload).toEqual({ nodeId: 'node-a', nodeName: '节点 A' })
    expect(terminalWindow?.title).toBe('终端')

    nodeStore.setCurrentNode('node-b')
    expect(terminalWindow?.payload).toEqual({ nodeId: 'node-a', nodeName: '节点 A' })

    i18n.global.locale.value = 'en'
    await nextTick()
    expect(terminalWindow?.title).toBe('Terminal')
    wrapper.unmount()
  })
})
