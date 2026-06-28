<script setup lang="ts">
import { SecLabCard, SecLabTag, SecLabCheckbox, SecLabButton } from '@/components/ui'
import { onMounted, onUnmounted, ref, watch, nextTick } from 'vue'
import * as echarts from 'echarts/core'
import { PieChart, LineChart } from 'echarts/charts'
import {
  GridComponent,
  LegendComponent,
  TitleComponent,
  TooltipComponent,
} from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import { useI18n } from 'vue-i18n'
import { useDockerStore } from '@/stores/docker'

echarts.use([
  PieChart,
  LineChart,
  GridComponent,
  LegendComponent,
  TitleComponent,
  TooltipComponent,
  CanvasRenderer,
])

const { t } = useI18n()
const store = useDockerStore()

/**
 * @file DockerOverview.vue
 * @description Docker 概览视图组件，严格遵循 SDL 设计规范。
 */

// 定义 Props 接收父组件传递的指标
const props = defineProps<{
  dockerStatus: boolean
  runningContainerCount: number
  totalImageCount: number
  totalContainerCount: number
  projectRunningCount: number
  projectTotalCount: number
  resourceUsage?: {
    cpuPercent: number
    memoryUsageBytes: number
    memoryLimitBytes: number
    memoryPercent: number
    networkRxBytes: number
    networkTxBytes: number
    containerCount: number
  } | null
  runningContainers?: Array<{ id: string; name: string }>
  selectedContainerIds?: string[]
  resourceUsageHistoryMap?: Record<
    string,
    {
      points: Array<{
        timestamp: number
        cpuPercent: number
        memoryUsageBytes: number
        memoryPercent: number
        networkRxBytes: number
        networkTxBytes: number
      }>
    }
  >
}>()

const emit = defineEmits<{
  (event: 'update:selected-container-ids', value: string[]): void
}>()

const cpuChartRef = ref<HTMLDivElement | null>(null)
const memoryChartRef = ref<HTMLDivElement | null>(null)
const trendChartRef = ref<HTMLDivElement | null>(null)
const chartInstances: echarts.ECharts[] = []
let cpuChart: echarts.ECharts | null = null
let memoryChart: echarts.ECharts | null = null
let trendChart: echarts.ECharts | null = null
let resizeObserver: ResizeObserver | null = null

const activeTab = ref<'resources' | 'system'>('resources')

watch(activeTab, (val) => {
  if (val === 'system') {
    void store.fetchDockerInfo()
  } else {
    nextTick(() => {
      initCharts()
    })
  }
})

const formatPercent = (value?: number) => {
  if (value === undefined) return '0.0%'
  return `${Math.min(Math.max(value, 0), 100).toFixed(1)}%`
}

const formatBytes = (bytes: number) => {
  if (bytes <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const base = 1024
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(base)), units.length - 1)
  const value = bytes / Math.pow(base, index)
  return `${value.toFixed(1)} ${units[index]}`
}

const getCssVar = (name: string) =>
  getComputedStyle(document.documentElement).getPropertyValue(name).trim()

const buildPieOption = (
  title: string,
  percent?: number,
  usageBytes?: number,
  limitBytes?: number,
) => ({
  backgroundColor: 'transparent',
  title: {
    text: title,
    left: 10,
    top: 'middle',
    textStyle: {
      fontSize: 12,
      color: getCssVar('--sdl-text-primary') || '#E6EDF7',
      fontWeight: 600,
    },
  },
  tooltip: {
    trigger: 'item',
    backgroundColor: getCssVar('--sdl-bg-panel') || '#162033',
    borderColor: getCssVar('--sdl-bg-muted') || '#202B40',
    textStyle: { color: getCssVar('--sdl-text-primary') || '#E6EDF7' },
    formatter: () => {
      if (percent === undefined) return t('app.docker.overview.noData')
      if (usageBytes !== undefined && limitBytes !== undefined) {
        return `${t('app.docker.overview.usagePercent')}: ${formatPercent(percent)}<br/>${t('app.docker.overview.usage')}: ${formatBytes(
          usageBytes,
        )}<br/>${t('app.docker.overview.limit')}: ${formatBytes(limitBytes)}`
      }
      return `${t('app.docker.overview.usagePercent')}: ${formatPercent(percent)}`
    },
  },
  legend: { show: false },
  series: [
    {
      type: 'pie',
      radius: ['48%', '72%'],
      center: ['70%', '50%'],
      avoidLabelOverlap: false,
      label: {
        show: true,
        position: 'center',
        formatter: () =>
          percent === undefined ? t('app.docker.overview.pending') : formatPercent(percent),
        color: getCssVar('--sdl-text-muted') || '#91A2B8',
        fontSize: 12,
      },
      data: [
        {
          value: percent === undefined ? 0 : Math.min(Math.max(percent, 0), 100),
          name: t('app.docker.overview.used'),
          itemStyle: { color: getCssVar('--sdl-primary') || '#00C8FF' },
        },
        {
          value: percent === undefined ? 100 : Math.max(0, 100 - percent),
          name: t('app.docker.overview.idle'),
          itemStyle: { color: getCssVar('--sdl-bg-muted') || '#202B40' },
        },
      ],
    },
  ],
})

