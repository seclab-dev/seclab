<script setup lang="ts">
/**
 * @file MonitorOverviewBar.vue
 * @description 系统监控概览栏，横向排列 4 个 Gauge 仪表卡片展示关键实时指标。
 */

import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { HostSystemSummary } from '@/api/interface/system'
import type { AlertThresholds } from '@/composables/useSystemMonitor'
import { formatBytes } from '@/utils/units'
import MonitorGaugeCard from './MonitorGaugeCard.vue'

const props = defineProps<{
  /** 系统摘要数据 */
  summary: HostSystemSummary | null
  /** 告警阈值配置 */
  thresholds: AlertThresholds
}>()

const cpuValue = computed(() => props.summary?.cpuPercent ?? 0)
const memValue = computed(() => props.summary?.memoryPercent ?? 0)
const loadValue = computed(() => props.summary?.loadAvg1 ?? 0)

/** 内存用量描述。 */
const memSubtitle = computed(() => {
  if (!props.summary) return '--'
  const used = formatBytes(props.summary.memoryUsedBytes)
  const total = formatBytes(props.summary.memoryTotalBytes)
  return `${used} / ${total}`
})

/** 负载描述。 */
const loadSubtitle = computed(() => {
  if (!props.summary) return '--'
  const { loadAvg1, loadAvg5, loadAvg15 } = props.summary
  return `${loadAvg1.toFixed(2)} / ${loadAvg5.toFixed(2)} / ${loadAvg15.toFixed(2)}`
})

const { t } = useI18n()
</script>

<template>
  <div class="overview-bar" data-ui="overview-bar">
    <MonitorGaugeCard
      :label="t('app.systemMonitor.overview.cpu')"
      :value="cpuValue"
      unit="%"
      :warning-threshold="thresholds.cpuWarning"
      :danger-threshold="thresholds.cpuDanger"
    />
    <MonitorGaugeCard
      :label="t('app.systemMonitor.overview.memory')"
      :value="memValue"
      unit="%"
      :subtitle="memSubtitle"
      :warning-threshold="thresholds.memoryWarning"
      :danger-threshold="thresholds.memoryDanger"
    />
    <MonitorGaugeCard
      :label="t('app.systemMonitor.overview.load')"
      :value="loadValue"
      :max="Math.max(loadValue * 1.5, 4)"
      unit=""
      :subtitle="loadSubtitle"
      :warning-threshold="Math.max(loadValue * 1.5, 4) * 0.7"
      :danger-threshold="Math.max(loadValue * 1.5, 4) * 0.9"
    />
  </div>
</template>

<style scoped>
.overview-bar {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: var(--sdl-space-3);
  flex-shrink: 0;
}

@media (max-width: 768px) {
  .overview-bar {
    grid-template-columns: 1fr;
  }
}
</style>
