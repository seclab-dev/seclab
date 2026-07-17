<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type {
  OperationImpact,
  OperationLogDetail,
  OperationLogQuery,
  OperationLogSummary,
  OperationModule,
  OperationOutcome,
} from '@/api/generated'
import { operationLogApi } from '@/api/modules/operation-log'
import {
  SecLabAlert,
  SecLabButton,
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
import type { SecLabTableColumn } from '@/components/ui/SecLabTable.vue'

const { t, te } = useI18n()
const props = defineProps<{ module?: OperationModule; nodeId?: string; embedded?: boolean }>()
const PAGE_SIZE = 20
const items = ref<OperationLogSummary[]>([])
const total = ref(0)
const totalPages = computed(() => Math.max(1, Math.ceil(total.value / PAGE_SIZE)))
const page = ref(1)
const initialLoading = ref(false)
const refreshing = ref(false)
const initialError = ref('')
const refreshWarning = ref('')
const loadedAt = ref<string>()
const filtersExpanded = ref(false)
const detailOpen = ref(false)
const detail = ref<OperationLogDetail>()
const detailLoading = ref(false)
const detailError = ref('')
let listSequence = 0
let detailSequence = 0
let listController: AbortController | undefined
let detailController: AbortController | undefined
let pollTimer: number | undefined

const filters = reactive({
  range: '24h',
  outcome: '' as OperationOutcome | '',
  keyword: '',
  module: '' as OperationModule | '',
  impact: '' as OperationImpact | '',
})

const columns = computed<SecLabTableColumn[]>(() => [
  { label: t('app.operationLog.columns.index'), width: 72, slot: 'index', align: 'center' },
  { label: t('app.operationLog.columns.time'), width: 190, slot: 'time', align: 'center' },
  { label: t('app.operationLog.columns.impact'), width: 100, slot: 'impact', align: 'center' },
  { label: t('app.operationLog.columns.actor'), minWidth: 160, slot: 'actor' },
  {
    label: t('app.operationLog.columns.operation'),
    minWidth: 210,
    slot: 'operation',
    align: 'center',
  },
  { label: t('app.operationLog.columns.outcome'), width: 110, slot: 'outcome', align: 'center' },
  { label: t('common.actions'), width: 90, slot: 'actions', align: 'center' },
])

const rangeOptions = computed(() => [
  { label: t('app.operationLog.ranges.hour'), value: '1h' },
  { label: t('app.operationLog.ranges.day'), value: '24h' },
  { label: t('app.operationLog.ranges.week'), value: '7d' },
])
const outcomeOptions = computed(() => [
  { label: t('app.operationLog.allOutcomes'), value: '' },
  ...(['success', 'failure', 'partial', 'canceled', 'timedOut'] as OperationOutcome[]).map(
    (value) => ({ label: label('outcomes', value), value }),
  ),
])
const moduleOptions = computed(() => [
  { label: t('app.operationLog.allModules'), value: '' },
  ...(
    [
      'auth',
      'nodes',
      'suites',
      'docker',
      'files',
      'processes',
      'disks',
      'monitoring',
      'scripts',
      'scheduledTasks',
      'upgrades',
      'terminal',
      'settings',
    ] as OperationModule[]
  ).map((value) => ({ label: label('modules', value), value })),
])
const impactOptions = computed(() => [
  { label: t('app.operationLog.allImpacts'), value: '' },
  ...(['info', 'warning', 'error'] as OperationImpact[]).map((value) => ({
    label: label('impacts', value),
    value,
  })),
])
const detailItems = computed(() => {
  if (!detail.value) return []
  const value = detail.value
  return [
    { label: t('app.operationLog.columns.time'), value: formatTime(value.occurredAt) },
    { label: t('app.operationLog.columns.operation'), value: eventLabel(value.eventCode) },
    { label: t('app.operationLog.columns.actor'), value: actorLabel(value.actor) },
    ...(value.clientIp
      ? [{ label: t('app.operationLog.detail.clientIp'), value: value.clientIp }]
      : []),
    { label: t('app.operationLog.columns.outcome'), value: label('outcomes', value.outcome) },
    ...(value.target?.displayName
      ? [{ label: t('app.operationLog.columns.target'), value: value.target.displayName }]
      : []),
    ...(value.target
      ? [
          { label: t('app.operationLog.detail.targetKind'), value: value.target.kind },
          { label: t('app.operationLog.detail.targetId'), value: value.target.id },
        ]
      : []),
    { label: t('app.operationLog.detail.traceId'), value: value.traceId },
    ...(value.taskId ? [{ label: t('app.operationLog.detail.taskId'), value: value.taskId }] : []),
    ...(value.requestMethod
      ? [
          {
            label: t('app.operationLog.detail.request'),
            value: `${value.requestMethod} ${value.routeTemplate ?? ''}`.trim(),
          },
        ]
      : []),
    ...(value.errorCode
      ? [{ label: t('app.operationLog.detail.errorCode'), value: value.errorCode }]
      : []),
    ...(value.errorSummary
      ? [{ label: t('app.operationLog.detail.errorSummary'), value: value.errorSummary }]
      : []),
  ]
})
const parameterItems = computed(() =>
  Object.entries(detail.value?.parameters ?? {}).map(([key, value]) => ({
    label: key,
    value: String(value),
  })),
)

function label(group: string, value: string) {
  const key = `app.operationLog.${group}.${value}`
  return te(key) ? t(key) : value
}
function eventLabel(value: string) {
  const key = `app.operationLog.events.${value}`
  return te(key) ? t(key) : value.replaceAll('_', ' ')
}
function actorLabel(actor: OperationLogSummary['actor']) {
  return actor.kind === 'anonymous' ? t('app.operationLog.actorKinds.anonymous') : actor.displayName
}
function actorText(value: OperationLogSummary) {
  const actor = actorLabel(value.actor)
  return value.clientIp ? `${actor} (${value.clientIp})` : actor
}
function formatTime(value: string) {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  const pad = (part: number) => String(part).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}
function rangeStart() {
  const hours = filters.range === '1h' ? 1 : filters.range === '7d' ? 168 : 24
  return new Date(Date.now() - hours * 60 * 60 * 1000).toISOString()
}
function buildQuery(): OperationLogQuery {
  return {
    page: page.value,
    pageSize: PAGE_SIZE,
    occurredFrom: rangeStart(),
    occurredTo: new Date().toISOString(),
    outcomes: filters.outcome ? [filters.outcome] : undefined,
    modules: props.module ? [props.module] : filters.module ? [filters.module] : undefined,
    nodeIds: props.nodeId && props.nodeId !== 'local' ? [props.nodeId] : undefined,
    impacts: filters.impact ? [filters.impact] : undefined,
    keyword: filters.keyword.trim() || undefined,
  }
}
async function load(options: { silent?: boolean } = {}) {
  const sequence = ++listSequence
  listController?.abort()
  listController = new AbortController()
  const hasData = loadedAt.value !== undefined
  if (!hasData) initialLoading.value = true
  else if (!options.silent) refreshing.value = true
  initialError.value = ''
  try {
    const response = await operationLogApi.query(buildQuery(), listController.signal)
    if (sequence !== listSequence) return
    if (!response.success || !response.data) throw new Error(response.message)
    items.value = response.data.items
    total.value = response.data.total
    loadedAt.value = new Date().toISOString()
    refreshWarning.value = ''
  } catch (error) {
    if (sequence !== listSequence || (error instanceof DOMException && error.name === 'AbortError'))
      return
    const message = error instanceof Error ? error.message : t('app.operationLog.loadFailed')
    if (hasData) refreshWarning.value = message
    else initialError.value = message
  } finally {
    if (sequence === listSequence) {
      initialLoading.value = false
      refreshing.value = false
    }
  }
}
function applyFilters() {
  page.value = 1
  void load()
}
async function openDetail(row: OperationLogSummary) {
  const sequence = ++detailSequence
  detailController?.abort()
  detailController = new AbortController()
  detailOpen.value = true
  detail.value = undefined
  detailError.value = ''
  detailLoading.value = true
  try {
    const response = await operationLogApi.detail(row.eventId, detailController.signal)
    if (sequence !== detailSequence) return
    if (!response.success || !response.data) throw new Error(response.message)
    detail.value = response.data
  } catch (error) {
    if (
      sequence !== detailSequence ||
      (error instanceof DOMException && error.name === 'AbortError')
    )
      return
    detailError.value =
      error instanceof Error ? error.message : t('app.operationLog.detail.loadFailed')
  } finally {
    if (sequence === detailSequence) detailLoading.value = false
  }
}
function impactTag(value: OperationImpact) {
  return value === 'error' ? 'danger' : value === 'warning' ? 'warning' : 'info'
}
function outcomeTag(value: OperationOutcome) {
  return value === 'failure' || value === 'timedOut'
    ? 'danger'
    : value === 'partial'
      ? 'warning'
      : value === 'success'
        ? 'success'
        : 'info'
}
function poll() {
  if (document.visibilityState === 'visible' && !initialLoading.value && !refreshing.value)
    void load({ silent: true })
}
watch(page, () => void load())
onMounted(() => {
  void load()
  pollTimer = window.setInterval(poll, 30_000)
})
onBeforeUnmount(() => {
  listController?.abort()
  detailController?.abort()
  if (pollTimer) window.clearInterval(pollTimer)
})
</script>

<template>
  <div
    class="operation-log"
    :class="{ embedded: props.embedded }"
    data-page="operation-log"
    data-seclab-app="operation-log"
  >
    <div class="toolbar" data-ui="toolbar" data-slot="controls">
      <SecLabFormItem
        class="filter-select"
        :label="t('app.operationLog.timeRange')"
        for="operation-log-range"
      >
        <SecLabSelect
          id="operation-log-range"
          v-model="filters.range"
          name="operationLogRange"
          :options="rangeOptions"
        />
      </SecLabFormItem>
      <SecLabFormItem
        class="filter-select"
        :label="t('app.operationLog.outcome')"
        for="operation-log-outcome"
      >
        <SecLabSelect
          id="operation-log-outcome"
          v-model="filters.outcome"
          name="operationLogOutcome"
          :options="outcomeOptions"
        />
      </SecLabFormItem>
      <SecLabFormItem
        class="search"
        :label="t('app.operationLog.keyword')"
        for="operation-log-keyword"
      >
        <SecLabInput
          id="operation-log-keyword"
          v-model="filters.keyword"
          name="operationLogKeyword"
          :aria-label="t('app.operationLog.keyword')"
          @keyup.enter="applyFilters"
        />
      </SecLabFormItem>
      <div class="toolbar-actions" data-ui="toolbar-actions" data-slot="actions">
        <SecLabButton @click="filtersExpanded = !filtersExpanded">{{
          t('app.operationLog.moreFilters')
        }}</SecLabButton>
        <SecLabButton type="primary" @click="applyFilters">{{
          t('app.operationLog.query')
        }}</SecLabButton>
        <SecLabButton :loading="refreshing" @click="load()">{{ t('common.refresh') }}</SecLabButton>
      </div>
    </div>

    <div v-if="filtersExpanded" class="advanced-filters" data-ui="filters" data-slot="controls">
      <SecLabFormItem
        v-if="!props.module"
        class="filter-select"
        :label="t('app.operationLog.module')"
        for="operation-log-module"
      >
        <SecLabSelect
          id="operation-log-module"
          v-model="filters.module"
          name="operationLogModule"
          :options="moduleOptions"
        />
      </SecLabFormItem>
      <SecLabFormItem
        class="filter-select"
        :label="t('app.operationLog.impact')"
        for="operation-log-impact"
      >
        <SecLabSelect
          id="operation-log-impact"
          v-model="filters.impact"
          name="operationLogImpact"
          :options="impactOptions"
        />
      </SecLabFormItem>
    </div>

    <SecLabAlert
      v-if="refreshWarning"
      data-ui="refresh-warning"
      type="warning"
      :title="t('app.operationLog.refreshFailed')"
      :description="refreshWarning"
      show-icon
    />
    <SecLabAlert
      v-if="initialError"
      data-ui="initial-error"
      type="error"
      :title="t('app.operationLog.loadFailed')"
      :description="initialError"
      show-icon
    />

    <div class="table-shell" data-ui="table" data-slot="content">
      <SecLabTable v-if="items.length" :data="items" :columns="columns" row-key="eventId" border>
        <template #index="{ index }: { index: number }">
          {{ (page - 1) * PAGE_SIZE + index + 1 }}
        </template>
        <template #time="{ row }: { row: OperationLogSummary }">{{
          formatTime(row.occurredAt)
        }}</template>
        <template #impact="{ row }: { row: OperationLogSummary }"
          ><SecLabTag :type="impactTag(row.impact)">{{
            label('impacts', row.impact)
          }}</SecLabTag></template
        >
        <template #actor="{ row }: { row: OperationLogSummary }"
          ><strong>{{ actorText(row) }}</strong
          ><small v-if="row.origin.kind === 'agent'" data-slot="origin">{{
            row.origin.nodeName || label('origins', row.origin.kind)
          }}</small></template
        >
        <template #operation="{ row }: { row: OperationLogSummary }">{{
          eventLabel(row.eventCode)
        }}</template>
        <template #outcome="{ row }: { row: OperationLogSummary }"
          ><SecLabTag :type="outcomeTag(row.outcome)">{{
            label('outcomes', row.outcome)
          }}</SecLabTag></template
        >
        <template #actions="{ row }: { row: OperationLogSummary }"
          ><SecLabButton size="small" @click="openDetail(row)">{{
            t('common.details')
          }}</SecLabButton></template
        >
      </SecLabTable>
      <SecLabEmpty
        v-else-if="!initialLoading"
        :description="
          initialError
            ? t('app.operationLog.unavailable')
            : filters.keyword || filters.outcome || filters.module || filters.impact
              ? t('app.operationLog.noMatches')
              : t('app.operationLog.empty')
        "
      />
      <SecLabLoading :loading="initialLoading" cover />
    </div>
    <SecLabPagination
      v-if="total > PAGE_SIZE"
      :current-page="page"
      :total-pages="totalPages"
      data-ui="pagination"
      @page-change="(value) => (page = value)"
    />

    <SecLabDrawer
      v-model="detailOpen"
      data-ui="detail"
      :title="t('app.operationLog.detail.title')"
      width="680px"
    >
      <div class="detail-content" data-slot="detail">
        <SecLabAlert
          v-if="detailError"
          type="error"
          :title="t('app.operationLog.detail.loadFailed')"
          :description="detailError"
          show-icon
        />
        <template v-if="detail">
          <SecLabDescriptions :items="detailItems" :column="2" border />
          <SecLabDescriptions
            v-if="parameterItems.length"
            :items="parameterItems"
            :column="2"
            border
            data-ui="parameters"
          />
        </template>
        <SecLabLoading :loading="detailLoading" cover />
      </div>
    </SecLabDrawer>
  </div>
