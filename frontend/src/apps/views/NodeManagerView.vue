/* 节点管理视图：展示节点列表，支持新增/编辑节点并进行预检与部署。 */
<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { upgradesApi, type UpgradeReleaseRecord } from '@/api/modules/upgrades'
import { DEFAULT_AGENT_PORT } from '@/utils/constants'
import {
  nodesApi,
  type NodeCheckResponse,
  type NodeDetailResponse,
  type NodePrecheckResponse,
} from '@/api/modules/nodes'
import { useNodeStore, type NodeStatus, type NodeSummary } from '@/stores/node'
import { useWindowManagerStore } from '@/stores/window-manager'
import SecLabTable, { type SecLabTableColumn } from '@/components/ui/SecLabTable.vue'
import {
  SecLabButton,
  SecLabCard,
  SecLabTag,
  SecLabDrawer,
  SecLabInput,
  SecLabSelect,
  SecLabAlert,
  SecLabFormItem,
  SecLabDescriptions,
  SecLabActionMenu,
  SecLabDialog,
  SecLabCheckbox,
} from '@/components/ui'
import { useNotificationStore } from '@/stores/notification'
import { useConfirmationModalStore } from '@/stores/confirmation-modal'
import { formatDateTime } from '@/utils/time'

const props = defineProps<{
  isMaximized?: boolean
  payload?: Record<string, unknown>
}>()

const router = useRouter()
const { t } = useI18n()
const nodeStore = useNodeStore()
const windowStore = useWindowManagerStore()
const notificationStore = useNotificationStore()
const confirmationModal = useConfirmationModalStore()

const nodes = computed(() => nodeStore.nodes)
const currentNodeId = computed(() => nodeStore.currentNodeId)
const selectedNodeId = computed({
  get: () => nodeStore.currentNodeId,
  set: (value) => {
    if (value === null) return
    void switchCurrentNode(String(value))
  },
})
const isCreateActive = ref(false)
const isEditActive = ref(false)
const editTarget = ref<NodeSummary | null>(null)
const precheckSubmitting = ref(false)
const editSubmitting = ref(false)
const editDetailLoading = ref(false)
const editError = ref('')
const isCheckDrawerVisible = ref(false)
const checkDetail = ref<NodeCheckResponse | null>(null)
const checkDetailTarget = ref<NodeSummary | null>(null)
const nodeDetailLoading = ref(false)
const selectedNodeDetail = ref<NodeDetailResponse | null>(null)
const precheckResult = ref<NodePrecheckResponse | null>(null)
const pendingDeployPayload = ref<Record<string, unknown> | null>(null)
const deployLogs = ref<string[]>([])
const deployError = ref('')
const isDeployDrawerVisible = ref(false)
const deployTarget = ref<NodeSummary | null>(null)
const deployPhase = ref<'precheck' | 'deploy' | null>(null)
const deployProgressPercent = ref(0)
let deployPollInterval: number | null = null
let deployOperationId: string | null = null
const countdownSeconds = ref(10)
let countdownTimer: number | null = null

const buildNodeSwitchBlockMessage = () => {
  const guard = windowStore.checkBeforeNodeSwitch()
  if (guard.allowed) return ''
  const details = guard.blockers
    .slice(0, 5)
    .map((item) => `${item.title}: ${item.reason}`)
    .join('；')
  const restCount = Math.max(0, guard.blockers.length - 5)
  const restText = restCount > 0 ? t('app.nodes.switchBlockedRest', { count: restCount }) : ''
  return t('app.nodes.switchBlocked', { details, rest: restText })
}

const switchCurrentNode = async (nodeId: string) => {
  if (nodeId === nodeStore.currentNodeId) return
  const guardMessage = buildNodeSwitchBlockMessage()
  if (guardMessage) {
    notificationStore.error(guardMessage)
    return
  }
  const result = nodeStore.requestSwitchCurrentNode(nodeId)
  if (!result.switched) {
    notificationStore.error(result.reason || t('app.nodes.switchFailed'))
    return
  }
  const targetNode = nodeStore.nodes.find((node) => node.id === nodeId)
  notificationStore.success(t('app.nodes.switchSuccess', { name: targetNode?.name || nodeId }))
}
const createForm = reactive({
  name: '',
  groupId: 'default',
  groupCustom: '',
  description: '',
  addr: '',
  port: '22',
  user: 'root',
  authMode: 'key',
  pwd: '',
  privateKey: '~/.ssh/id_ed25519',
  privateKeyPassphrase: '',
  tags: '',
  installDir: '/opt',
  servicePort: String(DEFAULT_AGENT_PORT),
  seclabUrl: '',
})
const editForm = reactive({
  name: '',
  groupId: 'default',
  groupCustom: '',
  tags: '',
  description: '',
  addr: '',
  port: '',
  user: 'root',
  authMode: 'key',
  pwd: '',
  privateKey: '~/.ssh/id_ed25519',
  privateKeyPassphrase: '',
  servicePort: '',
  seclabUrl: '',
})
let originalFormJson = ''
const checkingIds = ref<Set<string>>(new Set())
const deletingIds = ref<Set<string>>(new Set())
const refreshInterval = ref(5000)
const refreshOptions = computed(() => [
  { value: 0, label: t('app.nodes.refresh.off') },
  { value: 5000, label: t('app.nodes.refresh.seconds', { value: 5 }) },
  { value: 10000, label: t('app.nodes.refresh.seconds', { value: 10 }) },
  { value: 30000, label: t('app.nodes.refresh.seconds', { value: 30 }) },
])
const nodeDisplayName = (node: NodeSummary) => {
  if (node.id === 'local') return t('app.nodes.local')
  return node.name
}

const nodeOptions = computed(() =>
  nodes.value.map((node) => {
    const isAbnormal = node.status !== 'online'
    return {
      value: node.id,
      label: nodeDisplayName(node),
      disabled: isAbnormal,
    }
  }),
)

const statusLabel = (status: NodeStatus) => {
  switch (status) {
    case 'draft':
      return t('app.nodes.status.draft')
    case 'deploying':
      return t('app.nodes.status.deploying')
    case 'deploy_failed':
      return t('app.nodes.status.deployFailed')
    case 'awaiting_registration':
      return t('app.nodes.status.awaitingRegistration')
    case 'online':
      return t('app.nodes.status.online')
    case 'degraded':
      return t('app.nodes.status.degraded')
    case 'offline':
      return t('app.nodes.status.offline')
    case 'conflict':
      return t('app.nodes.status.conflict')
    case 'retired':
      return t('app.nodes.status.retired')
    default:
      return t('app.nodes.status.unknown')
  }
}

const statusTagType = (status: NodeStatus): 'success' | 'danger' | 'warning' | 'info' => {
  if (status === 'deploying' || status === 'awaiting_registration') return 'warning'
  if (status === 'online') return 'success'
  if (status === 'degraded') return 'warning'
  if (status === 'offline') return 'danger'
  return 'info'
}

const runtimeTagType = (status?: string): 'success' | 'warning' | 'info' => {
  const normalized = (status ?? '').toLowerCase()
  if (normalized === 'active') return 'success'
  if (!normalized) return 'info'
  return 'warning'
}

const healthTagType = (status?: string): 'success' | 'danger' | 'info' => {
  const normalized = (status ?? '').toLowerCase()
  if (normalized === 'online') return 'success'
  if (!normalized) return 'info'
  return 'danger'
}