const formatBytesPerSecond = (bytesPerSecond: number) => {
  if (bytesPerSecond <= 0) return '0 B/s'
  const units = ['B/s', 'KB/s', 'MB/s', 'GB/s', 'TB/s']
  const base = 1024
  const index = Math.max(
    0,
    Math.min(Math.floor(Math.log(bytesPerSecond) / Math.log(base)), units.length - 1),
  )
  const value = bytesPerSecond / Math.pow(base, index)
  return `${value.toFixed(1)} ${units[index]}`
}

type MetricType = 'cpu' | 'memory' | 'networkRx' | 'networkTx'

const metricType = ref<MetricType>('cpu')
const makeCompactLabel = (label: string) => label.replace('使用率', '').replace('Usage', '').trim()
const metricTabs: Array<{ type: MetricType; label: string }> = [
  { type: 'cpu', label: makeCompactLabel(t('app.docker.overview.cpuUsage')) },
  { type: 'memory', label: makeCompactLabel(t('app.docker.overview.memUsage')) },
  { type: 'networkRx', label: t('app.docker.overview.download') },
  { type: 'networkTx', label: t('app.docker.overview.upload') },
]

const maxSelectedContainers = 7
const isContainerSelectorOpen = ref(false)

const formatTimeLabel = (timestamp: number) =>
  new Date(timestamp * 1000).toLocaleTimeString('zh-CN', { hour12: false })

const resolveContainerName = (id: string) => {
  const item = props.runningContainers?.find((container) => container.id === id)
  return item?.name || id.slice(0, 12)
}

const onContainerToggle = (id: string, checked: boolean) => {
  const current = new Set(props.selectedContainerIds || [])
  if (checked) {
    current.add(id)
  } else {
    current.delete(id)
  }
  emit('update:selected-container-ids', Array.from(current))
}

const formatTrendValue = (value: number) => {
  if (metricType.value === 'cpu') {
    return `${Math.min(Math.max(value, 0), 100).toFixed(1)}%`
  }
  if (metricType.value === 'memory') {
    return formatBytes(value)
  }
  return formatBytesPerSecond(value)
}

