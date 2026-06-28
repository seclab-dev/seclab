<script setup lang="ts">
/**
 * @file SystemMonitorView.vue
 * @description 系统监控主视图 — 企业级监控仪表盘。
 *
 * 编排层组件，职责：
 * 1. 调用 useSystemMonitor() 获取数据与操作。
 * 2. 调用 chartOptions 构建器生成图表配置。
 * 3. 将配置分发给 MonitorChartPanel / MonitorOverviewBar 等子组件。
 * 4. 管理顶部工具栏（采集器开关、时间范围、刷新、清除）。
 */

import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LineChart, GaugeChart } from 'echarts/charts'
import {
  DataZoomComponent,
  GridComponent,
  LegendComponent,
  MarkLineComponent,
  TooltipComponent,
} from 'echarts/components'
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { useThemeStore, buildChartTheme } from '@/stores/theme'
import { useWindowManagerStore } from '@/stores/window-manager'
import { useSystemMonitor } from '@/composables/useSystemMonitor'
import {
  buildLoadChartOptions,
  buildCpuChartOptions,
  buildMemoryChartOptions,
  buildDiskIoChartOptions,
  buildNetworkChartOptions,
} from './chartOptions'
import MonitorChartPanel from './MonitorChartPanel.vue'
import MonitorOverviewBar from './MonitorOverviewBar.vue'
import MonitorHostInfo from './MonitorHostInfo.vue'
import MonitorTimeRangeSelector from './MonitorTimeRangeSelector.vue'
import { SecLabButton, SecLabCard, SecLabDialog, SecLabEmpty, SecLabLoading } from '@/components/ui'
import SecLabIcon from '@/components/icons/SecLabIcon.vue'

use([
  LineChart,
  GaugeChart,
  GridComponent,
  TooltipComponent,
  LegendComponent,
  DataZoomComponent,
  MarkLineComponent,
  CanvasRenderer,
])

const props = defineProps<{
  isMaximized?: boolean
  windowId?: string
  payload?: Record<string, unknown>
}>()

const { t } = useI18n()
const themeStore = useThemeStore()
const windowStore = useWindowManagerStore()

const {
  loading,
  switchingCollector,
  errorText,
  updatedAtText,
  summary,
  aboutInfo,
  points,
  collectorEnabled,
  timeRange,
  thresholds,
  labels,
  load1,
  load5,
  load15,
  cpu,
  memory,
  diskRead,
  diskWrite,
  netRx,
  netTx,
  diskScale,
  netScale,
  fetchData,
  toggleCollector,
  clearHistory,
} = useSystemMonitor()

// === 清除确认弹窗 ===
const showClearDialog = ref(false)

/** 打开清除确认弹窗。 */
const handleClearClick = () => {
  showClearDialog.value = true
}

/** 确认清除。 */
const handleClearConfirm = async () => {
  showClearDialog.value = false
  await clearHistory()
}

// === 主机信息展开 ===
const showHostInfo = ref(false)

// === 图表配置（响应式计算） ===
const chartTheme = computed(() => buildChartTheme(themeStore.currentTheme))

const loadChartOptions = computed(() =>
  buildLoadChartOptions(labels.value, load1.value, load5.value, load15.value, chartTheme.value, {
    load1: t('app.systemMonitor.charts.series.load1'),
    load5: t('app.systemMonitor.charts.series.load5'),
    load15: t('app.systemMonitor.charts.series.load15'),
  }),
)

const cpuChartOptions = computed(() =>
  buildCpuChartOptions(
    labels.value,
    cpu.value,
    chartTheme.value,
    t('app.systemMonitor.charts.series.cpu'),
    thresholds.value,
  ),
)

const memoryChartOptions = computed(() =>
  buildMemoryChartOptions(
    labels.value,
    memory.value,
    chartTheme.value,
    t('app.systemMonitor.charts.series.memory'),
    thresholds.value,
  ),
)

const diskIoChartOptions = computed(() =>
  buildDiskIoChartOptions(
    labels.value,
    diskRead.value,
    diskWrite.value,
    diskScale.value,
    chartTheme.value,
    {
      read: t('app.systemMonitor.charts.series.read'),
      write: t('app.systemMonitor.charts.series.write'),
    },
  ),
)

const networkChartOptions = computed(() =>
  buildNetworkChartOptions(
    labels.value,
    netRx.value,
    netTx.value,
    netScale.value,
    chartTheme.value,
    {
      rx: t('app.systemMonitor.charts.series.rx'),
      tx: t('app.systemMonitor.charts.series.tx'),
    },
  ),
)

/** 是否有历史数据可展示。 */
const hasData = computed(() => points.value.length > 0)
const monitorBusy = computed(() => loading.value || switchingCollector.value)

watch(
  monitorBusy,
  (busy) => {
    if (!props.windowId) return
    windowStore.updateWindowRuntimeState(props.windowId, {
      busy,
      allowsNodeSwitch: false,
      blockLevel: busy ? 'busy' : 'open',
      blockReason: busy ? t('app.systemMonitor.guardBusy') : t('app.systemMonitor.guardOpen'),
    })
  },
  { immediate: true },
)
</script>

