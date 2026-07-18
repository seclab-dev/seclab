import { createI18n } from 'vue-i18n'
import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import FileManagerView from '../FileManagerView.vue'
import zh from '@/locales/zh'
import type { FileListPage, FsEntry } from '@/api/interface/fs'

const api = vi.hoisted(() => ({
  home: vi.fn(),
  listEntries: vi.fn(),
  forNode: vi.fn(),
}))

vi.mock('@/api/modules/fs', () => ({ fsApi: { forNode: api.forNode } }))
vi.mock('@/stores/node', () => ({
  useNodeStore: () => ({ currentNodeId: 'global-node' }),
}))
vi.mock('@/stores/window-manager', () => ({
  useWindowManagerStore: () => ({
    updateWindowRuntimeState: vi.fn(),
    openWindowWithPayload: vi.fn(),
  }),
}))
vi.mock('@/stores/toast', () => ({
  useToastStore: () => ({ error: vi.fn(), success: vi.fn() }),
}))
vi.mock('@/composables/useFileOperations', () => ({
  useFileOperations: () => ({
    createFile: vi.fn(),
    mkdir: vi.fn(),
    removePath: vi.fn(),
    renamePath: vi.fn(),
    runPathTask: vi.fn(),
    downloadFile: vi.fn(),
    uploadFile: vi.fn(),
    resumeActiveTasks: vi.fn().mockResolvedValue(undefined),
    resumeActiveTransfers: vi.fn().mockResolvedValue(undefined),
  }),
}))

const response = <T>(data: T) => ({ success: true, code: 200, message: '', data })

const entry = (name: string, path: string): FsEntry => ({
  name,
  path,
  kind: 'file',
  sizeBytes: 1,
  revision: `revision-${name}`,
  management: { kind: 'custom' },
  capabilities: {
    canOpen: true,
    canRead: true,
    canWrite: true,
    canCreateChild: false,
    canRename: true,
    canCopy: true,
    canRemove: true,
    canUpload: false,
    canDownload: true,
  },
})

const page = (path: string, entries: FsEntry[]): FileListPage => ({
  path,
  entries,
  page: 1,
  pageSize: 50,
  total: entries.length,
  counts: {
    fileCount: entries.length,
    directoryCount: 0,
    symlinkCount: 0,
    otherCount: 0,
  },
  loadedAt: '2026-07-16T00:00:00.000Z',
})

const deferred = <T>() => {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((complete) => {
    resolve = complete
  })
  return { promise, resolve }
}

describe('FileManagerView', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    api.forNode.mockReturnValue({ home: api.home, listEntries: api.listEntries })
    api.home.mockResolvedValue(response({ path: '/home' }))
    api.listEntries.mockResolvedValue(response(page('/home', [])))
  })

  it('固定使用窗口节点并让最新路径请求获胜', async () => {
    const wrapper = mount(FileManagerView, {
      props: { payload: { nodeId: 'node-a' } },
      global: {
        plugins: [createI18n({ legacy: false, locale: 'zh', messages: { zh } })],
      },
    })
    await flushPromises()
    expect(api.forNode).toHaveBeenCalledWith('node-a')
    expect(wrapper.get('[data-page="file-manager"]').attributes('data-node-id')).toBe('node-a')

    const slow = deferred<ReturnType<typeof response<FileListPage>>>()
    const fast = deferred<ReturnType<typeof response<FileListPage>>>()
    api.listEntries.mockImplementation(({ path }: { path: string }) => {
      if (path === '/slow') return slow.promise
      if (path === '/fast') return fast.promise
      return Promise.resolve(response(page(path, [])))
    })
    const input = wrapper.get('input[name="fileManagerPath"]')
    await input.setValue('/slow')
    await input.trigger('keyup.enter')
    await input.setValue('/fast')
    await input.trigger('keyup.enter')
    fast.resolve(response(page('/fast', [entry('fast.txt', '/fast/fast.txt')])))
    await flushPromises()
    slow.resolve(response(page('/slow', [entry('slow.txt', '/slow/slow.txt')])))
    await flushPromises()

    expect(wrapper.text()).toContain('fast.txt')
    expect(wrapper.text()).not.toContain('slow.txt')
    wrapper.unmount()
  })

  it('后台刷新失败保留已有目录数据', async () => {
    api.listEntries.mockResolvedValue(
      response(page('/home', [entry('retained.txt', '/home/retained.txt')])),
    )
    const wrapper = mount(FileManagerView, {
      props: { payload: { nodeId: 'node-a' } },
      global: {
        plugins: [createI18n({ legacy: false, locale: 'zh', messages: { zh } })],
      },
    })
    await flushPromises()
    api.listEntries.mockResolvedValue({
      success: false,
      code: 503,
      message: 'node unavailable',
      data: null,
    })
    await wrapper.get('[data-ui="file-refresh"]').trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('retained.txt')
    expect(wrapper.text()).toContain('node unavailable')
    wrapper.unmount()
  })
})
