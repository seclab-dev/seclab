/**
 * @file chartOptions.ts
 * @description 系统监控图表配置构建器（纯函数模块）。
 *
 * 为五类图表（负载、CPU、内存、磁盘 IO、网络吞吐）生成完整 ECharts 配置，
 * 包含 dataZoom 缩放、markLine 阈值标线、crosshair 联动等企业级交互功能。
 */

import type { AlertThresholds } from '@/composables/useSystemMonitor'
import { formatScaledThroughput } from '@/utils/units'

/** 图表主题色集合。 */
interface ChartTheme {
  titleColor: string
  textColor: string
  gridLineColor: string
  tooltipBackground: string
  tooltipBorder: string
}

/** 吞吐量缩放信息。 */
interface ThroughputScale {
  base: number
  unit: string
}

/** SDL 标准图表配色。 */
const CHART_COLORS = ['#00C8FF', '#00D4B4', '#7C6CFF', '#FFB547', '#FF5E7A']

/** 通用 dataZoom 配置。 */
const buildDataZoom = () => [
  {
    type: 'inside',
    start: 0,
    end: 100,
    zoomOnMouseWheel: true,
    moveOnMouseMove: true,
  },
  {
    type: 'slider',
    show: true,
    height: 20,
    bottom: 0,
    borderColor: 'transparent',
    backgroundColor: 'rgba(148, 163, 184, 0.06)',
    fillerColor: 'rgba(0, 200, 255, 0.12)',
    handleStyle: { color: '#00C8FF', borderColor: '#00C8FF' },
    textStyle: { color: '#91A2B8', fontSize: 10 },
  },
]

/** 通用 tooltip 配置。 */
const buildTooltip = (theme: ChartTheme, valueFormatter?: (value: unknown) => string) => ({
  trigger: 'axis' as const,
  backgroundColor: theme.tooltipBackground,
  borderColor: theme.tooltipBorder,
  textStyle: { color: theme.titleColor, fontSize: 12 },
  axisPointer: {
    type: 'cross' as const,
    crossStyle: { color: 'rgba(0, 200, 255, 0.3)' },
    lineStyle: { color: 'rgba(0, 200, 255, 0.3)' },
    label: { backgroundColor: 'rgba(0, 200, 255, 0.2)' },
  },
  ...(valueFormatter ? { valueFormatter } : {}),
})

/** 通用 legend 配置。 */
const buildLegend = (theme: ChartTheme) => ({
  top: 4,
  textStyle: { color: theme.textColor, fontSize: 12 },
  itemWidth: 14,
  itemHeight: 8,
  itemGap: 16,
})

/** 通用 X 轴配置。 */
const buildXAxis = (theme: ChartTheme, data: string[]) => ({
  type: 'category' as const,
  data,
  axisLabel: { color: theme.textColor, fontSize: 11 },
  axisLine: { lineStyle: { color: theme.gridLineColor } },
  axisTick: { lineStyle: { color: theme.gridLineColor } },
})

/** 通用 Y 轴配置。 */
const buildYAxis = (theme: ChartTheme) => ({
  type: 'value' as const,
  axisLabel: { color: theme.textColor, fontSize: 11 },
  splitLine: { lineStyle: { color: theme.gridLineColor } },
})

/** 百分比 markLine（告警阈值）。 */
const buildThresholdMarkLine = (warning: number, danger: number) => ({
  silent: true,
  symbol: 'none',
  lineStyle: { width: 1, type: 'dashed' as const },
  data: [
    {
      yAxis: warning,
      lineStyle: { color: '#FFB547' },
      label: {
        show: true,
        position: 'insideEndTop' as const,
        formatter: `⚠ ${warning}%`,
        color: '#FFB547',
        fontSize: 10,
      },
    },
    {
      yAxis: danger,
      lineStyle: { color: '#FF5E7A' },
      label: {
        show: true,
        position: 'insideEndTop' as const,
        formatter: `✖ ${danger}%`,
        color: '#FF5E7A',
        fontSize: 10,
      },
    },
  ],
})

// ===== 图表配置构建函数 =====

/**
 * 构建系统负载趋势图配置。
 */
