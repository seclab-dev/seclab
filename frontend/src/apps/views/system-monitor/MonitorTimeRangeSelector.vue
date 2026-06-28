<script setup lang="ts">
/**
 * @file MonitorTimeRangeSelector.vue
 * @description 时间范围选择器组件，支持预设时间段快速切换。
 */

import { useI18n } from 'vue-i18n'
import type { TimeRange } from '@/composables/useSystemMonitor'

defineProps<{
  /** 当前选中的时间范围 */
  modelValue: TimeRange
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', value: TimeRange): void
}>()

const { t } = useI18n()

/** 预设时间范围选项。 */
const rangeOptions: { value: TimeRange; label: string }[] = [
  { value: '1h', label: '1H' },
  { value: '6h', label: '6H' },
  { value: '24h', label: '24H' },
  { value: '3d', label: '3D' },
  { value: '7d', label: '7D' },
]
</script>

<template>
  <div class="time-range-selector" data-ui="time-range-selector">
    <span class="range-label">{{ t('app.systemMonitor.timeRange') }}</span>
    <div class="range-buttons">
      <button
        v-for="option in rangeOptions"
        :key="option.value"
        class="range-btn"
        :class="{ 'is-active': modelValue === option.value }"
        @click="emit('update:modelValue', option.value)"
      >
        {{ option.label }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.time-range-selector {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
}

.range-label {
  font-size: var(--sdl-font-caption);
  color: var(--sdl-text-muted);
  white-space: nowrap;
}

.range-buttons {
  display: flex;
  gap: 2px;
  background: var(--sdl-bg-muted);
  border-radius: var(--sdl-radius-md);
  padding: 2px;
}

.range-btn {
  padding: 4px 10px;
  border: none;
  border-radius: var(--sdl-radius-sm);
  background: transparent;
  color: var(--sdl-text-muted);
  font-size: 11px;
  font-weight: 600;
  font-family: var(--sdl-font-mono, monospace);
  cursor: pointer;
  transition:
    background-color 0.15s ease,
    color 0.15s ease;
  line-height: 1.4;
}

.range-btn:hover {
  color: var(--sdl-text-primary);
  background: var(--sdl-bg-hover);
}

.range-btn.is-active {
  background: var(--sdl-bg-active);
  color: var(--sdl-primary);
}
</style>
