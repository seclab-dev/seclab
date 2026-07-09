<script setup lang="ts">
/**
 * @file DockerContainerDetail.vue
 * @description Docker 容器详情组件，提供容器的基本信息、性能监控趋势图、实时进程、容器日志和 Web 终端操作，符合 SDL 设计规范。
 */

import { computed, ref, onUnmounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDockerStore } from '@/stores/docker'
import { useNotificationStore } from '@/stores/notification'
import { formatDateTime as formatDate } from '@/utils/time'
import { getContainerIP, formatPorts } from '@/utils/docker-format'
import { SecLabButton, SecLabTag, SecLabTabs, SecLabLoading, SecLabEmpty } from '@/components/ui'
import DockerContainerLogsPanel from './DockerContainerLogsPanel.vue'
import DockerContainerTerminalPanel from './terminal/DockerContainerTerminalPanel.vue'
import { useContainerHistoryData } from './composables/useContainerHistoryData'
import { useContainerHistoryCharts } from './composables/useContainerHistoryCharts'
import { useContainerInspectData } from './composables/useContainerInspectData'
import { useContainerProcesses } from './composables/useContainerProcesses'
import type * as dockerType from '@/api/interface/docker'

const props = defineProps<{
  /** 容器 ID */
  containerId: string
  /** 目标节点 ID */
  nodeId: string
}>()

const emit = defineEmits<{
  /** 触发返回容器列表事件 */
  (e: 'back'): void
}>()

const { t, locale } = useI18n()
const store = useDockerStore()
const notificationStore = useNotificationStore()

const activeTab = ref<'basic' | 'processes' | 'logs' | 'terminal'>('basic')
const containerIdRef = computed(() => props.containerId)
const nodeIdRef = computed(() => props.nodeId)

const selectedContainer = computed(() => {
  return store.containers.find((c) => c.Id === props.containerId) || null
})