const buildTrendOption = (
  title: string,
  labels: string[],
  series: Array<{ name: string; data: Array<number | null> }>,
) => ({
  backgroundColor: 'transparent',
  title: { show: false },
  tooltip: {
    trigger: 'axis',
    backgroundColor: getCssVar('--sdl-bg-panel') || '#162033',
    borderColor: getCssVar('--sdl-bg-muted') || '#202B40',
    textStyle: { color: getCssVar('--sdl-text-primary') || '#E6EDF7' },
    formatter: (params: Array<{ seriesName: string; data: number | null; dataIndex: number }>) => {
      const first = params[0]
      if (!first) return ''
      const label = labels[first.dataIndex] || ''
      const lines = params.map((item) => {
        if (item.data === null || item.data === undefined) {
          return `${item.seriesName}: -`
        }
        return `${item.seriesName}: ${formatTrendValue(item.data)}`
      })
      return [label, ...lines].join('<br/>')
    },
  },
  legend: {
    bottom: 6,
    left: 'center',
    textStyle: { fontSize: 10, color: getCssVar('--sdl-text-muted') || '#91A2B8' },
  },
  grid: { left: 50, right: 20, bottom: 48, top: 40 },
  xAxis: {
    type: 'category',
    data: labels,
    axisLabel: { fontSize: 10, color: getCssVar('--sdl-text-muted') || '#91A2B8' },
    axisTick: { show: false },
    axisLine: { lineStyle: { color: getCssVar('--sdl-bg-muted') || '#202B40' } },
  },
  yAxis: {
    type: 'value',
    max: (value: { max: number }) => Math.max(value.max * 1.2, 1),
    axisLabel: {
      fontSize: 10,
      color: getCssVar('--sdl-text-muted') || '#91A2B8',
      formatter: (val: number) => formatTrendValue(val),
    },
    splitLine: { lineStyle: { color: getCssVar('--sdl-bg-muted') || '#202B40' } },
  },
  color: ['#00C8FF', '#00D4B4', '#7C6CFF', '#FFB547', '#FF5E7A'],
  series: series.map((item) => ({
    type: 'line',
    name: item.name,
    data: item.data,
    smooth: true,
    showSymbol: false,
  })),
})

const buildEmptyTrendOption = (message: string) => ({
  ...buildTrendOption(t('app.docker.overview.traffic'), [], []),
  title: {
    show: true,
    text: message,
    left: 'center',
    top: 'middle',
    textStyle: {
      fontSize: 12,
      color: getCssVar('--sdl-text-muted') || '#91A2B8',
      fontWeight: 500,
    },
  },
  tooltip: { show: false },
  legend: { show: false },
})

const initChartInstance = (
  el: HTMLDivElement,
  optionBuilder: () => echarts.EChartsCoreOption,
): echarts.ECharts | null => {
  if (el.clientWidth > 0 && el.clientHeight > 0) {
    const chart = echarts.init(el)
    chart.setOption(optionBuilder())
    chartInstances.push(chart)
    return chart
  }
  return null
}

const initCharts = () => {
  if (cpuChartRef.value && !cpuChart) {
    cpuChart = initChartInstance(cpuChartRef.value, () =>
      buildPieOption(t('app.docker.overview.cpuUsage')),
    )
  }
  if (memoryChartRef.value && !memoryChart) {
    memoryChart = initChartInstance(memoryChartRef.value, () =>
      buildPieOption(
        t('app.docker.overview.memUsage'),
        props.resourceUsage?.memoryPercent,
        props.resourceUsage?.memoryUsageBytes,
        props.resourceUsage?.memoryLimitBytes,
      ),
    )
  }
  if (trendChartRef.value && !trendChart) {
    trendChart = initChartInstance(trendChartRef.value, () =>
      buildTrendOption(t('app.docker.overview.traffic'), [], []),
    )
  }

  if (!resizeObserver) {
    resizeObserver = new ResizeObserver(() => {
      if (cpuChartRef.value && !cpuChart) {
        cpuChart = initChartInstance(cpuChartRef.value, () =>
          buildPieOption(t('app.docker.overview.cpuUsage')),
        )
      }
      if (memoryChartRef.value && !memoryChart) {
        memoryChart = initChartInstance(memoryChartRef.value, () =>
          buildPieOption(
            t('app.docker.overview.memUsage'),
            props.resourceUsage?.memoryPercent,
            props.resourceUsage?.memoryUsageBytes,
            props.resourceUsage?.memoryLimitBytes,
          ),
        )
      }
      if (trendChartRef.value && !trendChart) {
        trendChart = initChartInstance(trendChartRef.value, () =>
          buildTrendOption(t('app.docker.overview.traffic'), [], []),
        )
        if (trendChart) {
          buildTrendSeries()
        }
      }

      chartInstances.forEach((chart) => chart.resize())
    })

    if (cpuChartRef.value) {
      resizeObserver.observe(cpuChartRef.value)
    }
    if (memoryChartRef.value) {
      resizeObserver.observe(memoryChartRef.value)
    }
    if (trendChartRef.value) {
      resizeObserver.observe(trendChartRef.value)
    }
  }

  updateCharts()
}

