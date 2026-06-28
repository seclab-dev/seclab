<script setup lang="ts">
/**
 * @file MonitorHostInfo.vue
 * @description 主机信息摘要面板，调用 systemApi.fetchAbout() 展示主机硬件与系统信息。
 */

import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SystemAboutInfo } from '@/api/interface/system'
import { formatBytes } from '@/utils/units'
import { SecLabDescriptions } from '@/components/ui'

const props = defineProps<{
  /** 主机信息数据 */
  aboutInfo: SystemAboutInfo | null
}>()

const { t } = useI18n()

/** 运行时间格式化。 */
const uptimeText = computed(() => {
  if (!props.aboutInfo) return '--'
  const totalSeconds = props.aboutInfo.uptimeSeconds
  const days = Math.floor(totalSeconds / 86400)
  const hours = Math.floor((totalSeconds % 86400) / 3600)
  const minutes = Math.floor((totalSeconds % 3600) / 60)
  if (days > 0) return `${days}d ${hours}h ${minutes}m`
  if (hours > 0) return `${hours}h ${minutes}m`
  return `${minutes}m`
})

/** 描述列表项配置。 */
const items = computed(() => {
  const info = props.aboutInfo
  if (!info) return []
  return [
    { label: t('app.systemMonitor.hostInfo.hostname'), value: info.hostname },
    { label: t('app.systemMonitor.hostInfo.os'), value: info.osVersion },
    { label: t('app.systemMonitor.hostInfo.kernel'), value: info.kernelVersion },
    { label: t('app.systemMonitor.hostInfo.arch'), value: info.architecture },
    { label: t('app.systemMonitor.hostInfo.cpu'), value: info.cpuModel },
    {
      label: t('app.systemMonitor.hostInfo.cores'),
      value: `${info.cpuPhysicalCores}C / ${info.cpuLogicalCores}T`,
    },
    {
      label: t('app.systemMonitor.hostInfo.memory'),
      value: formatBytes(info.memoryTotalBytes),
    },
    { label: t('app.systemMonitor.hostInfo.uptime'), value: uptimeText.value },
    {
      label: t('app.systemMonitor.hostInfo.network'),
      value: info.ipv4Addresses.length > 0 ? info.ipv4Addresses.join(', ') : '--',
    },
    { label: t('app.systemMonitor.hostInfo.virtualization'), value: info.virtualization },
  ]
})
</script>

<template>
  <div class="host-info-panel" data-ui="host-info">
    <SecLabDescriptions v-if="aboutInfo" :items="items" :column="2" border />
    <div v-else class="loading-placeholder">--</div>
  </div>
</template>

<style scoped>
.host-info-panel {
  flex-shrink: 0;
}

.loading-placeholder {
  color: var(--sdl-text-muted);
  text-align: center;
  padding: var(--sdl-space-4);
}
</style>
