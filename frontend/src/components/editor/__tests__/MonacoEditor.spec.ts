import { mount } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import MonacoEditor from '../MonacoEditor.vue'
import zh from '@/locales/zh'

const monacoMock = vi.hoisted(() => ({
  models: [] as Array<{
    value: string
    language: string
    uri: unknown
    disposed: boolean
    getValue: () => string
    setValue: (value: string) => void
    dispose: () => void
  }>,
  createModel: vi.fn(),
  setModel: vi.fn(),
  saveViewState: vi.fn(() => ({ cursorState: [], viewState: {} })),
  restoreViewState: vi.fn(),
  setModelLanguage: vi.fn(),
  editorRoot: null as HTMLDivElement | null,
}))

vi.mock('../sdl-monaco-theme', () => ({
  SDL_THEME_DARK: 'sdl-dark',
  SDL_THEME_LIGHT: 'sdl-light',
  createSdlDarkTheme: () => ({}),
  createSdlLightTheme: () => ({}),
}))

vi.mock('../monaco-workers', () => {
  let activeModel: ReturnType<typeof monacoMock.createModel> | null = null
  const root = document.createElement('div')
  monacoMock.editorRoot = root
  const textarea = document.createElement('textarea')
  textarea.className = 'inputarea'
  root.appendChild(textarea)

  monacoMock.createModel.mockImplementation((value: string, language: string, uri: unknown) => {
    const model = {
      value,
      language,
      uri,
      disposed: false,
      getValue: () => model.value,
      setValue: (nextValue: string) => {
        model.value = nextValue
      },
      dispose: () => {
        model.disposed = true
      },
    }
    monacoMock.models.push(model)
    return model
  })

  const editor = {
    createModel: monacoMock.createModel,
    create: vi.fn((_container: HTMLElement, options: { model: typeof activeModel }) => {
      activeModel = options.model
      return {
        addCommand: vi.fn(),
        onDidChangeModelContent: vi.fn(),
        getValue: () => activeModel?.getValue() ?? '',
        setValue: (value: string) => activeModel?.setValue(value),
        getModel: () => activeModel,
        setModel: (model: typeof activeModel) => {
          activeModel = model
          monacoMock.setModel(model)
        },
        saveViewState: monacoMock.saveViewState,
        restoreViewState: monacoMock.restoreViewState,
        getDomNode: () => root,
        updateOptions: vi.fn(),
        layout: vi.fn(),
        focus: vi.fn(),
        dispose: vi.fn(),
      }
    }),
    setModelLanguage: (model: (typeof monacoMock.models)[number], language: string) => {
      model.language = language
      monacoMock.setModelLanguage(model, language)
    },
    defineTheme: vi.fn(),
    setTheme: vi.fn(),
  }

  return {
    setupMonacoWorkers: vi.fn(),
    monaco: {
      editor,
      Uri: { from: (value: unknown) => value },
      KeyMod: { CtrlCmd: 1 },
      KeyCode: { KeyS: 2 },
    },
  }
})

describe('MonacoEditor 多文档模型', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    monacoMock.models.length = 0
  })

  it('按 documentKey 隔离模型并恢复视图状态', async () => {
    const wrapper = mount(MonacoEditor, {
      props: {
        modelValue: 'content-a',
        documentKey: 'node-a:/a.txt',
        filePath: '/a.txt',
      },
      global: {
        plugins: [createI18n({ legacy: false, locale: 'zh', messages: { zh } })],
      },
    })
    const firstModel = monacoMock.models[0]

    await wrapper.setProps({
      modelValue: 'content-b',
      documentKey: 'node-a:/b.txt',
      filePath: '/b.txt',
    })
    const secondModel = monacoMock.models[1]
    expect(monacoMock.models).toHaveLength(2)
    expect(monacoMock.setModel).toHaveBeenLastCalledWith(secondModel)

    await wrapper.setProps({
      modelValue: 'content-a',
      documentKey: 'node-a:/a.txt',
      filePath: '/a.txt',
    })
    expect(monacoMock.models).toHaveLength(2)
    expect(monacoMock.setModel).toHaveBeenLastCalledWith(firstModel)
    expect(monacoMock.restoreViewState).toHaveBeenCalled()
    wrapper.unmount()
  })

  it('把 id/name 落到 Monaco 原生输入并释放关闭文档', () => {
    const wrapper = mount(MonacoEditor, {
      props: {
        id: 'file-editor-content',
        name: 'fileContent',
        modelValue: 'content',
        documentKey: 'node-a:/a.txt',
      },
      global: {
        plugins: [createI18n({ legacy: false, locale: 'zh', messages: { zh } })],
      },
    })

    const textarea = monacoMock.editorRoot?.querySelector<HTMLTextAreaElement>('textarea.inputarea')
    expect(textarea?.id).toBe('file-editor-content')
    expect(textarea?.name).toBe('fileContent')

    const model = monacoMock.models[0]
    ;(wrapper.vm as unknown as { disposeDocument: (key: string) => void }).disposeDocument(
      'node-a:/a.txt',
    )
    expect(model.disposed).toBe(true)
    wrapper.unmount()
  })
})