onMounted(() => {
  nextTick(() => {
    initCharts()
  })
})

onUnmounted(() => {
  resizeObserver?.disconnect()
  chartInstances.forEach((chart) => chart.dispose())
})

const buildTrendSeries = () => {
  if (!trendChart) return
  const selectedIds = props.selectedContainerIds ?? []
  const historyMap = props.resourceUsageHistoryMap ?? {}
  if (!selectedIds.length) {
    trendChart.setOption(buildEmptyTrendOption(t('app.docker.overview.noData')), true)
    return
  }

  const timestamps = new Set<number>()
  for (const id of selectedIds) {
    const points = historyMap[id]?.points || []
    for (const point of points) {
      timestamps.add(point.timestamp)
    }
  }
  const timeValues = Array.from(timestamps).sort((a, b) => a - b)
  if (!timeValues.length) {
    trendChart.setOption(buildEmptyTrendOption(t('app.docker.overview.noData')), true)
    return
  }

  const labels = timeValues.map((value) => formatTimeLabel(value))
  const series = selectedIds.map((id) => {
    const points = historyMap[id]?.points || []
    const valueMap = new Map<number, number>()
    if (metricType.value === 'cpu' || metricType.value === 'memory') {
      for (const point of points) {
        const metricValue = metricType.value === 'cpu' ? point.cpuPercent : point.memoryUsageBytes
        valueMap.set(point.timestamp, metricValue)
      }
    } else {
      if (points.length > 0) {
        valueMap.set(points[0]!.timestamp, 0)
        for (let i = 1; i < points.length; i += 1) {
          const prev = points[i - 1]!
          const current = points[i]!
          const deltaSeconds = Math.max(current.timestamp - prev.timestamp, 1)
          const deltaBytes =
            metricType.value === 'networkRx'
              ? current.networkRxBytes - prev.networkRxBytes
              : current.networkTxBytes - prev.networkTxBytes
          valueMap.set(current.timestamp, deltaBytes / deltaSeconds)
        }
      }
    }
    const data = timeValues.map((timestamp) => valueMap.get(timestamp) ?? null)
    return {
      name: resolveContainerName(id),
      data,
    }
  })

  const currentTab = metricTabs.find((tab) => tab.type === metricType.value)
  const title = currentTab?.label || t('app.docker.overview.traffic')
  trendChart.setOption(buildTrendOption(title, labels, series))
}

const updateCharts = () => {
  const usage = props.resourceUsage
  const cpuPercent = usage?.cpuPercent
  const memoryPercent = usage?.memoryPercent

  if (cpuChart) {
    cpuChart.setOption(buildPieOption(t('app.docker.overview.cpuUsage'), cpuPercent))
  }
  if (memoryChart) {
    memoryChart.setOption(
      buildPieOption(
        t('app.docker.overview.memUsage'),
        memoryPercent,
        usage?.memoryUsageBytes,
        usage?.memoryLimitBytes,
      ),
    )
  }
  buildTrendSeries()
}

watch(
  () => props.resourceUsageHistoryMap,
  () => {
    buildTrendSeries()
  },
  { deep: true, immediate: true },
)

watch(
  () => props.resourceUsage,
  () => {
    updateCharts()
  },
  { deep: true },
)

watch(
  () => [props.selectedContainerIds, metricType.value],
  () => {
    buildTrendSeries()
  },
  { deep: true },
)
</script>

