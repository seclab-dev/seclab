<script setup lang="ts">
/**
 * @file ScheduledTaskDetailDrawer.vue
 * @description 计划任务详情、执行记录和受限输出摘要。
 */

import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type {
  ScheduledTaskDetail,
  ScheduledTaskRun,
  ScheduledTaskRunOutput,
  ScheduledTaskSummary,
} from '@/api/generated/scheduled-tasks'
import type { TaskRequestState } from '@/composables/useTaskScheduler'
import {
  SecLabAlert,
  SecLabButton,
  SecLabDescriptions,
  SecLabDrawer,
  SecLabEmpty,
  SecLabLoading,
  SecLabTable,
  SecLabTabs,
  SecLabTag,
} from '@/components/ui'
import type { SecLabTableColumn } from '@/components/ui/SecLabTable.vue'

const props = defineProps<{
  modelValue: boolean
  task: ScheduledTaskSummary | null
  detail: ScheduledTaskDetail | null
  detailState: TaskRequestState
  runs: ScheduledTaskRun[]
  runsState: TaskRequestState
  output: ScheduledTaskRunOutput | null
  outputState: TaskRequestState
  isCancelPending: (runId: string) => boolean
}>()
const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  output: [run: ScheduledTaskRun]
  cancel: [run: ScheduledTaskRun]
}>()

const { t } = useI18n()
const activeTab = ref('detail')
const outputVisible = ref(false)
const runColumns = computed<SecLabTableColumn[]>(() => [
  {
    prop: 'queuedAt',
    label: t('app.taskScheduler.runs.columns.queuedAt'),
    minWidth: 170,
    slot: 'queuedAt',
  },
  {
    prop: 'triggerSource',
    label: t('app.taskScheduler.runs.columns.triggerSource'),
    width: 105,
    slot: 'triggerSource',
  },
  {
    prop: 'status',
    label: t('app.taskScheduler.runs.columns.status'),
    width: 120,
    align: 'center',
    slot: 'status',
  },
  {
    prop: 'duration',
    label: t('app.taskScheduler.runs.columns.duration'),
    width: 110,
    slot: 'duration',
  },
  {
    label: t('app.taskScheduler.columns.actions'),
    width: 160,
    align: 'center',
    slot: 'actions',
  },
])

const formatTime = (value?: string) => (value ? new Date(value).toLocaleString() : '')
const statusLabel = (status: string) => t(`app.taskScheduler.status.${status}`)
const triggerLabel = (source: string) => t(`app.taskScheduler.trigger.${source}`)
const statusTag = (status?: string): 'success' | 'danger' | 'info' | 'warning' | 'default' => {
  if (status === 'succeeded' || status === 'ready') return 'success'
  if (status === 'failed' || status === 'timedOut') return 'danger'
  if (status === 'waitingForNode' || status === 'partial' || status === 'cancelled')
    return 'warning'
  if (status && ['queued', 'starting', 'running', 'cancelling'].includes(status)) return 'info'
  return 'default'
}
const duration = (run: ScheduledTaskRun) => {
  if (!run.startedAt) return t('app.taskScheduler.status.notStarted')
  const end = run.finishedAt ? new Date(run.finishedAt).getTime() : Date.now()
  const seconds = Math.max(0, Math.round((end - new Date(run.startedAt).getTime()) / 1000))
  return t('app.taskScheduler.runs.seconds', { seconds })
}
const detailItems = computed(() => {
  if (!props.detail) return []
  return [
    { label: t('app.taskScheduler.detail.name'), value: props.detail.name },
    ...(props.detail.description
      ? [{ label: t('app.taskScheduler.detail.description'), value: props.detail.description }]
      : []),
    { label: t('app.taskScheduler.detail.node'), value: props.detail.node.nodeName },
    { label: t('app.taskScheduler.detail.schedule'), value: props.detail.schedule.summary },
    { label: t('app.taskScheduler.detail.cron'), value: props.detail.schedule.cronExpr },
    { label: t('app.taskScheduler.detail.timeZone'), value: props.detail.schedule.timeZone },
    {
      label: t('app.taskScheduler.detail.timeout'),
      value: t('app.taskScheduler.runs.seconds', {
        seconds: props.detail.execution.timeoutSeconds,
      }),
    },
    {
      label: t('app.taskScheduler.detail.preventOverlap'),
      value: props.detail.execution.preventOverlap
        ? t('app.taskScheduler.status.yes')
        : t('app.taskScheduler.status.no'),
    },
    { label: t('app.taskScheduler.detail.revision'), value: props.detail.deployment.revision },
    ...(props.detail.deployment.lastSyncedAt
      ? [
          {
            label: t('app.taskScheduler.detail.lastSyncedAt'),
            value: formatTime(props.detail.deployment.lastSyncedAt),
          },
        ]
      : []),
  ]
})

