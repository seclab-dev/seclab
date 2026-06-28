/**
 * @file useSystemMonitor.ts
 * @description 系统监控数据管理 composable。
 *
 * 封装系统摘要/历史数据的拉取与定时刷新、采集器开关、时间范围切换、
 * 告警阈值管理，以及所有派生数据（速率计算、标签、指标数组）的计算。
 */

import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { systemApi } from '@/api/modules/system'
import type { HostSystemSummary, SystemHistoryPoint, SystemAboutInfo } from '@/api/interface/system'
import { resolveThroughputUnit } from '@/utils/units'
import { useNodeStore } from '@/stores/node'

/** 时间范围预设。 */
export type TimeRange = '1h' | '6h' | '24h' | '3d' | '7d'

/** 时间范围到小时数的映射。 */
const TIME_RANGE_HOURS: Record<TimeRange, number> = {
  '1h': 1,
  '6h': 6,
  '24h': 24,
  '3d': 72,
  '7d': 168,
}

/** 告警阈值配置。 */
export interface AlertThresholds {
  cpuWarning: number
  cpuDanger: number
  memoryWarning: number
  memoryDanger: number
}

const DEFAULT_THRESHOLDS: AlertThresholds = {
  cpuWarning: 80,
  cpuDanger: 95,
  memoryWarning: 80,
  memoryDanger: 95,
}

/** 采样间隔（毫秒），与后端 5 分钟对齐。 */
const SAMPLE_INTERVAL_MS = 300_000

/**
 * 格式化 Unix 时间戳为可读时间字符串。
 */
export const formatTime = (timestamp: number) => {
  const time = new Date(timestamp * 1000)
  const y = time.getFullYear()
  const m = String(time.getMonth() + 1).padStart(2, '0')
  const d = String(time.getDate()).padStart(2, '0')
  const hh = String(time.getHours()).padStart(2, '0')
  const mm = String(time.getMinutes()).padStart(2, '0')
  const ss = String(time.getSeconds()).padStart(2, '0')
  return `${y}-${m}-${d} ${hh}:${mm}:${ss}`
}

/**
 * 将累计字节序列转换为速率序列（字节/秒）。
 */
export const buildRates = (
  items: SystemHistoryPoint[],
  selector: (point: SystemHistoryPoint) => number,
): number[] => {
  const values: number[] = []
  for (let i = 0; i < items.length; i += 1) {
    if (i === 0) {
      values.push(0)
      continue
    }
    const prev = items[i - 1]
    const current = items[i]
    if (!prev || !current) {
      values.push(0)
      continue
    }
    const delta = Math.max(0, selector(current) - selector(prev))
    const seconds = Math.max(1, current.createdAt - prev.createdAt)
    values.push(delta / seconds)
  }
  return values
}

/**
 * 系统监控数据管理 composable。
 */