<template>
  <div class="docker-overview">
    <SecLabCard shadow="never" class="docker-status-panel">
      <div class="docker-metrics">
        <div class="metric-item">
          <span class="metric-label">{{ $t('app.docker.overview.status') }}</span>
          <span class="metric-value" style="display: flex; align-items: center; height: 30px">
            <SecLabTag
              :type="props.dockerStatus ? 'success' : 'danger'"
              class="service-value-tag"
              size="small"
            >
              {{
                props.dockerStatus
                  ? $t('app.docker.overview.running')
                  : $t('app.docker.overview.stopped')
              }}
            </SecLabTag>
          </span>
        </div>
        <div class="metric-item">
          <span class="metric-label">{{ $t('app.docker.overview.projects') }}</span>
          <span class="metric-value">
            {{ props.projectRunningCount }}/{{ props.projectTotalCount }}
          </span>
        </div>
        <div class="metric-item running-count">
          <span class="metric-label">{{ $t('app.docker.overview.containers') }}</span>
          <span class="metric-value">
            {{ props.runningContainerCount }}/{{ props.totalContainerCount }}
          </span>
        </div>
        <div class="metric-item">
          <span class="metric-label">{{ $t('app.docker.overview.images') }}</span>
          <span class="metric-value">{{ props.totalImageCount }}</span>
        </div>
      </div>
    </SecLabCard>

    <!-- Tab 切换按钮 -->
    <div class="overview-tab-header">
      <button
        class="tab-btn"
        :class="{ 'tab-btn-active': activeTab === 'resources' }"
        @click="activeTab = 'resources'"
      >
        {{ t('app.docker.overview.tabs.resources') }}
      </button>
      <button
        class="tab-btn"
        :class="{ 'tab-btn-active': activeTab === 'system' }"
        @click="activeTab = 'system'"
      >
        {{ t('app.docker.overview.systemInfo.title') }}
      </button>
    </div>

    <!-- 资源监控内容 -->
    <div v-show="activeTab === 'resources'" class="docker-resource-container">
      <div class="resource-grid">
        <SecLabCard shadow="never" class="resource-card" full-height>
          <div class="resource-chart" ref="cpuChartRef"></div>
        </SecLabCard>
        <SecLabCard shadow="never" class="resource-card" full-height>
          <div class="resource-chart" ref="memoryChartRef"></div>
        </SecLabCard>
        <SecLabCard shadow="never" class="resource-card resource-wide" full-height>
          <div class="resource-controls">
            <div class="resource-tabs">
              <button
                v-for="tab in metricTabs"
                :key="tab.type"
                class="resource-tab"
                :class="{ 'resource-tab-active': metricType === tab.type }"
                @click="metricType = tab.type"
              >
                {{ tab.label }}
              </button>
            </div>
            <div class="resource-selector">
              <button
                class="resource-selector-toggle"
                type="button"
                @click="isContainerSelectorOpen = !isContainerSelectorOpen"
              >
                {{ t('app.docker.overview.containerSelector') }}
              </button>
              <div v-if="isContainerSelectorOpen" class="resource-selector-panel">
                <div class="resource-selector-list">
                  <div
                    v-for="container in props.runningContainers || []"
                    :key="container.id"
                    class="resource-selector-item-wrap"
                  >
                    <SecLabCheckbox
                      :model-value="props.selectedContainerIds?.includes(container.id) || false"
                      @update:model-value="(val) => onContainerToggle(container.id, val)"
                    >
                      <span class="resource-selector-text">{{ container.name }}</span>
                    </SecLabCheckbox>
                  </div>
                </div>
                <span class="resource-selector-hint">
                  {{ t('app.docker.overview.maxContainers', { count: maxSelectedContainers }) }}
                </span>
              </div>
            </div>
          </div>
          <div class="resource-chart resource-chart-wide" ref="trendChartRef"></div>
        </SecLabCard>
      </div>
    </div>

    <!-- 系统信息内容 -->
    <div v-if="activeTab === 'system'" class="docker-system-container">
      <SecLabCard shadow="never" class="system-info-card">
        <div class="system-info-grid">
          <div class="info-item">
            <span class="info-label">{{ t('app.docker.overview.systemInfo.version') }}</span>
            <span class="info-value">{{ store.systemInfo?.ServerVersion || '-' }}</span>
          </div>
          <div class="info-item">
            <span class="info-label">{{ t('app.docker.overview.systemInfo.driver') }}</span>
            <span class="info-value">{{ store.systemInfo?.Driver || '-' }}</span>
          </div>
          <div class="info-item">
            <span class="info-label">{{ t('app.docker.overview.systemInfo.kernel') }}</span>
            <span class="info-value">{{ store.systemInfo?.KernelVersion || '-' }}</span>
          </div>
          <div class="info-item">
            <span class="info-label">{{ t('app.docker.overview.systemInfo.os') }}</span>
            <span class="info-value">{{ store.systemInfo?.OperatingSystem || '-' }}</span>
          </div>
          <div class="info-item">
            <span class="info-label">{{ t('app.docker.overview.systemInfo.osType') }}</span>
            <span class="info-value">{{ store.systemInfo?.OSType || '-' }}</span>
          </div>
          <div class="info-item">
            <span class="info-label">{{ t('app.docker.overview.systemInfo.arch') }}</span>
            <span class="info-value">{{ store.systemInfo?.Architecture || '-' }}</span>
          </div>
          <div class="info-item">
            <span class="info-label">{{ t('app.docker.overview.systemInfo.cpu') }}</span>
            <span class="info-value">{{ store.systemInfo?.NCPU ?? '-' }}</span>
          </div>
          <div class="info-item">
            <span class="info-label">{{ t('app.docker.overview.systemInfo.memory') }}</span>
            <span class="info-value">
              {{ store.systemInfo?.MemTotal ? formatBytes(store.systemInfo.MemTotal) : '-' }}
            </span>
          </div>
        </div>
      </SecLabCard>

      <!-- 系统清理面板 -->
      <SecLabCard shadow="never" class="system-cleanup-card" style="margin-top: var(--sdl-space-4)">
        <template #header>
          <div class="system-cleanup-header">
            <h3>{{ t('app.docker.overview.cleanup.title') }}</h3>
          </div>
        </template>
        <div class="system-cleanup-content">
          <p class="cleanup-desc">{{ t('app.docker.overview.cleanup.desc') }}</p>
          <SecLabButton type="danger" :loading="store.isLoading" @click="store.handlePruneSystem">
            {{ t('app.docker.overview.cleanup.action') }}
          </SecLabButton>
        </div>
      </SecLabCard>
    </div>
  </div>
