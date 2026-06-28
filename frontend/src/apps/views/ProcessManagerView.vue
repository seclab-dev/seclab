<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { NetworkConnection, NetworkSummary, ProcessItem } from '@/api/interface/process'
import { useNotificationStore } from '@/stores/notification'
import { useConfirmationModalStore } from '@/stores/confirmation-modal'
import { useWindowManagerStore } from '@/stores/window-manager'
import { useProcessManagerWs } from '@/composables/useProcessManagerWs'
import {
  SecLabButton,
  SecLabCard,
  SecLabInput,
  SecLabSelect,
  SecLabTable,
  SecLabLoading,
} from '@/components/ui'
import type { SecLabTableColumn } from '@/components/ui/SecLabTable.vue'

const props = defineProps<{
  isMaximized?: boolean
  windowId?: string
  payload?: Record<string, unknown>
}>()

type ActiveTab = 'process' | 'network'
type ProcessSortKey = 'pid' | 'cpuPercent' | 'memoryPercent' | 'connectionCount'
type ProcessStatusKey =
  | 'running'
  | 'sleeping'
  | 'stopped'
  | 'idle'
  | 'waiting'
  | 'locked'
  | 'zombie'

const PROCESS_STATUS_OPTIONS: ProcessStatusKey[] = [
  'running',
  'sleeping',
  'stopped',
  'idle',
  'waiting',
  'locked',
  'zombie',
]

const { t } = useI18n()
const notificationStore = useNotificationStore()
const confirmationModal = useConfirmationModalStore()
const windowStore = useWindowManagerStore()
const processManagerWs = useProcessManagerWs()

const activeTab = ref<ActiveTab>('process')
const networkKeyword = ref('')
const processStatus = ref('ALL')
const networkState = ref('ALL')
const processSortKey = ref<ProcessSortKey>('pid')
const processSortDesc = ref(false)

const processRows = computed<ProcessItem[]>(() => processManagerWs.processRows.value)
const networkRows = computed<NetworkConnection[]>(() => processManagerWs.networkRows.value)
const networkSummary = computed<NetworkSummary | null>(() => processManagerWs.networkSummary.value)
const loadingProcess = computed(
  () => !processManagerWs.processLoaded.value && processManagerWs.connecting.value,
)
const loadingNetwork = computed(
  () => !processManagerWs.networkLoaded.value && processManagerWs.connecting.value,
)
const processManagerActive = computed(
  () => processManagerWs.connected.value || processManagerWs.connecting.value,
)

watch(
  processManagerActive,
  (active) => {
    if (!props.windowId) return
    windowStore.updateWindowRuntimeState(props.windowId, {
      activeSession: active,
      allowsNodeSwitch: false,
      blockLevel: active ? 'active' : 'open',
      blockReason: active ? t('app.processManager.guardActive') : t('app.processManager.guardOpen'),
    })
  },
  { immediate: true },
)

const networkStateOptions = computed(() => {
  const states = new Set<string>()
  for (const item of networkRows.value) {
    states.add(item.state)
  }
  return ['ALL', ...Array.from(states).sort()]
})

const filteredProcessRows = computed(() => {
  const rows = processRows.value.filter((item) => {
    return processStatus.value === 'ALL' || item.status === processStatus.value
  })

  rows.sort((a, b) => {
    const left = a[processSortKey.value]
    const right = b[processSortKey.value]
    const diff = Number(right) - Number(left)
    return processSortDesc.value ? diff : -diff
  })
  return rows
})

const filteredNetworkRows = computed(() => {
  const keyword = networkKeyword.value.trim().toLowerCase()
  return networkRows.value.filter((item) => {
    if (networkState.value !== 'ALL' && item.state !== networkState.value) return false
    if (!keyword) return true
    return (
      item.protocol.toLowerCase().includes(keyword) ||
      item.localAddress.toLowerCase().includes(keyword) ||
      item.remoteAddress.toLowerCase().includes(keyword) ||
      item.processName?.toLowerCase().includes(keyword) ||
      String(item.pid ?? '').includes(keyword)
    )
  })
})

