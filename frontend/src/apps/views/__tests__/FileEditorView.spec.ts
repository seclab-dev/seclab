import { createI18n } from 'vue-i18n'
import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import FileEditorView from '../FileEditorView.vue'
import zh from '@/locales/zh'
import type { EditorTab } from '@/composables/useFileEditor'

const editor = vi.hoisted(() => ({
  loadFileContent: vi.fn(),
  saveFileContent: vi.fn(),
  nodeId: '',
  updateWindowRuntimeState: vi.fn(),
}))

vi.mock('@/stores/node', () => ({ useNodeStore: () => ({ currentNodeId: 'global-node' }) }))
vi.mock('@/stores/window-manager', () => ({
  useWindowManagerStore: () => ({ updateWindowRuntimeState: editor.updateWindowRuntimeState }),
}))
vi.mock('@/composables/useFileEditor', () => ({
  useFileEditor: (nodeId: { value: string }) => {
    editor.nodeId = nodeId.value
    return {
      loadFileContent: editor.loadFileContent,
      saveFileContent: editor.saveFileContent,
    }
  },
}))

vi.mock('@/components/editor/MonacoEditor.vue', async () => {
  const { defineComponent, h } = await import('vue')
  return {
    default: defineComponent({
      name: 'MonacoEditor',
      inheritAttrs: false,
      props: {
        modelValue: { type: String, default: '' },
        documentKey: { type: String, default: '' },
        id: { type: String, default: '' },
        name: { type: String, default: '' },
        ariaLabel: { type: String, default: '' },
      },
      emits: ['change', 'save'],
      setup(props, { expose }) {
        expose({ disposeDocument: vi.fn() })
        return () => h('div', { 'data-ui': 'monaco-stub', 'data-document-key': props.documentKey })
      },
    }),
  }
})

const writableCapabilities = {
  canOpen: true,
  canRead: true,
  canWrite: true,
  canCreateChild: false,
  canRename: true,
  canCopy: true,
  canRemove: true,
  canUpload: false,
  canDownload: true,
}

const mountView = () =>
  mount(FileEditorView, {
    attachTo: document.body,
    props: {
      windowId: 'window-a',
      payload: { nodeId: 'node-a', path: '/tmp/file.txt' },
    },
    global: {
      plugins: [createI18n({ legacy: false, locale: 'zh', messages: { zh } })],
    },
  })

describe('FileEditorView', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    document.body.innerHTML = ''
    editor.loadFileContent.mockImplementation(async (tab: EditorTab) => {
      tab.content = 'disk content'
      tab.originalContent = 'disk content'
      tab.revision = 'revision-1'
      tab.fileSize = 12
      tab.loadedAt = '2026-07-16T00:00:00.000Z'
      tab.capabilities = writableCapabilities
      tab.loadState = 'ready'
      tab.saveState = 'idle'
      tab.isDirty = false
      return true
    })
    editor.saveFileContent.mockResolvedValue(true)
  })

  it('固定使用窗口节点并提供稳定的 tab 与编辑器语义', async () => {
    const wrapper = mountView()
    await flushPromises()

    expect(editor.nodeId).toBe('node-a')
    expect(wrapper.get('[data-page="file-editor"]').attributes('data-node-id')).toBe('node-a')
    expect(wrapper.get('[role="tablist"]').exists()).toBe(true)
    expect(wrapper.get('[role="tab"]').attributes('aria-selected')).toBe('true')
    const monaco = wrapper.getComponent({ name: 'MonacoEditor' })
    expect(monaco.props('documentKey')).toBe('node-a:/tmp/file.txt')
    expect(monaco.props('id')).toBe('file-editor-window-a-content')
    expect(monaco.props('name')).toBe('fileContent')
    wrapper.unmount()
  })

  it('脏内容重新加载前必须确认', async () => {
    const wrapper = mountView()
    await flushPromises()
    const monaco = wrapper.getComponent({ name: 'MonacoEditor' })
    monaco.vm.$emit('change', 'local content', 'node-a:/tmp/file.txt')
    await wrapper.vm.$nextTick()

    await wrapper.get('[data-ui="editor-reload"]').trigger('click')
    expect(editor.loadFileContent).toHaveBeenCalledTimes(1)
    expect(document.body.textContent).toContain('重新加载将丢失当前未保存内容')

    const confirmButton = document.querySelector<HTMLButtonElement>(
      '.confirmation-actions button:last-child',
    )
    expect(confirmButton).not.toBeNull()
    confirmButton?.click()
    await flushPromises()
    expect(editor.loadFileContent).toHaveBeenCalledTimes(2)
    expect(editor.loadFileContent.mock.calls[1][1]).toEqual({ discardLocalChanges: true })
    wrapper.unmount()
  })

  it('冲突标签禁用保存但允许确认重载', async () => {
    const wrapper = mountView()
    await flushPromises()
    const monaco = wrapper.getComponent({ name: 'MonacoEditor' })
    monaco.vm.$emit('change', 'local content', 'node-a:/tmp/file.txt')
    const exposed = wrapper.vm as unknown as { activeTab: EditorTab }
    exposed.activeTab.saveState = 'conflict'
    await wrapper.vm.$nextTick()

    expect(wrapper.get('[data-ui="editor-save"]').attributes('disabled')).toBeDefined()
    expect(wrapper.text()).toContain('内容冲突')
    wrapper.unmount()
  })
})
