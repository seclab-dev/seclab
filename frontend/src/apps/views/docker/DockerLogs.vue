<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import {
  SecLabAlert,
  SecLabButton,
  SecLabDateTimeRangePicker,
  SecLabDescriptions,
  SecLabDrawer,
  SecLabEmpty,
  SecLabFormItem,
  SecLabInput,
  SecLabLoading,
  SecLabPagination,
  SecLabSelect,
  SecLabTable,
  SecLabTag,
} from '@/components/ui'
import { dockerApi } from '@/api/modules/docker'
import type {
  DockerActivityActorKind,
  DockerActivityLevel,
  DockerActivityLogItem,
  DockerActivityLogQuery,
} from '@/api/interface/docker'
import { useI18n } from 'vue-i18n'
import { formatDateTime } from '@/utils/time'
import type { SecLabTableColumn } from '@/components/ui/SecLabTable.vue'

/** Docker 操作日志查询、四列表格与审计详情。 */
const { t, te, locale } = useI18n()
const logs = ref<DockerActivityLogItem[]>([])
const initialLoading = ref(false)
const refreshing = ref(false)
const errorMessage = ref('')
const currentPage = ref(1)
const pageSize = ref(20)
const totalLogs = ref(0)
const selectedLog = ref<DockerActivityLogItem | null>(null)
const detailOpen = ref(false)
let requestSequence = 0

const filters = ref<{
  level: '' | DockerActivityLevel
  actorKind: '' | DockerActivityActorKind
  keyword: string
  timeRange: { startAt: number | null; endAt: number | null }
}>({
  level: '',
  actorKind: '',
  keyword: '',
  timeRange: { startAt: null, endAt: null },
})

const totalPages = computed(() => Math.max(1, Math.ceil(totalLogs.value / pageSize.value)))
const columns = computed<SecLabTableColumn[]>(() => [
  { label: t('app.docker.activity.columns.time'), width: 180, slot: 'time', align: 'center' },
  { label: t('app.docker.activity.columns.actor'), width: 150, slot: 'actor', align: 'center' },
  { label: t('app.docker.activity.columns.level'), width: 100, slot: 'level', align: 'center' },
  { label: t('app.docker.activity.columns.message'), minWidth: 360, slot: 'message' },
])
const levelOptions = computed(() => [
  { label: t('app.docker.activity.filters.allLevels'), value: '' },
  { label: t('app.docker.activity.level.info'), value: 'info' },
  { label: t('app.docker.activity.level.warning'), value: 'warning' },
  { label: t('app.docker.activity.level.error'), value: 'error' },
])
const actorOptions = computed(() => [
  { label: t('app.docker.activity.filters.allSources'), value: '' },
  { label: t('app.docker.activity.actor.user'), value: 'user' },
  { label: t('app.docker.activity.actor.system'), value: 'system' },
])
const pageSizeOptions = computed(() =>
  [10, 20, 50].map((value) => ({
    label: t('common.pagination.perPage', { n: value }),
    value,
  })),
)
const timeRangeShortcuts = computed(() => [
  { label: t('app.platformLog.filters.shortcuts.last1h'), value: '1h' as const },
  { label: t('app.platformLog.filters.shortcuts.last24h'), value: '24h' as const },
  { label: t('app.platformLog.filters.shortcuts.last7d'), value: '7d' as const },
  { label: t('app.platformLog.filters.shortcuts.today'), value: 'today' as const },
])
const weekDayLabels = computed(() => [
  t('common.weekdays.sun'),
  t('common.weekdays.mon'),
  t('common.weekdays.tue'),
  t('common.weekdays.wed'),
  t('common.weekdays.thu'),
  t('common.weekdays.fri'),
  t('common.weekdays.sat'),
])

/** 返回日志级别对应的 SDL 标签类型。 */
const levelTagType = (level: DockerActivityLevel): 'info' | 'warning' | 'danger' => {
  if (level === 'warning') return 'warning'
  if (level === 'error') return 'danger'
  return 'info'
}

