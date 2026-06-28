<script setup lang="ts">
/**
 * @file TerminalView.vue
 * @description SecLab 平台自研远程宿主机伪终端（PTY）交互系统，支持全功能 Bash、Ctrl+C、Tab 补全与 Resize 宽高自适应。
 * 遵循多终端物理隔离锁定原则，并在首次连接前要求用户手动确认以保障生产安全。
 */

import { computed, ref, onMounted, onBeforeUnmount, watch, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'
import { useNodeStore } from '@/stores/node'
import { useWindowManagerStore } from '@/stores/window-manager'
import { useThemeStore, buildTerminalTheme } from '@/stores/theme'

import { systemApi } from '@/api/modules/system'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { SecLabButton } from '@/components/ui'
import '@xterm/xterm/css/xterm.css'

const themeStore = useThemeStore()

const props = defineProps<{
  isMaximized?: boolean
  windowId?: string
  payload?: Record<string, unknown>
}>()

const nodeStore = useNodeStore()
const windowStore = useWindowManagerStore()
const { t } = useI18n()
const terminalContainer = ref<HTMLDivElement | null>(null)

let term: Terminal | null = null
let fitAddon: FitAddon | null = null
let ws: WebSocket | null = null
const sessionId = ref<string | null>(null)

// A 方案：锁定打开瞬间的节点 ID 与名称，防止全局活跃节点切换导致终端异常关闭或越权切换
const targetNodeId = ref<string>('local')
const targetNodeName = ref<string>(t('app.terminal.localController'))

// B 方案：终端连接的声明周期与状态管控
const isConnected = ref(false)
const isConnecting = ref(false)
const connectError = ref<string | null>(null)
const hasActiveTerminalSession = computed(() => isConnected.value || isConnecting.value)

watch(
  () => themeStore.currentTheme,
  (newTheme) => {
    if (term) {
      term.options.theme = buildTerminalTheme(newTheme)
    }
  },
)

/**
 * 动态根据锁定的节点 ID 构造 WebSocket 信道路径
 */
const buildWsUrl = () => {
  const base = '/api/v1'
  const path = systemApi.getTerminalWsPath(targetNodeId.value)
  const origin = window.location.origin.replace(/^http/, 'ws')
  return `${origin}${base}${path}`
}

/**
 * 安全触发并咬合物理终端 WebSocket 信道
 */
const connectTerminal = async () => {
  if (isConnected.value || isConnecting.value) return
  isConnecting.value = true
  connectError.value = null

  try {
    initTerminal()
  } catch (err) {
    isConnecting.value = false
    connectError.value = t('app.terminal.initFailed')
    console.error(err)
  }
}

/**
 * 初始化 xterm.js 引擎并建立 WS 信道咬合
 */
const initTerminal = () => {
  if (!terminalContainer.value) return

  // 清退并阻断已有旧会话
  cleanSession(true)

  // 1. 初始化 xterm.js 实例并注入极致暗黑极客美学样式
  term = new Terminal({
    cursorBlink: true,
    fontFamily: 'Fira Code, Monaco, Consolas, Courier New, monospace',
    fontSize: 13,
    theme: buildTerminalTheme(themeStore.currentTheme),
    allowProposedApi: true,
  })

  fitAddon = new FitAddon()
  term.loadAddon(fitAddon)
  term.open(terminalContainer.value)

  // 强制重新适应当前视口大小
  nextTick(() => {
    try {
      fitAddon?.fit()
    } catch {
      // 忽略不可见视图计算异常
    }
  })

  // 2. 握手并拉起 WebSocket 信道
  const wsUrl = buildWsUrl()
  ws = new WebSocket(wsUrl)

  ws.onopen = () => {
    isConnecting.value = false
    isConnected.value = true
    connectError.value = null

    const cols = term?.cols || 80
    const rows = term?.rows || 24

    // 握手包：下发启动命令以在节点 spawn 对应宽高的 bash 进程
    ws?.send(
      JSON.stringify({
        type: 'terminalStart',
        payload: {
          container_id: 'host',
          shell: 'bash',
          cols,
          rows,
        },
      }),
    )

    // 重新拉伸自适应
    setTimeout(handleResize, 100)
  }

  ws.onmessage = (event) => {
    try {
      const dataStr = event.data.toString().trim()
      // 防御反向代理物理阻断引发的非 JSON HTML/Text 报错 (例如 Vite devServer proxy 抛出的 "Proxy error..."，或网关 502/504 等 HTML 报错)
      if (
        dataStr.startsWith('Proxy error') ||
        dataStr.includes('Proxy error') ||
        (!dataStr.startsWith('{') && !dataStr.startsWith('['))
      ) {
        isConnected.value = false
        isConnecting.value = false
        connectError.value = t('app.terminal.proxyBlocked', {
          message: dataStr.slice(0, 150),
        })
        cleanSession()
        return
      }

      const parsed = JSON.parse(dataStr)
      if (parsed.kind === 'terminalStarted') {
        sessionId.value = parsed.payload.session_id
        term?.focus()
      } else if (parsed.kind === 'terminalOutput') {
        term?.write(parsed.payload.data)
      } else if (parsed.kind === 'terminalExit') {
        term?.write(`\r\n\x1b[31m[${t('app.terminal.sessionTerminated')}]\x1b[0m\r\n`)
        cleanSession()
      } else if (parsed.kind === 'terminalError') {
        term?.write(`\r\n\x1b[31m[${t('common.error')}]: ${parsed.payload.message}\x1b[0m\r\n`)
      }
    } catch (e) {
      console.error('Failed to parse terminal socket frame:', e)
      isConnected.value = false
      isConnecting.value = false
      const rawMsg = event.data.toString()
      connectError.value = t('app.terminal.nonJsonBlocked', {
        message: rawMsg.slice(0, 100),
      })
      cleanSession()
    }
  }

  ws.onclose = () => {
    term?.write('\r\n\x1b[33m[Connection Closed]\x1b[0m\r\n')
    cleanSession()
  }

  ws.onerror = () => {
    term?.write('\r\n\x1b[31m[Connection Error]\x1b[0m\r\n')
    isConnected.value = false
    isConnecting.value = false
    if (targetNodeId.value === 'local') {
      connectError.value = t('app.terminal.websocketErrorLocal')
    } else {
      connectError.value = t('app.terminal.websocketErrorEdge')
    }
  }

  // 3. 拦截物理键盘数据流发往后端 PTY
  term.onData((data) => {
    if (ws && ws.readyState === WebSocket.OPEN && sessionId.value) {
      ws.send(
        JSON.stringify({
          type: 'terminalInput',
          payload: {
            session_id: sessionId.value,
            data,
          },
        }),
      )
    }
  })

  // 4. 监听视口 Resize 大小变更通知底层 PTY 同步更新 Winsize
  term.onResize((size) => {
    if (ws && ws.readyState === WebSocket.OPEN && sessionId.value) {
      ws.send(
        JSON.stringify({
          type: 'terminalResize',
          payload: {
            session_id: sessionId.value,
            cols: size.cols,
            rows: size.rows,
          },
        }),
      )
    }
  })
}

/**
 * 优雅卸载与连接切断
 */
const cleanSession = (keepConnectingState = false) => {
  if (ws) {
    if (ws.readyState === WebSocket.OPEN && sessionId.value) {
      try {
        ws.send(
          JSON.stringify({
            type: 'terminalClose',
            payload: { session_id: sessionId.value },
          }),
        )
      } catch {
        // 忽略可能已关闭的 WebSocket 写入错误
      }
    }
    ws.close()
    ws = null
  }
  sessionId.value = null
  isConnected.value = false
  if (!keepConnectingState) {
    isConnecting.value = false
  }
  if (term) {
    term.dispose()
    term = null
  }
  fitAddon = null
}

const handleResize = () => {
  nextTick(() => {
    if (fitAddon && term && isConnected.value) {
      try {
        fitAddon.fit()
      } catch {
        // 捕获不可见状态下 fit 抛出的错误
      }
    }
  })
}

// 侦测最大化和窗口大小变动，自适应拉伸
watch(
  () => isConnected.value,
  () => {
    if (isConnected.value) {
      setTimeout(handleResize, 150)
    }
  },
)

watch(
  hasActiveTerminalSession,
  (active) => {
    if (!props.windowId) return
    windowStore.updateWindowRuntimeState(props.windowId, {
      activeSession: active,
      allowsNodeSwitch: false,
      blockLevel: active ? 'active' : 'open',
      blockReason: active ? t('app.terminal.guardActive') : t('app.terminal.guardOpen'),
    })
  },
  { immediate: true },
)

onMounted(() => {
  // A 方案：锁定挂载瞬间的节点 ID，解除与 nodeStore 全局活跃节点切换的联动
  targetNodeId.value = nodeStore.currentNodeId || 'local'
  const activeNode = nodeStore.nodes.find((n) => n.id === targetNodeId.value)
  targetNodeName.value =
    targetNodeId.value === 'local'
      ? t('app.terminal.localController')
      : activeNode?.name || targetNodeId.value

  window.addEventListener('resize', handleResize)
})

onBeforeUnmount(() => {
  cleanSession()
  window.removeEventListener('resize', handleResize)
  if (term) {
    term.dispose()
    term = null
  }
})
</script>

<template>
  <div class="terminal-app flex-column" data-page="terminal">
    <!-- 1. 待命确认与断联重连卡片 (未连接状态下呈现) -->
    <div
      v-show="!isConnected"
      class="terminal-connect-panel flex-column align-center justify-center"
    >
      <div class="connect-card flex-column">
        <div class="connect-header">
          <div class="terminal-pulse" :class="{ 'is-connecting': isConnecting }"></div>
          <h3>{{ t('app.terminal.connectTitle') }}</h3>
        </div>

        <div class="connect-body">
          <div class="connect-item">
            <span class="label">{{ t('app.terminal.targetNode') }}</span>
            <span class="value code-text">{{ targetNodeName }}</span>
          </div>
          <div class="connect-item">
            <span class="label">{{ t('app.terminal.channel') }}</span>
            <span class="value">{{ t('app.terminal.channelValue') }}</span>
          </div>
          <div class="connect-item">
            <span class="label">{{ t('app.terminal.ptyDevice') }}</span>
            <span class="value">{{ t('app.terminal.ptyDeviceValue') }}</span>
          </div>

          <!-- 网络或者连接错误信息提示 -->
          <div v-if="connectError" class="connect-error-alert">⚠️ {{ connectError }}</div>
        </div>

        <div class="connect-footer">
          <SecLabButton
            type="primary"
            class="connect-btn"
            :loading="isConnecting"
            :disabled="isConnecting || isConnected"
            @click="connectTerminal"
          >
            {{ isConnecting ? t('app.terminal.connecting') : t('app.terminal.connectAction') }}
          </SecLabButton>
        </div>
      </div>
    </div>

    <!-- 2. xterm 终端物理画布 (一直存在于 DOM 中以确保 layout ref, 连接成功后流畅浮现) -->
    <div v-show="isConnected" class="terminal-canvas-wrapper flex-1">
      <div class="terminal-session-toolbar">
        <SecLabButton size="small" type="danger" @click="() => cleanSession()">
          {{ t('app.terminal.disconnectSession') }}
        </SecLabButton>
      </div>
      <div ref="terminalContainer" class="terminal-canvas" data-native-context-menu></div>
    </div>
  </div>
</template>

<style scoped>
.terminal-app {
  height: 100%;
  background-color: var(--sdl-bg-canvas);
  overflow: hidden;
  position: relative;
  display: flex;
  flex-direction: column;
}

/* 待命连接面板样式 */
.terminal-connect-panel {
  flex: 1;
  width: 100%;
  height: 100%;
  background-color: var(--sdl-bg-canvas); /* 与终端画布底色完全一致，确保极其顺畅的转场体验 */
  padding: 0;
  box-sizing: border-box;
}

.connect-card {
  width: 100%;
  height: 100%;
  background-color: var(--sdl-bg-canvas);
  border: none;
  border-radius: 0;
  box-shadow: none;
  padding: var(--sdl-space-8) var(--sdl-space-6);
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  justify-content: center;
  align-items: center;
}

/* 限制内部块最大宽度，保障大屏全屏拉伸时的内容集中与精美排布 */
.connect-header,
.connect-body,
.connect-footer {
  width: 100%;
  max-width: 460px;
}

.connect-header {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-3);
  margin-bottom: var(--sdl-space-5);
  border-bottom: 1px solid var(--sdl-border-subtle);
  padding-bottom: var(--sdl-space-3);
}

