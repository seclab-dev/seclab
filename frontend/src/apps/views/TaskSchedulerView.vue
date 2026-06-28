<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { formatDateTime } from '@/utils/time'
import { taskApi } from '@/api/modules/task'
import { nodesApi, type NodeSummaryResponse } from '@/api/modules/nodes'
import type { TaskItem, TaskRun, UpsertTaskPayload } from '@/api/interface/task'
import { useNotificationStore } from '@/stores/notification'
import { useConfirmationModalStore } from '@/stores/confirmation-modal'
import {
  SecLabButton,
  SecLabCard,
  SecLabCheckbox,
  SecLabInput,
  SecLabSelect,
  SecLabTable,
  SecLabLoading,
  SecLabDrawer,
  SecLabTag,
  SecLabDialog,
  SecLabActionMenu,
} from '@/components/ui'
import SecLabIcon from '@/components/icons/SecLabIcon.vue'
import { useCronBuilder } from '@/composables/useCronBuilder'
import MonacoEditor from '@/components/editor/MonacoEditor.vue'
import type { SecLabTableColumn } from '@/components/ui/SecLabTable.vue'

defineProps<{
  isMaximized?: boolean
  payload?: Record<string, unknown>
}>()

const { t } = useI18n()
const notificationStore = useNotificationStore()
const confirmationModal = useConfirmationModalStore()

const loading = ref(false)
const saving = ref(false)
const nodes = ref<NodeSummaryResponse[]>([])
const tasks = ref<TaskItem[]>([])
const selectedNodeId = ref('')
const keyword = ref('')

const getTaskActions = (row: TaskItem) => {
  return [
    {
      label: t('app.taskScheduler.actions.edit'),
      icon: 'edit',
      handler: () => openEdit(row),
    },
    {
      label: row.enabled
        ? t('app.taskScheduler.actions.disable')
        : t('app.taskScheduler.actions.enable'),
      icon: row.enabled ? 'x' : 'play',
      handler: () => toggleTask(row),
    },
    {
      label: t('app.taskScheduler.actions.runNow'),
      icon: 'play',
      handler: () => runTask(row),
    },
    {
      label:
        row.syncStatus === 'conflict'
          ? t('app.taskScheduler.actions.forceSync')
          : t('app.taskScheduler.actions.retrySync'),
      icon: 'refresh',
      handler: () => retrySyncTask(row),
    },
    {
      label: t('app.taskScheduler.actions.runs'),
      icon: 'log',
      handler: () => openRuns(row),
    },
    {
      label: t('app.taskScheduler.actions.delete'),
      icon: 'trash',
      handler: () => removeTask(row),
      type: 'danger',
    },
  ]
}

const editVisible = ref(false)
const editTaskId = ref<number | null>(null)

const runsVisible = ref(false)
const runsLoading = ref(false)
const runsTask = ref<TaskItem | null>(null)
const taskRuns = ref<TaskRun[]>([])

const logVisible = ref(false)
const currentLog = ref('')

const {
  cronMode,
  advancedCronExpr,
  simpleCron,
  weekdayOptions,
  cronExprForSubmit,
  cronSummary,
  switchCronMode,
  loadCron,
  resetCron,
} = useCronBuilder()

const viewFullLog = (row: TaskRun) => {
  currentLog.value = row.logExcerpt || row.errorMessage || ''
  logVisible.value = true
}

const form = reactive<UpsertTaskPayload>({
  name: '',
  nodeId: 'local',
  command: '',
  cronExpr: '0 */5 * * * *',
  enabled: true,
  timeoutSecs: 30,
  noOverlap: true,
})

const nodeOptions = computed(() => {
  const rows = nodes.value
    .filter((item) => item.nodeId)
    .map((item) => ({
      value: item.nodeId as string,
      label: item.name || item.nodeId || '-',
      status: (item.status || 'UNKNOWN').toUpperCase(),
    }))
    .filter((item) => item.value !== 'local')
  return [
    { value: '', label: t('app.taskScheduler.filters.allNodes') },
    { value: 'local', label: t('app.nodes.local') },
    ...rows,
  ]
})

