import { computed, ref, watch } from 'vue'
import { useWebSocket } from '@/composables/useWebSocket'
import { useNodeStore } from '@/stores/node'
import type {
  NetworkConnection,
  NetworkSummary,
  ProcessItem,
  ProcessManagerActiveView,
  ProcessManagerServerMessage,
  SignalResult,
} from '@/api/generated'

type ProcessSignal = 'TERM' | 'KILL'

const buildProcessManagerWsUrl = (nodeId?: string) => {
  const base = '/api/v1'
  const path =
    nodeId && nodeId !== 'local'
      ? `/node/${encodeURIComponent(nodeId)}/agent/websocket/process-manager/ws`
      : '/agent/websocket/process-manager/ws'

  if (base.startsWith('http')) {
    const url = new URL(base)
    url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
    url.pathname = url.pathname.replace(/\/$/, '') + path
    return url.toString()
  }

  const origin = window.location.origin.replace(/^http/, 'ws')
  const prefix = base ? base.replace(/\/$/, '') : '/api/v1'
  return `${origin}${prefix}${path}`
}

const createRequestId = () => {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return crypto.randomUUID()
  }
  return `process-${Date.now()}-${Math.random().toString(16).slice(2)}`
}

export const useProcessManagerWs = () => {
  const nodeStore = useNodeStore()
  const wsUrl = computed(() => buildProcessManagerWsUrl(nodeStore.currentNodeId))

  const processRows = ref<ProcessItem[]>([])
  const networkRows = ref<NetworkConnection[]>([])
  const networkSummary = ref<NetworkSummary | null>(null)
  const processLoaded = ref(false)
  const networkLoaded = ref(false)
  const activeView = ref<ProcessManagerActiveView>('process')
  const lastSignalResult = ref<SignalResult | null>(null)
  const lastProtocolError = ref<string | null>(null)

  watch(wsUrl, () => {
    processRows.value = []
    networkRows.value = []
    networkSummary.value = null
    processLoaded.value = false
    networkLoaded.value = false
    lastSignalResult.value = null
    lastProtocolError.value = null
  })

  let sendActiveViewMessage: (view: ProcessManagerActiveView) => void = () => {}

  const { connect, disconnect, send, connected, connecting, lastError } = useWebSocket({
    url: wsUrl,
    autoConnect: true,
    autoReconnect: true,
    reconnectInterval: 3000,
    onOpen: () => {
      sendActiveViewMessage(activeView.value)
    },
    onMessage: (event) => {
      try {
        const message = JSON.parse(event.data) as ProcessManagerServerMessage
        switch (message.kind) {
          case 'processSnapshot':
            processRows.value = message.payload.processes
            processLoaded.value = true
            break
          case 'networkSnapshot':
            networkRows.value = message.payload.connections
            networkSummary.value = message.payload.summary
            networkLoaded.value = true
            break
          case 'signalResult':
            lastSignalResult.value = message.payload
            break
          case 'error':
            lastProtocolError.value = message.payload.message
            break
          case 'heartbeat':
            break
        }
      } catch (err) {
        lastProtocolError.value =
          err instanceof Error ? err.message : 'failed to parse process manager message'
      }
    },
  })

  sendActiveViewMessage = (view: ProcessManagerActiveView) => {
    if (!connected.value) return
    send(
      JSON.stringify({
        type: 'setActiveView',
        payload: view,
      }),
    )
  }

  const sendSignal = (pid: number, signal: ProcessSignal) => {
    if (!connected.value) {
      connect()
      return null
    }
    const requestId = createRequestId()
    send(
      JSON.stringify({
        type: 'sendSignal',
        payload: {
          requestId,
          pid,
          signal,
        },
      }),
    )
    return requestId
  }

  const setActiveView = (view: ProcessManagerActiveView) => {
    activeView.value = view
    sendActiveViewMessage(view)
  }

  return {
    activeView,
    connected,
    connecting,
    disconnect,
    lastError,
    lastProtocolError,
    lastSignalResult,
    networkLoaded,
    networkRows,
    networkSummary,
    processLoaded,
    processRows,
    sendSignal,
    setActiveView,
  }
}
