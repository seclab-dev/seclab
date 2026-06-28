<script setup lang="ts">
import { SecLabButton, SecLabSelect } from '@/components/ui'
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useNotificationStore } from '@/stores/notification'
import { useDockerTerminalWs } from './useDockerTerminalWs'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { useI18n } from 'vue-i18n'
import '@xterm/xterm/css/xterm.css'

type ShellType = 'bash' | 'sh'

const props = defineProps<{
  containerId: string | null
  containerName?: string | null
  active?: boolean
}>()

const emit = defineEmits<{
  (e: 'activeChange', value: boolean): void
}>()

const notificationStore = useNotificationStore()
const { t } = useI18n()
const {
  connected,
  connecting,
  sessionId,
  lastError,
  lastTerminalMessage,
  openSession,
  writeInput,
  resizeTerminal,
  closeSession,
} = useDockerTerminalWs()

const selectedShell = ref<ShellType>('bash')
const actualShell = ref<ShellType | null>(null)
const terminalHost = ref<HTMLDivElement | null>(null)

let xterm: Terminal | null = null
let fitAddon: FitAddon | null = null
let resizeObserver: ResizeObserver | null = null
let dataDisposable: { dispose: () => void } | null = null
let resizeDisposable: { dispose: () => void } | null = null

const shellOptions = [
  { label: 'bash', value: 'bash' },
  { label: 'sh', value: 'sh' },
]

const getCssVar = (name: string) =>
  getComputedStyle(document.documentElement).getPropertyValue(name).trim()

const statusHint = computed(() => {
  if (!props.containerId) return t('app.docker.containers.terminalPanel.selectContainerFirst')
  if (connecting.value) return t('app.docker.containers.terminalPanel.connecting')
  if (sessionId.value && connected.value) {
    return t('app.docker.containers.terminalPanel.connectedWithShell', {
      shell: actualShell.value || 'bash',
    })
  }
  if (lastError.value) return lastError.value
  return t('app.docker.containers.terminalPanel.disconnected')
})

const isConnected = computed(() => !!sessionId.value && connected.value)
const hasActiveTerminalSession = computed(() => connecting.value || isConnected.value)

const mountTerminal = () => {
  if (!terminalHost.value || xterm) return
  xterm = new Terminal({
    cursorBlink: true,
    fontSize: 13,
    lineHeight: 1.2,
    convertEol: false,
    scrollback: 5000,
    theme: {
      background: getCssVar('--sdl-bg-canvas') || '#0f172a',
      foreground: getCssVar('--sdl-text-primary') || '#e2e8f0',
      cursor: getCssVar('--sdl-primary') || '#f8fafc',
      selectionBackground: getCssVar('--sdl-bg-hover') || '#334155',
    },
  })
  fitAddon = new FitAddon()
  xterm.loadAddon(fitAddon)
  xterm.open(terminalHost.value)
  fitAddon.fit()

  dataDisposable = xterm.onData((data) => {
    writeInput(data)
  })
  resizeDisposable = xterm.onResize(({ cols, rows }) => {
    resizeTerminal(cols, rows)
  })

  resizeObserver = new ResizeObserver(() => {
    if (!fitAddon || !xterm) return
    fitAddon.fit()
    resizeTerminal(xterm.cols, xterm.rows)
  })
  resizeObserver.observe(terminalHost.value)
}

const resetTerminalView = () => {
  if (!xterm) return
  xterm.clear()
  xterm.write('\x1b[2J\x1b[H')
}

const connectTerminal = () => {
  if (!props.containerId) {
    notificationStore.error(t('app.docker.containers.terminalPanel.missingContainerId'))
    return
  }
  if (!xterm || !fitAddon) return
  fitAddon.fit()
  resetTerminalView()
  xterm.writeln(
    t('app.docker.containers.terminalPanel.connectingToContainer', {
      name: props.containerName || props.containerId,
    }),
  )
  openSession({
    container_id: props.containerId,
    shell: selectedShell.value,
    cols: xterm.cols || 80,
    rows: xterm.rows || 24,
  })
}

const disconnectTerminal = () => {
  closeSession()
  if (xterm) {
    xterm.writeln(`\r\n${t('app.docker.containers.terminalPanel.disconnectedLine')}`)
  }
}