/** 将事件代码与结构化参数转换为当前语言的自然语言内容。 */
const formatMessage = (log: DockerActivityLogItem): string => {
  const terminalComposeCode = log.eventCode.match(/^compose\.(.+)\.(succeeded|failed)$/)
  const actionCode = (
    terminalComposeCode
      ? `compose.${terminalComposeCode[1]}`
      : /^compose\..+\.cancelled$/.test(log.eventCode)
        ? 'compose.taskCancelled'
        : log.eventCode
  )
    .replace('.submitted', 'Submitted')
    .replace('.cancelled', 'Cancelled')
    .replace('.cancel', 'Cancel')
  const actionKey = `app.docker.activity.actions.${actionCode}`
  const action = te(actionKey) ? t(actionKey) : log.eventCode
  const paramTarget = log.messageParams?.name
  const target = (typeof paramTarget === 'string' && paramTarget.trim()) || log.target?.id || '-'
  const paramContainer = log.messageParams?.container
  const container =
    log.eventCode.startsWith('network.') &&
    typeof paramContainer === 'string' &&
    paramContainer.trim()
      ? paramContainer
      : undefined
  if (log.outcome === 'failure' && log.errorMessage) {
    return t(
      container
        ? 'app.docker.activity.messages.networkFailureWithReason'
        : 'app.docker.activity.messages.failureWithReason',
      {
        action,
        target,
        container,
        reason: log.errorMessage,
      },
    )
  }
  if (container) {
    return t(
      `app.docker.activity.messages.network${log.outcome === 'success' ? 'Success' : 'Failure'}`,
      {
        action,
        target,
        container,
      },
    )
  }
  return t(`app.docker.activity.messages.${log.outcome}`, { action, target })
}

const detailItems = computed(() => {
  if (!selectedLog.value) return []
  const log = selectedLog.value
  return [
    { label: t('app.docker.activity.columns.time'), value: formatDateTime(log.occurredAt) },
    { label: t('app.docker.activity.columns.actor'), value: log.actor.name },
    {
      label: t('app.docker.activity.detail.outcome'),
      value: t(`app.docker.activity.outcome.${log.outcome}`),
    },
    { label: t('app.docker.activity.detail.eventCode'), value: log.eventCode },
    { label: t('app.docker.activity.detail.target'), value: log.target?.id || '-' },
    { label: t('app.docker.activity.detail.clientIp'), value: log.actor.clientIp || '-' },
    { label: t('app.docker.activity.detail.traceId'), value: log.traceId || '-' },
    { label: t('app.docker.activity.detail.error'), value: log.errorMessage || '-' },
  ]
})

const buildQuery = (): DockerActivityLogQuery => ({
  page: currentPage.value,
  pageSize: pageSize.value,
  levels: filters.value.level ? [filters.value.level] : undefined,
  actorKinds: filters.value.actorKind ? [filters.value.actorKind] : undefined,
  keyword: filters.value.keyword.trim() || undefined,
  startAt: filters.value.timeRange.startAt ?? undefined,
  endAt: filters.value.timeRange.endAt ?? undefined,
})

/** 查询操作日志；只有最后一次请求可以更新页面状态。 */
const fetchLogs = async () => {
  const sequence = ++requestSequence
  const isInitial = logs.value.length === 0
  initialLoading.value = isInitial
  refreshing.value = !isInitial
  errorMessage.value = ''
  const response = await dockerApi.fetchDockerActivityLogs(buildQuery())
  if (sequence !== requestSequence) return
  initialLoading.value = false
  refreshing.value = false
  if (!response.success || !response.data) {
    errorMessage.value = response.message || t('app.docker.activity.fetchFailed')
    return
  }
  logs.value = response.data.items ?? []
  totalLogs.value = response.data.total ?? 0
  const nextTotalPages = Math.max(1, Math.ceil(totalLogs.value / pageSize.value))
  if (currentPage.value > nextTotalPages) currentPage.value = nextTotalPages
}

