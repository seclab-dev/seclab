<script setup lang="ts">
import { SecLabButton, SecLabTabs, SecLabLoading } from '@/components/ui'
import { computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useContainerLogs, type LogMode } from './composables/useContainerLogs'

/**
 * @file DockerContainerLogsPanel.vue
 * @description 容器日志面板组件，嵌入在详情视图中使用。
 * 内部逻辑委托给 `useContainerLogs` composable。
 */

const props = defineProps<{
  containerId: string | null
  containerName?: string | null
  active?: boolean
}>()

const emit = defineEmits<{
  (e: 'activeChange', value: boolean): void
}>()

const { t } = useI18n()

// --- Composable ---

/** 将 props 映射为响应式 ref / computed，驱动 composable */
const containerIdRef = computed(() => props.containerId)
const activeRef = computed(() => props.active ?? false)

const {
  logLines,
  logText,
  isLoading,
  logMode,
  logModeTabs,
  statusText,
  autoScrollEl,
  realtimeSubscribed,
  loadLatestLogs,
  switchMode,
} = useContainerLogs({
  containerId: containerIdRef,
  active: activeRef,
})

// --- Panel-specific Logic ---

/**
 * 处理日志模式切换。
 */
const handleModeChange = (mode: LogMode) => {
  switchMode(mode)
}

/**
 * 刷新最新日志（仅 latest 模式下可用）。
 */
const refreshLatest = () => {
  if (logMode.value === 'latest') {
    void loadLatestLogs()
  }
}

/**
 * 将 realtimeSubscribed 状态同步到父组件的 activeChange 事件。
 */
watch(
  realtimeSubscribed,
  (value) => {
    emit('activeChange', value)
  },
  { immediate: true },
)
</script>

<template>
  <div class="container-logs-panel">
    <div class="logs-header">
      <div class="logs-title">
        <span class="logs-name">{{ t('app.docker.containers.logsPanel.title') }}</span>
        <span class="logs-subtitle">{{ props.containerName || props.containerId || '' }}</span>
      </div>
      <div class="logs-actions">
        <span class="logs-status">{{ statusText }}</span>
        <SecLabTabs
          :model-value="logMode"
          class="log-modes"
          :tabs="logModeTabs"
          @update:model-value="(val: any) => handleModeChange(val)"
        />
        <SecLabButton
          size="small"
          :disabled="logMode !== 'latest'"
          :loading="isLoading"
          @click="refreshLatest"
        >
          {{ t('common.refresh') }}
        </SecLabButton>
      </div>
    </div>

    <div class="logs-body">
      <SecLabLoading :loading="isLoading" />
      <div class="log-scroll" data-native-context-menu>
        <pre v-if="logLines.length" ref="autoScrollEl" class="log-content">{{ logText }}</pre>
        <div v-else class="log-empty">{{ t('app.docker.containers.logsPanel.empty') }}</div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.container-logs-panel {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
  flex: 1;
  min-height: 0;
}

.logs-header {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-2);
}

.logs-title {
  display: flex;
  align-items: baseline;
  gap: var(--sdl-space-3);
  flex-wrap: wrap;
}

.logs-name {
  font-weight: 700;
  color: var(--sdl-primary);
  font-size: var(--sdl-font-subtitle);
}

.logs-subtitle {
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-body-sm);
  word-break: break-all;
}

.logs-actions {
  display: flex;
  gap: var(--sdl-space-2);
  align-items: center;
  flex-wrap: wrap;
}

.log-modes {
  width: auto;
}

.log-modes :deep(.sl-tabs-nav) {
  border-bottom: none;
  height: 32px;
}

.log-modes :deep(.sl-tabs-item) {
  height: 32px;
  font-size: var(--sdl-font-body-sm);
}

.logs-status {
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-caption);
  margin-right: var(--sdl-space-2);
}

.logs-body {
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  background: var(--sdl-bg-canvas);
  color: var(--sdl-text-secondary);
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
  overflow: hidden;
  position: relative;
}

.log-content {
  margin: 0;
  padding: var(--sdl-space-4);
  font-family: var(--sdl-font-mono);
  font-size: var(--sdl-font-code);
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-all;
  flex: 1;
  min-height: 0;
  height: 100%;
  max-height: 100%;
  overflow: auto;
  scrollbar-width: thin;
}

.log-content::-webkit-scrollbar {
  width: 6px;
  height: 6px;
}

.log-content::-webkit-scrollbar-track {
  background: transparent;
}

.log-content::-webkit-scrollbar-thumb {
  background-color: var(--sdl-scrollbar-thumb);
  border-radius: var(--sdl-radius-pill);
}

.log-scroll {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.log-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-body-sm);
}
</style>