export function useSystemMonitor() {
  const { t } = useI18n()
  const nodeStore = useNodeStore()
  const systemClient = computed(() => systemApi.forNode(nodeStore.currentNodeId))

  // === 状态 ===
  const loading = ref(false)
  const switchingCollector = ref(false)
  const errorText = ref('')
  const updatedAtText = ref('--')
  const summary = ref<HostSystemSummary | null>(null)
  const aboutInfo = ref<SystemAboutInfo | null>(null)
  const points = ref<SystemHistoryPoint[]>([])
  const collectorEnabled = ref(false)
  const timeRange = ref<TimeRange>('24h')
  const thresholds = ref<AlertThresholds>({ ...DEFAULT_THRESHOLDS })

  let refreshTimer: number | null = null

  // === 派生数据 ===
  const labels = computed(() => points.value.map((item) => formatTime(item.createdAt)))
  const load1 = computed(() => points.value.map((item) => item.loadAvg1))
  const load5 = computed(() => points.value.map((item) => item.loadAvg5))
  const load15 = computed(() => points.value.map((item) => item.loadAvg15))
  const cpu = computed(() => points.value.map((item) => Number(item.cpuPercent.toFixed(2))))
  const memory = computed(() => points.value.map((item) => Number(item.memoryPercent.toFixed(2))))
  const diskRead = computed(() => buildRates(points.value, (item) => item.diskReadBytes))
  const diskWrite = computed(() => buildRates(points.value, (item) => item.diskWriteBytes))
  const netRx = computed(() => buildRates(points.value, (item) => item.networkRxBytes))
  const netTx = computed(() => buildRates(points.value, (item) => item.networkTxBytes))

  const diskScale = computed(() => resolveThroughputUnit([...diskRead.value, ...diskWrite.value]))
  const netScale = computed(() => resolveThroughputUnit([...netRx.value, ...netTx.value]))

  // === 操作 ===

  /** 拉取系统摘要与历史数据。 */
  const fetchData = async () => {
    loading.value = true
    errorText.value = ''
    try {
      const hours = TIME_RANGE_HOURS[timeRange.value]
      const [summaryRes, historyRes] = await Promise.all([
        systemClient.value.fetchSummary(),
        systemClient.value.fetchHistory({ hours }),
      ])
      if (!summaryRes.success) {
        throw new Error(summaryRes.message || t('app.systemMonitor.fetchFailed'))
      }
      if (!historyRes.success) {
        throw new Error(historyRes.message || t('app.systemMonitor.fetchFailed'))
      }
      summary.value = summaryRes.data ?? null
      points.value = historyRes.data ?? []
      if (summary.value?.collectedAt) {
        updatedAtText.value = formatTime(summary.value.collectedAt)
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : t('app.systemMonitor.fetchFailed')
      errorText.value = message
    } finally {
      loading.value = false
    }
  }

  /** 拉取主机基础信息。 */
  const fetchAbout = async () => {
    try {
      const res = await systemClient.value.fetchAbout()
      if (res.success && res.data) {
        aboutInfo.value = res.data
      }
    } catch {
      // 主机信息获取失败不阻塞主流程
    }
  }

  /** 拉取采集器开关状态。 */
  const loadCollectorStatus = async () => {
    const res = await systemClient.value.fetchCollectorStatus()
    if (!res.success || !res.data) {
      throw new Error(res.message || t('app.systemMonitor.fetchFailed'))
    }
    collectorEnabled.value = Boolean(res.data.enabled)
  }

  /** 切换采集器启停。 */
  const toggleCollector = async () => {
    switchingCollector.value = true
    errorText.value = ''
    try {
      const next = !collectorEnabled.value
      const res = await systemClient.value.setCollectorStatus(next)
      if (!res.success || !res.data) {
        throw new Error(res.message || t('app.systemMonitor.toggleCollectorFailed'))
      }
      collectorEnabled.value = Boolean(res.data.enabled)
    } catch (error) {
      const message =
        error instanceof Error ? error.message : t('app.systemMonitor.toggleCollectorFailed')
      errorText.value = message
    } finally {
      switchingCollector.value = false
    }
  }

  /** 清空历史数据。 */
  const clearHistory = async () => {
    const res = await systemClient.value.clearHistory()
    if (!res.success) {
      errorText.value = res.message || t('app.systemMonitor.clearFailed')
      return
    }
    points.value = []
  }

  /** 拉取告警阈值。 */
  const fetchThresholds = async () => {
    try {
      const res = await systemClient.value.fetchAlertThresholds()
      if (res.success && res.data) {
        thresholds.value = res.data
      }
    } catch {
      // 阈值获取失败使用默认值
    }
  }

  /** 保存告警阈值。 */
  const saveThresholds = async (newThresholds: AlertThresholds) => {
    try {
      const res = await systemClient.value.setAlertThresholds(newThresholds)
      if (res.success && res.data) {
        thresholds.value = res.data
      }
    } catch (error) {
      const message =
        error instanceof Error ? error.message : t('app.systemMonitor.saveThresholdsFailed')
      errorText.value = message
    }
  }

  /** 启动定时刷新。 */
  const startAutoRefresh = () => {
    stopAutoRefresh()
    refreshTimer = window.setInterval(() => {
      void fetchData()
    }, SAMPLE_INTERVAL_MS)
  }

  /** 停止定时刷新。 */
  const stopAutoRefresh = () => {
    if (refreshTimer !== null) {
      window.clearInterval(refreshTimer)
      refreshTimer = null
    }
  }

  // 时间范围变化时重新拉取数据
  watch(timeRange, () => {
    void fetchData()
  })

  // 生命周期
  onMounted(() => {
    void loadCollectorStatus()
      .then(fetchData)
      .catch((error: unknown) => {
        const message = error instanceof Error ? error.message : t('app.systemMonitor.fetchFailed')
        errorText.value = message
        void fetchData()
      })
    void fetchAbout()
    void fetchThresholds()
    startAutoRefresh()
  })

  onUnmounted(() => {
    stopAutoRefresh()
  })

  return {
    // 状态
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
    // 派生数据
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
    // 操作
    fetchData,
    fetchAbout,
    toggleCollector,
    clearHistory,
    saveThresholds,
  }
}
