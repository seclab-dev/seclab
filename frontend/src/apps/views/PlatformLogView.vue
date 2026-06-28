<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { platformLogApi } from '@/api/modules/platform-log'
import { nodesApi, type NodeSummaryResponse } from '@/api/modules/nodes'
import type { PlatformLog, PlatformLogQuery } from '@/api/interface/logging'
import type { RuntimeLogFile, RuntimeLogLine } from '@/api/generated'
import {
  SecLabButton,
  SecLabTable,
  SecLabTag,
  SecLabTabs,
  SecLabInput,
  SecLabSelect,
  SecLabDateTimeRangePicker,
  SecLabPagination,
  SecLabFormItem,
  SecLabCard,
  SecLabLoading,
  SecLabDrawer,
} from '@/components/ui'
import { useNotificationStore } from '@/stores/notification'
import { useI18n } from 'vue-i18n'
import { formatDateTime } from '@/utils/time'

// 应用窗口容器会统一透传这两个属性给所有应用视图。
defineProps<{
  isMaximized?: boolean
  payload?: Record<string, unknown>
}>()

const { t, te, locale } = useI18n()
const notificationStore = useNotificationStore()

const filters = ref<{
  eventPrefix: string
  status: '' | 'SUCCESS' | 'FAILED'
  keyword: string
  timeRange: {
    startAt: number | null
    endAt: number | null
  }
}>({
  eventPrefix: '',
  status: '',
  keyword: '',
  timeRange: {
    startAt: null,
    endAt: null,
  },
})

const logs = ref<PlatformLog[]>([])
const isLoading = ref(false)
const activeLogTab = ref<'events' | 'runtime'>('events')

const currentPage = ref(1)
const pageSize = ref(10)
const totalLogs = ref(0)
const totalPages = computed(() => Math.max(1, Math.ceil(totalLogs.value / pageSize.value)))
const filtersExpanded = ref(false)

/** 事件状态选项 */
const statusOptions = computed(() => [
  { label: t('app.platformLog.filters.statusPlaceholder'), value: '' },
  { label: t('app.platformLog.filters.statusOptions.success'), value: 'SUCCESS' },
  { label: t('app.platformLog.filters.statusOptions.failed'), value: 'FAILED' },
])

/** 事件日志表格列定义 */
const eventTableColumns = computed(() => [
  { label: t('common.index'), width: 70, slot: 'index', align: 'center' as const },
  { label: t('app.platformLog.time'), width: 180, slot: 'time', align: 'center' as const },
  { label: t('app.platformLog.module'), width: 100, slot: 'module', align: 'center' as const },
  { label: t('app.platformLog.userOrSource'), width: 220, slot: 'user', align: 'center' as const },
  { label: t('app.platformLog.eventType'), width: 180, slot: 'event', align: 'center' as const },
  { label: t('app.platformLog.result'), width: 110, slot: 'result', align: 'center' as const },
  { label: t('app.platformLog.details'), minWidth: 260, slot: 'details', align: 'center' as const },
])

/** 运行时日志表格列定义 */
const runtimeTableColumns = computed(() => [
  { label: t('common.index'), width: 70, slot: 'index', align: 'center' as const },
  { label: t('app.platformLog.time'), width: 180, slot: 'time', align: 'center' as const },
  {
    label: t('app.platformLog.clientIp'),
    width: 180,
    slot: 'source',
    align: 'center' as const,
  },
  {
    label: t('app.platformLog.runtime.level'),
    width: 100,
    slot: 'level',
    align: 'center' as const,
  },
  {
    label: t('app.platformLog.runtime.target'),
    width: 180,
    slot: 'target',
    align: 'center' as const,
  },
  {
    prop: 'content',
    label: t('app.platformLog.details'),
    minWidth: 320,
    slot: 'details',
  },
])

const runtimeFiles = ref<RuntimeLogFile[]>([])
const runtimeLines = ref<RuntimeLogLine[]>([])
const runtimeNodes = ref<NodeSummaryResponse[]>([])
const runtimeLoading = ref(false)
const runtimeFilesLoading = ref(false)
const runtimeQuery = ref({
  service: 'seclab',
  nodeId: '',
  fileName: '',
  level: '',
  target: '',
  keyword: '',
})

/** 运行时节点选项 */
const runtimeNodeOptions = computed(() => {
  const filtered = runtimeNodes.value.filter((n) => n.nodeId && n.nodeId !== 'local')
  return [
    { label: t('app.platformLog.runtime.localNode'), value: 'local' },
    ...filtered.map((n) => ({ label: n.name || n.nodeId, value: n.nodeId })),
  ]
})

/** 运行时服务选项 */
const runtimeServiceOptions = computed(() => [
  { label: t('app.platformLog.runtime.serviceOptions.seclab'), value: 'seclab' },
  { label: t('app.platformLog.runtime.serviceOptions.agent'), value: 'agent' },
])