const applyFilters = () => {
  if (currentPage.value === 1) void fetchLogs()
  else currentPage.value = 1
}
const resetFilters = () => {
  filters.value = {
    level: '',
    actorKind: '',
    keyword: '',
    timeRange: { startAt: null, endAt: null },
  }
  applyFilters()
}
const showDetail = (log: DockerActivityLogItem) => {
  selectedLog.value = log
  detailOpen.value = true
}

watch(currentPage, () => void fetchLogs())
watch(pageSize, () => {
  if (currentPage.value === 1) void fetchLogs()
  else currentPage.value = 1
})
onMounted(() => void fetchLogs())
</script>

<template>
  <div class="activity-page" data-page="docker-activity-logs">
    <div class="activity-toolbar" data-ui="activity-log-toolbar">
      <div class="filters" data-slot="filters">
        <fieldset class="filter-field">
          <legend class="filter-field-label">
            {{ t('app.docker.activity.filters.level') }}
          </legend>
          <SecLabSelect v-model="filters.level" :options="levelOptions" class="short-filter" />
        </fieldset>
        <fieldset class="filter-field">
          <legend class="filter-field-label">
            {{ t('app.docker.activity.filters.source') }}
          </legend>
          <SecLabSelect v-model="filters.actorKind" :options="actorOptions" class="short-filter" />
        </fieldset>
        <SecLabFormItem
          :label="t('app.docker.activity.filters.keyword')"
          for="docker-activity-log-keyword"
        >
          <SecLabInput
            id="docker-activity-log-keyword"
            v-model="filters.keyword"
            class="keyword-filter"
            :placeholder="t('app.docker.activity.filters.keywordPlaceholder')"
            @keyup.enter="applyFilters"
          />
        </SecLabFormItem>
        <fieldset class="filter-field">
          <legend class="filter-field-label">
            {{ t('app.docker.activity.filters.timeRange') }}
          </legend>
          <SecLabDateTimeRangePicker
            v-model="filters.timeRange"
            class="time-filter"
            :locale="locale"
            :week-days="weekDayLabels"
            :shortcuts="timeRangeShortcuts"
            :placeholder="t('app.platformLog.filters.timeRangePlaceholder')"
            :start-label="t('app.platformLog.filters.startTimeLabel')"
            :end-label="t('app.platformLog.filters.endTimeLabel')"
            :shortcuts-label="t('app.platformLog.filters.shortcutsLabel')"
            :calendar-label="t('app.platformLog.filters.calendarLabel')"
            :time-label="t('app.platformLog.filters.timeLabel')"
            :clear-label="t('common.clear')"
            :confirm-label="t('common.confirm')"
            :cancel-label="t('common.cancel')"
            @apply="applyFilters"
          />
        </fieldset>
      </div>
      <div class="toolbar-actions" data-slot="actions">
        <SecLabButton type="primary" :loading="refreshing" @click="applyFilters">
          {{ t('app.docker.activity.filters.query') }}
        </SecLabButton>
        <SecLabButton @click="resetFilters">
          {{ t('app.docker.activity.filters.reset') }}
        </SecLabButton>
        <SecLabButton :loading="refreshing" @click="fetchLogs">
          {{ t('common.refresh') }}
        </SecLabButton>
      </div>
    </div>

    <SecLabAlert
      v-if="errorMessage"
      type="error"
      :title="errorMessage"
      show-icon
      data-ui="activity-log-error"
    />

    <div class="activity-table-shell" data-ui="activity-log-table">
      <SecLabTable v-if="logs.length" :data="logs" :columns="columns" border>
        <template #time="{ row }: { row: DockerActivityLogItem }">
          <span class="time-cell">{{ formatDateTime(row.occurredAt) }}</span>
        </template>
        <template #actor="{ row }: { row: DockerActivityLogItem }">
          <span>{{ row.actor.name }}</span>
        </template>
        <template #level="{ row }: { row: DockerActivityLogItem }">
          <SecLabTag :type="levelTagType(row.level)" size="small">
            {{ t(`app.docker.activity.level.${row.level}`) }}
          </SecLabTag>
        </template>
        <template #message="{ row }: { row: DockerActivityLogItem }">
          <button class="message-cell" type="button" @click="showDetail(row)">
            {{ formatMessage(row) }}
          </button>
        </template>
      </SecLabTable>
      <SecLabEmpty v-else-if="!initialLoading" :description="t('app.docker.activity.empty')" />
      <SecLabLoading :loading="initialLoading" cover />
    </div>

    <div class="pagination-bar" data-ui="activity-log-pagination">
      <SecLabSelect v-model="pageSize" size="small" class="page-size" :options="pageSizeOptions" />
      <SecLabPagination
        :current-page="currentPage"
        :total-pages="totalPages"
        @page-change="(page) => (currentPage = page)"
      />
      <span>{{
        t('app.fileManager.pageStats', { currentPage, totalPages, total: totalLogs })
      }}</span>
    </div>

    <SecLabDrawer v-model="detailOpen" :title="t('app.docker.activity.detail.title')" width="600px">
      <div v-if="selectedLog" class="detail-content" data-ui="activity-log-detail">
        <p class="detail-message">{{ formatMessage(selectedLog) }}</p>
        <SecLabDescriptions :items="detailItems" :column="1" border />
      </div>
    </SecLabDrawer>
  </div>
