/**
 * @file useScriptLibrary.ts
 * @description 脚本库独立请求状态、latest-request-wins、幂等操作和串行运行轮询。
 */

import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { nodesApi, type NodeSummaryResponse } from '@/api/modules/nodes'
import {
  scriptsApi,
  type CreateScriptRequest,
  type ScriptDetail,
  type ScriptRun,
  type ScriptRunOutputPage,
  type ScriptSummary,
  type UpdateScriptRequest,
} from '@/api/modules/scripts'

const POLL_INTERVAL_MS = 2_000
const SEARCH_DEBOUNCE_MS = 250
const TRACKING_KEY = 'seclab.scriptLibrary.activeRuns'
const terminalStatuses = new Set(['succeeded', 'failed', 'timedOut', 'cancelled'])

export interface ScriptRequestState {
  initialLoading: boolean
  refreshing: boolean
  error: string
  warning: string
  loadedAt: Date | null
}

const requestState = (): ScriptRequestState => ({
  initialLoading: false,
  refreshing: false,
  error: '',
  warning: '',
  loadedAt: null,
})

/** 脚本库页面领域状态。 */
export function useScriptLibrary() {
  const { t } = useI18n()
  const scripts = ref<ScriptSummary[]>([])
  const total = ref(0)
  const nodes = ref<NodeSummaryResponse[]>([])
  const detail = ref<ScriptDetail | null>(null)
  const runs = ref<ScriptRun[]>([])
  const runsTotal = ref(0)
  const output = ref<ScriptRunOutputPage | null>(null)
  const trackedRuns = ref<Record<string, ScriptRun>>({})
  const pendingActions = ref(new Set<string>())
  const filters = reactive({
    keyword: '',
    page: 1,
    pageSize: 50,
    sortBy: 'updatedAt' as 'name' | 'updatedAt',
    sortOrder: 'desc' as 'asc' | 'desc',
  })
  const listState = ref<ScriptRequestState>(requestState())
  const detailState = ref<ScriptRequestState>(requestState())
  const saveState = ref<ScriptRequestState>(requestState())
  const nodesState = ref<ScriptRequestState>(requestState())
  const runsState = ref<ScriptRequestState>(requestState())
  const outputState = ref<ScriptRequestState>(requestState())

  let mounted = false
  let listSequence = 0
  let detailSequence = 0
  let runsSequence = 0
  let outputSequence = 0
  let listController: AbortController | null = null
  let detailController: AbortController | null = null
  let runsController: AbortController | null = null
  let outputController: AbortController | null = null
  let searchTimer: number | null = null
  let pollTimer: number | null = null
  let polling = false

  const activeRuns = computed(() =>
    Object.values(trackedRuns.value).filter((run) => !terminalStatuses.has(run.status)),
  )
  const errorMessage = (error: unknown, fallback: string) =>
    error instanceof Error && error.message ? error.message : fallback
  const isAbort = (error: unknown) =>
    error instanceof Error && (error.name === 'AbortError' || error.name === 'CanceledError')
  const unwrap = <T>(
    response: { success: boolean; data?: T; message: string },
    fallback: string,
  ) => {
    if (!response.success || response.data === undefined)
      throw new Error(response.message || fallback)
    return response.data
  }
  const criteriaKey = () =>
    JSON.stringify({
      keyword: filters.keyword.trim(),
      page: filters.page,
      pageSize: filters.pageSize,
      sortBy: filters.sortBy,
      sortOrder: filters.sortOrder,
    })

  /** 加载节点选项，失败不覆盖已有节点。 */
  const refreshNodes = async () => {
    const hasData = nodes.value.length > 0
    nodesState.value = {
      ...nodesState.value,
      initialLoading: !hasData,
      refreshing: hasData,
      error: hasData ? '' : nodesState.value.error,
      warning: '',
    }
    try {
      nodes.value = unwrap(await nodesApi.list(), t('app.scriptManager.messages.loadNodesFailed'))
      nodesState.value = { ...requestState(), loadedAt: new Date() }
    } catch (error) {
      const message = errorMessage(error, t('app.scriptManager.messages.loadNodesFailed'))
      nodesState.value = {
        ...nodesState.value,
        initialLoading: false,
        refreshing: false,
        error: hasData ? '' : message,
        warning: hasData ? message : '',
      }
    }
  }

  /** 刷新摘要列表，旧搜索响应不得覆盖当前筛选。 */
  const refreshScripts = async () => {
    listController?.abort()
    const controller = new AbortController()
    listController = controller
    const sequence = ++listSequence
    const key = criteriaKey()
    const hasData = listState.value.loadedAt !== null
    listState.value = {
      ...listState.value,
      initialLoading: !hasData,
      refreshing: hasData,
      error: hasData ? '' : listState.value.error,
      warning: '',
    }
    try {
      const page = unwrap(
        await scriptsApi.list(
          {
            keyword: filters.keyword.trim() || undefined,
            page: filters.page,
            pageSize: filters.pageSize,
            sortBy: filters.sortBy,
            sortOrder: filters.sortOrder,
          },
          controller.signal,
        ),
        t('app.scriptManager.messages.loadFailed'),
      )
      if (!mounted || sequence !== listSequence || key !== criteriaKey()) return
      scripts.value = page.items
      total.value = page.total
      listState.value = { ...requestState(), loadedAt: new Date(page.loadedAt) }
    } catch (error) {
      if (isAbort(error) || !mounted || sequence !== listSequence || key !== criteriaKey()) return
      const message = errorMessage(error, t('app.scriptManager.messages.loadFailed'))
      listState.value = {
        ...listState.value,
        initialLoading: false,
        refreshing: false,
        error: hasData ? '' : message,
        warning: hasData ? message : '',
      }
    }
  }

  /** 按需加载正文，脚本快照用于阻止切换时的旧响应。 */
  const loadDetail = async (scriptId: string) => {
    detailController?.abort()
    const controller = new AbortController()
    detailController = controller
    const sequence = ++detailSequence
    detailState.value = { ...requestState(), initialLoading: true }
    try {
      const value = unwrap(
        await scriptsApi.detail(scriptId, controller.signal),
        t('app.scriptManager.messages.loadDetailFailed'),
      )
      if (!mounted || sequence !== detailSequence || value.scriptId !== scriptId) return null
      detail.value = value
      detailState.value = { ...requestState(), loadedAt: new Date() }
      return value
    } catch (error) {
      if (isAbort(error) || !mounted || sequence !== detailSequence) return null
      detailState.value = {
        ...requestState(),
        error: errorMessage(error, t('app.scriptManager.messages.loadDetailFailed')),
      }
      return null
    }
  }

  /** 创建脚本，禁止保存请求重叠。 */
  const createScript = async (request: CreateScriptRequest) =>
    performAction('save', async () => {
      saveState.value = { ...requestState(), refreshing: true }
      try {
        const value = unwrap(
          await scriptsApi.create(request),
          t('app.scriptManager.messages.saveFailed'),
        )
        saveState.value = { ...requestState(), loadedAt: new Date() }
        await refreshScripts()
        return value
      } catch (error) {
        saveState.value = {
          ...requestState(),
          error: errorMessage(error, t('app.scriptManager.messages.saveFailed')),
        }
        throw error
      }
    })

  /** 使用 expectedRevision 更新，冲突时调用方保留本地表单。 */
  const updateScript = async (scriptId: string, request: UpdateScriptRequest) =>
    performAction('save', async () => {
      saveState.value = { ...requestState(), refreshing: true }
      try {
        const value = unwrap(
          await scriptsApi.update(scriptId, request),
          t('app.scriptManager.messages.saveFailed'),
        )
        detail.value = value
        saveState.value = { ...requestState(), loadedAt: new Date() }
        await refreshScripts()
        return value
      } catch (error) {
        saveState.value = {
          ...requestState(),
          error: errorMessage(error, t('app.scriptManager.messages.saveFailed')),
        }
        throw error
      }
    })

  const removeScript = async (scriptId: string) =>
    performAction(`remove:${scriptId}`, async () => {
      await scriptsApi.remove(scriptId)
      if (detail.value?.scriptId === scriptId) detail.value = null
      await refreshScripts()
    })

  /** 提交幂等异步运行并持久化跟踪 runId。 */
  const startRun = async (scriptId: string, nodeId: string, timeoutSeconds?: number) =>
    performAction(`run:${scriptId}`, async () => {
      const value = unwrap(
        await scriptsApi.startRun(scriptId, { nodeId, timeoutSeconds }, crypto.randomUUID()),
        t('app.scriptManager.messages.runFailed'),
      )
      trackedRuns.value = { ...trackedRuns.value, [value.runId]: value }
      persistTracking()
      schedulePoll(0)
      return value
    })

  const loadRuns = async (page = 1, scriptId?: string) => {
    runsController?.abort()
    const controller = new AbortController()
    runsController = controller
    const sequence = ++runsSequence
    runsState.value = { ...requestState(), initialLoading: true }
    try {
      const value = unwrap(
        await scriptsApi.runs({ scriptId, page, pageSize: 50 }, controller.signal),
        t('app.scriptManager.messages.loadRunsFailed'),
      )
      if (!mounted || sequence !== runsSequence) return
      runs.value = value.items
      runsTotal.value = value.total
      runsState.value = { ...requestState(), loadedAt: new Date(value.loadedAt) }
    } catch (error) {
      if (isAbort(error) || !mounted || sequence !== runsSequence) return
      runsState.value = {
        ...requestState(),
        error: errorMessage(error, t('app.scriptManager.messages.loadRunsFailed')),
      }
    }
  }

  const loadOutput = async (runId: string) => {
    outputController?.abort()
    const controller = new AbortController()
    outputController = controller
    const sequence = ++outputSequence
    outputState.value = { ...requestState(), initialLoading: true }
    try {
      const value = unwrap(
        await scriptsApi.output(runId, 0, 100, controller.signal),
        t('app.scriptManager.messages.loadOutputFailed'),
      )
      if (!mounted || sequence !== outputSequence || value.runId !== runId) return null
      output.value = value
      outputState.value = { ...requestState(), loadedAt: new Date() }
      return value
    } catch (error) {
      if (isAbort(error) || !mounted || sequence !== outputSequence) return null
      outputState.value = {
        ...requestState(),
        error: errorMessage(error, t('app.scriptManager.messages.loadOutputFailed')),
      }
      return null
    }
  }

  const cancelRun = async (runId: string) =>
    performAction(`cancel:${runId}`, async () => {
      const value = unwrap(
        await scriptsApi.cancel(runId),
        t('app.scriptManager.messages.cancelFailed'),
      )
      trackedRuns.value = { ...trackedRuns.value, [runId]: value }
      persistTracking()
      schedulePoll(0)
      return value
    })

  const performAction = async <T>(key: string, action: () => Promise<T>): Promise<T> => {
    if (pendingActions.value.has(key))
      throw new Error(t('app.scriptManager.messages.duplicateAction'))
    pendingActions.value = new Set(pendingActions.value).add(key)
    try {
      return await action()
    } finally {
      const next = new Set(pendingActions.value)
      next.delete(key)
      pendingActions.value = next
    }
  }
  const isActionPending = (key: string) => pendingActions.value.has(key)

  const persistTracking = () => {
    const ids = activeRuns.value.map((run) => run.runId)
    localStorage.setItem(TRACKING_KEY, JSON.stringify(ids))
  }
  const restoreTracking = () => {
    try {
      const ids = JSON.parse(localStorage.getItem(TRACKING_KEY) || '[]') as string[]
      trackedRuns.value = Object.fromEntries(ids.map((id) => [id, { runId: id } as ScriptRun]))
    } catch {
      localStorage.removeItem(TRACKING_KEY)
    }
  }
  const schedulePoll = (delay = POLL_INTERVAL_MS) => {
    if (!mounted || document.hidden || !activeRuns.value.length) return
    if (pollTimer !== null) window.clearTimeout(pollTimer)
    pollTimer = window.setTimeout(pollActiveRuns, delay)
  }
  const pollActiveRuns = async () => {
    if (polling || !mounted || document.hidden) return
    polling = true
    let terminalReached = false
    try {
      for (const run of activeRuns.value) {
        try {
          const value = unwrap(
            await scriptsApi.run(run.runId),
            t('app.scriptManager.messages.loadRunsFailed'),
          )
          trackedRuns.value = { ...trackedRuns.value, [value.runId]: value }
          terminalReached ||= terminalStatuses.has(value.status)
        } catch {
          // 保留 runId，下一轮继续恢复。
        }
      }
      persistTracking()
      if (terminalReached) {
        await Promise.all([refreshScripts(), loadRuns()])
      }
    } finally {
      polling = false
      schedulePoll()
    }
  }
  const handleVisibility = () => {
    if (!document.hidden) schedulePoll(0)
  }

  watch(
    () => filters.keyword,
    () => {
      filters.page = 1
      if (searchTimer !== null) window.clearTimeout(searchTimer)
      searchTimer = window.setTimeout(refreshScripts, SEARCH_DEBOUNCE_MS)
    },
  )
  watch(
    () => [filters.page, filters.sortBy, filters.sortOrder],
    () => void refreshScripts(),
  )

  onMounted(async () => {
    mounted = true
    restoreTracking()
    document.addEventListener('visibilitychange', handleVisibility)
    await Promise.all([refreshNodes(), refreshScripts()])
    schedulePoll(0)
  })
  onUnmounted(() => {
    mounted = false
    listController?.abort()
    detailController?.abort()
    runsController?.abort()
    outputController?.abort()
    if (searchTimer !== null) window.clearTimeout(searchTimer)
    if (pollTimer !== null) window.clearTimeout(pollTimer)
    document.removeEventListener('visibilitychange', handleVisibility)
  })

  return {
    scripts,
    total,
    nodes,
    detail,
    runs,
    runsTotal,
    output,
    trackedRuns,
    activeRuns,
    filters,
    listState,
    detailState,
    saveState,
    nodesState,
    runsState,
    outputState,
    refreshNodes,
    refreshScripts,
    loadDetail,
    createScript,
    updateScript,
    removeScript,
    startRun,
    loadRuns,
    loadOutput,
    cancelRun,
    isActionPending,
  }
}