/** 运行时日志级别选项 */
const runtimeLevelOptions = [
  { label: 'ALL', value: '' },
  { label: 'ERROR', value: 'ERROR' },
  { label: 'WARN', value: 'WARN' },
  { label: 'INFO', value: 'INFO' },
  { label: 'DEBUG', value: 'DEBUG' },
  { label: 'TRACE', value: 'TRACE' },
]

/** 平台日志时间快捷范围选项。 */
const timeRangeShortcuts = computed(() => [
  { label: t('app.platformLog.filters.shortcuts.last15m'), value: '15m' as const },
  { label: t('app.platformLog.filters.shortcuts.last1h'), value: '1h' as const },
  { label: t('app.platformLog.filters.shortcuts.last24h'), value: '24h' as const },
  { label: t('app.platformLog.filters.shortcuts.last7d'), value: '7d' as const },
  { label: t('app.platformLog.filters.shortcuts.today'), value: 'today' as const },
])

/** 平台日志日历星期标题。 */
const weekDayLabels = computed(() => [
  t('common.weekdays.sun'),
  t('common.weekdays.mon'),
  t('common.weekdays.tue'),
  t('common.weekdays.wed'),
  t('common.weekdays.thu'),
  t('common.weekdays.fri'),
  t('common.weekdays.sat'),
])

/** 当前事件日志筛选摘要。 */
const filterSummary = computed(() => {
  const segments: string[] = []
  const eventPrefix = filters.value.eventPrefix.trim()
  const keyword = filters.value.keyword.trim()

  if (eventPrefix) {
    segments.push(t('app.platformLog.filters.summary.eventPrefix', { value: eventPrefix }))
  }
  if (filters.value.status) {
    const statusLabel =
      statusOptions.value.find((option) => option.value === filters.value.status)?.label ??
      filters.value.status
    segments.push(t('app.platformLog.filters.summary.status', { value: statusLabel }))
  }
  if (keyword) {
    segments.push(t('app.platformLog.filters.summary.keyword', { value: keyword }))
  }
  if (filters.value.timeRange.startAt || filters.value.timeRange.endAt) {
    const start = filters.value.timeRange.startAt
      ? formatDateTime(filters.value.timeRange.startAt)
      : '--'
    const end = filters.value.timeRange.endAt ? formatDateTime(filters.value.timeRange.endAt) : '--'
    segments.push(t('app.platformLog.filters.summary.timeRange', { start, end }))
  }

  return segments.length ? segments.join(' / ') : t('app.platformLog.filters.summary.empty')
})

/** 运行时文件选项 */
const runtimeFileOptions = computed(() =>
  runtimeFiles.value
    .filter((f) => {
      if (f.service !== runtimeQuery.value.service) return false
      if (f.service === 'agent') {
        return (f.nodeId || 'local') === (runtimeQuery.value.nodeId || 'local')
      }
      return true
    })
    .map((f) => ({ label: f.fileName, value: f.fileName })),
)

const getResultType = (result: string): 'success' | 'danger' | 'info' => {
  const res = (result || '').toUpperCase()
  if (res === 'SUCCESS') return 'success'
  if (res === 'FAILED' || res === 'FAILURE') return 'danger'
  return 'info'
}

const isDetailDrawerOpen = ref(false)

/** 判断字符串是否为 Rust Debug 结构体格式。 */
const rustDebugPattern = /^[A-Za-z0-9_]+\s*\{[\s\S]*\}$/

/** 提取 Rust Debug 结构体的根对象内容，隐藏根类型名。 */
const extractRustDebugBody = (str: string): string | null => {
  const trimmed = str.trim()
  if (!rustDebugPattern.test(trimmed)) return null

  const firstBraceIndex = trimmed.indexOf('{')
  if (firstBraceIndex === -1) return null

  return trimmed.slice(firstBraceIndex)
}

const beautifyRustDebug = (str: string): string => {
  if (!str) return str
  const body = extractRustDebugBody(str)
  if (!body) return str
  if (body === '{}') return '{}'

  let level = 0
  let result = ''
  let inQuotes = false

  for (let i = 0; i < body.length; i++) {
    const char = body[i]
    if (char === '"' && body[i - 1] !== '\\') {
      inQuotes = !inQuotes
      result += char
      continue
    }
    if (inQuotes) {
      result += char
      continue
    }

    // 处理连在一起的空大括号 {}
    if (char === '{' && body[i + 1] === '}') {
      result += '{}'
      i++ // 跳过 '}'
      continue
    }

    if (char === '{') {
      level++
      result += '{\n' + '  '.repeat(level)
      while (body[i + 1] === ' ') i++
    } else if (char === '}') {
      result = result.trimEnd()
      level = Math.max(0, level - 1)
      result += '\n' + '  '.repeat(level) + '}'
    } else if (char === ',' && level > 0) {
      result += ',\n' + '  '.repeat(level)
      while (body[i + 1] === ' ') i++
    } else {
      result += char
    }
  }
  return result.trimEnd()
}