watch(
  () => props.active,
  (active) => {
    if (active) return
    closeSession()
  },
)

watch(
  hasActiveTerminalSession,
  (value) => {
    emit('activeChange', value)
  },
  { immediate: true },
)

watch(
  () => props.containerId,
  (next, prev) => {
    if (prev && prev !== next) {
      closeSession()
      actualShell.value = null
      if (xterm) {
        resetTerminalView()
      }
    }
  },
)

watch(
  () => lastTerminalMessage.value,
  (message) => {
    if (!message || !xterm) return
    if (message.kind === 'terminalStarted') {
      actualShell.value = message.payload.shell
      if (message.payload.shell !== selectedShell.value) {
        notificationStore.info(
          t('app.docker.containers.terminalPanel.shellFallback', {
            shell: message.payload.shell,
          }),
        )
      }
      xterm.writeln(
        t('app.docker.containers.terminalPanel.connectedLine', {
          shell: message.payload.shell,
        }),
      )
      return
    }
    if (message.kind === 'terminalOutput') {
      if (sessionId.value && sessionId.value !== message.payload.session_id) return
      xterm.write(message.payload.data)
      return
    }
    if (message.kind === 'terminalExit') {
      if (sessionId.value && sessionId.value !== message.payload.session_id) return
      const code = message.payload.exit_code
      xterm.writeln(
        `\r\n${t('app.docker.containers.terminalPanel.processExited', {
          code: code === null || code === undefined ? '-' : String(code),
        })}`,
      )
      closeSession()
      return
    }
    if (message.kind === 'terminalError') {
      notificationStore.error(
        message.payload.message || t('app.docker.containers.terminalPanel.terminalError'),
      )
      xterm.writeln(
        `\r\n${t('app.docker.containers.terminalPanel.errorLine', {
          message: message.payload.message || t('app.docker.containers.terminalPanel.unknownError'),
        })}`,
      )
    }
  },
)

onMounted(() => {
  mountTerminal()
})

onBeforeUnmount(() => {
  closeSession()
  dataDisposable?.dispose()
  resizeDisposable?.dispose()
  resizeObserver?.disconnect()
  xterm?.dispose()
  xterm = null
  fitAddon = null
})
</script>

<template>
  <div class="container-terminal-panel">
    <div class="terminal-toolbar">
      <div class="terminal-actions">
        <span
          class="terminal-status-dot"
          :class="{ 'is-online': isConnected }"
          :title="statusHint"
        />
        <SecLabSelect
          id="docker-terminal-shell"
          v-model="selectedShell"
          class="terminal-shell"
          size="small"
          :disabled="!!sessionId"
          :options="shellOptions"
        />
        <SecLabButton
          type="primary"
          size="small"
          :disabled="!props.containerId || !!sessionId || connecting"
          @click="connectTerminal"
        >
          {{ t('app.docker.containers.terminalPanel.connect') }}
        </SecLabButton>
        <SecLabButton size="small" :disabled="!sessionId" @click="disconnectTerminal">
          {{ t('app.docker.containers.terminalPanel.disconnect') }}
        </SecLabButton>
      </div>
    </div>
    <div class="terminal-host-container">
      <div ref="terminalHost" class="terminal-host" data-native-context-menu />
    </div>
  </div>
</template>

<style scoped>
.container-terminal-panel {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
  gap: var(--sdl-space-3);
}

.terminal-toolbar {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  gap: var(--sdl-space-4);
  flex-wrap: wrap;
}

.terminal-actions {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
  flex-wrap: wrap;
}

.terminal-status-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: var(--sdl-danger);
  box-shadow: 0 0 0 2px var(--sdl-bg-muted);
}

.terminal-status-dot.is-online {
  background: var(--sdl-success);
  box-shadow: 0 0 0 2px var(--sdl-bg-muted);
}

.terminal-shell {
  width: 80px;
  min-width: 80px;
}

.terminal-host-container {
  flex: 1;
  min-height: 300px;
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  overflow: hidden;
  background: var(--sdl-bg-canvas);
  padding: var(--sdl-space-2);
  box-sizing: border-box;
}

.terminal-host {
  width: 100%;
  height: 100%;
  box-sizing: border-box;
}

:deep(.xterm) {
  height: 100%;
  padding: 0;
}
</style>
