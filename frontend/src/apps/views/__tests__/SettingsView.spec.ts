import { createPinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import { createMemoryHistory, createRouter } from 'vue-router'
import { defineComponent } from 'vue'
import { flushPromises, shallowMount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import SettingsView from '../SettingsView.vue'
import AboutSettings from '../settings/AboutSettings.vue'
import zh from '@/locales/zh'

const api = vi.hoisted(() => ({
  fetchAbout: vi.fn(),
  fetchInterfaces: vi.fn(),
  fetchNetwork: vi.fn(),
  fetchSecurity: vi.fn(),
  listReleases: vi.fn(),
}))

const windowManager = vi.hoisted(() => ({
  updateWindowRuntimeState: vi.fn(),
  openWindowWithPayload: vi.fn(),
}))

vi.mock('@/api/modules/system', () => ({
  systemApi: {
    forNode: () => ({ fetchAbout: api.fetchAbout }),
  },
}))

vi.mock('@/api/modules/seclab', () => ({
  seclabApi: {
    fetchInterfaces: api.fetchInterfaces,
    fetchNetwork: api.fetchNetwork,
    updateNetwork: vi.fn(),
  },
}))

vi.mock('@/api/modules/security', () => ({
  securityApi: {
    fetchSettings: api.fetchSecurity,
    updateSettings: vi.fn(),
    updateUsername: vi.fn(),
    updatePassword: vi.fn(),
  },
}))

vi.mock('@/api/modules/upgrades', () => ({
  upgradesApi: {
    listReleases: api.listReleases,
    uploadRelease: vi.fn(),
    createPlan: vi.fn(),
    startPlan: vi.fn(),
    detail: vi.fn(),
    deleteRelease: vi.fn(),
  },
}))

vi.mock('@/stores/window-manager', () => ({
  useWindowManagerStore: () => windowManager,
}))

vi.mock('@/stores/confirmation-modal', () => ({
  useConfirmationModalStore: () => ({ showConfirmation: vi.fn() }),
}))

const MenuStub = defineComponent({
  name: 'SecLabMenu',
  props: {
    modelValue: { type: String, required: true },
    items: { type: Array, required: true },
  },
  emits: ['update:modelValue'],
  template: `
    <div>
      <button
        v-for="item in items.flatMap((group) => group.children)"
        :key="item.key"
        :data-menu-key="item.key"
        @click="$emit('update:modelValue', item.key)"
      >
        {{ item.label }}
      </button>
    </div>
  `,
})

const CardStub = defineComponent({
  name: 'SecLabCard',
  template: '<div><slot name="header" /><slot /></div>',
})

const success = <T>(data: T) => ({ success: true, code: 200, message: '', data })

const mountSettings = async () => {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/', component: { template: '<div />' } }],
  })
  await router.push('/')
  await router.isReady()

  return shallowMount(SettingsView, {
    props: { windowId: 'settings-test' },
    global: {
      plugins: [
        createPinia(),
        router,
        createI18n({ legacy: false, locale: 'zh', messages: { zh } }),
      ],
      stubs: {
        SecLabCard: CardStub,
        SecLabMenu: MenuStub,
        SystemMonitoringSettings: true,
      },
    },
  })
}

describe('SettingsView', () => {
  beforeEach(() => {
    api.fetchAbout.mockResolvedValue(success(null))
    api.fetchInterfaces.mockResolvedValue(success([]))
    api.fetchNetwork.mockResolvedValue(
      success({ host: '::', port: 8443, publicHost: 'seclab.example.test' }),
    )
    api.fetchSecurity.mockResolvedValue(
      success({ safeEntry: '', safeEntryEnabled: false, passwordComplexity: false }),
    )
    api.listReleases.mockResolvedValue(success([]))
  })

  it('初次打开由分区组件自行加载数据', async () => {
    const wrapper = await mountSettings()
    await flushPromises()

    expect(wrapper.attributes('data-page')).toBe('settings')
    expect(api.fetchAbout).not.toHaveBeenCalled()
    expect(api.fetchInterfaces).not.toHaveBeenCalled()
    expect(api.fetchNetwork).not.toHaveBeenCalled()
    expect(api.fetchSecurity).not.toHaveBeenCalled()
    expect(api.listReleases).not.toHaveBeenCalled()
  })

  it('关于分区挂载后加载当前节点信息', async () => {
    shallowMount(AboutSettings, {
      global: {
        plugins: [createPinia(), createI18n({ legacy: false, locale: 'zh', messages: { zh } })],
      },
    })
    await flushPromises()

    expect(api.fetchAbout).toHaveBeenCalledTimes(1)
  })

  it('切换菜单时渲染对应分区组件', async () => {
    const wrapper = await mountSettings()
    await flushPromises()

    await wrapper.get('[data-menu-key="network"]').trigger('click')
    await flushPromises()
    expect(wrapper.findComponent({ name: 'NetworkSettings' }).exists()).toBe(true)
  })

  it('没有运行中升级计划时不创建后台轮询', async () => {
    const setIntervalSpy = vi.spyOn(window, 'setInterval')
    const wrapper = await mountSettings()

    await wrapper.get('[data-menu-key="upgrade"]').trigger('click')
    await flushPromises()

    expect(api.listReleases).toHaveBeenCalledTimes(1)
    expect(setIntervalSpy).not.toHaveBeenCalled()
  })
})