const selectedContainerName = computed(() => {
  const summaryName = selectedContainer.value?.Names?.[0]?.replace(/^\//, '')
  const inspectName = inspectDetail.value?.Name?.replace(/^\//, '')
  return summaryName || inspectName || ''
})

// ─── 容器审查信息 Composable ───
const { inspectDetail, isInspectLoading } = useContainerInspectData({
  selectedContainerId: containerIdRef,
  nodeId: nodeIdRef,
  onError: (message) => notificationStore.error(message),
})

// ─── 容器性能指标趋势 Composable ───
const {
  containerHistory,
  historyLoading,
  historyError,
  historyStatus,
  dispose: disposeContainerHistory,
} = useContainerHistoryData({
  selectedContainerId: containerIdRef,
  nodeId: nodeIdRef,
  activeTab,
  t,
})

const {
  historyCpuChartRef,
  historyMemoryChartRef,
  historyNetworkChartRef,
  renderHistory,
  clearHistory,
  dispose: disposeHistoryCharts,
} = useContainerHistoryCharts({ t })

// ─── 容器内进程列表 Composable ───
const {
  processLoading,
  processError,
  processColumns,
  processSort,
  sortedProcessRows,
  loadProcessList,
  toggleProcessSort,
  getProcessColumnWidth,
  getProcessColumnLabel,
} = useContainerProcesses({
  selectedContainerId: containerIdRef,
  nodeId: nodeIdRef,
  activeTab,
  t,
})

// ─── 格式化工具函数 ───
const formatList = (items?: string[]) => {
  if (!items || items.length === 0) return '-'
  return items.join('\n')
}

const formatCommand = (command?: string[] | string | null) => {
  if (!command || (Array.isArray(command) && command.length === 0)) return '-'
  return Array.isArray(command) ? command.join(' ') : command
}

const formatLabels = (labels?: Record<string, string>) => {
  if (!labels || Object.keys(labels).length === 0) return '-'
  return Object.entries(labels)
    .map(([key, value]) => `${key}=${value}`)
    .join('\n')
}

const formatMounts = (mounts?: dockerType.ContainerInspect['Mounts']) => {
  if (!mounts || mounts.length === 0) return '-'
  return mounts
    .map((mount) => {
      const source = mount.Source || '-'
      const destination = mount.Destination || '-'
      return `${source} -> ${destination}`
    })
    .join('\n')
}

const formatNetworks = (networks?: dockerType.ContainerInspect['NetworkSettings']) => {
  const entries = networks?.Networks ? Object.entries(networks.Networks) : []
  if (entries.length === 0) return '-'
  return (
    entries
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      .map(([name, detail]: [string, any]) => `${name}: ${detail.IPAddress || '-'}`)
      .join('\n')
  )
}

// ─── 监视容器列表（防止当前详情页中的容器被其他地方删除了） ───
watch(
  () => store.containers,
  (next) => {
    const exists = next.some((c) => c.Id === props.containerId)
    if (!exists) {
      emit('back')
    }
  },
  { deep: true },
)

// ─── 监视图表渲染 ───
watch(
  () => [activeTab.value, historyStatus.value, containerHistory.value] as const,
  async ([tab, status, history]) => {
    if (tab !== 'basic') return
    if (status !== 'ready' || !history || !history.points.length) {
      clearHistory()
      return
    }
    await renderHistory(history)
  },
  { deep: true },
)

watch(locale, async () => {
  if (activeTab.value !== 'basic') return
  if (historyStatus.value !== 'ready' || !containerHistory.value?.points.length) return
  await renderHistory(containerHistory.value)
})

onUnmounted(() => {
  disposeContainerHistory()
  disposeHistoryCharts()
})
</script>

<template>
  <div class="card-scroll-wrapper card-scroll-detail" data-ui="docker-container-detail">
    <div class="container-detail">
      <div class="container-detail-header">
        <SecLabButton size="small" @click="emit('back')" data-ui="detail-back-btn">
          {{ t('app.docker.containers.backToList') }}
        </SecLabButton>
        <div class="container-detail-title">
          <div class="container-detail-name" data-ui="detail-container-name">
            {{ selectedContainerName || t('app.docker.containers.containerDetails') }}
          </div>
        </div>
        <div class="container-detail-status">
          <SecLabTag :type="inspectDetail?.State?.Status === 'running' ? 'success' : 'info'">
            {{ inspectDetail?.State?.Status || selectedContainer?.State || '-' }}
          </SecLabTag>
        </div>
      </div>

      <SecLabTabs
        v-model="activeTab"
        class="detail-tabs"
        :tabs="[
          { label: t('app.docker.containers.basicInfo'), name: 'basic' },
          { label: t('app.docker.containers.processes'), name: 'processes' },
          { label: t('app.docker.containers.logs'), name: 'logs' },
          { label: t('app.docker.containers.terminal'), name: 'terminal' },
        ]"
        data-ui="detail-tabs"
      />

      <div
        class="detail-content"
        :class="{ 'detail-content-processes': activeTab === 'processes' }"
      >
        <!-- 基本信息标签页 -->
        <div v-show="activeTab === 'basic'" class="detail-section">
          <div v-if="isInspectLoading" class="detail-loading">
            <SecLabLoading :loading="true" :text="t('app.docker.containers.loadingDetails')" />
          </div>
          <div v-else class="detail-grid">
            <div class="detail-item">
              <span class="detail-label">{{ t('app.docker.containers.containerId') }}</span>
              <span class="detail-value detail-mono">{{ props.containerId }}</span>
            </div>
            <div class="detail-item">
              <span class="detail-label">{{ t('app.docker.containers.status') }}</span>
              <span class="detail-value">
                {{ inspectDetail?.State?.Status || selectedContainer?.State || '-' }}
              </span>
            </div>
            <div class="detail-item">
              <span class="detail-label">{{ t('app.docker.containers.imageFull') }}</span>
              <span class="detail-value">
                {{ inspectDetail?.Config?.Image || selectedContainer?.Image || '-' }}
              </span>
            </div>
            <div class="detail-item">
              <span class="detail-label">{{ t('app.docker.containers.runtime') }}</span>
              <span class="detail-value">{{ selectedContainer?.Status || '-' }}</span>
            </div>
            <div class="detail-item">
              <span class="detail-label">{{ t('app.docker.containers.ipAddress') }}</span>
              <span class="detail-value">
                {{ getContainerIP(selectedContainer?.NetworkSettings) }}
              </span>
            </div>
            <div class="detail-item">
              <span class="detail-label">{{ t('app.docker.containers.portMappings') }}</span>
              <span class="detail-value">
                {{ formatPorts(selectedContainer?.Ports) }}
              </span>
            </div>
            <div class="detail-item">
              <span class="detail-label">{{ t('app.docker.containers.createdAt') }}</span>
              <span class="detail-value">
                {{ formatDate(inspectDetail?.Created || selectedContainer?.Created) }}
              </span>
            </div>
            <div class="detail-item">
              <span class="detail-label">{{ t('app.docker.containers.startedAt') }}</span>
              <span class="detail-value">
                {{ formatDate(inspectDetail?.State?.StartedAt) }}
              </span>
            </div>
            <div class="detail-item">
              <span class="detail-label">{{ t('app.docker.containers.network') }}</span>
              <span class="detail-value">
                {{ formatNetworks(inspectDetail?.NetworkSettings) }}
              </span>
            </div>
            <div class="detail-item">
              <span class="detail-label">{{ t('app.docker.containers.restartPolicy') }}</span>
              <span class="detail-value">
                {{ inspectDetail?.HostConfig?.RestartPolicy?.Name || '-' }}
              </span>
            </div>
            <div class="detail-item detail-full">
              <span class="detail-label">{{ t('app.docker.containers.startCommand') }}</span>
              <span class="detail-value detail-mono">
                {{
                  formatCommand(inspectDetail?.Config?.Cmd || selectedContainer?.Command || null)
                }}
              </span>
            </div>
            <div class="detail-item detail-full">
              <span class="detail-label">{{ t('app.docker.containers.envVars') }}</span>
              <span class="detail-value detail-mono">
                {{ formatList(inspectDetail?.Config?.Env) }}
              </span>
            </div>
            <div class="detail-item detail-full">
              <span class="detail-label">{{ t('app.docker.containers.labels') }}</span>
              <span class="detail-value detail-mono">
                {{ formatLabels(inspectDetail?.Config?.Labels || selectedContainer?.Labels) }}
              </span>
            </div>
            <div class="detail-item detail-full">
              <span class="detail-label">{{ t('app.docker.containers.mounts') }}</span>
              <span class="detail-value detail-mono">
                {{ formatMounts(inspectDetail?.Mounts) }}
              </span>
            </div>
          </div>

          <!-- 资源占用监控趋势 -->
          <div class="detail-history">
            <div class="detail-history-header">
              <span class="detail-label">{{ t('app.docker.containers.resourceTrend') }}</span>
              <span class="detail-hint">{{ t('app.docker.containers.trendWindow') }}</span>
            </div>
            <div class="detail-history-body">
              <div class="history-charts">
                <div class="history-chart" ref="historyCpuChartRef"></div>
                <div class="history-chart" ref="historyMemoryChartRef"></div>
                <div class="history-chart" ref="historyNetworkChartRef"></div>
              </div>
              <div v-if="historyLoading" class="history-overlay detail-loading">
                <SecLabLoading :loading="true" :text="t('app.docker.containers.loadingTrend')" />
              </div>
              <div v-else-if="historyError" class="history-overlay detail-error">
                <SecLabAlert type="error" :title="historyError" />
              </div>
              <div
                v-else-if="!containerHistory || !containerHistory.points.length"
                class="history-overlay detail-empty"
              >
                <SecLabEmpty :description="t('app.docker.containers.noTrendData')" />
              </div>
            </div>
          </div>
        </div>

        <!-- 进程监控标签页 -->
        <div v-show="activeTab === 'processes'" class="detail-section detail-processes">
          <div class="process-header">
            <span class="process-title">{{ t('app.docker.containers.containerProcesses') }}</span>
            <SecLabButton size="small" @click="loadProcessList">
              {{ t('app.docker.containers.refresh') }}
            </SecLabButton>
          </div>
          <div v-if="processLoading" class="detail-loading">
            <SecLabLoading :loading="true" :text="t('app.docker.containers.loadingProcesses')" />
          </div>
          <div v-else-if="processError" class="detail-error">
            <SecLabAlert type="error" :title="processError" />
          </div>
          <div v-else class="process-table-wrapper" data-native-context-menu>
            <table v-if="processColumns.length" class="process-table">
              <thead>
                <tr>
                  <th
                    v-for="(col, index) in processColumns"
                    :key="`${col}-${index}`"
                    :style="{ width: getProcessColumnWidth(index) }"
                    @click="toggleProcessSort(index)"
                  >
                    <div class="process-header-cell">
                      <span>{{ getProcessColumnLabel(index, col) }}</span>
                      <span class="sort-indicator">
                        {{
                          processSort && processSort.index === index
                            ? processSort.direction === 'asc'
                              ? '▲'
                              : '▼'
                            : ''
                        }}
                      </span>
                    </div>
                  </th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="(pRow, pIndex) in sortedProcessRows" :key="pIndex">
                  <td v-for="(_, colIndex) in processColumns" :key="colIndex">
                    <span
                      :class="[
                        colIndex === 3 ? 'process-command-cell' : '',
                        colIndex === 0 ? 'col-pid' : '',
                        colIndex === 1 ? 'col-cpu' : '',
                        colIndex === 2 ? 'col-mem' : '',
                      ]"
                    >
                      {{ pRow?.[colIndex] || '' }}
                    </span>
                  </td>
                </tr>
              </tbody>
            </table>
            <div v-else class="terminal-output" data-native-context-menu>
              {{ t('app.docker.containers.noProcessInfo') }}
            </div>
          </div>
        </div>

        <!-- 日志查看标签页 -->
        <div class="detail-section detail-fill" :class="{ 'detail-hidden': activeTab !== 'logs' }">
          <DockerContainerLogsPanel
            :container-id="props.containerId"
            :container-name="selectedContainerName"
            :node-id="props.nodeId"
            :active="activeTab === 'logs'"
          />
        </div>

        <!-- Web 终端标签页 -->
        <div
          class="detail-section detail-fill"
          :class="{ 'detail-hidden': activeTab !== 'terminal' }"
        >
          <DockerContainerTerminalPanel
            :container-id="props.containerId"
            :container-name="selectedContainerName"
            :node-id="props.nodeId"
            :active="activeTab === 'terminal'"
          />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.card-scroll-wrapper {
  flex-grow: 1;
  overflow-y: auto;
  min-height: 0;
}