</template>

<style scoped>
.operation-log {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
  padding: var(--sdl-space-3);
  background: var(--sdl-bg-canvas);
  color: var(--sdl-text-primary);
}
.operation-log.embedded {
  padding: var(--sdl-space-3) 0 0;
}
.toolbar,
.advanced-filters {
  display: flex;
  align-items: end;
  flex-wrap: wrap;
  gap: var(--sdl-space-2);
}
.search {
  flex: 0 1 260px;
  inline-size: 260px;
  max-inline-size: 100%;
}
.filter-select {
  flex: 0 0 200px;
  inline-size: 200px;
  max-inline-size: 100%;
}
.toolbar-actions {
  display: flex;
  align-items: flex-end;
  justify-content: flex-end;
  gap: var(--sdl-space-2);
  margin-block-end: var(--sdl-space-4);
  margin-inline-start: auto;
}
.advanced-filters {
  padding-block: var(--sdl-space-2);
  border-block: 1px solid var(--sdl-border-subtle);
}
.table-shell {
  position: relative;
  flex: 1;
  min-height: 0;
  overflow: auto;
}
.detail-content {
  position: relative;
  min-height: 180px;
  display: grid;
  gap: var(--sdl-space-3);
}
small {
  display: block;
  color: var(--sdl-text-muted);
  margin-top: var(--sdl-space-1);
}
@media (max-width: 760px) {
  .toolbar > *,
  .advanced-filters > * {
    flex: 1 1 180px;
  }
}
</style>