const filteredTasks = computed(() => {
  const search = keyword.value.trim().toLowerCase()
  return tasks.value.filter((item) => {
    if (selectedNodeId.value && item.nodeId !== selectedNodeId.value) return false
    if (!search) return true
    return (
      item.name.toLowerCase().includes(search) ||
      item.command.toLowerCase().includes(search) ||
      item.cronExpr.toLowerCase().includes(search) ||
      item.nodeId.toLowerCase().includes(search)
    )
  })
})

const selectedTaskIds = ref<number[]>([])

const isAllSelected = computed(() => {
  return (
    filteredTasks.value.length > 0 && selectedTaskIds.value.length === filteredTasks.value.length
  )
})

const toggleSelectAll = (val: boolean) => {
  if (val) {
    selectedTaskIds.value = filteredTasks.value.map((t) => t.id)
  } else {
    selectedTaskIds.value = []
  }
}

const toggleSelectTask = (id: number, val: boolean) => {
  if (val) {
    if (!selectedTaskIds.value.includes(id)) {
      selectedTaskIds.value.push(id)
    }
  } else {
    selectedTaskIds.value = selectedTaskIds.value.filter((x) => x !== id)
  }
}

const taskColumns = computed<SecLabTableColumn[]>(() => [
  { label: '', width: 50, align: 'center', slot: 'selection', headerSlot: 'selectionHeader' },
  { prop: 'name', label: t('app.taskScheduler.columns.name'), minWidth: 150, slot: 'name' },
  { prop: 'nodeId', label: t('app.taskScheduler.columns.node'), width: 100, align: 'center' },
  {
    prop: 'cronExpr',
    label: t('app.taskScheduler.columns.cron'),
    width: 150,
    align: 'center',
    slot: 'cron',
  },
  { prop: 'nextRunAt', label: t('app.taskScheduler.columns.nextRun'), width: 180, slot: 'nextRun' },
  {
    prop: 'lastStatus',
    label: t('app.taskScheduler.columns.lastStatus'),
    width: 120,
    align: 'center',
    slot: 'lastStatus',
  },
  {
    prop: 'syncStatus',
    label: t('app.taskScheduler.columns.syncStatus'),
    width: 120,
    align: 'center',
    slot: 'syncStatus',
  },
  {
    prop: 'command',
    label: t('app.taskScheduler.columns.command'),
    minWidth: 200,
    slot: 'command',
  },
  {
    label: t('app.taskScheduler.columns.actions'),
    width: 280,
    align: 'center',
    slot: 'actions',
    fixed: 'right',
  },
])

const runColumns = computed<SecLabTableColumn[]>(() => [
  {
    prop: 'triggeredAt',
    label: t('app.taskScheduler.runs.columns.triggeredAt'),
    width: 180,
    slot: 'time',
  },
  {
    prop: 'status',
    label: t('app.taskScheduler.runs.columns.status'),
    width: 100,
    align: 'center',
    slot: 'status',
  },
  {
    prop: 'exitCode',
    label: t('app.taskScheduler.runs.columns.exitCode'),
    width: 80,
    align: 'center',
  },
  {
    prop: 'logExcerpt',
    label: t('app.taskScheduler.runs.columns.log'),
    minWidth: 250,
    slot: 'log',
  },
])

const statusTagType = (status?: string): 'success' | 'danger' | 'info' | 'warning' | 'default' => {
  const value = (status || '').toLowerCase()
  if (value === 'success') return 'success'
  if (value === 'failed' || value === 'timeout') return 'danger'
  if (value === 'running') return 'info'
  return 'default'
}

const taskStatusText = (status?: string) => {
  const value = (status || '').toLowerCase()
  if (value === 'success') return t('app.taskScheduler.status.success')
  if (value === 'failed') return t('app.taskScheduler.status.failed')
  if (value === 'timeout') return t('app.taskScheduler.status.timeout')
  if (value === 'running') return t('app.taskScheduler.status.running')
  return '--'
}

const syncStatusTagType = (
  status?: string,
): 'success' | 'danger' | 'info' | 'warning' | 'default' => {
  const value = (status || '').toLowerCase()
  if (value === 'synced') return 'success'
  if (value === 'pending') return 'info'
  if (value === 'failed') return 'danger'
  if (value === 'target_offline') return 'warning'
  if (value === 'conflict') return 'danger'
  return 'default'
}