.card-scroll-detail {
  display: flex;
  flex-direction: column;
  overflow: hidden;
  flex: 1;
}

.container-detail {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
  min-height: 0;
  flex: 1;
}

.container-detail-header {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-4);
}

.container-detail-title {
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.container-detail-name {
  font-size: var(--sdl-font-title);
  font-weight: 700;
  color: var(--sdl-primary);
  word-break: break-all;
}

.container-detail-status {
  font-size: var(--sdl-font-caption);
  color: var(--sdl-text-muted);
}

.detail-tabs {
  margin-bottom: var(--sdl-space-2);
}

.detail-content {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding-right: var(--sdl-space-1);
  display: flex;
  flex-direction: column;
}

.detail-content-processes {
  overflow: hidden;
}

.detail-section {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
  min-height: 0;
}

.detail-processes {
  flex: 1;
}

.detail-fill {
  flex: 1;
  min-height: 0;
}

.detail-hidden {
  display: none !important;
}

.detail-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: var(--sdl-space-3);
}

.detail-item {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-1);
  padding: var(--sdl-space-3);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  background: var(--sdl-bg-card);
}

.detail-full {
  grid-column: 1 / -1;
}

.detail-label {
  font-weight: 600;
  color: var(--sdl-text-secondary);
  font-size: var(--sdl-font-caption);
}