.connect-header h3 {
  margin: 0;
  font-size: var(--sdl-font-subtitle);
  color: var(--sdl-text-primary);
  font-weight: 600;
}

.terminal-pulse {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background-color: var(--sdl-warning);
  box-shadow: 0 0 8px var(--sdl-warning);
  transition: all 0.3s ease;
}

.terminal-pulse.is-connecting {
  background-color: var(--sdl-info);
  box-shadow: 0 0 8px var(--sdl-info);
  animation: pulse-glow 1.5s infinite ease-in-out;
}

@keyframes pulse-glow {
  0% {
    transform: scale(1);
    opacity: 0.8;
  }
  50% {
    transform: scale(1.35);
    opacity: 0.3;
  }
  100% {
    transform: scale(1);
    opacity: 0.8;
  }
}

.connect-body {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
  margin-bottom: var(--sdl-space-6);
}

.connect-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: var(--sdl-font-body-sm);
}

.connect-item .label {
  color: var(--sdl-text-secondary);
}

.connect-item .value {
  color: var(--sdl-text-primary);
  font-weight: 500;
}

.connect-item .code-text {
  font-family: monospace;
  background-color: var(--sdl-bg-muted);
  padding: 2px 6px;
  border-radius: var(--sdl-radius-sm);
  border: 1px solid var(--sdl-border-subtle);
}