const syncStatusText = (status?: string) => {
  const value = (status || '').toLowerCase()
  if (value === 'synced') return t('app.taskScheduler.status.synced')
  if (value === 'pending') return t('app.taskScheduler.status.pending')
  if (value === 'failed') return t('app.taskScheduler.status.syncFailed')
  if (value === 'target_offline') return t('app.taskScheduler.status.targetOffline')
  if (value === 'conflict') return t('app.taskScheduler.status.conflict')
  return value || '--'
}

let pollTimer: ReturnType<typeof setTimeout> | null = null

const stopPolling = () => {
  if (pollTimer) {
    clearTimeout(pollTimer)
    pollTimer = null
  }
}

const checkAndPoll = () => {
  const hasRunning = tasks.value.some((t) => t.lastStatus === 'running')
  if (hasRunning) {
    pollTimer = setTimeout(async () => {
      await fetchTasks(true)
    }, 3000)
  }
}

const fetchTasks = async (silent = false) => {
  if (!silent) {
    loading.value = true
  }
  stopPolling()
  try {
    const response = await taskApi.list()
    if (!response.success) {
      throw new Error(response.message || t('app.taskScheduler.messages.loadTasksFailed'))
    }
    tasks.value = response.data ?? []
    checkAndPoll()
  } finally {
    if (!silent) {
      loading.value = false
    }
  }
}

const refreshAll = async () => {
  loading.value = true
  try {
    const nodesResponse = await nodesApi.list()
    if (!nodesResponse.success) {
      throw new Error(nodesResponse.message || t('app.taskScheduler.messages.loadAgentsFailed'))
    }
    nodes.value = nodesResponse.data ?? []
    await fetchTasks(true)
  } catch (error) {
    const message =
      error instanceof Error ? error.message : t('app.taskScheduler.messages.loadTasksFailed')
    notificationStore.error(message)
  } finally {
    loading.value = false
  }
}

const handleBatchDelete = async () => {
  if (!selectedTaskIds.value.length) return
  const confirmed = await confirmationModal.showConfirmation(
    t('app.taskScheduler.actions.batchDeleteConfirmContent', {
      count: selectedTaskIds.value.length,
    }),
    t('app.taskScheduler.actions.batchDeleteConfirmTitle'),
    t('app.nodes.deploy.confirm'),
    t('app.nodes.deploy.cancel'),
  )
  if (!confirmed) return

  loading.value = true
  try {
    await Promise.all(selectedTaskIds.value.map((id) => taskApi.remove(id)))
    notificationStore.success(t('app.taskScheduler.messages.batchDeleteSuccess'))
    selectedTaskIds.value = []
    await fetchTasks(true)
  } catch (err: unknown) {
    notificationStore.error(
      err instanceof Error ? err.message : t('app.taskScheduler.messages.batchDeleteFailed'),
    )
  } finally {
    loading.value = false
  }
}

const handleBatchEnable = async () => {
  if (!selectedTaskIds.value.length) return
  loading.value = true
  try {
    await Promise.all(selectedTaskIds.value.map((id) => taskApi.toggle(id, true)))
    notificationStore.success(t('app.taskScheduler.messages.batchEnableSuccess'))
    await fetchTasks(true)
  } catch (err: unknown) {
    notificationStore.error(
      err instanceof Error ? err.message : t('app.taskScheduler.messages.batchEnableFailed'),
    )
  } finally {
    loading.value = false
  }
}

const handleBatchDisable = async () => {
  if (!selectedTaskIds.value.length) return
  loading.value = true
  try {
    await Promise.all(selectedTaskIds.value.map((id) => taskApi.toggle(id, false)))
    notificationStore.success(t('app.taskScheduler.messages.batchDisableSuccess'))
    await fetchTasks(true)
  } catch (err: unknown) {
    notificationStore.error(
      err instanceof Error ? err.message : t('app.taskScheduler.messages.batchDisableFailed'),
    )
  } finally {
    loading.value = false
  }
}