type MetadataTokenClass =
  | 'json-key'
  | 'json-string'
  | 'json-number'
  | 'json-boolean'
  | 'json-null'
  | 'rust-key'
  | 'rust-string'
  | 'rust-number'
  | 'rust-type'
  | 'rust-none'

interface MetadataToken {
  text: string
  className?: MetadataTokenClass
}

const pushPlainToken = (tokens: MetadataToken[], text: string) => {
  if (text) tokens.push({ text })
}

const tokenizeRustDebugLine = (line: string): MetadataToken[] => {
  const tokens: MetadataToken[] = []
  const regex =
    /("[^"]*"|\b(None|Some|BadRequest|InternalError|Ok|Error)\b|\b\d+\b|([a-zA-Z0-9_]+)\s*:)/g
  let lastIndex = 0
  let match: RegExpExecArray | null

  while ((match = regex.exec(line)) !== null) {
    pushPlainToken(tokens, line.slice(lastIndex, match.index))

    const [raw, quoted, knownType, key] = match
    if (key) {
      tokens.push({ text: key, className: 'rust-key' })
      pushPlainToken(tokens, raw.slice(key.length))
    } else if (quoted) {
      tokens.push({ text: raw, className: 'rust-string' })
    } else if (knownType) {
      tokens.push({
        text: raw,
        className: knownType === 'None' ? 'rust-none' : 'rust-type',
      })
    } else {
      tokens.push({ text: raw, className: 'rust-number' })
    }

    lastIndex = regex.lastIndex
  }

  pushPlainToken(tokens, line.slice(lastIndex))
  return tokens
}

const tokenizeRustDebug = (val: unknown): MetadataToken[][] => {
  if (!val) return []
  const str = typeof val === 'string' ? val : JSON.stringify(val, null, 2)
  return beautifyRustDebug(str).split('\n').map(tokenizeRustDebugLine)
}

/** 格式化模块显示，应用国际化映射 */
const formatModule = (mod: string) => {
  if (!mod) return '-'
  const key = `app.platformLog.modules.${mod}`
  return te(key) ? t(key) : mod
}

/** 格式化事件类型显示，应用国际化映射 */
const formatEventType = (evt: string) => {
  if (!evt) return '-'
  const key = `app.platformLog.events.${evt}`
  return te(key) ? t(key) : evt
}

const formatUserSource = (row: PlatformLog | null | undefined) => {
  if (!row) return 'System'
  const username = row.username
  const ip = row.clientIp || ((row as Record<string, unknown>).client_ip as string)
  if (username && ip) {
    return `${username} (${ip})`
  }
  return username || ip || 'System'
}

const selectedLog = ref<PlatformLog | null>(null)

const showLogDetail = (log: PlatformLog) => {
  selectedLog.value = log
  isDetailDrawerOpen.value = true
}

const formatJson = (val: unknown) => {
  if (!val) return ''
  try {
    const str = JSON.stringify(val)
    if (str === '{}') return '{}'
    return JSON.stringify(val, null, 2)
  } catch {
    return String(val)
  }
}

const tokenizeJsonLine = (line: string): MetadataToken[] => {
  const tokens: MetadataToken[] = []
  const regex =
    /("(\\u[a-zA-Z0-9]{4}|\\[^u]|[^\\"])*"(\s*:)?|\b(true|false|null)\b|-?\d+(?:\.\d*)?(?:[eE][+-]?\d+)?)/g
  let lastIndex = 0
  let match: RegExpExecArray | null

  while ((match = regex.exec(line)) !== null) {
    const raw = match[0]
    pushPlainToken(tokens, line.slice(lastIndex, match.index))

    if (raw.startsWith('"')) {
      if (raw.endsWith(':')) {
        tokens.push({ text: raw.slice(0, -1), className: 'json-key' })
        pushPlainToken(tokens, ':')
      } else {
        let rawText = ''
        try {
          rawText = JSON.parse(raw) as string
        } catch {
          rawText = ''
        }

        if (rustDebugPattern.test(rawText.trim())) {
          tokens.push({ text: '"', className: 'json-string' })
          tokens.push(...tokenizeRustDebugLine(rawText))
          tokens.push({ text: '"', className: 'json-string' })
        } else {
          tokens.push({ text: raw, className: 'json-string' })
        }
      }
    } else if (raw === 'true' || raw === 'false') {
      tokens.push({ text: raw, className: 'json-boolean' })
    } else if (raw === 'null') {
      tokens.push({ text: raw, className: 'json-null' })
    } else {
      tokens.push({ text: raw, className: 'json-number' })
    }

    lastIndex = regex.lastIndex
  }

  pushPlainToken(tokens, line.slice(lastIndex))
  return tokens
}