const formatBytes = (bytes?: number) => {
  if (!bytes || bytes <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB']
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  const value = bytes / Math.pow(1024, index)
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[index]}`
}

const formatPercent = (value?: number) => {
  if (value === undefined || Number.isNaN(value)) return '--'
  return `${value.toFixed(1)}%`
}

const resourceLabel = (node: NodeSummary) => {
  const resource = node.metadata?.resource as Record<string, unknown> | undefined
  if (!resource) return 'CPU -- · MEM --'
  const cpu = resource.cpuPercent as number | undefined
  const memUsed = resource.memoryUsedBytes as number | undefined
  const memTotal = resource.memoryTotalBytes as number | undefined
  return `CPU ${formatPercent(cpu)} · MEM ${formatBytes(memUsed)}/${formatBytes(memTotal)}`
}

const checkDetailItems = computed(() => {
  if (!checkDetail.value) return []
  return [
    { key: 'ssh', label: t('app.nodes.check.checks.ssh'), item: checkDetail.value.ssh },
    { key: 'service', label: t('app.nodes.check.checks.service'), item: checkDetail.value.service },
    { key: 'api', label: t('app.nodes.check.checks.api'), item: checkDetail.value.api },
  ]
})

const checkItemDetail = (item: NodeCheckResponse['ssh']) => {
  const base = item.message || ''
  if (item.code === 'service_missing') {
    const hint = t('app.nodes.check.messages.serviceMissingHint')
    return base ? `${base} · ${hint}` : hint
  }
  return base
}

const checkStatusLabel = computed(() => {
  const status = (checkDetail.value?.status ?? '').toLowerCase()
  if (status === 'online') return t('app.nodes.check.statusOnline')
  if (status === 'offline') return t('app.nodes.check.statusOffline')
  return t('app.nodes.check.statusUnknown')
})

/** 节点基础详情定义 */
const nodeBaseDetailItems = computed(() => [
  { label: t('app.nodes.detail.lifecycleStatus'), slot: 'lifecycle' },
  { label: t('app.nodes.detail.runtimeStatus'), slot: 'runtime' },
  { label: t('app.nodes.detail.healthStatus'), slot: 'health' },
  { label: t('app.nodes.detail.lastSeenAt'), slot: 'lastSeen' },
  { label: t('app.nodes.detail.group'), slot: 'group' },
])

/** 节点部署详情定义 */
const nodeDeployDetailItems = computed(() => {
  const base = [
    { label: t('app.nodes.detail.deployMethod'), slot: 'deployMethod' },
    { label: t('app.nodes.detail.deployStatus'), slot: 'deployStatus' },
    { label: t('app.nodes.detail.sshTarget'), slot: 'sshTarget' },
    { label: t('app.nodes.detail.installDir'), slot: 'installDir' },
    { label: t('app.nodes.detail.deployAt'), slot: 'deployAt' },
  ]
  if (selectedNodeDetail.value?.provisioning?.lastDeployErrorSummary) {
    base.push({ label: t('app.nodes.detail.deployError'), slot: 'deployError' })
  }
  return base
})

/** 节点会话详情定义 */
const nodeSessionDetailItems = computed(() => [
  { label: t('app.nodes.detail.sessionId'), slot: 'sessionId' },
  { label: t('app.nodes.detail.registeredAt'), slot: 'registeredAt' },
  { label: t('app.nodes.detail.lastHeartbeatAt'), slot: 'lastHeartbeatAt' },
  { label: t('app.nodes.detail.leaseExpiresAt'), slot: 'leaseExpiresAt' },
  { label: t('app.nodes.detail.agentVersion'), slot: 'agentVersion' },
])

/** 批量升级选中的节点 ID 列表 */
const selectedNodeIds = ref<string[]>([])

/** 是否展示精细化升级引导 */
const showUpgradeGuide = ref(!!props.payload?.highlightUpgrade)

watch(
  () => props.payload,
  (newPayload) => {
    if (newPayload?.highlightUpgrade) {
      showUpgradeGuide.value = true
    }
  },
  { deep: true },
)

/** 判断节点是否允许独立升级（排除 local 节点，且必须在线） */
const isNodeUpgradable = (node: NodeSummary) => {
  return node.id !== 'local' && node.status === 'online'
}

/** 是否全部在线可升级节点均已选中 */
const isAllSelected = computed(() => {
  const upgradableNodes = nodes.value.filter(isNodeUpgradable)
  if (upgradableNodes.length === 0) return false
  return upgradableNodes.every((n) => selectedNodeIds.value.includes(n.id))
})

/** 全选/全取消 */
const handleSelectAll = (val: boolean) => {
  if (val) {
    const upgradableNodes = nodes.value.filter(isNodeUpgradable)
    selectedNodeIds.value = upgradableNodes.map((n) => n.id)
  } else {
    selectedNodeIds.value = []
  }
}

/** 单选/取消单选 */
const handleSelectChange = (nodeId: string, val: boolean) => {
  if (val) {
    if (!selectedNodeIds.value.includes(nodeId)) {
      selectedNodeIds.value.push(nodeId)
    }
  } else {
    selectedNodeIds.value = selectedNodeIds.value.filter((id) => id !== nodeId)
  }
}

/** 本地节点表格列定义 */
const columns = computed<SecLabTableColumn[]>(() => [
  { label: '', slot: 'selection', headerSlot: 'selectionHeader', width: 50, align: 'center' },
  { label: t('app.nodes.columns.name'), minWidth: 180, slot: 'name', align: 'center' },
  { prop: 'address', label: t('app.nodes.columns.address'), minWidth: 180, align: 'center' },
  { label: t('app.nodes.columns.status'), minWidth: 120, slot: 'status', align: 'center' },
  { label: t('app.nodes.columns.resources'), minWidth: 240, slot: 'resource', align: 'center' },
  { label: t('app.nodes.columns.tags'), minWidth: 180, slot: 'tags', align: 'center' },
  {
    label: t('app.nodes.columns.actions'),
    width: 140,
    fixed: 'right',
    slot: 'actions',
    align: 'center',
  },
])

const closeCheckDrawer = () => {
  isCheckDrawerVisible.value = false
}

const selectedRemoteNodeId = computed(() => {
  const value = selectedNodeId.value
  if (!value || value === 'local') return null
  return value
})

const selectedNodeName = computed(() => {
  const nodeId = selectedRemoteNodeId.value
  if (!nodeId) return ''
  const found = nodeStore.nodes.find((n) => n.id === nodeId)
  return found ? nodeDisplayName(found) : nodeId
})

const loadingNodeId = ref<string | null>(null)

const loadSelectedNodeDetail = async (silent = false) => {
  const nodeId = selectedRemoteNodeId.value
  if (!nodeId) {
    selectedNodeDetail.value = null
    return
  }
  if (loadingNodeId.value === nodeId) return
  loadingNodeId.value = nodeId
  if (!silent) {
    nodeDetailLoading.value = true
  }
  try {
    const res = await nodesApi.detail(nodeId)
    if (selectedRemoteNodeId.value === nodeId) {
      selectedNodeDetail.value = res.success && res.data ? res.data : null
    }
  } catch {
    if (selectedRemoteNodeId.value === nodeId) {
      selectedNodeDetail.value = null
    }
  } finally {
    if (loadingNodeId.value === nodeId) {
      loadingNodeId.value = null
      nodeDetailLoading.value = false
    }
  }
}

const fetchNodes = async () => {
  try {
    await nodeStore.refreshNodes()
    await loadSelectedNodeDetail()
  } catch {
    return
  }
}

const isChecking = (id?: string) => {
  if (!id) return false
  return checkingIds.value.has(id)
}

const setChecking = (id: string, active: boolean) => {
  const next = new Set(checkingIds.value)
  if (active) {
    next.add(id)
  } else {
    next.delete(id)
  }
  checkingIds.value = next
}

const isDeleting = (id?: string) => {
  if (!id) return false
  return deletingIds.value.has(id)
}

const setDeleting = (id: string, active: boolean) => {
  const next = new Set(deletingIds.value)
  if (active) {
    next.add(id)
  } else {
    next.delete(id)
  }
  deletingIds.value = next
}

const checkNode = async (node: NodeSummary) => {
  if (!node.id || node.id === 'local') return
  if (isChecking(node.id)) return
  setChecking(node.id, true)
  try {
    const res = await nodesApi.check(node.id)
    if (!res.success) {
      notificationStore.error(res.message || t('app.nodes.check.failed'))
      return
    }
    const payload = res.data as NodeCheckResponse | undefined
    if (!payload) {
      notificationStore.error(t('app.nodes.check.failed'))
      return
    }
    checkDetail.value = payload
    checkDetailTarget.value = node
    isCheckDrawerVisible.value = true
    await fetchNodes()
  } catch {
    notificationStore.error(t('app.nodes.check.failed'))
  } finally {
    setChecking(node.id, false)
  }
}

const startCreate = () => {
  isCreateActive.value = true
  isEditActive.value = false
  editTarget.value = null
  precheckResult.value = null
  pendingDeployPayload.value = null
  deployError.value = ''
}

const cancelCreate = () => {
  isCreateActive.value = false
  precheckResult.value = null
  pendingDeployPayload.value = null
}

const validateCreateForm = () => {
  if (createForm.addr.trim() === '') {
    notificationStore.error(t('app.nodes.create.addrRequired'))
    return false
  }
  if (createForm.user.trim() === '') {
    notificationStore.error(t('app.nodes.create.userRequired'))
    return false
  }
  if (createForm.authMode === 'password' && createForm.pwd.trim() === '') {
    notificationStore.error(t('app.nodes.create.passwordRequired'))
    return false
  }
  if (createForm.authMode === 'key' && createForm.privateKey.trim() === '') {
    notificationStore.error(t('app.nodes.create.privateKeyRequired'))
    return false
  }
  if (createForm.seclabUrl.trim() !== '') {
    try {
      const url = new URL(createForm.seclabUrl.trim())
      if (url.protocol !== 'https:') {
        notificationStore.error(t('app.nodes.create.seclabUrlHttpsRequired'))
        return false
      }
      if (url.pathname !== '/' || url.search !== '' || url.hash !== '') {
        notificationStore.error(t('app.nodes.create.seclabUrlBaseRequired'))
        return false
      }
    } catch {
      notificationStore.error(t('app.nodes.create.seclabUrlInvalid'))
      return false
    }
  }
  return true
}

const submitCreate = async () => {
  precheckSubmitting.value = true
  precheckResult.value = null
  deployError.value = ''
  if (!validateCreateForm()) {
    precheckSubmitting.value = false
    return
  }
  const groupId = createForm.groupId === 'custom' ? createForm.groupCustom : createForm.groupId
  const payload = {
    name: createForm.name || undefined,
    groupId: groupId || undefined,
    description: createForm.description || undefined,
    addr: createForm.addr || undefined,
    port: createForm.port || undefined,
    user: createForm.user || undefined,
    authMode: createForm.authMode || undefined,
    pwd: createForm.pwd || undefined,
    privateKey: createForm.privateKey || undefined,
    privateKeyPassphrase: createForm.privateKeyPassphrase || undefined,
    tags: createForm.tags
      ? createForm.tags
          .split(',')
          .map((item) => item.trim())
          .filter(Boolean)
      : undefined,
    installDir: createForm.installDir || undefined,
    servicePort: createForm.servicePort || undefined,
    seclabUrl: createForm.seclabUrl.trim() || undefined,
  }
  const precheckPayload = {
    name: createForm.name || undefined,
    addr: createForm.addr || undefined,
    port: createForm.port || undefined,
    user: createForm.user || undefined,
    authMode: createForm.authMode || undefined,
    pwd: createForm.authMode === 'password' ? createForm.pwd || undefined : undefined,
    privateKey: createForm.authMode === 'key' ? createForm.privateKey || undefined : undefined,
    privateKeyPassphrase:
      createForm.authMode === 'key' ? createForm.privateKeyPassphrase || undefined : undefined,
    servicePort: createForm.servicePort || undefined,
    installDir: createForm.installDir || undefined,
    seclabUrl: createForm.seclabUrl.trim() || undefined,
  }
  try {
    const res = await nodesApi.precheck(precheckPayload)
    if (!res.success || !res.data) {
      const message = res.message || t('app.nodes.precheck.failed')
      notificationStore.error(message)
      precheckSubmitting.value = false
      return
    }
    precheckResult.value = res.data
    pendingDeployPayload.value = res.data.passed ? payload : null
    isDeployDrawerVisible.value = true
    deployPhase.value = 'precheck'
    deployLogs.value = []
    deployError.value = ''
    deployTarget.value = {
      id: 'pending',
      name: createForm.name || createForm.addr || '-',
      groupId: undefined,
      address: createForm.addr || '-',
      status: 'unknown',
      tags: [],
    }
    if (!res.data.passed) {
      precheckSubmitting.value = false
      return
    }
  } catch {
    notificationStore.error(t('app.nodes.precheck.failed'))
    precheckSubmitting.value = false
    return
  }
  precheckSubmitting.value = false
}

const parseAddress = (address: string) => {
  const trimmed = address.trim()
  const match = trimmed.match(/^(.*):(\d+)$/)
  if (match) {
    return { addr: match[1] ?? '', port: match[2] ?? '' }
  }
  return { addr: trimmed, port: '' }
}

const startEdit = async (node: NodeSummary) => {
  if (!node.id || node.id === 'local') return
  isCreateActive.value = false
  isEditActive.value = true
  editTarget.value = node
  editError.value = ''
  const parsed = parseAddress(node.address)

  // 默认初始占位
  editForm.name = node.name
  if (node.groupId === 'default') {
    editForm.groupId = 'default'
    editForm.groupCustom = ''
  } else {
    editForm.groupId = 'custom'
    editForm.groupCustom = node.groupId || ''
  }
  editForm.tags = node.tags ? node.tags.join(', ') : ''
  editForm.description = node.description || ''
  editForm.addr = parsed.addr
  editForm.port = parsed.port || '22'
  editForm.user = 'root'
  editForm.authMode = 'key'
  editForm.pwd = ''
  editForm.privateKey = ''
  editForm.privateKeyPassphrase = ''
  editForm.servicePort = node.servicePort ?? ''
  editForm.seclabUrl = ''

  editDetailLoading.value = true
  try {
    await Promise.all([
      (async () => {
        try {
          const res = await nodesApi.detail(node.id)
          if (editTarget.value?.id === node.id && res.success && res.data) {
            const prov = res.data.provisioning
            if (prov) {
              editForm.addr = prov.sshAddr || parsed.addr
              editForm.port = prov.sshPort ? String(prov.sshPort) : parsed.port || '22'
              editForm.user = prov.sshUser || 'root'
              editForm.authMode = prov.sshAuthMode || 'password'
              editForm.servicePort = prov.expectedListenPort
                ? String(prov.expectedListenPort)
                : (node.servicePort ?? '')
              if (prov.sshAuthMode === 'key') {
                editForm.privateKey = '~/.ssh/id_ed25519'
              }
            }
          }
        } catch (err) {
          console.error('Failed to load node detail for editing:', err)
        }
      })(),
      (async () => {
        try {
          const res = await nodesApi.about(node.id)
          if (editTarget.value?.id === node.id && res.success && res.data) {
            editForm.seclabUrl = res.data.seclabUrl || ''
          }
        } catch (err) {
          console.error('Failed to load remote node seclabUrl:', err)
        }
      })(),
    ])
  } finally {
    editDetailLoading.value = false
    if (editTarget.value?.id === node.id) {
      // 在所有异步详情数据填充后，保存表单初始状态快照
      originalFormJson = JSON.stringify(editForm)
    }
  }
}

const cancelEdit = () => {
  editTarget.value = null
  editError.value = ''
  isEditActive.value = false
}

const submitEdit = async () => {
  if (!editTarget.value?.id || editTarget.value.id === 'local') return

  // 检查表单是否有改动。如果没有改动，直接关闭弹窗返回，不发送请求
  if (JSON.stringify(editForm) === originalFormJson) {
    cancelEdit()
    return
  }

  const originalForm = JSON.parse(originalFormJson)
  const isSeclabUrlChanged = editForm.seclabUrl !== originalForm.seclabUrl

  editSubmitting.value = true
  editError.value = ''
  const payload = {
    name: editForm.name || undefined,
    groupId: editForm.groupId === 'custom' ? editForm.groupCustom || 'default' : 'default',
    tags: editForm.tags
      ? editForm.tags
          .split(',')
          .map((t) => t.trim())
          .filter(Boolean)
      : [],
    description: editForm.description || '',
    addr: editForm.addr || undefined,
    port: editForm.port || undefined,
    user: editForm.user || undefined,
    authMode: editForm.authMode || undefined,
    pwd: editForm.authMode === 'password' ? editForm.pwd || undefined : undefined,
    privateKey: editForm.authMode === 'key' ? editForm.privateKey || undefined : undefined,
    privateKeyPassphrase:
      editForm.authMode === 'key' ? editForm.privateKeyPassphrase || undefined : undefined,
    servicePort: editForm.servicePort || undefined,
    seclabUrl: isSeclabUrlChanged ? editForm.seclabUrl || undefined : undefined,
  }
  try {
    const res = await nodesApi.update(editTarget.value.id, payload)
    if (!res.success) {
      const msg = res.message || t('app.nodes.edit.failed')
      notificationStore.error(msg)
      editError.value = msg
      editSubmitting.value = false
      return
    }
  } catch {
    const msg = t('app.nodes.edit.failed')
    notificationStore.error(msg)
    editError.value = msg
    editSubmitting.value = false
    return
  }
  editSubmitting.value = false
  cancelEdit()
  await fetchNodes()
}

const handleDeleteAction = async (node: NodeSummary) => {
  if (!node.id || node.id === 'local') return
  const useSimpleMessage = node.status === 'retired' || node.status === 'draft'
  const message = useSimpleMessage
    ? t('app.nodes.delete.confirmMessageSimple', { name: node.name })
    : t('app.nodes.delete.confirmMessage', { name: node.name })
  const confirmed = await confirmationModal.showConfirmation(
    message,
    t('app.nodes.delete.confirmTitle'),
    t('app.nodes.delete.confirmAction'),
    t('confirmation.cancel'),
  )
  if (!confirmed) return
  if (isDeleting(node.id)) return
  setDeleting(node.id, true)
  try {
    const res = await nodesApi.remove(node.id)
    if (!res.success) {
      notificationStore.error(res.message || t('app.nodes.delete.failed'))
      return
    }
    notificationStore.success(t('app.nodes.delete.success'))
    await fetchNodes()
  } catch {
    notificationStore.error(t('app.nodes.delete.failed'))
  } finally {
    setDeleting(node.id, false)
  }
}

/**
 * 根据节点的当前生命周期状态，动态计算其可用的操作菜单项。
 *
 * @param node 节点摘要信息
 * @returns 过滤并设置状态后的操作项数组
 */
const getNodeActions = (node: NodeSummary) => {
  const actions = []
  const status = node.status

  // 1. 检测 (Check) - 当节点非草稿、非部署中、且非退役时可用
  const canCheck = !['draft', 'deploying', 'retired'].includes(status)
  if (canCheck) {
    actions.push({
      label: isChecking(node.id) ? t('app.nodes.check.submitting') : t('app.nodes.check.action'),
      handler: () => checkNode(node),
      disabled: isChecking(node.id) || status === 'deploying',
    })
  }

  // 2. 编辑 (Edit) - 非部署中、且非退役状态下可用（已退役节点无法编辑，只能删除重新录入）
  const canEdit = !['deploying', 'retired'].includes(status)
  if (canEdit) {
    actions.push({
      label: t('app.nodes.actions.edit'),
      handler: () => startEdit(node),
    })
  }
  // 3. 修复/部署 (Repair) - 在非退役、非部署中状态下可用
  const canRepair = !['retired', 'deploying'].includes(status)
  if (canRepair) {
    actions.push({
      label: t('app.nodes.actions.repair'),
      handler: () => executeNodeAction(node, 'repair'),
    })
  }

  // 4. 退役 (Retire) - 在非草稿、非部署中、且非退役的状态下可用
  const canRetire = !['draft', 'deploying', 'retired'].includes(status)
  if (canRetire) {
    actions.push({
      label: t('app.nodes.actions.retire'),
      handler: () => executeNodeAction(node, 'retire'),
      class: 'btn-stop',
    })
  }

  // 5. 卸载 (Uninstall) - 在非草稿、非部署中、且非退役的状态下可用
  const canUninstall = !['draft', 'deploying', 'retired'].includes(status)
  if (canUninstall) {
    actions.push({
      label: t('app.nodes.actions.uninstall'),
      handler: () => executeNodeAction(node, 'uninstall'),
      class: 'btn-delete',
    })
  }

  // 6. 删除 (Delete) - 部署中节点禁止删除，防止写冲突
  actions.push({
    label: isDeleting(node.id) ? t('app.nodes.delete.submitting') : t('app.nodes.actions.delete'),
    handler: () => handleDeleteAction(node),
    disabled: isDeleting(node.id) || status === 'deploying',
    class: 'btn-delete',
  })

  return actions
}

const startCountdown = () => {
  stopCountdown()
  countdownSeconds.value = 10
  countdownTimer = window.setInterval(() => {
    countdownSeconds.value -= 1
    if (countdownSeconds.value <= 0) {
      stopCountdown()
      closeDeployDrawer()
    }
  }, 1000)
}

const stopCountdown = () => {
  if (countdownTimer) {
    clearInterval(countdownTimer)
    countdownTimer = null
  }
}

const startPollingProgress = (nodeId: string) => {
  stopPollingProgress()
  deployPollInterval = window.setInterval(async () => {
    try {
      const res = await nodesApi.getDeployProgress(nodeId)
      if (res.success && res.data) {
        deployLogs.value = res.data.logs ?? []
        deployProgressPercent.value = res.data.progressPercent ?? 0
        if (res.data.isFinished) {
          stopPollingProgress()
          if (deployOperationId) {
            windowStore.finishGlobalOperation(deployOperationId)
            deployOperationId = null
          }
          if (res.data.error) {
            deployError.value = res.data.error
          } else {
            notificationStore.success(
              t('app.nodes.deploy.success', { name: deployTarget.value?.name || '' }),
            )
            startCountdown()
          }
          void fetchNodes()
        }
      }
    } catch {
      // Ignore poll failures
    }
  }, 1500)
}

const stopPollingProgress = () => {
  if (deployPollInterval) {
    clearInterval(deployPollInterval)
    deployPollInterval = null
  }
}

const startDeploy = async (payload: Record<string, unknown>) => {
  deployError.value = ''
  deployLogs.value = []
  deployProgressPercent.value = 0
  isDeployDrawerVisible.value = true
  deployPhase.value = 'deploy'
  deployTarget.value = {
    id: 'pending',
    name: createForm.name || createForm.addr || '-',
    groupId: undefined,
    address: createForm.addr || '-',
    status: 'unknown',
    tags: [],
  }
  try {
    const res = await nodesApi.deployCreate(payload)
    if (!res.success || !res.data?.node?.nodeId) {
      deployError.value = res.message || t('app.nodes.deploy.failed')
      deployLogs.value = [deployError.value]
      return false
    }
    const nodeId = res.data.node.nodeId
    deployOperationId = `node-deploy:${nodeId}`
    windowStore.registerGlobalOperation({
      operationId: deployOperationId,
      sourceAppId: 'node-manager',
      title: deployTarget.value?.name
        ? t('app.nodes.deploy.operationTitle', { name: deployTarget.value.name })
        : t('app.nodes.deploy.drawerTitle'),
      cancellable: false,
      reason: t('app.nodes.deploy.operationBusy'),
    })
    startPollingProgress(nodeId)
    return true
  } catch {
    deployError.value = t('app.nodes.deploy.failed')
    deployLogs.value = [deployError.value]
    return false
  }
}

const closeDeployDrawer = () => {
  isDeployDrawerVisible.value = false
  if (deployOperationId) {
    stopCountdown()
    return
  }
  stopPollingProgress()
  stopCountdown()
  deployLogs.value = []
  deployError.value = ''
  deployProgressPercent.value = 0
  deployTarget.value = null
  deployPhase.value = null
  pendingDeployPayload.value = null
  void fetchNodes()
}

const confirmDeploy = async () => {
  if (!pendingDeployPayload.value || !precheckResult.value?.passed) return
  isCreateActive.value = false
  isEditActive.value = false
  deployPhase.value = 'deploy'
  const deployed = await startDeploy(pendingDeployPayload.value)
  if (deployed) {
    cancelCreate()
  }
}

const deployDrawerTitle = computed(() => {
  return deployPhase.value === 'precheck'
    ? t('app.nodes.precheck.drawerTitle')
    : t('app.nodes.deploy.drawerTitle')
})

type PrecheckItemStatus = 'passed' | 'warning' | 'failed' | 'skipped'

interface PrecheckItem {
  key: string
  label: string
  status: PrecheckItemStatus
  message: string
  slot: string
}

const precheckStatusTagType = (status: PrecheckItemStatus) => {
  if (status === 'passed') return 'success'
  if (status === 'warning') return 'warning'
  if (status === 'skipped') return 'info'
  return 'danger'
}

const precheckStatusText = (status: PrecheckItemStatus) => {
  return t(`app.nodes.precheck.status.${status}Short`)
}

const precheckItems = computed<PrecheckItem[]>(() => {
  if (!precheckResult.value) return []
  return [
    {
      key: 'ssh',
      label: t('app.nodes.precheck.indicators.ssh'),
      status: precheckResult.value.ssh.status,
      message: precheckResult.value.ssh.message,
      slot: 'ssh',
    },
    {
      key: 'callback',
      label: t('app.nodes.precheck.indicators.callback'),
      status: precheckResult.value.callback.status,
      message: precheckResult.value.callback.message,
      slot: 'callback',
    },
    {
      key: 'agentStatus',
      label: t('app.nodes.precheck.indicators.agentStatus'),
      status: precheckResult.value.agentStatus.blocking ? 'failed' : 'passed',
      message: precheckResult.value.agentStatus.message,
      slot: 'agentStatus',
    },
    {
      key: 'os',
      label: t('app.nodes.precheck.indicators.os'),
      status: precheckResult.value.os.status,
      message: precheckResult.value.os.message,
      slot: 'os',
    },
    {
      key: 'permission',
      label: t('app.nodes.precheck.indicators.permission'),
      status: precheckResult.value.permission.status,
      message: precheckResult.value.permission.message,
      slot: 'permission',
    },
    {
      key: 'service',
      label: t('app.nodes.precheck.indicators.service'),
      status: precheckResult.value.service.status,
      message: precheckResult.value.service.message,
      slot: 'service',
    },
    {
      key: 'systemd',
      label: t('app.nodes.precheck.indicators.systemd'),
      status: precheckResult.value.systemd.status,
      message: precheckResult.value.systemd.message,
      slot: 'systemd',
    },
    {
      key: 'directory',
      label: t('app.nodes.precheck.indicators.directory'),
      status: precheckResult.value.directory.status,
      message: precheckResult.value.directory.message,
      slot: 'directory',
    },
    {
      key: 'docker',
      label: t('app.nodes.precheck.indicators.docker'),
      status: precheckResult.value.docker.status,
      message: precheckResult.value.docker.message,
      slot: 'docker',
    },
    {
      key: 'port',
      label: t('app.nodes.precheck.indicators.port'),
      status: precheckResult.value.port.status,
      message: precheckResult.value.port.message,
      slot: 'port',
    },
  ]
})

const precheckFailedItems = computed(() => {
  return precheckItems.value.filter((item) => item.status === 'failed')
})

const precheckPrimaryFailureMessage = computed(() => {
  if (precheckResult.value?.passed) return ''
  const serviceMessage = precheckResult.value?.service?.message || ''
  if (
    precheckResult.value?.service &&
    precheckResult.value.service.status === 'failed' &&
    serviceMessage
  ) {
    return serviceMessage
  }
  return precheckFailedItems.value[0]?.message || t('app.nodes.precheck.failed')
})

const executeNodeAction = async (node: NodeSummary, action: 'repair' | 'retire' | 'uninstall') => {
  const actionLabel = t(`app.nodes.actions.${action}`)
  const isUninstallingCurrentNode = action === 'uninstall' && currentNodeId.value === node.id
  if (isUninstallingCurrentNode) {
    const guardMessage = buildNodeSwitchBlockMessage()
    if (guardMessage) {
      notificationStore.error(guardMessage)
      return
    }
  }
  const confirmMessage = isUninstallingCurrentNode
    ? t('app.nodes.actions.confirmCurrentUninstallMessage', { name: node.name })
    : t('app.nodes.actions.confirmMessage', { action: actionLabel, name: node.name })
  const confirmed = await confirmationModal.showConfirmation(
    confirmMessage,
    t('app.nodes.actions.confirmTitle'),
    actionLabel,
    t('confirmation.cancel'),
  )
  if (!confirmed) return

  try {
    let res
    if (action === 'repair') {
      res = await nodesApi.repair(node.id)
    } else if (action === 'retire') {
      res = await nodesApi.retire(node.id)
    } else {
      res = await nodesApi.uninstall(node.id)
    }
    if (!res.success) {
      notificationStore.error(res.message || t('app.nodes.actions.failed', { action: actionLabel }))
      return
    }
    if (isUninstallingCurrentNode) {
      const switchResult = nodeStore.requestSwitchCurrentNode('local')
      if (switchResult.switched) {
        notificationStore.success(t('app.nodes.actions.currentUninstallSuccess'))
      } else {
        notificationStore.error(switchResult.reason || t('app.nodes.switchFailed'))
      }
    } else {
      notificationStore.success(t('app.nodes.actions.success', { action: actionLabel }))
    }
    await fetchNodes()
  } catch {
    notificationStore.error(t('app.nodes.actions.failed', { action: actionLabel }))
  }
}

watch(
  refreshInterval,
  (value) => {
    nodeStore.startAutoRefresh(value)
  },
  { immediate: true },
)

watch(selectedRemoteNodeId, () => {
  void loadSelectedNodeDetail()
})

watch(
  () => nodeStore.nodes,
  () => {
    void loadSelectedNodeDetail(true)
  },
)

onUnmounted(() => {
  nodeStore.startAutoRefresh(10000)
  if (!deployOperationId) {
    stopPollingProgress()
  }
  stopCountdown()
})

const isUpgradeDialogOpen = ref(false)
const upgradeSubmitting = ref(false)
const releases = ref<UpgradeReleaseRecord[]>([])
const upgradeForm = reactive({
  targetVersion: '',
  overwriteSameVersion: false,
})

const releaseOptions = computed(() =>
  releases.value.map((r) => ({
    value: r.version,
    label: `${r.version} (${r.channel})`,
  })),
)

const getNodeNameById = (id: string) => {
  const node = nodes.value.find((n) => n.id === id)
  return node ? nodeDisplayName(node) : id
}

const openUpgradeDialog = async () => {
  upgradeForm.targetVersion = ''
  upgradeForm.overwriteSameVersion = false
  isUpgradeDialogOpen.value = true
  try {
    const res = await upgradesApi.listReleases()
    if (res.success && res.data) {
      releases.value = res.data.filter((r) => r.upgradeEligible)
    }
  } catch (err) {
    console.error('Failed to load releases', err)
    notificationStore.error(t('app.nodes.upgradeDialog.noReleases'))
  }
}

const submitUpgradePlan = async () => {
  if (!upgradeForm.targetVersion) return
  upgradeSubmitting.value = true
  try {
    const payload = {
      targetVersion: upgradeForm.targetVersion,
      nodeIds: selectedNodeIds.value,
      overwriteSameVersion: upgradeForm.overwriteSameVersion,
    }
    const createResponse = await upgradesApi.createPlan(payload)
    if (!createResponse.success || !createResponse.data) {
      notificationStore.error(
        createResponse.message || t('app.nodes.upgradeDialog.createPlanFailed'),
      )
      return
    }
    const startResponse = await upgradesApi.startPlan(createResponse.data.plan.planId)
    if (!startResponse.success || !startResponse.data) {
      notificationStore.error(startResponse.message || t('app.nodes.upgradeDialog.startPlanFailed'))
      return
    }

    selectedNodeIds.value = []
    isUpgradeDialogOpen.value = false

    // 缓存升级状态并跳转
    window.sessionStorage.setItem('seclab.activeUpgradePlanId', startResponse.data.plan.planId)
    window.sessionStorage.setItem(
      'seclab.activeUpgradePlanDetail',
      JSON.stringify(startResponse.data),
    )

    notificationStore.success(t('app.nodes.upgradeDialog.startPlanSuccess') || '升级任务已成功启动')
    router.push({ path: '/upgrade-progress', query: { planId: startResponse.data.plan.planId } })
  } catch (err) {
    console.error('Failed to submit upgrade plan', err)
    notificationStore.error(t('app.nodes.upgradeDialog.startPlanFailed'))
  } finally {
    upgradeSubmitting.value = false
  }
}

onMounted(() => {
  fetchNodes()
})
</script>

<template>
  <div class="node-manager" data-seclab-app="nodes">
    <!-- 升级引导 Alert 提示条 -->
    <SecLabAlert
      v-if="showUpgradeGuide"
      :title="t('app.nodes.upgradeGuideTitle')"
      type="info"
      closeable
      style="margin-bottom: 12px"
      @close="showUpgradeGuide = false"
    >
      {{ t('app.nodes.upgradeGuideDesc') }}
    </SecLabAlert>

    <SecLabCard shadow="never" class="toolbar-card">
      <div class="toolbar">
        <div class="toolbar-left">
          <SecLabButton type="primary" @click="startCreate">
            {{ t('app.nodes.create.action') }}
          </SecLabButton>
          <SecLabButton
            type="primary"
            :class="{ 'upgrade-btn-highlight': showUpgradeGuide }"
            :disabled="selectedNodeIds.length === 0"
            @click="openUpgradeDialog"
          >
            {{ t('app.nodes.upgradeSelected') }}
            {{ selectedNodeIds.length > 0 ? `(${selectedNodeIds.length})` : '' }}
          </SecLabButton>
        </div>

        <div class="toolbar-right">
          <SecLabSelect
            v-model="selectedNodeId"
            class="agent-select"
            :options="nodeOptions"
            :placeholder="t('desktop.header.nodePlaceholder')"
          />

          <SecLabSelect
            v-model="refreshInterval"
            class="refresh-select"
            :options="refreshOptions"
          />

          <SecLabButton @click="fetchNodes">{{ t('app.nodes.refresh.manual') }}</SecLabButton>
        </div>
      </div>
    </SecLabCard>

    <SecLabCard v-if="selectedRemoteNodeId" shadow="never" class="detail-card">
      <template #header>
        <div class="drawer-list-header">
          <span>{{ t('app.nodes.detail.title') }}</span>
          <SecLabTag :type="selectedNodeDetail?.node?.status === 'online' ? 'success' : 'warning'">
            {{ selectedNodeName }}
          </SecLabTag>
        </div>
      </template>

      <div class="detail-body">
        <SecLabAlert
          v-if="selectedNodeDetail?.node?.lifecycleStatus === 'conflict'"
          :title="t('app.nodes.detail.conflict')"
          type="error"
          show-icon
        />

        <div class="detail-grid">
          <SecLabDescriptions
            :items="nodeBaseDetailItems"
            :data="selectedNodeDetail?.node as any"
            border
          >
            <template #lifecycle="{ data }: { data: any }">
              <SecLabTag type="info">{{ data?.lifecycleStatus ?? '--' }}</SecLabTag>
            </template>
            <template #runtime="{ data }: { data: any }">
              <SecLabTag :type="runtimeTagType(data?.runtimeStatus)">
                {{ data?.runtimeStatus ?? '--' }}
              </SecLabTag>
            </template>
            <template #health="{ data }: { data: any }">
              <SecLabTag :type="healthTagType(data?.healthStatus)">
                {{ data?.healthStatus ?? '--' }}
              </SecLabTag>
            </template>
            <template #lastSeen="{ data }: { data: any }">
              {{ formatDateTime(data?.lastSeenAt) }}
            </template>
            <template #group="{ data }: { data: any }">
              {{ data?.groupName ?? '--' }}
            </template>
          </SecLabDescriptions>

          <SecLabDescriptions
            :items="nodeDeployDetailItems"
            :data="selectedNodeDetail?.provisioning as any"
            border
          >
            <template #deployMethod="{ data }: { data: any }">
              {{ data?.deployMethod ?? '--' }}
            </template>
            <template #deployStatus="{ data }: { data: any }">
              <SecLabTag :type="data?.lastDeployResultStatus === 'failed' ? 'danger' : 'success'">
                {{ data?.lastDeployResultStatus ?? '--' }}
              </SecLabTag>
            </template>
            <template #sshTarget="{ data }: { data: any }">
              {{ data?.sshAddr ? `${data.sshAddr}:${data.sshPort ?? 22}` : '--' }}
            </template>
            <template #installDir="{ data }: { data: any }">
              {{ data?.installDir ?? '--' }}
            </template>
            <template #deployAt="{ data }: { data: any }">
              {{ formatDateTime(data?.lastDeployAt) }}
            </template>
            <template #deployError="{ data }: { data: any }">
              {{ data?.lastDeployErrorSummary }}
            </template>
          </SecLabDescriptions>

          <SecLabDescriptions
            :items="nodeSessionDetailItems"
            :data="selectedNodeDetail?.session as any"
            border
          >
            <template #sessionId="{ data }: { data: any }">
              {{ data?.sessionId ? data.sessionId.split('-')[0] : '--' }}
            </template>
            <template #registeredAt="{ data }: { data: any }">
              {{ formatDateTime(data?.registeredAt) }}
            </template>
            <template #lastHeartbeatAt="{ data }: { data: any }">
              {{ formatDateTime(data?.lastHeartbeatAt) }}
            </template>
            <template #leaseExpiresAt="{ data }: { data: any }">
              {{ formatDateTime(data?.leaseExpiresAt) }}
            </template>
            <template #agentVersion>
              {{ selectedNodeDetail?.node?.metadata?.resource?.version || '--' }}
            </template>
          </SecLabDescriptions>
        </div>
      </div>
      <!-- 节点加载遮罩：仅覆盖详情卡片，不遮挡右上角切换下拉框和窗口 Header 强关按钮 -->
      <SecLabLoading :loading="nodeDetailLoading" cover />
    </SecLabCard>

    <SecLabDialog
      :visible="isEditActive"
      :title="t('app.nodes.form.editNode')"
      width="720px"
      :close-on-click-overlay="false"
      @close="cancelEdit"
    >
      <form class="node-edit-form" style="position: relative" @submit.prevent>
        <!-- 基础配置 -->
        <div class="form-section">
          <div class="form-section-title">{{ t('app.nodes.sections.basic') }}</div>
          <div class="form-grid-2col">
            <SecLabFormItem :label="t('app.nodes.create.name')">
              <SecLabInput v-model="editForm.name" />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.nodes.create.addr')">
              <SecLabInput v-model="editForm.addr" />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.nodes.create.port')">
              <SecLabInput v-model="editForm.port" />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.nodes.create.user')">
              <SecLabInput v-model="editForm.user" autocomplete="username" />
            </SecLabFormItem>
          </div>
        </div>

        <!-- 认证配置 -->
        <div class="form-section">
          <div class="form-section-title">{{ t('app.nodes.sections.auth') }}</div>
          <div class="form-grid-2col">
            <SecLabFormItem :label="t('app.nodes.create.authMode')">
              <SecLabSelect
                v-model="editForm.authMode"
                :options="[
                  { label: t('app.nodes.create.authPassword'), value: 'password' },
                  { label: t('app.nodes.create.authKey'), value: 'key' },
                ]"
              />
            </SecLabFormItem>
            <SecLabFormItem
              v-if="editForm.authMode === 'password'"
              :label="t('app.nodes.create.password')"
            >
              <input type="text" autocomplete="username" style="display: none" />
              <SecLabInput
                v-model="editForm.pwd"
                type="password"
                showPassword
                autocomplete="new-password"
              />
            </SecLabFormItem>
            <SecLabFormItem v-else :label="t('app.nodes.create.privateKeyPassphrase')">
              <input type="text" autocomplete="username" style="display: none" />
              <SecLabInput
                v-model="editForm.privateKeyPassphrase"
                type="password"
                showPassword
                autocomplete="new-password"
              />
            </SecLabFormItem>

            <template v-if="editForm.authMode === 'key'">
              <div class="grid-col-2">
                <SecLabFormItem :label="t('app.nodes.create.privateKey')">
                  <SecLabInput
                    v-model="editForm.privateKey"
                    :placeholder="t('app.nodes.create.privateKeyPlaceholder')"
                  />
                </SecLabFormItem>
              </div>
            </template>
          </div>
        </div>

        <!-- 分组与描述 -->
        <div class="form-section">
          <div class="form-section-title">{{ t('app.nodes.sections.group') }}</div>
          <div class="form-grid-2col">
            <SecLabFormItem :label="t('app.nodes.create.groupId')">
              <SecLabSelect
                v-model="editForm.groupId"
                :options="[
                  { label: t('app.nodes.create.groupDefault'), value: 'default' },
                  { label: t('app.nodes.create.groupCustom'), value: 'custom' },
                ]"
              />
            </SecLabFormItem>
            <SecLabFormItem
              v-if="editForm.groupId === 'custom'"
              :label="t('app.nodes.create.groupCustomInput')"
            >
              <SecLabInput v-model="editForm.groupCustom" />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.nodes.create.tags')">
              <SecLabInput v-model="editForm.tags" />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.nodes.create.description')" class="grid-col-2">
              <SecLabInput v-model="editForm.description" />
            </SecLabFormItem>
          </div>
        </div>

        <!-- 部署配置 -->
        <div class="form-section">
          <div class="form-section-title">{{ t('app.nodes.sections.install') }}</div>
          <div class="form-grid-2col">
            <SecLabFormItem :label="t('app.nodes.create.servicePort')">
              <SecLabInput v-model="editForm.servicePort" />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.nodes.create.seclabUrl')">
              <SecLabInput
                v-model="editForm.seclabUrl"
                :placeholder="t('app.nodes.create.seclabUrlPlaceholder')"
              />
            </SecLabFormItem>
          </div>
        </div>

        <SecLabAlert v-if="editError" :title="editError" type="error" show-icon />
        <SecLabLoading :loading="editDetailLoading" cover />
      </form>

      <template #footer>
        <SecLabButton @click="cancelEdit">{{ t('app.nodes.edit.cancel') }}</SecLabButton>
        <SecLabButton
          type="primary"
          :loading="editSubmitting"
          :disabled="editDetailLoading"
          @click="submitEdit"
        >
          {{ t('app.nodes.edit.submit') }}
        </SecLabButton>
      </template>
    </SecLabDialog>

    <SecLabCard v-if="deployPhase === 'deploy'" shadow="never" class="content-card deploying-card">
      <SecLabLoading :loading="true" :text="t('app.nodes.deploy.inProgress')" />
    </SecLabCard>

    <SecLabCard v-else shadow="never" class="table-card">
      <SecLabTable :data="nodes" :columns="columns" border class="nodes-table">
        <!-- 这里只需处理具有 slot 的列 -->
        <template #selectionHeader>
          <SecLabCheckbox
            :model-value="isAllSelected"
            :disabled="nodes.filter(isNodeUpgradable).length === 0"
            @change="handleSelectAll"
          />
        </template>

        <template #selection="{ row: node }: { row: any }">
          <SecLabCheckbox
            :model-value="selectedNodeIds.includes(node.id)"
            :disabled="!isNodeUpgradable(node)"
            @change="(val) => handleSelectChange(node.id, val)"
          />
        </template>

        <template #name="{ row: node }: { row: any }">
          <div class="name-cell">
            <span>{{ nodeDisplayName(node) }}</span>
            <SecLabTag v-if="node.id === currentNodeId" type="primary">
              {{ t('app.nodes.current') }}
            </SecLabTag>
          </div>
        </template>

        <template #address="{ row: node }: { row: any }">
          {{ node.address || '-' }}
        </template>

        <template #status="{ row: node }: { row: any }">
          <SecLabTag :type="statusTagType(node.status)">{{ statusLabel(node.status) }}</SecLabTag>
        </template>

        <template #resource="{ row: node }: { row: any }">
          <span class="resource-text">{{ resourceLabel(node) }}</span>
        </template>

        <template #tags="{ row: node }: { row: any }">
          <div class="tag-list">
            <SecLabTag v-if="node.tags.length === 0" type="default">
              {{ t('app.nodes.noTags') }}
            </SecLabTag>
            <SecLabTag v-for="tag in node.tags" :key="tag">{{ tag }}</SecLabTag>
          </div>
        </template>

        <template #actions="{ row: node }: { row: any }">
          <SecLabActionMenu
            v-if="node.id !== 'local'"
            :label="t('app.nodes.actions.menu')"
            :actions="getNodeActions(node)"
          />
          <span v-else class="sl-text-muted">-</span>
        </template>

        <template #empty>
          <div class="empty-state">
            <span>{{ t('app.nodes.messages.empty') }}</span>
          </div>
        </template>
      </SecLabTable>
      <!-- 节点加载遮罩：仅覆盖表格卡片，不遮挡右上角切换下拉框和窗口 Header 强关按钮 -->
      <SecLabLoading :loading="nodeDetailLoading" cover />
    </SecLabCard>

    <SecLabDrawer
      v-model="isCheckDrawerVisible"
      :title="t('app.nodes.check.drawerTitle')"
      @close="closeCheckDrawer"
    >
      <SecLabDescriptions
        :items="[
          { label: t('app.nodes.check.target'), slot: 'target' },
          { label: t('app.nodes.check.statusLabel'), slot: 'status' },
        ]"
        border
        class="drawer-meta"
      >
        <template #target>
          {{ checkDetailTarget?.name ?? '-' }}
        </template>
        <template #status>
          <SecLabTag :type="checkDetail?.status?.toLowerCase() === 'online' ? 'success' : 'danger'">
            {{ checkStatusLabel }}
          </SecLabTag>
        </template>
      </SecLabDescriptions>

      <div class="drawer-list">
        <SecLabCard
          v-for="detail in checkDetailItems"
          :key="detail.key"
          shadow="never"
          class="drawer-list-item"
        >
          <template #header>
            <div class="drawer-list-header">
              <span>{{ detail.label }}</span>
              <SecLabTag :type="precheckStatusTagType(detail.item.status)">
                {{ precheckStatusText(detail.item.status) }}
              </SecLabTag>
            </div>
          </template>
          <div class="check-item-detail">{{ checkItemDetail(detail.item) || '-' }}</div>
        </SecLabCard>
      </div>

      <template #footer>
        <SecLabButton type="primary" @click="closeCheckDrawer">{{
          t('app.nodes.check.close')
        }}</SecLabButton>
      </template>
    </SecLabDrawer>

    <SecLabDialog
      :visible="isDeployDrawerVisible"
      :title="deployDrawerTitle"
      width="650px"
      :close-on-click-overlay="false"
      z-index="calc(var(--sdl-z-index-modal) + 10)"
      @close="closeDeployDrawer"
    >
      <div v-if="deployPhase === 'precheck'" class="drawer-section">
        <div class="drawer-list-header">
          <span>{{ t('app.nodes.precheck.title') }}</span>
          <SecLabTag :type="precheckResult?.passed ? 'success' : 'danger'">
            {{
              precheckResult?.passed
                ? t('app.nodes.precheck.statusPassed')
                : t('app.nodes.precheck.statusFailed')
            }}
          </SecLabTag>
        </div>

        <SecLabAlert
          v-if="!precheckResult?.passed"
          :title="t('app.nodes.precheck.conflictTitle')"
          :description="precheckPrimaryFailureMessage"
          type="error"
          show-icon
        />

        <SecLabDescriptions :items="precheckItems" border>
          <template v-for="detail in precheckItems" :key="detail.key" #[detail.slot]>
            <div class="precheck-item-row">
              <SecLabTag :type="precheckStatusTagType(detail.status)">
                {{ precheckStatusText(detail.status) }}
              </SecLabTag>
              <span class="precheck-detail-msg">{{ detail.message || '-' }}</span>
            </div>
          </template>
        </SecLabDescriptions>
      </div>

      <div v-else class="drawer-section">
        <SecLabDescriptions
          :items="[
            { label: t('app.nodes.deploy.target'), slot: 'target' },
            { label: t('app.nodes.deploy.statusLabel'), slot: 'status' },
          ]"
          border
          class="drawer-meta"
        >
          <template #target>
            {{ deployTarget?.name ?? '-' }}
          </template>
          <template #status>
            <SecLabTag
              :type="deployError ? 'danger' : deployProgressPercent === 100 ? 'success' : 'warning'"
            >
              {{
                deployError
                  ? t('app.nodes.deploy.statusFailed')
                  : deployProgressPercent === 100
                    ? t('app.nodes.deploy.statusSuccess')
                    : `${t('app.nodes.deploy.statusRunning')}${deployProgressPercent > 0 ? ` (${deployProgressPercent}%)` : ''}`
              }}
            </SecLabTag>
          </template>
        </SecLabDescriptions>

        <SecLabAlert v-if="deployError" :title="deployError" type="error" show-icon />

        <div class="logs-container">
          <div v-if="deployLogs.length === 0" class="deploy-empty">
            {{ deployError ? t('app.nodes.deploy.noLogs') : t('app.nodes.deploy.deploying') }}
          </div>
          <div v-for="(line, index) in deployLogs" :key="index" class="deploy-line">{{ line }}</div>
        </div>
      </div>

      <template #footer>
        <div v-if="deployPhase === 'precheck'" class="nodes-panel-footer">
          <SecLabButton v-if="!precheckResult?.passed" type="primary" @click="closeDeployDrawer">
            {{ t('app.nodes.precheck.acknowledge') }}
          </SecLabButton>
          <template v-else>
            <SecLabButton class="precheck-footer-button" @click="closeDeployDrawer">
              {{ t('app.nodes.precheck.cancelSaved') }}
            </SecLabButton>
            <SecLabButton class="precheck-footer-button" type="primary" @click="confirmDeploy">
              {{ t('app.nodes.precheck.confirmDeploy') }}
            </SecLabButton>
          </template>
        </div>
        <div v-else class="nodes-panel-footer">
          <SecLabButton
            v-if="deployProgressPercent === 100 && !deployError"
            type="primary"
            @click="closeDeployDrawer"
          >
            {{ t('app.nodes.deploy.close') + ' (' + countdownSeconds + 's)' }}
          </SecLabButton>
          <SecLabButton v-else type="primary" @click="closeDeployDrawer">
            {{ t('app.nodes.deploy.close') }}
          </SecLabButton>
        </div>
      </template>
    </SecLabDialog>

    <SecLabDialog
      :visible="isCreateActive"
      :title="t('app.nodes.create.title')"
      width="720px"
      :close-on-click-overlay="false"
      @close="cancelCreate"
    >
      <form class="node-create-form" @submit.prevent>
        <!-- 基础配置 -->
        <div class="form-section">
          <div class="form-section-title">{{ t('app.nodes.sections.basic') }}</div>
          <div class="form-grid-2col">
            <SecLabFormItem :label="t('app.nodes.create.name')">
              <SecLabInput v-model="createForm.name" />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.nodes.create.addr')">
              <SecLabInput v-model="createForm.addr" />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.nodes.create.port')">
              <SecLabInput v-model="createForm.port" />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.nodes.create.user')">
              <SecLabInput v-model="createForm.user" autocomplete="username" />
            </SecLabFormItem>
          </div>
        </div>

        <!-- 认证配置 -->
        <div class="form-section">
          <div class="form-section-title">{{ t('app.nodes.sections.auth') }}</div>
          <div class="form-grid-2col">
            <SecLabFormItem :label="t('app.nodes.create.authMode')">
              <SecLabSelect
                v-model="createForm.authMode"
                :options="[
                  { label: t('app.nodes.create.authPassword'), value: 'password' },
                  { label: t('app.nodes.create.authKey'), value: 'key' },
                ]"
              />
            </SecLabFormItem>
            <SecLabFormItem
              v-if="createForm.authMode === 'password'"
              :label="t('app.nodes.create.password')"
            >
              <input type="text" autocomplete="username" style="display: none" />
              <SecLabInput
                v-model="createForm.pwd"
                type="password"
                show-password
                autocomplete="new-password"
              />
            </SecLabFormItem>
            <SecLabFormItem v-else :label="t('app.nodes.create.privateKeyPassphrase')">
              <input type="text" autocomplete="username" style="display: none" />
              <SecLabInput
                v-model="createForm.privateKeyPassphrase"
                type="password"
                show-password
                autocomplete="new-password"
              />
            </SecLabFormItem>

            <template v-if="createForm.authMode === 'key'">
              <div class="grid-col-2">
                <SecLabFormItem :label="t('app.nodes.create.privateKey')">
                  <SecLabInput
                    v-model="createForm.privateKey"
                    :placeholder="t('app.nodes.create.privateKeyPlaceholder')"
                  />
                  <div class="form-hint">{{ t('app.nodes.create.privateKeyHint') }}</div>
                </SecLabFormItem>
              </div>
            </template>
          </div>
        </div>

        <!-- 分组与描述 -->
        <div class="form-section">
          <div class="form-section-title">{{ t('app.nodes.sections.group') }}</div>
          <div class="form-grid-2col">
            <SecLabFormItem :label="t('app.nodes.create.groupId')">
              <SecLabSelect
                v-model="createForm.groupId"
                :options="[
                  { label: t('app.nodes.create.groupDefault'), value: 'default' },
                  { label: t('app.nodes.create.groupCustom'), value: 'custom' },
                ]"
              />
            </SecLabFormItem>
            <SecLabFormItem
              v-if="createForm.groupId === 'custom'"
              :label="t('app.nodes.create.groupCustomInput')"
            >
              <SecLabInput v-model="createForm.groupCustom" />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.nodes.create.tags')">
              <SecLabInput v-model="createForm.tags" />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.nodes.create.description')" class="grid-col-2">
              <SecLabInput v-model="createForm.description" />
            </SecLabFormItem>
          </div>
        </div>

        <!-- 部署配置 -->
        <div class="form-section">
          <div class="form-section-title">{{ t('app.nodes.sections.install') }}</div>
          <div class="form-grid-2col">
            <SecLabFormItem :label="t('app.nodes.create.installDir')">
              <SecLabInput
                v-model="createForm.installDir"
                :placeholder="t('app.nodes.create.installDirPlaceholder')"
              />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.nodes.create.servicePort')">
              <SecLabInput v-model="createForm.servicePort" />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.nodes.create.seclabUrl')" class="grid-col-2">
              <SecLabInput
                v-model="createForm.seclabUrl"
                :placeholder="t('app.nodes.create.seclabUrlPlaceholder')"
              />
              <div class="form-hint">{{ t('app.nodes.create.seclabUrlHint') }}</div>
            </SecLabFormItem>
          </div>
        </div>
      </form>

      <template #footer>
        <SecLabButton @click="cancelCreate">{{ t('app.nodes.create.cancel') }}</SecLabButton>
        <SecLabButton type="primary" :loading="precheckSubmitting" @click="submitCreate">
          {{ t('app.nodes.create.submit') }}
        </SecLabButton>
      </template>
    </SecLabDialog>

    <!-- 弹窗：精细化升级配置 -->
    <SecLabDialog
      :visible="isUpgradeDialogOpen"
      :title="t('app.nodes.upgradeDialog.title')"
      width="600px"
      @close="isUpgradeDialogOpen = false"
    >
      <div class="dialog-detail-content flex-column gap-layout">
        <SecLabAlert
          :title="t('app.nodes.upgradeDialog.selectedCount', { count: selectedNodeIds.length })"
          type="info"
          show-icon
        />

        <div
          style="
            max-height: 100px;
            overflow-y: auto;
            display: flex;
            flex-wrap: wrap;
            gap: 6px;
            padding: 4px;
            border: 1px dashed var(--sdl-border-default);
            border-radius: 4px;
          "
        >
          <SecLabTag v-for="id in selectedNodeIds" :key="id" type="default">
            {{ getNodeNameById(id) }}
          </SecLabTag>
        </div>

        <div class="form-group" style="margin-top: 10px">
          <label style="display: block; margin-bottom: 6px; font-weight: 600">
            {{ t('app.nodes.upgradeDialog.targetVersion') }}
          </label>
          <SecLabSelect
            v-model="upgradeForm.targetVersion"
            :options="releaseOptions"
            :placeholder="t('app.nodes.upgradeDialog.versionPlaceholder')"
            style="width: 100%"
          />
        </div>

        <div class="form-group" style="margin-top: 10px">
          <SecLabCheckbox v-model="upgradeForm.overwriteSameVersion">
            {{ t('app.nodes.upgradeDialog.overwriteSameVersion') }}
          </SecLabCheckbox>
          <div class="form-hint">
            {{ t('app.nodes.upgradeDialog.overwriteSameVersionTip') }}
          </div>
        </div>
      </div>

      <template #footer>
        <SecLabButton @click="isUpgradeDialogOpen = false">
          {{ t('app.nodes.upgradeDialog.cancel') }}
        </SecLabButton>
        <SecLabButton
          type="primary"
          :loading="upgradeSubmitting"
          :disabled="!upgradeForm.targetVersion"
          @click="submitUpgradePlan"
        >
          {{ t('app.nodes.upgradeDialog.submit') }}
        </SecLabButton>
      </template>
    </SecLabDialog>
  </div>
</template>

<style scoped>
/* 确保表格最后一列操作栏下拉菜单能够正常弹出越界显示 */
:deep(.sl-table-cell:last-child),
:deep(.sl-table-cell:last-child .sl-cell) {
  overflow: visible !important;
}

.node-manager {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  gap: var(--sdl-space-3);
  padding: var(--sdl-space-3);
  background: var(--sdl-bg-canvas);
  box-sizing: border-box;
}

/* --- 表单页面布局 --- */
.form-page {
  display: flex;
  justify-content: center;
  overflow-y: auto;
  padding-bottom: var(--sdl-space-8);
}

.form-container {
  width: 100%;
  max-width: 800px;
  height: fit-content;
  flex-shrink: 0;
}

.form-header {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-4);
}

.back-btn {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
  background: transparent;
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-sm);
  color: var(--sdl-text-secondary);
  padding: var(--sdl-space-1) var(--sdl-space-3);
  font-size: var(--sdl-font-caption);
  cursor: pointer;
  transition: all 0.2s;
}

.back-btn:hover {
  background: var(--sdl-bg-hover);
  color: var(--sdl-text-primary);
  border-color: var(--sdl-border-brand);
}

.back-icon {
  font-size: 16px;
  line-height: 1;
}

.form-title {
  font-size: var(--sdl-font-subtitle);
  font-weight: 700;
  color: var(--sdl-text-primary);
}

.toolbar-card {
  flex-shrink: 0;
  position: relative;
  z-index: 10;
}

.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: var(--sdl-space-3);
}

.toolbar-right {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
}

.agent-select {
  width: 180px;
}

.refresh-select {
  width: 150px;
}

.content-card,
.table-card {
  flex: 1;
  min-height: 0;
  position: relative;
  z-index: 1;
}

.detail-card {
  min-height: auto;
  position: relative;
  z-index: 1;
}

.name-cell {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--sdl-space-2);
}

.resource-text {
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-caption);
}

.tag-list {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: var(--sdl-space-1);
}

/* --- 自研表单系统布局 --- */
.form {
  display: flex;
  flex-direction: column;
}

.sl-form-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--sdl-space-3) var(--sdl-space-6);
}

.sl-form-col-full {
  grid-column: span 2;
}

.sl-form-divider {
  margin: var(--sdl-space-5) 0 var(--sdl-space-3);
  padding-bottom: var(--sdl-space-2);
  border-bottom: 1px solid var(--sdl-border-subtle);
  color: var(--sdl-text-primary);
  font-size: var(--sdl-font-body-sm);
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.sl-form-divider:first-child {
  margin-top: 0;
}

.form-hint {
  margin-top: var(--sdl-space-1);
  font-size: var(--sdl-font-caption);
  color: var(--sdl-text-muted);
}

.panel-footer {
  margin-top: var(--sdl-space-4);
  padding-top: var(--sdl-space-4);
  border-top: 1px solid var(--sdl-border-subtle);
  display: flex;
  justify-content: flex-end;
  gap: var(--sdl-space-3);
}

.nodes-panel-footer {
  display: flex;
  justify-content: flex-end;
  gap: var(--sdl-space-3);
}

:deep(.precheck-footer-button) {
  width: 104px;
}

/* --- 抽屉与详情内容 --- */
.detail-body {
  position: relative;
  min-height: 100px;
}

.detail-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: var(--sdl-space-4);
}

.drawer-meta {
  margin-bottom: var(--sdl-space-3);
}

.drawer-list {
  display: grid;
  gap: var(--sdl-space-2);
}

.drawer-list-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sdl-space-2);
}

.drawer-section {
  display: grid;
  gap: var(--sdl-space-3);
}

.history-section {
  margin-top: var(--sdl-space-6);
}

.history-list {
  display: grid;
  gap: var(--sdl-space-2);
}

.history-card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sdl-space-2);
  margin-bottom: var(--sdl-space-1);
}

.history-card-body {
  font-size: var(--sdl-font-body-sm);
  color: var(--sdl-text-secondary);
}

.history-card-meta {
  margin-top: var(--sdl-space-1);
  font-size: var(--sdl-font-caption);
  color: var(--sdl-text-muted);
}

.check-item-detail {
  font-size: var(--sdl-font-body-sm);
  line-height: 1.5;
}

.precheck-item-row {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
}

/* --- 空状态与加载 --- */
.empty-state {
  padding: var(--sdl-space-8);
  text-align: center;
  color: var(--sdl-text-muted);
}

.empty-state-sm {
  padding: var(--sdl-space-4);
  text-align: center;
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-caption);
}

.deploying-card {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 200px;
}

/* --- 日志容器 --- */
.logs-container {
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  padding: var(--sdl-space-3);
  max-height: 360px;
  overflow-y: auto;
  background-color: var(--sdl-bg-muted);
  scrollbar-width: thin;
}

.deploy-empty {
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-caption);
}

.deploy-line {
  font-family: var(--sdl-font-mono);
  font-size: var(--sdl-font-code);
  line-height: 1.5;
  padding: 2px 0;
  color: var(--sdl-text-secondary);
  word-break: break-all;
}

@media (max-width: 1200px) {
  .detail-grid {
    grid-template-columns: 1fr;
  }
}

/* 对话框内的新增节点表单 */
.node-create-form {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-4);
}

.form-section {
  background-color: var(--sdl-bg-card);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  padding: var(--sdl-space-4);
}

.form-section-title {
  font-size: var(--sdl-font-body);
  font-weight: 700;
  color: var(--sdl-text-primary);
  margin-bottom: var(--sdl-space-3);
  padding-left: var(--sdl-space-2);
  border-left: 3px solid var(--sdl-primary);
  line-height: 1.2;
}

.form-grid-2col {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--sdl-space-3) var(--sdl-space-4);
}

.grid-col-2 {
  grid-column: span 2;
}

@media (max-width: 960px) {
  .sl-form-grid {
    grid-template-columns: 1fr;
  }
  .sl-form-col-full {
    grid-column: span 1;
  }
}

.toolbar-left {
  display: flex;
  gap: var(--sdl-space-3);
  align-items: center;
}

@keyframes pulse-border {
  0% {
    box-shadow: 0 0 0 0 rgba(59, 130, 246, 0.7);
  }
  70% {
    box-shadow: 0 0 0 6px rgba(59, 130, 246, 0);
  }
  100% {
    box-shadow: 0 0 0 0 rgba(59, 130, 246, 0);
  }
}

.upgrade-btn-highlight {
  animation: pulse-border 1.5s infinite;
  border: 1px solid var(--sdl-primary);
}
</style>