const handleBatchRun = async () => {
  if (!selectedTaskIds.value.length) return
  loading.value = true
  try {
    await Promise.all(selectedTaskIds.value.map((id) => taskApi.run(id)))
    notificationStore.success(t('app.taskScheduler.messages.batchRunSuccess'))
    await fetchTasks(true)
  } catch (err: unknown) {
    notificationStore.error(
      err instanceof Error ? err.message : t('app.taskScheduler.messages.batchRunFailed'),
    )
  } finally {
    loading.value = false
  }
}

onUnmounted(() => {
  stopPolling()
})

const resetForm = () => {
  editTaskId.value = null
  form.name = ''
  form.nodeId = selectedNodeId.value || 'local'
  form.command = ''
  form.enabled = true
  form.timeoutSecs = 30
  form.noOverlap = true

  resetCron()
}

const openCreate = () => {
  resetForm()
  editVisible.value = true
}

const openEdit = (task: TaskItem) => {
  editTaskId.value = task.id
  form.name = task.name
  form.nodeId = task.nodeId
  form.command = task.command
  form.enabled = task.enabled
  form.timeoutSecs = task.timeoutSecs
  form.noOverlap = task.noOverlap

  loadCron(task.cronExpr)

  editVisible.value = true
}

const submitTask = async () => {
  if (!form.name.trim()) {
    notificationStore.error(t('app.taskScheduler.messages.nameRequired'))
    return
  }
  if (!form.nodeId.trim()) {
    notificationStore.error(t('app.taskScheduler.messages.agentRequired'))
    return
  }
  if (!form.command.trim()) {
    notificationStore.error(t('app.taskScheduler.messages.commandRequired'))
    return
  }
  if (!cronExprForSubmit.value) {
    notificationStore.error(t('app.taskScheduler.messages.cronRequired'))
    return
  }

  saving.value = true
  try {
    const payload: UpsertTaskPayload = {
      name: form.name.trim(),
      nodeId: form.nodeId.trim(),
      command: form.command.trim(),
      cronExpr: cronExprForSubmit.value,
      enabled: form.enabled,
      timeoutSecs: Math.max(1, Number(form.timeoutSecs) || 30),
      noOverlap: form.noOverlap,
    }

    const response = editTaskId.value
      ? await taskApi.update(editTaskId.value, payload)
      : await taskApi.create(payload)

    if (!response.success) {
      throw new Error(response.message || t('app.taskScheduler.messages.saveFailed'))
    }

    notificationStore.success(t('app.taskScheduler.messages.saveSuccess'))
    editVisible.value = false
    await fetchTasks()
  } catch (error) {
    const message =
      error instanceof Error ? error.message : t('app.taskScheduler.messages.saveFailed')
    notificationStore.error(message)
  } finally {
    saving.value = false
  }
}

const toggleTask = async (task: TaskItem) => {
  const nextEnabled = !task.enabled
  const response = await taskApi.toggle(task.id, nextEnabled)
  if (!response.success) {
    notificationStore.error(response.message || t('app.taskScheduler.messages.toggleFailed'))
    return
  }
  await fetchTasks()
}

const runTask = async (task: TaskItem) => {
  const response = await taskApi.run(task.id)
  if (!response.success) {
    notificationStore.error(response.message || t('app.taskScheduler.messages.runFailed'))
    return
  }
  notificationStore.success(t('app.taskScheduler.messages.runSubmitted'))
  await fetchTasks()
}

const retrySyncTask = async (task: TaskItem) => {
  loading.value = true
  try {
    const isConflict = task.syncStatus === 'conflict'
    const response = await taskApi.sync(task.id, isConflict)
    if (!response.success) {
      throw new Error(response.message || t('app.taskScheduler.messages.syncFailed'))
    }
    notificationStore.success(t('app.taskScheduler.messages.syncSuccess'))
    await fetchTasks(true)
  } catch (error) {
    const message =
      error instanceof Error ? error.message : t('app.taskScheduler.messages.syncFailed')
    notificationStore.error(message)
  } finally {
    loading.value = false
  }
}