</template>

<style scoped>
.activity-page {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
  padding-top: var(--sdl-space-3);
}
.activity-toolbar {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: var(--sdl-space-4);
  padding: var(--sdl-space-3) var(--sdl-space-4);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  background: var(--sdl-bg-panel);
}
.filters {
  display: flex;
  align-items: flex-end;
  flex-wrap: wrap;
  gap: var(--sdl-space-3);
  min-width: 0;
}
.filter-field {
  min-width: 0;
  margin: 0 0 var(--sdl-space-4);
  padding: 0;
  border: 0;
}
.filter-field-label {
  margin-bottom: var(--sdl-space-2);
  padding: 0;
  color: var(--sdl-text-secondary);
  font-size: var(--sdl-font-body-sm);
  font-weight: 600;
  line-height: 1.4;
}
.toolbar-actions {
  display: flex;
  gap: var(--sdl-space-2);
  flex-shrink: 0;
}
.short-filter {
  width: 130px;
}
.keyword-filter {
  width: 220px;
}
.time-filter {
  width: 300px;
}
.activity-table-shell {
  position: relative;
  flex: 1;
  min-height: 240px;
  overflow: auto;
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  background: var(--sdl-bg-panel);
}
.time-cell {
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
}
.message-cell {
  width: 100%;
  padding: 0;
  overflow: hidden;
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  border: 0;
  background: transparent;
  color: var(--sdl-text-secondary);
  font: inherit;
  font-family: var(--sdl-font-mono);
  line-height: 1.5;
  text-align: left;
  cursor: pointer;
  word-break: break-word;
}
.message-cell:hover {
  color: var(--sdl-color-primary);
}
.pagination-bar {
  display: grid;
  grid-template-columns: 120px 1fr auto;
  align-items: center;
  gap: var(--sdl-space-3);
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-caption);
}
.page-size {
  width: 120px;
}
.detail-content {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-4);
}
.detail-message {
  margin: 0;
  padding: var(--sdl-space-3);
  border-left: 3px solid var(--sdl-color-primary);
  background: var(--sdl-bg-subtle);
  color: var(--sdl-text-primary);
  font-family: var(--sdl-font-mono);
  white-space: pre-wrap;
  word-break: break-word;
}
@media (max-width: 960px) {
  .activity-toolbar {
    align-items: stretch;
    flex-direction: column;
  }
  .toolbar-actions {
    justify-content: flex-end;
  }
  .pagination-bar {
    grid-template-columns: 1fr;
    justify-items: center;
  }
}
</style>