export function buildLoadChartOptions(
  labels: string[],
  load1: number[],
  load5: number[],
  load15: number[],
  theme: ChartTheme,
  seriesNames: { load1: string; load5: string; load15: string },
): Record<string, unknown> {
  return {
    color: CHART_COLORS,
    tooltip: buildTooltip(theme),
    legend: buildLegend(theme),
    grid: { left: 44, right: 18, top: 36, bottom: 36 },
    xAxis: buildXAxis(theme, labels),
    yAxis: buildYAxis(theme),
    dataZoom: buildDataZoom(),
    series: [
      {
        name: seriesNames.load1,
        type: 'line',
        smooth: true,
        data: load1,
        symbol: 'none',
        lineStyle: { width: 1.5 },
      },
      {
        name: seriesNames.load5,
        type: 'line',
        smooth: true,
        data: load5,
        symbol: 'none',
        lineStyle: { width: 1.5 },
      },
      {
        name: seriesNames.load15,
        type: 'line',
        smooth: true,
        data: load15,
        symbol: 'none',
        lineStyle: { width: 1.5 },
      },
    ],
  }
}

/**
 * 构建 CPU 使用率趋势图配置。
 */
export function buildCpuChartOptions(
  labels: string[],
  cpu: number[],
  theme: ChartTheme,
  seriesName: string,
  thresholds?: AlertThresholds,
): Record<string, unknown> {
  return {
    color: CHART_COLORS,
    tooltip: buildTooltip(theme, (value) => `${Number(value ?? 0).toFixed(1)}%`),
    grid: { left: 44, right: 18, top: 16, bottom: 36 },
    xAxis: buildXAxis(theme, labels),
    yAxis: {
      ...buildYAxis(theme),
      min: 0,
      max: 100,
      axisLabel: {
        color: theme.textColor,
        fontSize: 11,
        formatter: '{value}%',
      },
    },
    dataZoom: buildDataZoom(),
    series: [
      {
        name: seriesName,
        type: 'line',
        smooth: true,
        data: cpu,
        symbol: 'none',
        lineStyle: { width: 1.5 },
        areaStyle: { opacity: 0.08 },
        ...(thresholds
          ? {
              markLine: buildThresholdMarkLine(thresholds.cpuWarning, thresholds.cpuDanger),
            }
          : {}),
      },
    ],
  }
}

/**
 * 构建内存使用率趋势图配置。
 */
export function buildMemoryChartOptions(
  labels: string[],
  memory: number[],
  theme: ChartTheme,
  seriesName: string,
  thresholds?: AlertThresholds,
): Record<string, unknown> {
  return {
    color: [CHART_COLORS[1]],
    tooltip: buildTooltip(theme, (value) => `${Number(value ?? 0).toFixed(1)}%`),
    grid: { left: 44, right: 18, top: 16, bottom: 36 },
    xAxis: buildXAxis(theme, labels),
    yAxis: {
      ...buildYAxis(theme),
      min: 0,
      max: 100,
      axisLabel: {
        color: theme.textColor,
        fontSize: 11,
        formatter: '{value}%',
      },
    },
    dataZoom: buildDataZoom(),
    series: [
      {
        name: seriesName,
        type: 'line',
        smooth: true,
        data: memory,
        symbol: 'none',
        lineStyle: { width: 1.5 },
        areaStyle: { opacity: 0.08 },
        ...(thresholds
          ? {
              markLine: buildThresholdMarkLine(thresholds.memoryWarning, thresholds.memoryDanger),
            }
          : {}),
      },
    ],
  }
}

/**
 * 构建磁盘 I/O 趋势图配置。
 */
export function buildDiskIoChartOptions(
  labels: string[],
  diskRead: number[],
  diskWrite: number[],
  scale: ThroughputScale,
  theme: ChartTheme,
  seriesNames: { read: string; write: string },
): Record<string, unknown> {
  const formatter = (value: unknown) =>
    `${formatScaledThroughput(Number(value ?? 0), scale.base)} ${scale.unit}`
  return {
    color: CHART_COLORS,
    tooltip: buildTooltip(theme, formatter),
    legend: buildLegend(theme),
    grid: { left: 54, right: 18, top: 36, bottom: 36 },
    xAxis: buildXAxis(theme, labels),
    yAxis: {
      ...buildYAxis(theme),
      axisLabel: {
        color: theme.textColor,
        fontSize: 11,
        formatter: (value: number) => `${formatScaledThroughput(value, scale.base)} ${scale.unit}`,
      },
    },
    dataZoom: buildDataZoom(),
    series: [
      {
        name: seriesNames.read,
        type: 'line',
        smooth: true,
        data: diskRead,
        symbol: 'none',
        lineStyle: { width: 1.5 },
      },
      {
        name: seriesNames.write,
        type: 'line',
        smooth: true,
        data: diskWrite,
        symbol: 'none',
        lineStyle: { width: 1.5 },
      },
    ],
  }
}