const metadataTokenLines = computed<MetadataToken[][]>(() => {
  if (!selectedLog.value?.metadata) return []
  const meta = selectedLog.value.metadata
  const keys = Object.keys(meta)
  if (keys.length === 1 && (keys[0] === 'error' || keys[0] === 'message')) {
    const val = meta[keys[0]]
    if (typeof val === 'string' && rustDebugPattern.test(val.trim())) {
      return tokenizeRustDebug(val)
    }
  }
  const formatted = formatJson(meta)
  return formatted.split('\n').map(tokenizeJsonLine)
})

const getRuntimeLevelTagType = (level: string): 'success' | 'warning' | 'danger' | 'info' => {
  const lvl = (level || '').toUpperCase()
  if (lvl === 'ERROR' || lvl === 'FATAL') return 'danger'
  if (lvl === 'WARN' || lvl === 'WARNING') return 'warning'
  if (lvl === 'INFO') return 'success'
  return 'info'
}

const hasMetadata = (metadata: unknown): boolean => {
  if (!metadata) return false

  if (typeof metadata === 'string') {
    const trimmed = metadata.trim()
    if (!trimmed || trimmed === '{}') return false

    try {
      return hasMetadata(JSON.parse(trimmed) as unknown)
    } catch {
      return true
    }
  }

  if (Array.isArray(metadata)) return metadata.length > 0
  if (typeof metadata !== 'object') return false

  return Object.keys(metadata).length > 0
}

const handleApplyFilters = () => {
  currentPage.value = 1
  fetchPlatformLogs()
}

const handleResetFilters = () => {
  filters.value.eventPrefix = ''
  filters.value.status = ''
  filters.value.keyword = ''
  filters.value.timeRange = {
    startAt: null,
    endAt: null,
  }
  currentPage.value = 1
  fetchPlatformLogs()
}

const fetchPlatformLogs = async () => {
  isLoading.value = true

  const eventPrefix = filters.value.eventPrefix.trim()
  const nextEventPrefixes = eventPrefix ? [eventPrefix] : undefined

  const nextStatuses = filters.value.status ? [filters.value.status] : undefined

  const payload: PlatformLogQuery = {
    page: currentPage.value,
    pageSize: pageSize.value,
    eventPrefixes: nextEventPrefixes,
    statuses: nextStatuses,
    keyword: filters.value.keyword.trim() || undefined,
    startAt: filters.value.timeRange.startAt ?? undefined,
    endAt: filters.value.timeRange.endAt ?? undefined,
  }

  const res = await platformLogApi.fetchFilteredLogs(payload)

  if (res.success && res.data) {
    logs.value = res.data.logs ?? []
    totalLogs.value = res.data.total ?? 0

    const nextTotalPages = Math.max(1, Math.ceil(totalLogs.value / pageSize.value))
    if (currentPage.value > nextTotalPages && nextTotalPages > 0) {
      currentPage.value = nextTotalPages
      isLoading.value = false
      return
    }
  } else {
    logs.value = []
    totalLogs.value = 0
    notificationStore.error(res.message || t('app.platformLog.fetchFailed'))
  }

  isLoading.value = false
}

watch(pageSize, () => {
  currentPage.value = 1
  fetchPlatformLogs()
})

watch(currentPage, fetchPlatformLogs)

onMounted(fetchPlatformLogs)

const fetchRuntimeFiles = async () => {
  runtimeFilesLoading.value = true
  const params =
    runtimeQuery.value.service === 'agent'
      ? { service: 'agent' as const, nodeId: runtimeQuery.value.nodeId || 'local' }
      : { service: 'seclab' as const }
  const res = await platformLogApi.fetchRuntimeLogFiles(params)
  if (res.success && res.data) {
    const otherFiles = runtimeFiles.value.filter((file) => {
      if (file.service !== runtimeQuery.value.service) return true
      if (file.service !== 'agent') return false
      return (file.nodeId || 'local') !== (runtimeQuery.value.nodeId || 'local')
    })
    runtimeFiles.value = [...otherFiles, ...res.data]
    const first = res.data[0]
    if (first && !runtimeQuery.value.fileName) {
      runtimeQuery.value.fileName = first.fileName
    }
  } else {
    notificationStore.error(res.message || t('app.platformLog.runtime.fetchFilesFailed'))
  }
  runtimeFilesLoading.value = false
}

const hasMoreRuntimeLogs = ref(false)
const nextRuntimeCursor = ref<number | null>(null)