const openOutput = (run: ScheduledTaskRun) => {
  outputVisible.value = true
  emit('output', run)
}
</script>

<template>
  <SecLabDrawer
    :model-value="modelValue"
    data-ui="runs"
    :title="task?.name ?? t('app.taskScheduler.detail.title')"
    width="860px"
    @update:model-value="emit('update:modelValue', $event)"
  >
    <div class="detail-content" data-slot="detail">
      <SecLabTabs
        v-model="activeTab"
        :tabs="[
          { name: 'detail', label: t('app.taskScheduler.detail.tab') },
          { name: 'runs', label: t('app.taskScheduler.runs.tab') },
        ]"
      />
      <template v-if="activeTab === 'detail'">
        <SecLabAlert
          v-if="detail?.deployment.errorSummary"
          type="warning"
          :title="statusLabel(detail.deployment.status)"
          :description="detail.deployment.errorSummary"
          show-icon
        />
        <SecLabDescriptions v-if="detail" :items="detailItems" :column="2" border />
        <div v-if="detail" class="command-detail">
          <span class="block-title">{{ t('app.taskScheduler.detail.command') }}</span>
          <pre>{{ detail.execution.command }}</pre>
        </div>
        <SecLabEmpty v-else-if="detailState.error" :description="detailState.error" />
        <SecLabLoading :loading="detailState.initialLoading" cover />
      </template>
      <template v-else>
        <div class="runs-shell">
          <SecLabTable :data="runs" :columns="runColumns" row-key="runId" border>
            <template #queuedAt="{ row }: { row: ScheduledTaskRun }">
              {{ formatTime(row.queuedAt) }}
            </template>
            <template #triggerSource="{ row }: { row: ScheduledTaskRun }">
              {{ triggerLabel(row.triggerSource) }}
            </template>
            <template #status="{ row }: { row: ScheduledTaskRun }">
              <SecLabTag :type="statusTag(row.status)">{{ statusLabel(row.status) }}</SecLabTag>
              <span v-if="row.phase && !row.finishedAt" class="run-phase">{{ row.phase }}</span>
            </template>
            <template #duration="{ row }: { row: ScheduledTaskRun }">
              {{ duration(row) }}
            </template>
            <template #actions="{ row }: { row: ScheduledTaskRun }">
              <div class="inline-actions">
                <SecLabButton v-if="row.output.available" size="small" @click="openOutput(row)">
                  {{ t('app.taskScheduler.runs.output') }}
                </SecLabButton>
                <SecLabButton
                  v-if="row.capabilities.canCancel"
                  size="small"
                  type="danger"
                  :loading="isCancelPending(row.runId)"
                  @click="emit('cancel', row)"
                >
                  {{ t('common.cancel') }}
                </SecLabButton>
              </div>
            </template>
            <template #empty>
              <SecLabEmpty :description="t('app.taskScheduler.runs.empty')" />
            </template>
          </SecLabTable>
          <SecLabLoading :loading="runsState.initialLoading" cover />
        </div>
      </template>
    </div>
  </SecLabDrawer>

  <SecLabDrawer
    v-model="outputVisible"
    data-ui="run-output"
    :title="t('app.taskScheduler.runs.outputTitle')"
    width="760px"
  >
    <div class="output-content" data-slot="content">
      <SecLabAlert
        v-if="output?.truncated"
        type="warning"
        :title="t('app.taskScheduler.runs.outputTruncated')"
        show-icon
      />
      <pre v-if="output">{{ output.content }}</pre>
      <SecLabEmpty v-else-if="outputState.error" :description="outputState.error" />
      <SecLabLoading :loading="outputState.initialLoading" cover />
    </div>
  </SecLabDrawer>
</template>

<style scoped>
.detail-content,
.command-detail,
.output-content {
  position: relative;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
}

.runs-shell {
  position: relative;
  min-height: 260px;
  overflow: auto;
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  background: var(--sdl-bg-panel);
}

.command-detail pre,
.output-content pre {
  margin: 0;
  padding: var(--sdl-space-3);
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-word;
  border: 1px solid var(--sdl-border-subtle);
  border-radius: var(--sdl-radius-md);
  background: var(--sdl-bg-subtle);
  color: var(--sdl-text-primary);
  font-family: var(--sdl-font-mono);
}

.output-content {
  height: 100%;
}

.output-content pre {
  flex: 1;
  min-height: 240px;
}

.block-title {
  color: var(--sdl-text-primary);
  font-weight: 600;
}

.run-phase {
  display: block;
  margin-top: var(--sdl-space-1);
  color: var(--sdl-text-secondary);
  font-size: var(--sdl-font-caption);
}

.inline-actions {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
}
</style>