</template>

<style scoped>
.docker-overview {
  flex-grow: 1;
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  overflow: hidden;
}

.docker-status-panel {
  margin-top: var(--sdl-space-3);
  margin-bottom: var(--sdl-space-4);
  background-color: var(--sdl-bg-panel);
  flex-shrink: 0;
}

.docker-resource-container {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.resource-grid {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  grid-template-rows: minmax(180px, 1fr) minmax(240px, 1fr);
  gap: var(--sdl-space-4);
}

.resource-card {
  padding: var(--sdl-space-4);
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
}

.resource-controls {
  display: flex;
  flex-wrap: wrap;
  gap: var(--sdl-space-3);
  justify-content: space-between;
  align-items: flex-start;
  margin-bottom: var(--sdl-space-2);
}

.resource-tabs {
  display: flex;
  flex-wrap: wrap;
  gap: var(--sdl-space-2);
}

.resource-tab {
  border: 1px solid var(--sdl-border-default);
  background: var(--sdl-bg-muted);
  color: var(--sdl-text-secondary);
  border-radius: var(--sdl-radius-sm);
  padding: var(--sdl-space-1) var(--sdl-space-3);
  font-size: var(--sdl-font-caption);
  cursor: pointer;
  transition: all 0.2s;
}

.resource-tab-active {
  background: var(--sdl-primary);
  color: var(--sdl-text-inverse);
  border-color: var(--sdl-primary);
}

.resource-selector {
  position: relative;
  display: flex;
  align-items: flex-start;
  gap: var(--sdl-space-2);
}

.resource-selector-toggle {
  border: 1px solid var(--sdl-border-default);
  background: var(--sdl-bg-card);
  color: var(--sdl-text-secondary);
  border-radius: var(--sdl-radius-sm);
  padding: var(--sdl-space-1) var(--sdl-space-3);
  font-size: var(--sdl-font-caption);
  cursor: pointer;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.04);
}

.resource-selector-panel {
  position: absolute;
  top: 32px;
  right: 0;
  background: var(--sdl-bg-panel);
  border: 1px solid var(--sdl-border-strong);
  border-radius: var(--sdl-radius-md);
  padding: var(--sdl-space-3);
  box-shadow: var(--sdl-shadow-panel);
  z-index: var(--sdl-z-index-popover);
}

