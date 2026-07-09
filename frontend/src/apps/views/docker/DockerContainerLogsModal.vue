<script setup lang="ts">
import { SecLabButton, SecLabTabs, SecLabLoading } from '@/components/ui'
import { onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useContainerLogs, type LogMode } from './composables/useContainerLogs'
import { ansiToHtml } from '@/utils/ansi'

/**
 * @file DockerContainerLogsModal.vue
 * @description 一个用于显示 Docker 容器日志的模态框组件，严格遵循 SDL 设计规范。
 * 内部逻辑委托给 `useContainerLogs` composable。
 */

// --- Component Props and Emits ---

const props = defineProps<{
  /**
   * 控制模态框是否可见。
   */
  visible: boolean
  /**
   * 要显示日志的容器 ID。
   */
  containerId: string | null
  /**
   * 要显示日志的容器名称（用于 UI 展示）。
   */
  containerName: string | null
}>()

const emit = defineEmits<{
  (e: 'close'): void
}>()

const { t } = useI18n()

// --- Composable ---

/** 将 visible prop 映射为 active ref，驱动 composable 的启停 */
const active = ref(props.visible)

/** 将 containerId prop 映射为响应式 ref */
const containerIdRef = ref(props.containerId)

const {
  logLines,
  isLoading,
  logMode,
  logModeTabs,
  statusText,
  autoScrollEl,
  stopRealtime,
  loadLatestLogs,
  switchMode,
  clearLogs,
  streamEnded,
} = useContainerLogs({
  containerId: containerIdRef,
  active,
})

// --- Modal-specific Logic ---

/**
 * 监听模态框的可见性。
 * 当模态框变为可见时，同步 active 并启动实时日志。
 * 当模态框关闭时，停止实时日志并清空日志内容。
 */
watch(
  () => props.visible,
  (visible) => {
    active.value = visible
    if (visible) {
      logMode.value = 'realtime'
      streamEnded.value = false
    } else {
      clearLogs()
    }
  },
)

/**
 * 监听 `containerId` 的变化，同步到 composable 的 ref。
 */
watch(
  () => props.containerId,
  (newId) => {
    containerIdRef.value = newId
  },
)

/**
 * 处理日志模式切换。
 */
const handleModeChange = (mode: LogMode) => {
  switchMode(mode)
}

/**
 * 处理模态框关闭事件。
 */
const handleClose = () => {
  stopRealtime()
  emit('close')
}

/**
 * 在组件卸载前，确保取消订阅。
 */
onBeforeUnmount(() => {
  stopRealtime()
})
</script>

<template>
  <div v-if="visible" class="modal-overlay log-modal-overlay">
    <div class="modal-container container-log-modal">
      <div class="modal-header">
        <button class="modal-close-dot" :title="t('common.close')" @click="handleClose"></button>
        <h3>
          {{
            t('app.docker.containers.logsPanel.modalTitle', {
              name:
                containerName ||
                containerId ||
                t('app.docker.containers.logsPanel.unknownContainer'),
            })
          }}
        </h3>
      </div>
      <div class="log-toolbar">
        <SecLabTabs
          :model-value="logMode"
          class="log-modes"
          :tabs="logModeTabs"
          @update:model-value="(val: any) => handleModeChange(val)"
        />
        <div class="log-actions">
          <span class="log-status">{{ statusText }}</span>
          <SecLabButton
            v-if="logMode === 'latest'"
            size="small"
            type="primary"
            :loading="isLoading"
            @click="loadLatestLogs"
          >
            {{ t('app.docker.containers.logsPanel.manualRefresh') }}
          </SecLabButton>
        </div>
      </div>
      <div class="log-content-area">
        <SecLabLoading :loading="isLoading" />
        <div ref="autoScrollEl" class="log-lines-wrapper" data-native-context-menu>
          <pre v-if="logLines.length" class="log-lines"><span
              v-for="(line, idx) in logLines"
              :key="idx"
              v-html="ansiToHtml(line)"
            ></span></pre>
          <div v-else class="log-empty">{{ t('app.docker.containers.logsPanel.empty') }}</div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.log-modal-overlay {
  z-index: var(--sdl-z-index-modal);
  position: fixed;
  inset: 0;
  background-color: var(--sdl-bg-backdrop);
  display: flex;
  justify-content: center;
  align-items: center;
}

.container-log-modal {
  width: 960px;
  max-width: 95vw;
  max-height: 90vh;
  display: flex;
  flex-direction: column;
  background-color: var(--sdl-bg-panel);
  border: 1px solid var(--sdl-border-strong);
  border-radius: var(--sdl-radius-lg);
  box-shadow: var(--sdl-shadow-window);
  overflow: hidden;
  animation: sl-modal-pop 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
}

.modal-header {
  padding: var(--sdl-space-4) var(--sdl-space-5);
  border-bottom: 1px solid var(--sdl-border-subtle);
  background: var(--sdl-bg-muted);
  display: flex;
  align-items: center;
  justify-content: center;
  position: relative;
}
.modal-header h3 {
  font-size: var(--sdl-font-subtitle);
  font-weight: 600;
  color: var(--sdl-text-primary);
  margin: 0;
}

.modal-close-dot {
  position: absolute;
  left: var(--sdl-space-5);
  top: 50%;
  transform: translateY(-50%);
  width: 12px;
  height: 12px;
  background-color: var(--sdl-danger);
  border-radius: 50%;
  border: none;
  cursor: pointer;
  padding: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 0;
  transition: opacity 0.2s ease;
}
.modal-close-dot:hover {
  opacity: 0.8;
}

.log-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--sdl-space-3) var(--sdl-space-5);
  background-color: var(--sdl-bg-panel);
  border-bottom: 1px solid var(--sdl-border-subtle);
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

.log-actions {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-3);
}

.log-status {
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-caption);
}

.log-content-area {
  flex: 1;
  min-height: 420px;
  margin: var(--sdl-space-5);
  position: relative;
  display: flex;
  flex-direction: column;
}

.log-lines-wrapper {
  flex: 1;
  background: var(--sdl-bg-canvas);
  color: var(--sdl-text-secondary);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  padding: var(--sdl-space-4);
  overflow: auto;
  scrollbar-width: thin;
}

.log-lines-wrapper::-webkit-scrollbar {
  width: 6px;
  height: 6px;
}

.log-lines-wrapper::-webkit-scrollbar-track {
  background: transparent;
}

.log-lines-wrapper::-webkit-scrollbar-thumb {
  background-color: var(--sdl-scrollbar-thumb);
  border-radius: var(--sdl-radius-pill);
}

.log-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--sdl-text-muted);
}

.log-lines {
  margin: 0;
  white-space: pre-wrap;
  font-family: var(--sdl-font-mono);
  font-size: var(--sdl-font-code);
  line-height: 1.6;
}

.log-lines span {
  display: block;
}

@keyframes sl-modal-pop {
  from {
    transform: scale(0.95);
    opacity: 0;
  }
  to {
    transform: scale(1);
    opacity: 1;
  }
}
</style>