<template>
  <div class="system-monitor" data-page="system-monitor">
    <!-- 顶部工具栏 -->
    <SecLabCard shadow="never" class="header-card">
      <div class="header" data-slot="header" data-ui="toolbar">
        <div class="title-block">
          <h2>{{ t('app.systemMonitor.title') }}</h2>
          <div class="meta-info">
            <span
              >{{ t('app.systemMonitor.sampleInterval') }}:
              {{ t('app.systemMonitor.unitMinute', { n: 5 }) }}</span
            >
            <span class="divider">·</span>
            <span
              >{{ t('app.systemMonitor.retention') }}:
              {{ t('app.systemMonitor.unitDay', { n: 7 }) }}</span
            >
            <span class="divider">·</span>
            <span>{{ t('app.systemMonitor.updatedAt') }}: {{ updatedAtText }}</span>
          </div>
        </div>
        <div class="actions">
          <MonitorTimeRangeSelector v-model="timeRange" />
          <SecLabButton
            :type="collectorEnabled ? 'danger' : 'primary'"
            :loading="switchingCollector"
            @click="toggleCollector"
          >
            {{
              collectorEnabled
                ? t('app.systemMonitor.disableCollector')
                : t('app.systemMonitor.enableCollector')
            }}
          </SecLabButton>
          <SecLabButton type="secondary" :loading="loading" @click="fetchData">
            {{ t('app.systemMonitor.refresh') }}
          </SecLabButton>
          <SecLabButton type="secondary" @click="showHostInfo = !showHostInfo">
            {{ t('app.systemMonitor.hostInfoTitle') }}
          </SecLabButton>
          <SecLabButton type="danger" @click="handleClearClick">
            {{ t('app.systemMonitor.clearHistory') }}
          </SecLabButton>
        </div>
      </div>

      <div v-if="errorText" class="error-state">
        <SecLabIcon class="error-icon" name="warning" :size="16" />
        <span>{{ errorText }}</span>
      </div>
    </SecLabCard>

    <!-- 主机信息展开面板 -->
    <MonitorHostInfo v-if="showHostInfo" :about-info="aboutInfo" />

    <!-- 实时概览仪表盘 -->
    <MonitorOverviewBar v-if="summary" :summary="summary" :thresholds="thresholds" />

    <!-- 空数据引导态 -->
    <div v-if="!loading && !hasData && collectorEnabled" class="empty-wrapper">
      <SecLabEmpty :description="t('app.systemMonitor.fetchFailed')">
        <template #extra>
          <SecLabButton type="primary" @click="fetchData">
            {{ t('app.systemMonitor.refresh') }}
          </SecLabButton>
        </template>
      </SecLabEmpty>
    </div>

    <!-- 采集器关闭引导态 -->
    <div v-if="!loading && !hasData && !collectorEnabled" class="empty-wrapper">
      <SecLabEmpty :description="t('app.systemMonitor.collectorDisabledDesc')">
        <template #extra>
          <SecLabButton type="primary" :loading="switchingCollector" @click="toggleCollector">
            {{ t('app.systemMonitor.enableCollector') }}
          </SecLabButton>
        </template>
      </SecLabEmpty>
    </div>

    <!-- 图表网格 -->
    <div v-if="hasData" class="chart-grid">
      <MonitorChartPanel
        :title="t('app.systemMonitor.charts.load')"
        :options="loadChartOptions"
        full-width
      />
      <MonitorChartPanel :title="t('app.systemMonitor.charts.cpu')" :options="cpuChartOptions" />
      <MonitorChartPanel
        :title="t('app.systemMonitor.charts.memory')"
        :options="memoryChartOptions"
      />
      <MonitorChartPanel
        :title="t('app.systemMonitor.charts.diskIo')"
        :options="diskIoChartOptions"
      />
      <MonitorChartPanel
        :title="t('app.systemMonitor.charts.network')"
        :options="networkChartOptions"
      />
    </div>

    <SecLabLoading :loading="loading && points.length === 0" cover />

    <!-- 清除确认弹窗 -->
    <SecLabDialog
      :visible="showClearDialog"
      :title="t('app.systemMonitor.clearConfirmTitle')"
      width="420px"
      @close="showClearDialog = false"
    >
      <p>{{ t('app.systemMonitor.clearConfirm') }}</p>
      <template #footer>
        <SecLabButton type="secondary" @click="showClearDialog = false">
          {{ t('common.cancel') }}
        </SecLabButton>
        <SecLabButton type="danger" @click="handleClearConfirm">
          {{ t('common.confirm') }}
        </SecLabButton>
      </template>
    </SecLabDialog>
  </div>
</template>

<style scoped>
.system-monitor {
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
  flex-wrap: wrap;
}

.title-block h2 {
  margin: 0;
  font-size: var(--sdl-font-title);
  color: var(--sdl-text-primary);
  font-weight: 700;
}

.meta-info {
  margin-top: var(--sdl-space-2);
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-caption);
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
}

.meta-info .divider {
  opacity: 0.5;
}

.actions {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
  flex-wrap: wrap;
}

.error-state {
  margin-top: var(--sdl-space-2);
  color: var(--sdl-danger);
  font-size: var(--sdl-font-body-sm);
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
}

.empty-wrapper {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}

.chart-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--sdl-space-3);
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding-bottom: var(--sdl-space-4);
}

@media (max-width: 1024px) {
  .chart-grid {
    grid-template-columns: 1fr;
  }
}
</style>