const queryRuntimeLogs = async (isAppend = false) => {
  if (!runtimeQuery.value.fileName) {
    runtimeLines.value = []
    hasMoreRuntimeLogs.value = false
    nextRuntimeCursor.value = null
    return
  }
  runtimeLoading.value = true
  try {
    const res = await platformLogApi.queryRuntimeLogs({
      service: runtimeQuery.value.service as 'seclab' | 'agent',
      nodeId:
        runtimeQuery.value.service === 'agent' ? runtimeQuery.value.nodeId || 'local' : undefined,
      fileName: runtimeQuery.value.fileName,
      level: runtimeQuery.value.level || undefined,
      target: runtimeQuery.value.target || undefined,
      keyword: runtimeQuery.value.keyword || undefined,
      limit: 300,
      cursor: isAppend ? (nextRuntimeCursor.value ?? undefined) : undefined,
    })
    if (res.success && res.data) {
      const currentLines = runtimeLines.value.filter((line) => line.message !== '__LOAD_MORE__')
      const newLines = res.data.lines ?? []
      const merged = isAppend ? [...currentLines, ...newLines] : newLines

      hasMoreRuntimeLogs.value = res.data.hasMore ?? false
      nextRuntimeCursor.value = res.data.nextCursor ?? null
      if (hasMoreRuntimeLogs.value) {
        merged.push({
          offset: 0,
          timestamp: '',
          level: '',
          target: '',
          source: '',
          message: '__LOAD_MORE__',
          parseError: false,
        })
      }
      runtimeLines.value = merged
    } else {
      if (!isAppend) {
        runtimeLines.value = []
      }
      notificationStore.error(res.message || t('app.platformLog.runtime.fetchLogsFailed'))
    }
  } finally {
    runtimeLoading.value = false
  }
}

const handleQueryRuntimeLogs = () => {
  void queryRuntimeLogs(false)
}

const handleLoadMoreRuntimeLogs = () => {
  void queryRuntimeLogs(true)
}

const fetchRuntimeNodes = async () => {
  const res = await nodesApi.list()
  if (res.success && res.data) {
    runtimeNodes.value = res.data
  }
}

const clientIp = computed(() => {
  return selectedLog.value?.clientIp || (selectedLog.value as Record<string, unknown>).client_ip
})

watch(activeLogTab, (tab) => {
  if (tab === 'runtime' && runtimeFiles.value.length === 0) {
    void fetchRuntimeNodes()
    void fetchRuntimeFiles()
  }
})

watch(
  () => [runtimeQuery.value.service, runtimeQuery.value.nodeId],
  () => {
    runtimeQuery.value.fileName = ''
    runtimeLines.value = []
    void fetchRuntimeFiles()
  },
)
</script>