.resource-selector-list {
  min-width: 200px;
  max-width: 260px;
  max-height: 180px;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-2);
  border: 1px solid var(--sdl-border-subtle);
  border-radius: var(--sdl-radius-sm);
  padding: var(--sdl-space-2);
  background: var(--sdl-bg-muted);
}

.resource-selector-text {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-size: var(--sdl-font-caption);
}

.resource-selector-hint {
  font-size: 11px;
  color: var(--sdl-text-muted);
  margin-top: var(--sdl-space-2);
  display: block;
}

.resource-wide {
  grid-column: span 2;
}

.resource-chart {
  width: 100%;
  height: 100%;
  flex: 1 1 0;
  min-height: 160px;
}

.resource-chart-wide {
  min-height: 220px;
}

@media (max-width: 900px) {
  .resource-grid {
    grid-template-columns: 1fr;
    grid-template-rows: auto auto auto;
    overflow-y: auto;
  }
  .resource-wide {
    grid-column: span 1;
  }
}

/* 指标面板样式 */
.docker-metrics {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--sdl-space-4);
  width: 100%;
}

.metric-item {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  background-color: var(--sdl-bg-card);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  padding: var(--sdl-space-4) var(--sdl-space-5);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.08);
}

@media (max-width: 1200px) {
  .docker-metrics {
    grid-template-columns: repeat(2, 1fr);
  }
}

@media (max-width: 600px) {
  .docker-metrics {
    grid-template-columns: 1fr;
  }
}

.metric-label {
  color: var(--sdl-text-muted);
  margin-bottom: var(--sdl-space-1);
  font-size: var(--sdl-font-body-sm);
  font-weight: 500;
  display: inline-flex;
  align-items: center;
  gap: var(--sdl-space-1);
}

.metric-value {
  font-weight: 700;
  color: var(--sdl-text-primary);
  line-height: 1.2;
  font-size: var(--sdl-font-title);
  font-family: var(--sdl-font-mono);
}

.running-count .metric-value {
  color: var(--sdl-success);
}

/* Tab 切换 */
.overview-tab-header {
  display: flex;
  gap: var(--sdl-space-2);
  margin-bottom: var(--sdl-space-4);
  flex-shrink: 0;
}

.tab-btn {
  border: 1px solid var(--sdl-border-default);
  background: var(--sdl-bg-panel);
  color: var(--sdl-text-secondary);
  border-radius: var(--sdl-radius-sm);
  padding: var(--sdl-space-2) var(--sdl-space-4);
  font-size: var(--sdl-font-body-sm);
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s;
}

.tab-btn:hover {
  background: var(--sdl-bg-hover);
  color: var(--sdl-text-primary);
}

.tab-btn-active {
  background: var(--sdl-primary);
  color: var(--sdl-text-inverse);
  border-color: var(--sdl-primary);
}

.tab-btn-active:hover {
  background: var(--sdl-primary);
  opacity: 0.9;
}

/* 系统信息 */
.docker-system-container {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}

.system-info-card {
  background-color: var(--sdl-bg-panel);
  padding: var(--sdl-space-5);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-lg);
}

.system-cleanup-card {
  background-color: var(--sdl-bg-panel);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-lg);
}

.system-cleanup-header h3 {
  margin: 0;
  font-size: var(--sdl-font-body);
  font-weight: 600;
  color: var(--sdl-text-primary);
}

.system-cleanup-content {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
  align-items: flex-start;
}

.cleanup-desc {
  margin: 0;
  color: var(--sdl-text-secondary);
  font-size: var(--sdl-font-body-sm);
  line-height: 1.5;
}

.system-info-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--sdl-space-4);
}

.info-item {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-1);
  background: var(--sdl-bg-card);
  border: 1px solid var(--sdl-border-subtle);
  border-radius: var(--sdl-radius-md);
  padding: var(--sdl-space-4);
}

.info-label {
  font-weight: 600;
  color: var(--sdl-text-secondary);
  font-size: var(--sdl-font-caption);
}

.info-value {
  color: var(--sdl-text-primary);
  word-break: break-all;
  font-size: var(--sdl-font-body-sm);
  font-family: var(--sdl-font-mono);
}

@media (max-width: 768px) {
  .system-info-grid {
    grid-template-columns: 1fr;
  }
}
</style>
