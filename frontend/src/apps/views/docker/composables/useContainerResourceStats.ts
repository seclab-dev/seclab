import type * as dockerType from '@/api/interface/docker'
import { ref, watch, type ComputedRef } from 'vue'

type ContainerStatsEntry = {
  data: {
    cpuCorePercent: number
    memoryWorkingSetBytes: number
    memoryLimitBytes: number
    memoryPercent: number
    networkRxBytes: number
    networkTxBytes: number
  }
  fetchedAt: number
}

type UseContainerResourceStatsOptions = {
  containerResourceStats: ComputedRef<Record<string, ContainerStatsEntry> | undefined>
  onFetchContainerStats: (id: string) => void
  formatBytes: (bytes?: number) => string
}

export const useContainerResourceStats = ({
  containerResourceStats,
  onFetchContainerStats,
  formatBytes,
}: UseContainerResourceStatsOptions) => {
  const networkRateMap = ref<Record<string, { rxRate: number; txRate: number }>>({})
  const networkTotalsMap = ref<Record<string, { rx: number; tx: number; at: number }>>({})

  watch(
    containerResourceStats,
    (stats) => {
      if (!stats) return
      const nextRates: Record<string, { rxRate: number; txRate: number }> = {
        ...networkRateMap.value,
      }
      const nextTotals: Record<string, { rx: number; tx: number; at: number }> = {
        ...networkTotalsMap.value,
      }

      for (const [id, entry] of Object.entries(stats)) {
        const prev = networkTotalsMap.value[id]
        const rx = entry.data.networkRxBytes
        const tx = entry.data.networkTxBytes
        const at = entry.fetchedAt
        if (prev && at > prev.at) {
          const deltaSeconds = Math.max((at - prev.at) / 1000, 0.001)
          const rxRate = Math.max(rx - prev.rx, 0) / deltaSeconds
          const txRate = Math.max(tx - prev.tx, 0) / deltaSeconds
          nextRates[id] = { rxRate, txRate }
        }
        nextTotals[id] = { rx, tx, at }
      }

      networkRateMap.value = nextRates
      networkTotalsMap.value = nextTotals
    },
    { deep: true },
  )

  const getContainerStats = (id: string | undefined) => {
    if (!id) return null
    return containerResourceStats.value?.[id]?.data || null
  }

  const requestContainerStats = (id: string | undefined, state?: string) => {
    if (!id) return
    if (state && state !== 'running') return
    onFetchContainerStats(id)
  }

  const handleContainerRowMouseEnter = (row: dockerType.ContainerSummary) => {
    requestContainerStats(row?.Id, row?.State)
  }

  const formatNetworkUsage = (id: string | undefined) => {
    if (!id) return '-'
    const rate = networkRateMap.value[id]
    if (!rate) {
      const totals = networkTotalsMap.value[id]
      if (!totals) return '-'
      return `↓ ${formatBytes(0)}/s / ↑ ${formatBytes(0)}/s`
    }
    return `↓ ${formatBytes(rate.rxRate)}/s / ↑ ${formatBytes(rate.txRate)}/s`
  }

  const reset = () => {
    networkRateMap.value = {}
    networkTotalsMap.value = {}
  }

  return {
    getContainerStats,
    requestContainerStats,
    handleContainerRowMouseEnter,
    formatNetworkUsage,
    reset,
  }
}