<template>
  <div class="platform-log" data-seclab-app="platform-log">
    <SecLabCard shadow="never" class="tabs-card" content-role="header">
      <SecLabTabs
        v-model="activeLogTab"
        class="kind-tabs"
        :tabs="[
          { label: t('app.platformLog.tabs.events'), name: 'events' },
          { label: t('app.platformLog.tabs.runtime'), name: 'runtime' },
        ]"
      />
    </SecLabCard>

    <div v-if="activeLogTab === 'events'" class="events-panel">
      <SecLabCard shadow="never" class="toolbar-card">
        <div class="filter-collapse-header" data-ui="platform-log-filter-summary">
          <div class="filter-summary">
            <span class="summary-label">{{ t('app.platformLog.filters.summary.label') }}</span>
            <span class="summary-value">{{ filterSummary }}</span>
          </div>
          <div class="filter-summary-actions">
            <SecLabButton size="small" :loading="isLoading" @click="fetchPlatformLogs">
              {{ t('common.refresh') }}
            </SecLabButton>
            <SecLabButton size="small" @click="filtersExpanded = !filtersExpanded">
              {{
                filtersExpanded
                  ? t('app.platformLog.filters.collapse')
                  : t('app.platformLog.filters.expand')
              }}
            </SecLabButton>
          </div>
        </div>

        <div v-if="filtersExpanded" class="toolbar">
          <div class="filters">
            <SecLabFormItem :label="t('app.platformLog.filters.eventPrefixLabel')">
              <SecLabInput
                v-model="filters.eventPrefix"
                class="filter-input"
                :placeholder="t('app.platformLog.filters.eventPrefixPlaceholder')"
                @keyup.enter="handleApplyFilters"
              />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.platformLog.filters.statusLabel')">
              <SecLabSelect
                v-model="filters.status"
                class="filter-select"
                :options="statusOptions"
              />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.platformLog.filters.keywordLabel')">
              <SecLabInput
                v-model="filters.keyword"
                class="filter-input-long"
                :placeholder="t('app.platformLog.filters.keywordPlaceholder')"
                @keyup.enter="handleApplyFilters"
              />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.platformLog.filters.timeRangeLabel')">
              <SecLabDateTimeRangePicker
                v-model="filters.timeRange"
                class="filter-datetime-range"
                :placeholder="t('app.platformLog.filters.timeRangePlaceholder')"
                :start-label="t('app.platformLog.filters.startTimeLabel')"
                :end-label="t('app.platformLog.filters.endTimeLabel')"
                :shortcuts-label="t('app.platformLog.filters.shortcutsLabel')"
                :calendar-label="t('app.platformLog.filters.calendarLabel')"
                :time-label="t('app.platformLog.filters.timeLabel')"
                :clear-label="t('common.clear')"
                :confirm-label="t('common.confirm')"
                :cancel-label="t('common.cancel')"
                :locale="locale"
                :week-days="weekDayLabels"
                :shortcuts="timeRangeShortcuts"
                @apply="handleApplyFilters"
              />
            </SecLabFormItem>
          </div>
          <div class="actions">
            <SecLabButton type="primary" :loading="isLoading" @click="handleApplyFilters">
              {{ t('app.platformLog.filters.apply') }}
            </SecLabButton>
            <SecLabButton @click="handleResetFilters">
              {{ t('app.platformLog.filters.reset') }}
            </SecLabButton>
          </div>
        </div>
      </SecLabCard>

      <SecLabCard shadow="never" class="table-card" full-height>
        <SecLabTable :data="logs" :columns="eventTableColumns" border>
          <template #index="{ index }: { index: number }">
            <span class="time">{{ (currentPage - 1) * pageSize + index + 1 }}</span>
          </template>
          <template #time="{ row }: { row: any }">
            <span class="time">{{ formatDateTime(row.timestamp) }}</span>
          </template>
          <template #module="{ row }: { row: any }">
            <span>{{ formatModule(row.module) }}</span>
          </template>
          <template #user="{ row }: { row: any }">
            <span class="user">{{ formatUserSource(row) }}</span>
          </template>
          <template #event="{ row }: { row: any }">
            <span>{{ formatEventType(row.event) }}</span>
          </template>
          <template #result="{ row }: { row: any }">
            <SecLabTag :type="getResultType(row.status || '')" size="small">
              {{ row.status || '-' }}
            </SecLabTag>
          </template>
          <template #details="{ row }: { row: any }">
            <div
              v-if="hasMetadata(row.metadata)"
              class="message-cell clickable"
              @click="showLogDetail(row)"
            >
              <span class="view-detail-link">{{ t('common.details') }} &raquo;</span>
            </div>
            <div v-else class="message-cell">-</div>
          </template>
          <template #empty>
            <div class="empty-placeholder">
              <p>{{ t('app.platformLog.noLogs') }}</p>
            </div>
          </template>
        </SecLabTable>
      </SecLabCard>

      <SecLabCard shadow="never" class="pagination-card">
        <div class="pagination">
          <div class="size-selector">
            <SecLabSelect
              v-model="pageSize"
              class="size-select"
              :options="
                [10, 20, 50, 100].map((v) => ({
                  label: t('common.pagination.perPage', { n: v }),
                  value: v,
                }))
              "
            />
          </div>
          <SecLabPagination
            :current-page="currentPage"
            :total-pages="totalPages"
            @page-change="(p) => (currentPage = p)"
          />
          <div class="stats">
            {{ t('app.fileManager.pageStats', { currentPage, totalPages, total: totalLogs }) }}
          </div>
        </div>
      </SecLabCard>
    </div>

    <div v-else class="runtime-panel">
      <SecLabCard shadow="never" class="toolbar-card">
        <div class="toolbar">
          <div class="filters">
            <SecLabFormItem :label="t('app.platformLog.runtime.service')">
              <SecLabSelect
                v-model="runtimeQuery.service"
                class="filter-select-sm"
                :options="runtimeServiceOptions"
              />
            </SecLabFormItem>
            <SecLabFormItem
              v-if="runtimeQuery.service === 'agent'"
              :label="t('app.platformLog.runtime.node')"
            >
              <SecLabSelect
                v-model="runtimeQuery.nodeId"
                class="filter-select"
                :options="runtimeNodeOptions"
              />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.platformLog.runtime.file')">
              <SecLabSelect
                v-model="runtimeQuery.fileName"
                class="filter-select-lg"
                :options="runtimeFileOptions"
                :placeholder="t('app.platformLog.runtime.selectFile')"
              />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.platformLog.runtime.level')">
              <SecLabSelect
                v-model="runtimeQuery.level"
                class="filter-select-sm"
                :options="runtimeLevelOptions"
              />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.platformLog.filters.keywordLabel')">
              <SecLabInput
                v-model="runtimeQuery.keyword"
                class="filter-input"
                @keyup.enter="handleQueryRuntimeLogs"
              />
            </SecLabFormItem>
          </div>
          <div class="actions">
            <SecLabButton type="primary" :loading="runtimeLoading" @click="handleQueryRuntimeLogs">
              {{ t('app.platformLog.filters.apply') }}
            </SecLabButton>
            <SecLabButton plain @click="fetchRuntimeFiles">
              {{ t('app.platformLog.runtime.refreshFiles') }}
            </SecLabButton>
          </div>
        </div>
      </SecLabCard>

      <SecLabCard shadow="never" class="table-card" full-height>
        <SecLabTable :data="runtimeLines" :columns="runtimeTableColumns" border>
          <template #index="{ row, index }: { row: any; index: number }">
            <span v-if="row.message === '__LOAD_MORE__'">-</span>
            <span v-else class="time">{{ index + 1 }}</span>
          </template>
          <template #time="{ row }: { row: any }">
            <span v-if="row.message === '__LOAD_MORE__'">-</span>
            <span v-else class="time">{{ formatDateTime(row.timestamp) }}</span>
          </template>
          <template #source="{ row }: { row: any }">
            <span v-if="row.message === '__LOAD_MORE__'">-</span>
            <span v-else class="user">{{ row.source || '-' }}</span>
          </template>
          <template #level="{ row }: { row: any }">
            <span v-if="row.message === '__LOAD_MORE__'">-</span>
            <SecLabTag v-else :type="getRuntimeLevelTagType(row.level)" size="small">
              {{ row.level || 'INFO' }}
            </SecLabTag>
          </template>
          <template #target="{ row }: { row: any }">
            <span v-if="row.message === '__LOAD_MORE__'">-</span>
            <code v-else class="target-tag">{{ row.target || '-' }}</code>
          </template>
          <template #details="{ row }: { row: any }">
            <div v-if="row.message === '__LOAD_MORE__'" class="table-load-more-btn-wrapper">
              <SecLabButton
                :loading="runtimeLoading"
                plain
                size="small"
                @click="handleLoadMoreRuntimeLogs"
              >
                {{ t('app.platformLog.runtime.loadMore') }}
              </SecLabButton>
            </div>
            <div v-else class="message-cell">{{ row.message || '-' }}</div>
          </template>
          <template #empty>
            <div class="empty-placeholder">
              {{ t('app.platformLog.runtime.noLines') }}
            </div>
          </template>
        </SecLabTable>
      </SecLabCard>
    </div>

    <!-- 事件详情抽屉 -->
    <SecLabDrawer v-model="isDetailDrawerOpen" :title="t('app.platformLog.details')" width="600px">
      <div v-if="selectedLog" class="detail-drawer-content">
        <div class="detail-section">
          <h4>{{ t('app.platformLog.basicInfo') }}</h4>
          <table class="detail-table">
            <tbody>
              <tr>
                <th>{{ t('app.platformLog.time') }}</th>
                <td>{{ formatDateTime(selectedLog.timestamp) }}</td>
              </tr>
              <tr>
                <th>{{ t('app.platformLog.module') }}</th>
                <td>{{ formatModule(selectedLog.module) }}</td>
              </tr>
              <tr>
                <th>{{ t('app.platformLog.eventType') }}</th>
                <td>{{ formatEventType(selectedLog.event) }}</td>
              </tr>
              <tr>
                <th>{{ t('app.platformLog.userOrSource') }}</th>
                <td>{{ formatUserSource(selectedLog) }}</td>
              </tr>
              <tr v-if="clientIp">
                <th>{{ t('app.platformLog.clientIp') }}</th>
                <td>{{ clientIp }}</td>
              </tr>
              <tr>
                <th>{{ t('app.platformLog.result') }}</th>
                <td>
                  <SecLabTag :type="getResultType(selectedLog.status || '')" size="small">
                    {{ selectedLog.status || '-' }}
                  </SecLabTag>
                </td>
              </tr>
            </tbody>
          </table>
        </div>

        <div class="detail-section" v-if="selectedLog.metadata">
          <h4>{{ t('app.platformLog.metadata') }}</h4>
          <div class="json-wrapper">
            <code class="json-code">
              <span
                v-for="(line, lineIndex) in metadataTokenLines"
                :key="lineIndex"
                class="json-line"
                ><span
                  v-for="(token, tokenIndex) in line"
                  :key="`${lineIndex}-${tokenIndex}`"
                  :class="token.className"
                  >{{ token.text }}</span
                ></span
              >
            </code>
          </div>
        </div>
      </div>
    </SecLabDrawer>

    <SecLabLoading :loading="isLoading && logs.length === 0" cover />
  </div>