/**
 * 构建网络吞吐趋势图配置。
 */
export function buildNetworkChartOptions(
  labels: string[],
  netRx: number[],
  netTx: number[],
  scale: ThroughputScale,
  theme: ChartTheme,
  seriesNames: { rx: string; tx: string },
): Record<string, unknown> {
  const formatter = (value: unknown) =>
    `${formatScaledThroughput(Number(value ?? 0), scale.base)} ${scale.unit}`
  return {
    color: [CHART_COLORS[2], CHART_COLORS[3]],
    tooltip: buildTooltip(theme, formatter),
    legend: buildLegend(theme),
    grid: { left: 54, right: 18, top: 36, bottom: 36 },
    xAxis: buildXAxis(theme, labels),
    yAxis: {
      ...buildYAxis(theme),
      axisLabel: {
        color: theme.textColor,
        fontSize: 11,
        formatter: (value: number) => `${formatScaledThroughput(value, scale.base)} ${scale.unit}`,
      },
    },
    dataZoom: buildDataZoom(),
    series: [
      {
        name: seriesNames.rx,
        type: 'line',
        smooth: true,
        data: netRx,
        symbol: 'none',
        lineStyle: { width: 1.5 },
      },
      {
        name: seriesNames.tx,
        type: 'line',
        smooth: true,
        data: netTx,
        symbol: 'none',
        lineStyle: { width: 1.5 },
      },
    ],
  }
}

/**
 * 构建 ECharts Gauge 仪表盘配置。
 *
 * @param value 当前值（0-100 百分比，或负载值）。
 * @param max 最大值。
 * @param unit 单位后缀（如 '%' 或空字符串）。
 * @param warningThreshold 警告阈值。
 * @param dangerThreshold 危险阈值。
 */
export function buildGaugeOptions(
  value: number,
  max: number,
  unit: string,
  warningThreshold: number,
  dangerThreshold: number,
): Record<string, unknown> {
  const warningRatio = warningThreshold / max
  const dangerRatio = dangerThreshold / max

  return {
    series: [
      {
        type: 'gauge',
        startAngle: 220,
        endAngle: -40,
        radius: '90%',
        center: ['50%', '58%'],
        min: 0,
        max,
        splitNumber: 5,
        progress: {
          show: true,
          width: 8,
          roundCap: true,
          itemStyle: {
            color: {
              type: 'linear',
              x: 0,
              y: 0,
              x2: 1,
              y2: 0,
              colorStops: [
                { offset: 0, color: '#00C8FF' },
                {
                  offset: Math.min(warningRatio, 1),
                  color: value >= warningThreshold ? '#FFB547' : '#00C8FF',
                },
                {
                  offset: Math.min(dangerRatio, 1),
                  color: value >= dangerThreshold ? '#FF5E7A' : '#00C8FF',
                },
                { offset: 1, color: value >= dangerThreshold ? '#FF5E7A' : '#00C8FF' },
              ],
            },
          },
        },
        axisLine: {
          lineStyle: {
            width: 8,
            color: [[1, 'rgba(148, 163, 184, 0.1)']],
          },
          roundCap: true,
        },
        axisTick: { show: false },
        splitLine: { show: false },
        axisLabel: { show: false },
        pointer: { show: false },
        anchor: { show: false },
        title: { show: false },
        detail: {
          valueAnimation: true,
          fontSize: 22,
          fontWeight: 700,
          fontFamily: 'var(--sdl-font-mono, monospace)',
          color:
            value >= dangerThreshold
              ? '#FF5E7A'
              : value >= warningThreshold
                ? '#FFB547'
                : '#E6EDF7',
          offsetCenter: [0, 0],
          formatter: (val: number) => {
            if (unit === '%') return `${val.toFixed(1)}${unit}`
            return val.toFixed(2)
          },
        },
        data: [{ value }],
      },
    ],
  }
}