const formatPercent = (value: number) => `${value.toFixed(2)}%`

const formatStartTime = (timestamp: number) => {
  if (!timestamp) return '--'
  const date = new Date(timestamp * 1000)
  if (Number.isNaN(date.getTime())) return '--'
  const y = date.getFullYear()
  const m = String(date.getMonth() + 1).padStart(2, '0')
  const d = String(date.getDate()).padStart(2, '0')
  const hh = String(date.getHours()).padStart(2, '0')
  const mm = String(date.getMinutes()).padStart(2, '0')
  const ss = String(date.getSeconds()).padStart(2, '0')
  return `${y}-${m}-${d} ${hh}:${mm}:${ss}`
}

const setProcessSort = (key: ProcessSortKey) => {
  if (processSortKey.value === key) {
    processSortDesc.value = !processSortDesc.value
    return
  }
  processSortKey.value = key
  processSortDesc.value = true
}

const processSortIndicator = (key: ProcessSortKey) => {
  if (processSortKey.value !== key) return ''
  return processSortDesc.value ? ' ↓' : ' ↑'
}

const formatProcessStatus = (status: string) => {
  if ((PROCESS_STATUS_OPTIONS as string[]).includes(status)) {
    return t(`app.processManager.process.statuses.${status}`)
  }
  return t('app.processManager.process.statuses.sleeping')
}

const terminateProcess = async (pid: number, signal: 'TERM' | 'KILL') => {
  const confirmed = await confirmationModal.showConfirmation(
    t('app.processManager.confirmTerminate', {
      pid,
      signal,
    }),
    t('app.processManager.title'),
    signal,
    t('app.nodes.deploy.cancel'),
  )
  if (!confirmed) return
  const requestId = processManagerWs.sendSignal(pid, signal)
  if (!requestId) {
    notificationStore.error(t('app.processManager.messages.realtimeDisconnected'))
  }
}

const processColumns = computed<SecLabTableColumn[]>(() => [
  { prop: 'pid', label: 'PID', width: 80, align: 'center', headerSlot: 'header-pid' },
  { prop: 'name', label: t('app.processManager.process.columns.name'), minWidth: 150 },
  { prop: 'parentPid', label: 'PPID', width: 80, align: 'center' },
  {
    prop: 'threadCount',
    label: t('app.processManager.process.columns.threads'),
    width: 80,
    align: 'center',
  },
  { prop: 'user', label: t('app.processManager.process.columns.user'), width: 100 },
  {
    prop: 'cpuPercent',
    label: 'CPU',
    width: 100,
    align: 'right',
    slot: 'cpu',
    headerSlot: 'header-cpu',
  },
  {
    prop: 'memoryPercent',
    label: 'MEM',
    width: 100,
    align: 'right',
    slot: 'mem',
    headerSlot: 'header-mem',
  },
  {
    prop: 'connectionCount',
    label: t('app.processManager.process.columns.connections'),
    width: 100,
    align: 'center',
    headerSlot: 'header-connections',
  },
  {
    prop: 'status',
    label: t('app.processManager.process.columns.status'),
    width: 100,
    slot: 'status',
  },
  {
    prop: 'startTime',
    label: t('app.processManager.process.columns.startTime'),
    width: 180,
    slot: 'startTime',
  },
  {
    label: t('app.processManager.process.columns.actions'),
    width: 150,
    align: 'center',
    slot: 'actions',
    fixed: 'right',
  },
])

const networkColumns = computed<SecLabTableColumn[]>(() => [
  { prop: 'protocol', label: t('app.processManager.network.columns.protocol'), width: 80 },
  {
    prop: 'localAddress',
    label: t('app.processManager.network.columns.localAddress'),
    minWidth: 200,
  },
  {
    prop: 'remoteAddress',
    label: t('app.processManager.network.columns.remoteAddress'),
    minWidth: 200,
  },
  { prop: 'state', label: t('app.processManager.network.columns.state'), width: 120 },
  { prop: 'pid', label: 'PID', width: 80, align: 'center' },
  {
    prop: 'processName',
    label: t('app.processManager.network.columns.processName'),
    minWidth: 150,
  },
])