</template>

<style scoped>
.platform-log {
  height: 100%;
  padding: var(--sdl-space-3);
  background: var(--sdl-bg-canvas);
  overflow: hidden;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
  min-height: 0;
  box-sizing: border-box;
}

.tabs-card {
  flex-shrink: 0;
}

.events-panel,
.runtime-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
  min-height: 0;
}

.toolbar-card {
  flex-shrink: 0;
}

.filter-collapse-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sdl-space-4);
}

.filter-summary {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
  min-width: 0;
  font-size: var(--sdl-font-body-sm);
}

.summary-label {
  flex: 0 0 auto;
  color: var(--sdl-text-muted);
  font-weight: 600;
}

.summary-value {
  min-width: 0;
  overflow: hidden;
  color: var(--sdl-text-primary);
  font-family: var(--sdl-font-code);
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.filter-summary-actions {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
  flex-shrink: 0;
}

.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: flex-end;
  gap: var(--sdl-space-4);
  margin-top: var(--sdl-space-4);
  padding-top: var(--sdl-space-4);
  border-top: 1px solid var(--sdl-border-subtle);
}

.filters {
  flex: 1;
  display: flex;
  flex-wrap: wrap;
  gap: var(--sdl-space-4);
}

.filter-input {
  width: 160px;
}