const removeTask = async (task: TaskItem) => {
  const confirmed = await confirmationModal.showConfirmation(
    t('app.taskScheduler.delete.confirmMessage', { name: task.name }),
    t('app.taskScheduler.delete.confirmTitle'),
    t('app.taskScheduler.delete.confirmAction'),
    t('app.nodes.deploy.cancel'),
  )
  if (!confirmed) return

  const response = await taskApi.remove(task.id)
  if (!response.success) {
    notificationStore.error(response.message || t('app.taskScheduler.messages.deleteFailed'))
    return
  }
  notificationStore.success(t('app.taskScheduler.messages.deleteSuccess'))
  await fetchTasks()
}

const openRuns = async (task: TaskItem) => {
  runsTask.value = task
  runsVisible.value = true
  runsLoading.value = true
  try {
    const response = await taskApi.runs(task.id, 100)
    if (!response.success) {
      throw new Error(response.message || t('app.taskScheduler.messages.loadRunsFailed'))
    }
    taskRuns.value = response.data ?? []
  } catch (error) {
    const message =
      error instanceof Error ? error.message : t('app.taskScheduler.messages.loadRunsFailed')
    notificationStore.error(message)
  } finally {
    runsLoading.value = false
  }
}

onMounted(async () => {
  await refreshAll()
})
</script>