.detail-value {
  color: var(--sdl-text-primary);
  font-size: var(--sdl-font-body-sm);
  word-break: break-word;
  white-space: pre-wrap;
}

.detail-mono {
  font-family: var(--sdl-font-mono);
}

.detail-loading,
.detail-error,
.detail-empty {
  font-size: var(--sdl-font-body-sm);
}

.detail-history {
  margin-top: var(--sdl-space-4);
  padding: var(--sdl-space-3);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-lg);
  background: var(--sdl-bg-card);
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-2);
}

.detail-history-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.detail-hint {
  font-size: var(--sdl-font-caption);
  color: var(--sdl-text-muted);
}

.detail-history-body {
  position: relative;
  min-height: 180px;
}

.history-charts {
  display: grid;
  grid-template-columns: 1fr;
  gap: var(--sdl-space-3);
}

.history-chart {
  width: 100%;
  min-width: 0;
  height: 180px;
  border-radius: var(--sdl-radius-md);
  border: 1px solid var(--sdl-border-subtle);
  background: var(--sdl-bg-muted);
}

.history-overlay {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--sdl-bg-backdrop);
  backdrop-filter: blur(4px);
  border-radius: var(--sdl-radius-md);
}

.process-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sdl-space-3);
  margin-bottom: var(--sdl-space-2);
}

.process-title {
  font-weight: 700;
  color: var(--sdl-primary);
  font-size: var(--sdl-font-body);
}

.terminal-output {
  background: var(--sdl-bg-muted);
  color: var(--sdl-text-primary);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  padding: var(--sdl-space-3);
  font-family: var(--sdl-font-mono);
  font-size: var(--sdl-font-code);
  line-height: 1.5;
  min-height: 180px;
  white-space: pre-wrap;
  word-break: break-word;
}

.process-table-wrapper {
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  overflow: hidden;
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.process-table {
  width: 100%;
  border-collapse: collapse;
  font-size: var(--sdl-font-caption);
  min-width: 520px;
}

.process-table th,
.process-table td {
  border: 1px solid var(--sdl-border-subtle);
  padding: var(--sdl-space-2) var(--sdl-space-2);
  text-align: left;
  vertical-align: top;
  word-break: break-word;
}

.process-table th {
  position: sticky;
  top: 0;
  background: var(--sdl-bg-muted);
  color: var(--sdl-text-secondary);
  z-index: 1;
  cursor: pointer;
  user-select: none;
  white-space: nowrap;
}

.process-header-cell {
  display: inline-flex;
  align-items: center;
  gap: var(--sdl-space-1);
  white-space: nowrap;
  cursor: pointer;
}

.process-table {
  table-layout: fixed;
}

.col-pid {
  width: 80px;
}
.col-cpu,
.col-mem {
  width: 110px;
}

.process-command-cell {
  display: inline-block;
  width: 100%;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  color: var(--sdl-text-primary);
}

.sort-indicator {
  font-size: 10px;
  color: var(--sdl-primary);
}
</style>