watch(
  activeTab,
  (tab) => {
    processManagerWs.setActiveView(tab)
  },
  { immediate: true },
)

watch(
  () => processManagerWs.lastSignalResult.value,
  (result) => {
    if (!result) return
    if (result.success) {
      notificationStore.success(
        t('app.processManager.messages.killSuccess', { pid: result.pid, signal: result.signal }),
      )
      return
    }
    notificationStore.error(result.message || t('app.processManager.messages.killFailed'))
  },
)

watch(
  () => processManagerWs.lastProtocolError.value,
  (message) => {
    if (message) {
      notificationStore.error(message)
    }
  },
)

watch(
  () => processManagerWs.lastError.value,
  (message) => {
    if (message) {
      notificationStore.error(message)
    }
  },
)
</script>

<template>
  <div class="process-manager" data-seclab-app="process-manager">
    <SecLabCard shadow="never" class="header-card">
      <div class="header" data-slot="header">
        <div v-if="activeTab === 'process'" class="filters">
          <span class="filter-label">{{ t('app.processManager.process.columns.status') }}:</span>
          <SecLabSelect
            v-model="processStatus"
            class="status-select"
            :options="[
              { label: t('app.processManager.process.filters.allStatuses'), value: 'ALL' },
              ...PROCESS_STATUS_OPTIONS.map((s) => ({
                label: t(`app.processManager.process.statuses.${s}`),
                value: s,
              })),
            ]"
          />
        </div>
        <div v-else class="network-summary">
          <div class="summary-metric">
            <span class="summary-label">{{ t('app.processManager.network.summary.total') }}</span>
            <strong class="summary-value">{{ networkSummary?.total ?? 0 }}</strong>
          </div>
          <div class="summary-metric">
            <span class="summary-label">{{
              t('app.processManager.network.summary.protocol')
            }}</span>
            <strong class="summary-value">
              TCP: {{ networkSummary?.byProtocol?.TCP ?? 0 }} · UDP:
              {{ (networkSummary?.byProtocol?.UDP ?? 0) + (networkSummary?.byProtocol?.UDP6 ?? 0) }}
            </strong>
          </div>
        </div>
        <div class="tabs">
          <SecLabButton
            :type="activeTab === 'process' ? 'primary' : 'secondary'"
            @click="activeTab = 'process'"
          >
            {{ t('app.processManager.tabs.process') }}
          </SecLabButton>
          <SecLabButton
            :type="activeTab === 'network' ? 'primary' : 'secondary'"
            @click="activeTab = 'network'"
          >
            {{ t('app.processManager.tabs.network') }}
          </SecLabButton>
        </div>
      </div>
    </SecLabCard>

    <div v-if="activeTab === 'process'" class="content-wrapper">
      <SecLabCard shadow="never" class="table-card" full-height>
        <SecLabTable :data="filteredProcessRows" :columns="processColumns" border>
          <template #header-pid>
            <div class="sortable-header" @click="setProcessSort('pid')">
              PID{{ processSortIndicator('pid') }}
            </div>
          </template>
          <template #header-cpu>
            <div class="sortable-header" @click="setProcessSort('cpuPercent')">
              CPU{{ processSortIndicator('cpuPercent') }}
            </div>
          </template>
          <template #header-mem>
            <div class="sortable-header" @click="setProcessSort('memoryPercent')">
              MEM{{ processSortIndicator('memoryPercent') }}
            </div>
          </template>
          <template #header-connections>
            <div class="sortable-header" @click="setProcessSort('connectionCount')">
              {{ t('app.processManager.process.columns.connections')
              }}{{ processSortIndicator('connectionCount') }}
            </div>
          </template>

          <template #cpu="{ row }">
            {{ formatPercent(row.cpuPercent) }}
          </template>
          <template #mem="{ row }">
            {{ formatPercent(row.memoryPercent) }}
          </template>
          <template #status="{ row }">
            <span class="status-tag" :class="row.status">
              {{ formatProcessStatus(row.status) }}
            </span>
          </template>
          <template #startTime="{ row }">
            {{ formatStartTime(row.startTime) }}
          </template>
          <template #actions="{ row }">
            <div class="action-buttons">
              <SecLabButton
                type="danger"
                size="small"
                :disabled="!processManagerWs.connected.value"
                @click="terminateProcess(row.pid, 'TERM')"
              >
                TERM
              </SecLabButton>
              <SecLabButton
                type="danger"
                size="small"
                plain
                :disabled="!processManagerWs.connected.value"
                @click="terminateProcess(row.pid, 'KILL')"
              >
                KILL
              </SecLabButton>
            </div>
          </template>
          <template #empty>
            <div class="empty-placeholder">
              {{ t('app.processManager.process.empty') }}
            </div>
          </template>
        </SecLabTable>
      </SecLabCard>
    </div>

    <div v-else class="content-wrapper">
      <SecLabCard shadow="never" class="toolbar-card">
        <div class="toolbar">
          <SecLabInput
            v-model="networkKeyword"
            class="search-input"
            :placeholder="t('app.processManager.network.searchPlaceholder')"
          />
          <SecLabSelect
            v-model="networkState"
            class="state-select"
            :options="networkStateOptions.map((s) => ({ label: s, value: s }))"
          />
        </div>
      </SecLabCard>

      <SecLabCard shadow="never" class="table-card" full-height>
        <SecLabTable :data="filteredNetworkRows" :columns="networkColumns" border>
          <template #empty>
            <div class="empty-placeholder">
              {{ t('app.processManager.network.empty') }}
            </div>
          </template>
        </SecLabTable>
      </SecLabCard>
    </div>

    <SecLabLoading :loading="activeTab === 'process' ? loadingProcess : loadingNetwork" cover />
  </div>