<template>
  <div class="task-scheduler" data-seclab-app="task-scheduler">
    <SecLabCard shadow="never" class="header-card">
      <div class="header" data-slot="header">
        <div class="title-block">
          <h2>{{ t('app.taskScheduler.title') }}</h2>
          <p>{{ t('app.taskScheduler.subtitle') }}</p>
        </div>
        <div class="actions">
          <template v-if="selectedTaskIds.length > 0">
            <SecLabButton type="danger" plain @click="handleBatchDelete">
              <SecLabIcon name="trash" :size="14" />
              <span
                >{{ t('app.taskScheduler.actions.batchDelete') }} ({{
                  selectedTaskIds.length
                }})</span
              >
            </SecLabButton>
            <SecLabButton type="secondary" plain @click="handleBatchEnable">
              <SecLabIcon name="play" :size="14" />
              <span>{{ t('app.taskScheduler.actions.batchEnable') }}</span>
            </SecLabButton>
            <SecLabButton type="secondary" plain @click="handleBatchDisable">
              <SecLabIcon name="x" :size="14" />
              <span>{{ t('app.taskScheduler.actions.batchDisable') }}</span>
            </SecLabButton>
            <SecLabButton type="primary" plain @click="handleBatchRun">
              <SecLabIcon name="play" :size="14" />
              <span>{{ t('app.taskScheduler.actions.batchRun') }}</span>
            </SecLabButton>
          </template>
          <SecLabButton :loading="loading" @click="refreshAll">
            <SecLabIcon name="refresh" :size="14" />
            <span>{{ t('app.taskScheduler.actions.refresh') }}</span>
          </SecLabButton>
          <SecLabButton type="primary" @click="openCreate">
            <SecLabIcon name="plus" :size="14" />
            <span>{{ t('app.taskScheduler.actions.create') }}</span>
          </SecLabButton>
        </div>
      </div>
    </SecLabCard>

    <SecLabCard shadow="never" class="toolbar-card">
      <div class="toolbar" data-ui="task-scheduler-toolbar">
        <SecLabSelect v-model="selectedNodeId" class="node-select" :options="nodeOptions" />
        <SecLabInput
          v-model="keyword"
          class="search-input"
          :placeholder="t('app.taskScheduler.filters.searchPlaceholder')"
        />
      </div>
    </SecLabCard>

    <SecLabCard shadow="never" class="table-card" full-height>
      <SecLabTable :data="filteredTasks" :columns="taskColumns" border data-ui="task-table">
        <template #selectionHeader>
          <SecLabCheckbox :model-value="isAllSelected" @update:model-value="toggleSelectAll" />
        </template>
        <template #selection="{ row }: { row: TaskItem }">
          <SecLabCheckbox
            :model-value="selectedTaskIds.includes(row.id)"
            @update:model-value="(val) => toggleSelectTask(row.id, val)"
          />
        </template>
        <template #name="{ row }: { row: TaskItem }">
          <div class="name-cell">
            <span class="name">{{ row.name }}</span>
            <SecLabTag :type="row.enabled ? 'success' : 'default'" size="small">
              {{ row.enabled ? t('app.taskScheduler.enabled') : t('app.taskScheduler.disabled') }}
            </SecLabTag>
          </div>
        </template>
        <template #cron="{ row }: { row: TaskItem }">
          <code class="mono-expr">{{ row.cronExpr }}</code>
        </template>
        <template #nextRun="{ row }: { row: TaskItem }">
          <span class="time">{{ formatDateTime(row.nextRunAt) }}</span>
        </template>
        <template #lastStatus="{ row }: { row: TaskItem }">
          <SecLabTag :type="statusTagType(row.lastStatus)">
            {{ taskStatusText(row.lastStatus) }}
          </SecLabTag>
        </template>
        <template #syncStatus="{ row }: { row: TaskItem }">
          <SecLabTag :type="syncStatusTagType(row.syncStatus)" :title="row.syncError">
            {{ syncStatusText(row.syncStatus) }}
          </SecLabTag>
        </template>
        <template #command="{ row }: { row: TaskItem }">
          <div class="command-cell">{{ row.command }}</div>
        </template>
        <template #actions="{ row }: { row: TaskItem }">
          <SecLabActionMenu
            :label="t('app.taskScheduler.actions.menu')"
            :actions="getTaskActions(row)"
          />
        </template>
        <template #empty>
          <div class="empty-placeholder">
            {{ t('app.taskScheduler.empty') }}
          </div>
        </template>
      </SecLabTable>
    </SecLabCard>

    <!-- 编辑抽屉 -->
    <SecLabDrawer
      v-model="editVisible"
      data-ui="task-edit-drawer"
      :title="
        t(editTaskId ? 'app.taskScheduler.form.editTitle' : 'app.taskScheduler.form.createTitle')
      "
      width="600px"
    >
      <div class="sl-form task-form">
        <div class="sl-form-grid">
          <div class="sl-form-col-full">
            <label class="sl-form-label">{{ t('app.taskScheduler.form.name') }}</label>
            <SecLabInput v-model="form.name" />
          </div>
          <div class="sl-form-col-full">
            <label class="sl-form-label">{{ t('app.taskScheduler.form.node') }}</label>
            <SecLabSelect v-model="form.nodeId" :options="nodeOptions.filter((v) => v.value)" />
          </div>

          <div class="sl-form-col-full cron-section">
            <label class="sl-form-label">{{ t('app.taskScheduler.form.cron') }}</label>
            <div class="mode-tabs">
              <button
                class="tab-btn"
                :class="{ active: cronMode === 'simple' }"
                @click="switchCronMode('simple')"
              >
                {{ t('app.taskScheduler.form.simpleMode') }}
              </button>
              <button
                class="tab-btn"
                :class="{ active: cronMode === 'advanced' }"
                @click="switchCronMode('advanced')"
              >
                {{ t('app.taskScheduler.form.advancedMode') }}
              </button>
            </div>

            <div v-if="cronMode === 'simple'" class="cron-simple-panel">
              <div class="simple-row">
                <label>{{ t('app.taskScheduler.form.simple.type') }}</label>
                <SecLabSelect
                  v-model="simpleCron.type"
                  :options="[
                    {
                      label: t('app.taskScheduler.form.simple.everyMinutes'),
                      value: 'every_minutes',
                    },
                    { label: t('app.taskScheduler.form.simple.hourly'), value: 'hourly' },
                    { label: t('app.taskScheduler.form.simple.daily'), value: 'daily' },
                    { label: t('app.taskScheduler.form.simple.weekly'), value: 'weekly' },
                  ]"
                />
              </div>

              <div v-if="simpleCron.type === 'every_minutes'" class="simple-row">
                <label>{{ t('app.taskScheduler.form.simple.intervalMinutes') }}</label>
                <SecLabInput
                  v-model.number="simpleCron.intervalMinutes"
                  type="number"
                  :min="1"
                  :max="59"
                />
              </div>

              <div v-if="simpleCron.type === 'hourly'" class="simple-row">
                <label>{{ t('app.taskScheduler.form.simple.minute') }}</label>
                <SecLabInput v-model.number="simpleCron.minute" type="number" :min="0" :max="59" />
              </div>

              <template v-if="simpleCron.type === 'daily' || simpleCron.type === 'weekly'">
                <div class="simple-row">
                  <label>{{ t('app.taskScheduler.form.simple.hour') }}</label>
                  <SecLabInput v-model.number="simpleCron.hour" type="number" :min="0" :max="23" />
                </div>
                <div class="simple-row">
                  <label>{{ t('app.taskScheduler.form.simple.minute') }}</label>
                  <SecLabInput
                    v-model.number="simpleCron.minute"
                    type="number"
                    :min="0"
                    :max="59"
                  />
                </div>
              </template>

              <div v-if="simpleCron.type === 'weekly'" class="simple-row">
                <label>{{ t('app.taskScheduler.form.simple.weekday') }}</label>
                <SecLabSelect v-model.number="simpleCron.weekday" :options="weekdayOptions" />
              </div>

              <div class="cron-preview">
                <span class="summary">{{ cronSummary }}</span>
                <code class="expr">{{ cronExprForSubmit }}</code>
              </div>
            </div>

            <div v-else class="cron-advanced-panel">
              <SecLabInput
                v-model="advancedCronExpr"
                :placeholder="t('app.taskScheduler.form.cronPlaceholder')"
              />
            </div>
          </div>

          <div>
            <label class="sl-form-label">{{ t('app.taskScheduler.form.timeout') }}</label>
            <SecLabInput v-model.number="form.timeoutSecs" type="number" :min="1" />
          </div>

          <div class="sl-form-col-full">
            <label class="sl-form-label">{{ t('app.taskScheduler.form.command') }}</label>
            <div
              style="
                height: 200px;
                border: 1px solid var(--sdl-border-color);
                border-radius: var(--sdl-radius-md);
                overflow: hidden;
              "
            >
              <MonacoEditor v-model="form.command" language="shell" />
            </div>
          </div>

          <div class="checkbox-group sl-form-col-full">
            <label class="checkbox-item">
              <input v-model="form.enabled" type="checkbox" />
              <span>{{ t('app.taskScheduler.form.enabled') }}</span>
            </label>
            <label class="checkbox-item">
              <input v-model="form.noOverlap" type="checkbox" />
              <span>{{ t('app.taskScheduler.form.noOverlap') }}</span>
            </label>
          </div>
        </div>
      </div>
      <template #footer>
        <div class="drawer-footer">
          <SecLabButton @click="editVisible = false">{{
            t('app.nodes.deploy.cancel')
          }}</SecLabButton>
          <SecLabButton type="primary" :loading="saving" @click="submitTask">
            {{ t('app.taskScheduler.form.submit') }}
          </SecLabButton>
        </div>
      </template>
    </SecLabDrawer>

    <!-- 执行历史抽屉 -->
    <SecLabDrawer
      v-model="runsVisible"
      data-ui="task-runs-drawer"
      :title="t('app.taskScheduler.runs.title', { name: runsTask?.name || '-' })"
      width="800px"
    >
      <div class="runs-container">
        <SecLabTable :data="taskRuns" :columns="runColumns" border data-ui="task-runs-table">
          <template #time="{ row }: { row: TaskRun }">
            <span class="time">{{ formatDateTime(row.triggeredAt) }}</span>
          </template>
          <template #status="{ row }: { row: TaskRun }">
            <SecLabTag :type="statusTagType(row.status)">
              {{ taskStatusText(row.status) }}
            </SecLabTag>
          </template>
          <template #log="{ row }: { row: TaskRun }">
            <SecLabButton size="small" type="secondary" plain @click="viewFullLog(row)">
              {{ t('app.taskScheduler.runs.viewLog') }}
            </SecLabButton>
          </template>
          <template #empty>
            <div class="empty-placeholder">
              {{ t('app.taskScheduler.runs.empty') }}
            </div>
          </template>
        </SecLabTable>
      </div>
      <SecLabLoading :loading="runsLoading" cover />
    </SecLabDrawer>

    <SecLabLoading :loading="loading && tasks.length === 0" cover />

    <!-- 日志弹窗 -->
    <SecLabDialog
      :visible="logVisible"
      @close="logVisible = false"
      :title="t('app.taskScheduler.runs.fullLog')"
      width="800px"
    >
      <div
        style="
          height: 400px;
          border: 1px solid var(--sdl-border-color);
          border-radius: var(--sdl-radius-md);
          overflow: hidden;
        "
      >
        <MonacoEditor v-model="currentLog" language="shell" :read-only="true" />
      </div>
    </SecLabDialog>
  </div>
