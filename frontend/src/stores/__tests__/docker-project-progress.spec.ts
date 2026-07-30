import { defineComponent, h } from 'vue'
import { createI18n } from 'vue-i18n'
import { createPinia } from 'pinia'
import { mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { DockerProjectTask, DockerProjectTaskProgressUpdate } from '@/api/interface/docker'
import en from '@/locales/en'
import zh from '@/locales/zh'
import { useDockerStore } from '@/stores/docker'

const api = vi.hoisted(() => ({
  createComposeProject: vi.fn(),
  composeProjectOperationEventsUrl: vi.fn(
    (operationId: string) =>
      `/api/v1/agent/docker/compose/project-operations/${operationId}/events`,
  ),
}))

vi.mock('@/api/modules/docker', () => ({
  dockerApi: {
    forNode: () => api,
  },
}))

class EventSourceMock {
  static instances: EventSourceMock[] = []
  readonly url: string
  readonly withCredentials: boolean
  onopen: ((event: Event) => void) | null = null
  onerror: ((event: Event) => void) | null = null
  closed = false
  private readonly listeners = new Map<string, Set<EventListener>>()

  constructor(url: string | URL, init?: EventSourceInit) {
    this.url = String(url)
    this.withCredentials = init?.withCredentials ?? false
    EventSourceMock.instances.push(this)
  }

  addEventListener(type: string, listener: EventListener) {
    const listeners = this.listeners.get(type) ?? new Set<EventListener>()
    listeners.add(listener)
    this.listeners.set(type, listeners)
  }

  emit(type: string, data: unknown) {
    const event = new MessageEvent(type, { data: JSON.stringify(data) })
    this.listeners.get(type)?.forEach((listener) => listener(event))
  }

  close() {
    this.closed = true
  }
}

const activeTask = (): DockerProjectTask => ({
  id: 'task-live',
  projectName: 'demo',
  operation: 'create',
  status: 'running',
  stage: 'pulling',
  progressPercent: 25,
  progressMode: 'structured',
  progressItems: [],
  pullImages: true,
  createdAt: 1,
})

describe('DockerStore Compose deployment SSE', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    EventSourceMock.instances = []
    vi.stubGlobal('EventSource', EventSourceMock)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('实时合并工作中的分层事件并在停止跟踪时关闭连接', async () => {
    const task = activeTask()
    api.createComposeProject.mockResolvedValue({
      success: true,
      code: 202,
      message: '',
      data: task,
    })
    let store!: ReturnType<typeof useDockerStore>
    const wrapper = mount(
      defineComponent({
        setup() {
          store = useDockerStore()
          return () => h('div')
        },
      }),
      {
        global: {
          plugins: [
            createPinia(),
            createI18n({ legacy: false, locale: 'zh', messages: { zh, en } }),
          ],
        },
      },
    )

    expect(await store.createComposeProject({ name: 'demo', composeYaml: 'services: {}' })).toBe(
      true,
    )
    const source = EventSourceMock.instances[0]
    expect(source.url).toBe('/api/v1/agent/docker/compose/project-operations/task-live/events')
    expect(source.withCredentials).toBe(true)

    const update: DockerProjectTaskProgressUpdate = {
      progressMode: 'structured',
      progressPercent: 37,
      item: {
        id: 'pulling:sha256:abc',
        parentId: 'pulling:Image demo',
        phase: 'pulling',
        status: 'working',
        label: 'sha256:abc',
        action: 'Downloading',
        currentBytes: 25,
        totalBytes: 100,
        percent: 25,
      },
    }
    source.emit('progress', update)

    expect(store.projectDeploymentProgress?.progressPercent).toBe(37)
    expect(store.projectDeploymentProgress?.progressItems).toEqual([update.item])
    store.stopProjectOperationPolling()
    expect(source.closed).toBe(true)
    wrapper.unmount()
  })
})