.filter-input-long {
  width: 240px;
}

.filter-select {
  width: 140px;
}

.filter-select-sm {
  width: 100px;
}

.filter-select-lg {
  width: 200px;
}

.filter-datetime-range {
  width: 280px;
}

.actions {
  display: flex;
  gap: var(--sdl-space-2);
  padding-bottom: var(--sdl-space-1);
}

.table-card {
  flex: 1;
  min-height: 0;
}

.time {
  font-family: var(--sdl-font-mono);
  font-size: 12px;
  color: var(--sdl-text-secondary);
}

.user {
  font-weight: 500;
  color: var(--sdl-text-primary);
}

.message-cell {
  font-family: var(--sdl-font-code);
  font-size: 12px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--sdl-text-primary);
}

.log-excerpt {
  max-height: 200px;
  overflow-y: auto;
  font-family: var(--sdl-font-code);
  font-size: 12px;
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--sdl-text-muted);
}

.table-load-more-btn-wrapper {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--sdl-space-1) 0;
}

.clickable {
  cursor: pointer;
}

.view-detail-link {
  margin-left: 8px;
  font-size: 11px;
  color: var(--sdl-primary);
  opacity: 0.8;
  transition: opacity 0.2s;
}

.clickable:hover .view-detail-link {
  opacity: 1;
  text-decoration: underline;
}

.detail-drawer-content {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-5);
}

.detail-section h4 {
  margin: 0 0 var(--sdl-space-2) 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--sdl-text-primary);
  border-left: 3px solid var(--sdl-primary);
  padding-left: 8px;
}

.detail-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}

.detail-table th,
.detail-table td {
  padding: var(--sdl-space-2) var(--sdl-space-3);
  text-align: left;
  border-bottom: 1px solid var(--sdl-border-subtle);
}

.detail-table th {
  width: 120px;
  color: var(--sdl-text-secondary);
  font-weight: 500;
  background-color: var(--sdl-bg-muted);
}

.detail-table td {
  color: var(--sdl-text-primary);
  word-break: break-all;
}

.json-wrapper {
  background-color: #090d16;
  border-radius: var(--sdl-radius-md);
  padding: var(--sdl-space-3);
  border: 1px solid var(--sdl-border-subtle);
  max-height: 450px;
  overflow-y: auto;
}

.json-code {
  display: block;
  margin: 0;
  font-family: var(--sdl-font-code);
  font-size: 12px;
  line-height: 1.6;
  color: #c9d1d9;
  white-space: pre;
  word-break: normal;
  tab-size: 2;
  font-variant-ligatures: none;
}

.json-line {
  display: block;
  min-height: 1.6em;
  white-space: pre;
}

.json-key {
  color: #79c0ff;
  font-weight: 500;
}
.json-string {
  color: #a5d6ff;
}
.json-number {
  color: #ffab70;
}
.json-boolean {
  color: #ff7b72;
}
.json-null {
  color: #8b949e;
}

.rust-key {
  color: #f9758b;
  font-weight: 500;
}
.rust-string {
  color: #ffab70;
}
.rust-number {
  color: #ff7b72;
}
.rust-type {
  color: #b392f0;
  font-weight: 600;
}
.rust-none {
  color: #8b949e;
  font-style: italic;
}

.target-tag {
  background: var(--sdl-bg-muted);
  padding: 2px 6px;
  border-radius: var(--sdl-radius-sm);
  font-size: 11px;
  color: var(--sdl-text-muted);
}

.empty-placeholder {
  padding: var(--sdl-space-10) 0;
  text-align: center;
  color: var(--sdl-text-muted);
}

.empty-placeholder p {
  font-size: var(--sdl-font-body);
  margin-bottom: var(--sdl-space-2);
}

.pagination-card {
  flex-shrink: 0;
}

.pagination {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sdl-space-4);
}

.size-select {
  width: 120px;
}

.stats {
  font-size: var(--sdl-font-caption);
  color: var(--sdl-text-muted);
}

@media (max-width: 1200px) {
  .toolbar {
    flex-direction: column;
    align-items: stretch;
  }
  .actions {
    justify-content: flex-end;
  }
}
</style>