.connect-error-alert {
  background-color: rgba(248, 81, 73, 0.15);
  border: 1px solid var(--sdl-danger);
  color: var(--sdl-danger);
  padding: var(--sdl-space-3);
  border-radius: var(--sdl-radius-md);
  font-size: var(--sdl-font-body-sm);
  margin-top: var(--sdl-space-3);
  line-height: 1.4;
  text-align: left;
}

.connect-footer {
  display: flex;
  justify-content: center;
}

.connect-btn {
  width: 100%;
}

/* 终端画布容器 */
.terminal-canvas-wrapper {
  position: relative;
  width: 100%;
  height: 100%;
  padding: var(--sdl-space-2) var(--sdl-space-3);
  box-sizing: border-box;
  overflow: hidden;
  background-color: var(--sdl-bg-canvas);
}

.terminal-session-toolbar {
  position: absolute;
  top: var(--sdl-space-2);
  right: var(--sdl-space-3);
  z-index: 2;
}

.terminal-canvas {
  width: 100%;
  height: 100%;
  box-sizing: border-box;
}

/* 深度定制 xterm 原生样式 */
:deep(.xterm) {
  height: 100%;
  padding: 0;
}

:deep(.xterm-viewport) {
  background-color: var(--sdl-bg-canvas) !important;
  overflow-y: auto;
}

:deep(.xterm-viewport::-webkit-scrollbar) {
  width: 8px;
}

:deep(.xterm-viewport::-webkit-scrollbar-track) {
  background: var(--sdl-bg-canvas);
}

:deep(.xterm-viewport::-webkit-scrollbar-thumb) {
  background: var(--sdl-scrollbar-thumb);
  border-radius: 4px;
}

:deep(.xterm-viewport::-webkit-scrollbar-thumb:hover) {
  background: var(--sdl-scrollbar-thumb);
  filter: brightness(0.85);
}
</style>