</template>

<style scoped>
.task-scheduler {
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

.header-card {
  flex-shrink: 0;
}

.header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: var(--sdl-space-3);
}

.title-block h2 {
  margin: 0;
  font-size: var(--sdl-font-title);
  color: var(--sdl-text-primary);
  font-weight: 700;
}

.title-block p {
  margin: var(--sdl-space-1) 0 0;
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-caption);
}

.actions {
  display: flex;
  gap: var(--sdl-space-2);
}

.toolbar-card {
  flex-shrink: 0;
}

.toolbar {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-3);
}

.node-select {
  width: 200px;
}

.search-input {
  flex: 1;
}

.table-card {
  flex: 1;
  min-height: 0;
}

.name-cell {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
}

.name-cell .name {
  font-weight: 600;
  color: var(--sdl-text-primary);
}

.mono-expr {
  font-family: var(--sdl-font-code);
  background: var(--sdl-bg-muted);
  padding: 2px 6px;
  border-radius: var(--sdl-radius-sm);
  font-size: 12px;
}

.time {
  font-size: 12px;
  color: var(--sdl-text-secondary);
}

.command-cell {
  max-width: 400px;
  word-break: break-all;
  white-space: pre-wrap;
  font-size: 12px;
  color: var(--sdl-text-muted);
}

