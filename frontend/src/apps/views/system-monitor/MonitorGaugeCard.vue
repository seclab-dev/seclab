<script setup lang="ts">
/**
 * @file MonitorGaugeCard.vue
 * @description 单指标仪表盘卡片组件，使用 ECharts Gauge 图表展示 CPU/内存等使用率。
 */

import { ref, watch } from 'vue'
import { GaugeChart } from 'echarts/charts'
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { useEChartsInstance } from '@/composables/useEChartsInstance'
import { buildGaugeOptions } from './chartOptions'
import { SecLabCard } from '@/components/ui'

use([GaugeChart, CanvasRenderer])

const props = withDefaults(
  defineProps<{
    /** 指标标签名称 */
    label: string
    /** 当前值 */
    value: number
    /** 最大值 */
    max?: number
    /** 单位后缀 */
    unit?: string
    /** 副标题描述行 */
    subtitle?: string
    /** 警告阈值 */
    warningThreshold?: number
    /** 危险阈值 */
    dangerThreshold?: number
  }>(),
  {
    max: 100,
    unit: '%',
    subtitle: '',
    warningThreshold: 80,
    dangerThreshold: 95,
  },
)

const gaugeRef = ref<HTMLElement | null>(null)
const { instance, setOption } = useEChartsInstance(gaugeRef)

/** 应用 gauge 配置。 */
const applyGauge = () => {
  if (!instance.value) return
  const options = buildGaugeOptions(
    props.value,
    props.max,
    props.unit,
    props.warningThreshold,
    props.dangerThreshold,
  )
  setOption(options)
}

watch(
  () => [props.value, props.max, props.warningThreshold, props.dangerThreshold],
  () => applyGauge(),
)

watch(instance, (chart) => {
  if (chart) applyGauge()
})
</script>

<template>
  <SecLabCard shadow="never" class="gauge-card" data-ui="gauge-card">
    <div class="gauge-label">{{ label }}</div>
    <div ref="gaugeRef" class="gauge-chart"></div>
    <div v-if="subtitle" class="gauge-subtitle">{{ subtitle }}</div>
  </SecLabCard>
</template>

<style scoped>
.gauge-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: var(--sdl-space-3) var(--sdl-space-3) var(--sdl-space-2);
  min-width: 0;
}

.gauge-label {
  font-size: var(--sdl-font-body-sm);
  color: var(--sdl-text-secondary);
  font-weight: 600;
  text-align: center;
  white-space: nowrap;
}

.gauge-chart {
  width: 100%;
  height: 120px;
  flex-shrink: 0;
}

.gauge-subtitle {
  font-size: 11px;
  color: var(--sdl-text-muted);
  font-family: var(--sdl-font-mono, monospace);
  text-align: center;
  margin-top: -2px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 100%;
}
</style>