</template>

<style scoped>
.process-manager {
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

.tabs {
  display: flex;
  gap: var(--sdl-space-2);
}

.content-wrapper {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
  min-height: 0;
}

.network-summary {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-4);
  min-width: 0;
}

.summary-metric {
  display: flex;
  align-items: baseline;
  gap: var(--sdl-space-2);
  min-width: 0;
}

.summary-label {
  font-size: var(--sdl-font-caption);
  color: var(--sdl-text-muted);
  white-space: nowrap;
}

.summary-value {
  font-size: var(--sdl-font-body);
  color: var(--sdl-text-primary);
  font-weight: 700;
  white-space: nowrap;
}

.toolbar-card {
  flex-shrink: 0;
}

.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: var(--sdl-space-3);
}

.filters {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
}

.filter-label {
  font-size: var(--sdl-font-body-sm);
  color: var(--sdl-text-secondary);
  white-space: nowrap;
}

.status-select {
  width: 160px;
}

.search-input {
  flex: 1;
}

.state-select {
  width: 180px;
}

.table-card {
  flex: 1;
  min-height: 0;
  padding: 0;
}

.sortable-header {
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 100%;
}

.sortable-header:hover {
  color: var(--sdl-primary);
}

.status-tag {
  font-size: var(--sdl-font-caption);
  padding: 2px 8px;
  border-radius: var(--sdl-radius-sm);
  background: var(--sdl-bg-muted);
  color: var(--sdl-text-secondary);
}

.status-tag.running {
  background: var(--sdl-success-soft);
  color: var(--sdl-success);
}

.status-tag.zombie {
  background: var(--sdl-danger-soft);
  color: var(--sdl-danger);
}

.action-buttons {
  display: flex;
  justify-content: center;
  gap: var(--sdl-space-2);
}

.empty-placeholder {
  display: flex;
  justify-content: center;
  align-items: center;
  height: 200px;
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-body-sm);
}

@media (max-width: 1024px) {
  .header {
    align-items: stretch;
    flex-direction: column;
  }

  .network-summary {
    align-items: flex-start;
    flex-wrap: wrap;
  }
}
</style>