.empty-placeholder {
  display: flex;
  justify-content: center;
  align-items: center;
  height: 200px;
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-body-sm);
}

/* --- 表单样式 --- */
.task-form {
  padding: var(--sdl-space-2);
}

.cron-section {
  background: var(--sdl-bg-panel);
  padding: var(--sdl-space-3);
  border-radius: var(--sdl-radius-md);
  border: 1px solid var(--sdl-border-subtle);
}

.mode-tabs {
  display: flex;
  gap: var(--sdl-space-2);
  margin-bottom: var(--sdl-space-3);
}

.mode-tabs .tab-btn {
  background: var(--sdl-bg-muted);
  border: 1px solid var(--sdl-border-default);
  padding: 4px 12px;
  border-radius: var(--sdl-radius-md);
  font-size: 12px;
  cursor: pointer;
  color: var(--sdl-text-secondary);
  transition: all 0.2s;
}

.mode-tabs .tab-btn.active {
  background: var(--sdl-primary);
  color: var(--sdl-text-inverse);
  border-color: var(--sdl-primary);
}

.cron-simple-panel {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-2);
}

.simple-row {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-3);
}

.simple-row label {
  width: 80px;
  font-size: 12px;
  color: var(--sdl-text-muted);
}

.cron-preview {
  margin-top: var(--sdl-space-2);
  padding: var(--sdl-space-2) var(--sdl-space-3);
  background: var(--sdl-bg-canvas);
  border-left: 3px solid var(--sdl-primary);
  border-radius: var(--sdl-radius-sm);
  display: flex;
  flex-direction: column;
}

.cron-preview .summary {
  font-size: 12px;
  color: var(--sdl-text-secondary);
  margin-bottom: 2px;
}

.cron-preview .expr {
  font-family: var(--sdl-font-code);
  font-size: 13px;
  color: var(--sdl-primary);
}

.checkbox-group {
  display: flex;
  gap: var(--sdl-space-5);
  padding: var(--sdl-space-2) 0;
}

.checkbox-item {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
  cursor: pointer;
  font-size: 13px;
  color: var(--sdl-text-primary);
}

.drawer-footer {
  display: flex;
  justify-content: flex-end;
  gap: var(--sdl-space-2);
}

:deep(.sl-button-content) {
  display: inline-flex;
  align-items: center;
  gap: var(--sdl-space-1);
}

.runs-container {
  height: 600px;
}

.log-excerpt {
  max-width: 500px;
  max-height: 80px;
  overflow-y: auto;
  font-size: 12px;
  font-family: var(--sdl-font-code);
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--sdl-text-muted);
}
</style>
