<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  SecLabButton,
  SecLabTable,
  SecLabTag,
  SecLabInput,
  SecLabSelect,
  SecLabLoading,
  SecLabPagination,
  SecLabDialog,
  SecLabTooltip,
  SecLabCheckbox,
} from '@/components/ui'
import type { SecLabTableColumn } from '@/components/ui/SecLabTable.vue'
import {
  simulationApi,
  type SimRule,
  type SimInstance,
  type SimLog,
  type SimRulePackage,
} from '@/api/modules/simulation'
import { nodesApi, type NodeSummaryResponse } from '@/api/modules/nodes'
import { formatDateTime } from '@/utils/time'
import { useNotificationStore } from '@/stores/notification'
import { useConfirmationModalStore } from '@/stores/confirmation-modal'
import { useWindowManagerStore } from '@/stores/window-manager'
import SimulationRuleDetailDialog from './simulation/SimulationRuleDetailDialog.vue'

defineProps<{
  isMaximized?: boolean
  payload?: Record<string, unknown>
}>()

const { t, locale } = useI18n()
const notificationStore = useNotificationStore()
const modalStore = useConfirmationModalStore()
const windowManagerStore = useWindowManagerStore()

/**
 * 根据当前多语言设置获取仿真诱捕规则的显示名称
 * @param rule 仿真规则对象
 * @returns 国际化处理后的规则名称
 */
const getRuleName = (rule?: SimRule | null) => {
  if (!rule) return ''
  return locale.value === 'en' ? rule.nameEn || rule.name : rule.name
}

/**
 * 根据节点 ID 获取节点名称
 * @param nodeId 节点 ID
 * @returns 节点名称
 */
const getNodeName = (nodeId: string) => {
  if (nodeId === 'local') {
    return t('app.nodes.local')
  }
  const node = nodes.value.find((n) => n.nodeId === nodeId)
  return node ? node.name : nodeId
}

/**
 * 界面核心页签导航
 * 'deployments' - 运行实例面板
 * 'rules' - 规则整编面板
 * 'logs' - 威胁审计日志
 */
const activeTab = ref<'deployments' | 'rules' | 'logs'>('deployments')

/** 全局及局部异步操作的加载动画控制状态 */
const isLoading = ref(false)

/** 仿真诱捕规则集合 */
const rules = ref<SimRule[]>([])

/** 规则列表分页：当前页码 */
const rulesCurrentPage = ref(1)

/** 规则列表分页：单页条数限制 */
const rulesPageSize = ref(10)

/** 规则整编筛选检索条件 */
const rulesFilter = ref({
  name: '',
  cve: '',
  category: 'all',
  protocol: 'all',
})

/** 规则类型筛选选项 */
const categoryFilterOptions = computed(() => [
  { value: 'all', label: t('app.simulation.rules.filters.allCategories') },
  { value: 'cve_sim', label: getCategoryLabel('cve_sim') },
  { value: 'vuln_sim', label: getCategoryLabel('vuln_sim') },
  { value: 'honeypot', label: getCategoryLabel('honeypot') },
  { value: 'test_env', label: getCategoryLabel('test_env') },
])

/** 协议类型筛选选项 */
const protocolFilterOptions = computed(() => [
  { value: 'all', label: t('app.simulation.rules.filters.allProtocols') },
  { value: 'http', label: 'HTTP' },
  { value: 'redis', label: 'Redis' },
  { value: 'smtp', label: 'SMTP' },
  { value: 'pop3', label: 'POP3' },
  { value: 'imap', label: 'IMAP' },
  { value: 'ssh', label: 'SSH' },
  { value: 'ftp', label: 'FTP' },
  { value: 'rdp', label: 'RDP' },
])

/** 根据检索条件过滤后的规则列表 */
const filteredRules = computed(() => {
  return rules.value.filter((rule) => {
    // 规则名称模糊匹配 (支持中英文不区分大小写模糊匹配)
    if (rulesFilter.value.name) {
      const searchName = rulesFilter.value.name.toLowerCase()
      const matchName = rule.name.toLowerCase().includes(searchName)
      const matchNameEn = rule.nameEn ? rule.nameEn.toLowerCase().includes(searchName) : false
      if (!matchName && !matchNameEn) return false
    }

    // CVE 编号模糊匹配
    if (rulesFilter.value.cve) {
      const searchCve = rulesFilter.value.cve.toLowerCase()
      const matchCve = rule.cve ? rule.cve.toLowerCase().includes(searchCve) : false
      if (!matchCve) return false
    }

    // 规则类型精确匹配
    if (rulesFilter.value.category !== 'all') {
      if (rule.category !== rulesFilter.value.category) return false
    }

    // 协议类型精确匹配
    if (rulesFilter.value.protocol !== 'all') {
      if (rule.protocol !== rulesFilter.value.protocol) return false
    }

    return true
  })
})

/** 计算过滤后的总页数，防止除以零，至少保持 1 页 */
const rulesTotalPages = computed(() =>
  Math.max(1, Math.ceil(filteredRules.value.length / rulesPageSize.value)),
)

const BUILTIN_RULE_MAX_ID = 999999

/** 分页切片后的过滤规则数据集，用于表格驱动渲染 */
const pagedRules = computed(() => {
  const start = (rulesCurrentPage.value - 1) * rulesPageSize.value
  return filteredRules.value.slice(start, start + rulesPageSize.value)
})

// 监听筛选条件变化，自动重置分页至第一页，避免在空页面滞留
watch(
  () => ({ ...rulesFilter.value }),
  () => {
    rulesCurrentPage.value = 1
  },
)

/** 控制规则详情抽屉（Drawer）的打开状态 */
const isDetailDrawerOpen = ref(false)

/** 当前选中查看详情的规则对象 */
const ruleDetail = ref<SimRule | null>(null)

/** 系统默认的伪造 Nginx 欢迎页 HTML 代码，用于高保真预览兜底 */
const defaultNginxHtml = `<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>Welcome to nginx!</title>
<style>
    html { color-scheme: light dark; }
    body { width: 35em; margin: 0 auto; font-family: Tahoma, Verdana, Arial, sans-serif; padding: 2em; background-color: #f8fafc; color: #1e293b; }
    h1 { color: #0f172a; font-size: 2em; margin-bottom: 0.5em; }
    p { line-height: 1.5; }
    a { color: #3b82f6; text-decoration: none; }
    a:hover { text-decoration: underline; }
</style>
</head>
<body>
<h1>Welcome to nginx!</h1>
<p>If you see this page, the nginx web server is successfully installed and working. Further configuration is required.</p>
<p>For online documentation and support please refer to <a href="http://nginx.org/">nginx.org</a>.<br/>
Commercial support is available at <a href="http://nginx.com/">nginx.com</a>.</p>
<p><em>Thank you for using nginx.</em></p>
</body>
</html>`

/** 控制部署确认弹窗（Dialog）的显示状态 */
const isDeployDialogOpen = ref(false)

/** 计划部署至目标节点的规则对象 */
const ruleToDeploy = ref<SimRule | null>(null)

/** 所有可用的边缘/本地节点概览列表 */
const nodes = ref<NodeSummaryResponse[]>([])

/** 当前选中的过滤节点 ID（默认为 'all'，展示所有节点的部署实例与日志） */
const selectedNodeId = ref('all')

/** 仿真服务运行实例列表 */
const instances = ref<SimInstance[]>([])

/** 被勾选的多选实例 ID 集合 */
const selectedInstanceIds = ref<string[]>([])

/** 仿真实例部署表单模型 */
const deployForm = ref({
  nodeId: '',
  port: 8080,
  ruleId: null as number | null,
  callbackUrl: '',
})

/** 部署确认按钮可用状态，必须选择明确节点，过滤用的 all 不能作为部署目标。 */
const canConfirmDeploy = computed(
  () => !!deployForm.value.nodeId && deployForm.value.nodeId !== 'all',
)

/** 部署弹窗默认目标节点：优先本地节点，其次第一个在线节点。 */
const defaultDeployNodeId = computed(() => {
  const localNode = nodes.value.find((node) => node.nodeId === 'local')
  return localNode?.nodeId ?? nodes.value[0]?.nodeId ?? ''
})

/** 威胁审计日志数据列表 */
const logs = ref<SimLog[]>([])

// 威胁审计日志的分页及自动刷新相关状态
const logPage = ref(1)
const logPageSize = ref(50)
const logTotal = ref(0)
const logTotalPages = computed(() => Math.max(1, Math.ceil(logTotal.value / logPageSize.value)))
const isAutoRefreshLogs = ref(false)
let logRefreshTimer: ReturnType<typeof setInterval> | null = null

/** 将在线节点转化为下拉框组件直接可用的选项数据源 */
const nodeOptions = computed(() =>
  nodes.value.map((n) => ({
    value: n.nodeId,
    label: `${n.nodeId === 'local' ? t('app.nodes.local') : n.name} (${n.address})`,
  })),
)

/** 包含“全部节点”选项的过滤下拉框数据源 */
const filterNodeOptions = computed(() => [
  { value: 'all', label: t('app.taskScheduler.filters.allNodes') || '全部节点' },
  ...nodeOptions.value,
])

/** 运行中实例列表表格列配置项 */
const instanceColumns = computed<SecLabTableColumn[]>(() => [
  { label: '', slot: 'selection', headerSlot: 'selectionHeader', width: 50, align: 'center' },
  { label: t('common.index'), width: 70, align: 'center', slot: 'index' },
  {
    label: t('app.simulation.deployments.fields.nodeName'),
    width: 140,
    slot: 'nodeName',
    align: 'center',
  },
  { label: t('app.simulation.rules.fields.name'), minWidth: 180, align: 'center', slot: 'rule' },
  { prop: 'listenPort', label: t('app.simulation.deployments.port'), width: 100, align: 'center' },
  {
    label: t('app.simulation.deployments.fields.status'),
    width: 120,
    align: 'center',
    slot: 'status',
  },
  {
    label: t('app.simulation.deployments.fields.forensic'),
    width: 240,
    align: 'center',
    slot: 'forensic',
  },
  { label: t('common.actions'), width: 100, align: 'center', slot: 'actions' },
])

/** 规则管理列表表格列配置项 */
const extendedRuleColumns = computed<SecLabTableColumn[]>(() => [
  { label: t('common.index'), width: 70, align: 'center', slot: 'index' },
  { label: t('app.simulation.rules.fields.name'), minWidth: 180, slot: 'name', align: 'center' },
  {
    prop: 'cve',
    label: t('app.simulation.rules.fields.cve'),
    width: 140,
    align: 'center',
    slot: 'cve',
  },
  {
    prop: 'category',
    label: t('app.simulation.rules.fields.category'),
    width: 120,
    align: 'center',
    slot: 'category',
  },
  {
    prop: 'protocol',
    label: t('app.simulation.rules.fields.protocol'),
    width: 100,
    align: 'center',
    slot: 'protocol',
  },
  { label: t('common.actions'), width: 220, align: 'center', slot: 'actions' },
])

/** 诱捕审计日志列表表格列配置项 */
const logColumns = computed<SecLabTableColumn[]>(() => [
  { label: t('common.index'), width: 70, align: 'center', slot: 'index' },
  {
    label: t('app.simulation.logs.columns.nodeName'),
    width: 140,
    slot: 'nodeName',
    align: 'center',
  },
  { label: t('app.simulation.logs.columns.time'), width: 180, align: 'center', slot: 'time' },
  {
    prop: 'clientIp',
    label: t('app.simulation.logs.columns.clientIp'),
    width: 140,
    align: 'center',
  },
  {
    prop: 'clientPort',
    label: t('app.simulation.logs.columns.clientPort'),
    width: 100,
    align: 'center',
  },
  { label: t('app.simulation.logs.columns.eventType'), width: 140, align: 'center', slot: 'type' },
  {
    label: t('app.simulation.logs.columns.summary'),
    width: 450,
    slot: 'summary',
  },
])

const currentPackage = ref<SimRulePackage | null>(null)
const packageFileInput = ref<HTMLInputElement | null>(null)
const isUploadingPackage = ref(false)

const loadCurrentPackage = async () => {
  try {
    const res = await simulationApi.getCurrentRulePackage()
    if (res.success && res.data) {
      currentPackage.value = res.data
    } else {
      currentPackage.value = null
    }
  } catch {
    currentPackage.value = null
  }
}

const triggerPackageUpload = () => {
  packageFileInput.value?.click()
}

const handleUploadPackage = async (event: Event) => {
  const input = event.target as HTMLInputElement
  if (!input.files || input.files.length === 0) return
  const file = input.files[0]

  isUploadingPackage.value = true
  try {
    const res = await simulationApi.importRulePackage(file)
    if (res.success) {
      const isAlreadyLatest =
        res.messageKey === 'app.simulation.rules.messages.packageImportAlreadyLatest'
      const successMsg = isAlreadyLatest
        ? t('app.simulation.rules.messages.packageImportAlreadyLatest')
        : t('app.simulation.rules.messages.packageImportSuccess')

      if (isAlreadyLatest) {
        notificationStore.info(successMsg)
      } else {
        notificationStore.success(successMsg)
      }
      await loadCurrentPackage()
      await loadRules()
    } else {
      notificationStore.error(res.message || t('app.simulation.rules.messages.packageImportFailed'))
    }
  } catch (err) {
    const errMsg =
      (err as { response?: { data?: { message?: string } } }).response?.data?.message ||
      t('app.simulation.rules.messages.packageImportError')
    notificationStore.error(errMsg)
  } finally {
    isUploadingPackage.value = false
    input.value = ''
  }
}

/**
 * 异步拉取并加载数据库中存储的协议仿真规则列表
 */
const loadRules = async () => {
  try {
    const res = await simulationApi.listRules()
    if (res.success && res.data) {
      rules.value = res.data
    }
  } catch {
    notificationStore.error(t('app.simulation.rules.messages.loadFailed'))
  }
}

/**
 * 获取可用节点列表，并在缺失本地节点时追加 127.0.0.1 本地虚拟节点，以此确保本地环境调试可用性
 */
const loadNodes = async () => {
  try {
    const res = await nodesApi.list()
    if (res.success && res.data) {
      const onlineNodes = res.data.filter((n) => n.status.toLowerCase() === 'online')

      const hasLocal = onlineNodes.some((n) => n.nodeId === 'local')
      if (!hasLocal) {
        onlineNodes.unshift({
          nodeId: 'local',
          name: t('app.nodes.local'),
          groupName: 'default',
          address: '127.0.0.1',
          status: 'online',
          tags: ['local'],
        })
      }

      nodes.value = onlineNodes
      if (nodes.value.length > 0 && !selectedNodeId.value) {
        selectedNodeId.value = nodes.value[0].nodeId
      }
    }
  } catch {
    notificationStore.error(t('app.simulation.deployments.messages.loadNodesFailed'))
  }
}

/**
 * 获取指定节点上的威胁审计日志列表。
 */
const loadLogs = async (silent = false) => {
  if (!selectedNodeId.value) return
  if (!silent) isLoading.value = true
  try {
    const logRes = await simulationApi.listLogs(selectedNodeId.value, {
      page: logPage.value,
      pageSize: logPageSize.value,
    })
    if (logRes.success && logRes.data) {
      logs.value = logRes.data.records
      logTotal.value = logRes.data.total
    }
  } catch {
    notificationStore.error(t('app.simulation.deployments.messages.loadInstancesFailed'))
  } finally {
    if (!silent) isLoading.value = false
  }
}

/**
 * 获取指定节点上正在运行的仿真实例列表及威胁审计日志。
 * @param silent 若为 true，则启用静默加载，不展示可能导致屏幕闪烁的全局 Loading 遮罩层。这常用于静默轮询。
 */
const loadNodeInstancesAndLogs = async (silent = false) => {
  if (!selectedNodeId.value) return
  selectedInstanceIds.value = []
  if (!silent) isLoading.value = true
  try {
    const instRes = await simulationApi.listInstances(selectedNodeId.value)
    if (instRes.success && instRes.data) {
      instances.value = instRes.data
    }
    await loadLogs(true)
  } catch {
    notificationStore.error(t('app.simulation.deployments.messages.loadInstancesFailed'))
  } finally {
    if (!silent) isLoading.value = false
  }
}

watch(activeTab, (tab) => {
  if (tab === 'rules') {
    void loadCurrentPackage()
    void loadRules()
  } else if (tab === 'deployments' || tab === 'logs') {
    void loadNodes()
    if (selectedNodeId.value) {
      void loadNodeInstancesAndLogs()
    }
  }
  if (tab !== 'logs') {
    isAutoRefreshLogs.value = false
  }
})

watch(selectedNodeId, () => {
  logPage.value = 1
  if (activeTab.value === 'deployments' || activeTab.value === 'logs') {
    void loadNodeInstancesAndLogs()
  }
})

// 监听分页状态变化以更新日志
watch([logPage, logPageSize], () => {
  if (activeTab.value === 'logs') {
    void loadLogs()
  }
})

// 监听自动刷新开关
watch(isAutoRefreshLogs, (val) => {
  if (logRefreshTimer) {
    clearInterval(logRefreshTimer)
    logRefreshTimer = null
  }
  if (val) {
    logRefreshTimer = setInterval(() => {
      if (activeTab.value === 'logs' && selectedNodeId.value) {
        void loadLogs(true)
      }
    }, 10000)
  }
})

/** 秒级递增的系统当前时间戳，用于本地实时倒计时/正计时运算 */
const nowSec = ref(Math.floor(Date.now() / 1000))
let timerId: ReturnType<typeof setInterval> | null = null

/**
 * 计算并格式化当前实例开启抓包取证的累计运行流逝时间（例如: 01:23 / 05:00）
 * @param row 目标仿真运行实例
 */
const getCaptureTimeStr = (row: SimInstance) => {
  if (!row.pcapStartTime) return '00:00 / 05:00'
  const elapsed = Math.max(0, nowSec.value - row.pcapStartTime)
  const pad = (num: number) => String(num).padStart(2, '0')
  const m = Math.floor(elapsed / 60)
  const s = elapsed % 60
  return `${pad(m)}:${pad(s)} / 05:00`
}

/** 状态锁：防止快速重复点击触发并发的取证状态切换请求 */
const isTogglingPcap = ref(false)

/**
 * 下发指令开始对目标协议仿真实例进行流量取证（PCAP 包捕获）
 * @param row 目标仿真运行实例
 */
const handleStartCapture = async (row: SimInstance) => {
  if (isTogglingPcap.value) return
  isTogglingPcap.value = true
  try {
    const res = await simulationApi.startCapture(row.instanceId)
    if (res.success) {
      notificationStore.success(t('app.simulation.forensic.messages.startSuccess'))
      await loadNodeInstancesAndLogs()
    } else {
      notificationStore.error(res.message || t('app.simulation.forensic.messages.startFailed'))
    }
  } catch (err: unknown) {
    const errMsg =
      err instanceof Error ? err.message : t('app.simulation.forensic.messages.startFailed')
    notificationStore.error(errMsg)
  } finally {
    isTogglingPcap.value = false
  }
}

/**
 * 下发指令停止当前实例的抓包取证行为。
 * 包含对空包（文件体积不满足 24 字节链路帧底线）的拦截设计。
 * 如果检测到生成的是空 PCAP（触发后台状态变更为 idle，信标重置），将弹出警告说明流量全空；
 * 否则异步轮询等待数据包回传成功并由 Agent 上报转换为 ready。
 * @param row 目标仿真运行实例
 */
const handleStopCapture = async (row: SimInstance) => {
  if (isTogglingPcap.value) return
  isTogglingPcap.value = true
  try {
    const res = await simulationApi.stopCapture(row.instanceId)
    if (res.success) {
      notificationStore.success(t('app.simulation.forensic.messages.stopCommandSent'))
      await loadNodeInstancesAndLogs()

      let pollCount = 0
      const pollTimer = setInterval(async () => {
        pollCount++
        await loadNodeInstancesAndLogs(true)
        const current = instances.value.find((i) => i.instanceId === row.instanceId)

        // 空包捕获阻断警告拦截
        if (current && current.pcapStatus === 'idle') {
          notificationStore.warning(t('app.simulation.forensic.messages.emptyPcap'))
          clearInterval(pollTimer)
          return
        }

        // 成功回传落盘就绪
        if (current && current.pcapStatus === 'ready') {
          notificationStore.success(t('app.simulation.forensic.messages.stopSuccess'))
          clearInterval(pollTimer)
          return
        }

        // 超时或实例已被注销则退出轮询
        if (!current || pollCount > 10) {
          clearInterval(pollTimer)
        }
      }, 1000)
    } else {
      notificationStore.error(res.message || t('app.simulation.forensic.messages.stopFailed'))
    }
  } catch (err: unknown) {
    const errMsg =
      err instanceof Error ? err.message : t('app.simulation.forensic.messages.stopFailed')
    notificationStore.error(errMsg)
  } finally {
    isTogglingPcap.value = false
  }
}

/**
 * 重置仿真实例的抓包取证状态，物理擦除之前已持久化存盘的 PCAP 数据文件
 * @param row 目标仿真运行实例
 */
const handleResetPcap = async (row: SimInstance) => {
  const confirmed = await modalStore.showConfirmation(
    t('app.simulation.forensic.resetTip'),
    t('app.simulation.forensic.resetTitle'),
  )
  if (confirmed) {
    try {
      const res = await simulationApi.resetCapture(row.instanceId)
      if (res.success) {
        notificationStore.success(t('app.simulation.forensic.messages.resetSuccess'))
        await loadNodeInstancesAndLogs()
      } else {
        notificationStore.error(res.message || t('app.simulation.forensic.messages.resetFailed'))
      }
    } catch (err: unknown) {
      const errMsg =
        err instanceof Error ? err.message : t('app.simulation.forensic.messages.resetFailed')
      notificationStore.error(errMsg)
    }
  }
}

onMounted(() => {
  // 动态拼装用于与沙箱仿真配合的 SecLab 系统回调监听端点
  deployForm.value.callbackUrl = simulationApi.getCallbackUrl(window.location.origin)

  void loadNodes()
  void loadCurrentPackage()
  void loadRules()
  if (selectedNodeId.value) {
    void loadNodeInstancesAndLogs()
  }

  // 维持秒级累加计时器，服务于流量捕获倒计时的状态刷新
  timerId = setInterval(() => {
    nowSec.value = Math.floor(Date.now() / 1000)
  }, 1000)
})

onUnmounted(() => {
  if (timerId) {
    clearInterval(timerId)
  }
  if (logRefreshTimer) {
    clearInterval(logRefreshTimer)
  }
})

/** 辅助类型断言，安全强制转换类型便于在插槽中安全提取模型字段 */
const toInstance = (row: unknown) => row as SimInstance

/** 控制新建自定义规则弹窗（Dialog）的打开状态 */
const isRuleDialogOpen = ref(false)

/** 新建诱捕规则的数据表单模型 */
const ruleForm = ref({
  name: '',
  protocol: 'http',
  serverHeader: 'nginx/1.24.0 (Ubuntu)',
  html: '',
})

interface ExploitPathForm {
  path: string
  triggerMethod: string
  responseStatus: string
  responseBody: string
}

/** 动态添加/删除的蜜罐特定触发蜜饵路径及其静态伪装响应内容数据模型 */
const exploitPaths = ref<ExploitPathForm[]>([])

/**
 * 重置表单状态，调起创建自定义仿真协议规则弹窗
 */
const openNewRuleDialog = () => {
  ruleForm.value = {
    name: '',
    protocol: 'http',
    serverHeader: 'nginx/1.24.0 (Ubuntu)',
    html: '',
  }
  exploitPaths.value = []
  isRuleDialogOpen.value = true
}

/**
 * 为当前自定义规则的配置项动态新增一条漏洞模拟响应路径
 */
const addExploitPath = () => {
  exploitPaths.value.push({
    path: '',
    triggerMethod: 'ANY',
    responseStatus: '200',
    responseBody: '',
  })
}

/**
 * 根据索引移除配置项中指定的漏洞模拟响应路径
 * @param index 计划移除的数组索引下标
 */
const removeExploitPath = (index: number) => {
  exploitPaths.value.splice(index, 1)
}

/**
 * 向控制端提交创建新的诱捕仿真规则请求。
 * 转换前台模型数据至符合后端接口定义的 YAML/JSON 序列化配置结构。
 */
const handleCreateRule = async () => {
  if (!ruleForm.value.name) {
    notificationStore.error(t('app.simulation.rules.messages.nameRequired'))
    return
  }

  isLoading.value = true
  try {
    const config = {
      server_header: ruleForm.value.serverHeader || undefined,
      html: ruleForm.value.html || undefined,
      exploit_paths: exploitPaths.value.map((ep) => ({
        path: ep.path,
        trigger_method: ep.triggerMethod === 'ANY' ? null : ep.triggerMethod,
        response_status: parseInt(ep.responseStatus, 10) || 200,
        response_body: ep.responseBody,
      })),
    }

    const res = await simulationApi.createRule({
      name: ruleForm.value.name,
      protocol: 'http',
      configYaml: JSON.stringify(config),
    })

    if (res.success && res.data) {
      notificationStore.success(t('app.simulation.rules.messages.createSuccess'))
      isRuleDialogOpen.value = false
      await loadRules()
    } else {
      notificationStore.error(res.message || t('app.simulation.rules.messages.createFailed'))
    }
  } catch {
    notificationStore.error(t('app.simulation.rules.messages.createError'))
  } finally {
    isLoading.value = false
  }
}

/**
 * 物理删除特定的自定义仿真规则，内置保护防止删除系统预置规则（ID <= 999999 属于内置）。
 * @param id 规则物理 ID
 * @param name 规则文本描述名称
 */
const handleDeleteRule = async (id: number, name: string) => {
  if (id <= BUILTIN_RULE_MAX_ID) {
    notificationStore.error(t('app.simulation.rules.messages.builtinDeleteNotAllowed'))
    return
  }

  const confirmed = await modalStore.showConfirmation(
    t('app.simulation.rules.deleteConfirm', { name }),
    t('app.simulation.rules.deleteBtn'),
    t('app.simulation.rules.deleteBtn'),
    t('confirmation.cancel'),
  )
  if (!confirmed) return

  isLoading.value = true
  try {
    const res = await simulationApi.deleteRule(id)
    if (res.success) {
      notificationStore.success(t('app.simulation.rules.messages.deleteSuccess'))
      await loadRules()
    } else {
      notificationStore.error(res.message || t('app.simulation.rules.messages.deleteFailed'))
    }
  } catch {
    notificationStore.error(t('app.simulation.rules.messages.deleteError'))
  } finally {
    isLoading.value = false
  }
}

/**
 * 打开规则详细信息查看抽屉
 * @param row 选中的规则数据模型
 */
const openDetailDrawer = (row: SimRule) => {
  ruleDetail.value = row
  isDetailDrawerOpen.value = true
}

/**
 * 快速查看已部署实例所绑定的规则详情
 * @param row 运行实例对象
 */
const handleShowInstanceRuleDetail = async (row: SimInstance) => {
  let rule = rules.value.find((r) => r.id === row.ruleId)
  if (!rule) {
    await loadRules()
    rule = rules.value.find((r) => r.id === row.ruleId)
  }
  if (rule) {
    openDetailDrawer(rule)
  } else {
    notificationStore.error(t('app.simulation.rules.messages.loadFailed'))
  }
}

/**
 * 调起桌面通用 Web 浏览器应用，并在其中高保真预览当前协议仿真的首屏网页
 */
const handleLaunchBrowser = (html?: string) => {
  isDetailDrawerOpen.value = false
  windowManagerStore.openWindowWithPayload('web-browser', {
    html: html || defaultNginxHtml,
  })
}

/**
 * 开启部署仿真模块的表单对话框，初始化部署端口与所属规则参数
 * @param row 待部署的诱捕仿真规则模型
 */
const openDeployDialog = (row: SimRule) => {
  ruleToDeploy.value = row
  deployForm.value.ruleId = row.id
  deployForm.value.port = row.defaultPort || 8080
  if (!deployForm.value.nodeId && selectedNodeId.value && selectedNodeId.value !== 'all') {
    deployForm.value.nodeId = selectedNodeId.value
  }
  if (!canConfirmDeploy.value) {
    deployForm.value.nodeId = defaultDeployNodeId.value
  }
  isDeployDialogOpen.value = true
}

/**
 * 执行向边缘或本地在线节点部署并动态开启仿真服务监听指令。
 * 内置对监听端口段（1-65535）的安全校验。
 */
const executeDeploy = async () => {
  if (!canConfirmDeploy.value) {
    notificationStore.error(t('app.simulation.deployments.messages.nodeRequired'))
    return
  }
  if (!deployForm.value.ruleId) {
    notificationStore.error(t('app.simulation.deployments.messages.ruleRequired'))
    return
  }
  const portNum = parseInt(String(deployForm.value.port), 10)
  if (isNaN(portNum) || portNum <= 0 || portNum > 65535) {
    notificationStore.error(t('app.simulation.deployments.messages.portInvalid'))
    return
  }

  const deployRule = rules.value.find((p) => p.id === deployForm.value.ruleId)
  const ruleName = deployRule ? getRuleName(deployRule) : deployForm.value.ruleId

  isLoading.value = true
  try {
    const res = await simulationApi.deploySimulation({
      nodeId: deployForm.value.nodeId,
      port: portNum,
      ruleId: deployForm.value.ruleId,
      seclabCallbackUrl: deployForm.value.callbackUrl,
    })

    if (res.success) {
      notificationStore.success(
        t('app.simulation.deployments.messages.deploySuccess', { name: ruleName }),
      )
      isDeployDialogOpen.value = false
      selectedNodeId.value = deployForm.value.nodeId
      activeTab.value = 'deployments'
    } else {
      notificationStore.error(
        res.message || t('app.simulation.deployments.messages.deployFailed', { name: ruleName }),
      )
    }
  } catch {
    notificationStore.error(
      t('app.simulation.deployments.messages.deployError', { name: ruleName }),
    )
  } finally {
    isLoading.value = false
  }
}

/**
 * 下架/注销目标端口上的指定协议仿真运行实例，物理关闭监听 Socket
 * @param id 仿真运行实例 ID
 */
const handleUndeploy = async (id: string) => {
  const instance = instances.value.find((inst) => inst.instanceId === id)
  const rule = instance ? rules.value.find((p) => p.id === instance.ruleId) : null
  const ruleName = rule ? getRuleName(rule) : instance ? instance.ruleId : id

  const confirmed = await modalStore.showConfirmation(
    t('app.simulation.deployments.undeployConfirm', { name: ruleName }),
    t('app.simulation.deployments.btnUndeploy'),
    t('app.simulation.deployments.btnUndeploy'),
    t('confirmation.cancel'),
  )
  if (!confirmed) return

  isLoading.value = true
  try {
    const res = await simulationApi.undeploySimulation(id)
    if (res.success) {
      notificationStore.success(
        t('app.simulation.deployments.messages.undeploySuccess', { name: ruleName }),
      )
      await loadNodeInstancesAndLogs()
    } else {
      notificationStore.error(
        res.message || t('app.simulation.deployments.messages.undeployFailed', { name: ruleName }),
      )
    }
  } catch {
    notificationStore.error(
      t('app.simulation.deployments.messages.undeployError', { name: ruleName }),
    )
  } finally {
    isLoading.value = false
  }
}

const isAllSelected = computed(() => {
  if (instances.value.length === 0) return false
  return instances.value.every((inst) => selectedInstanceIds.value.includes(inst.instanceId))
})

const handleSelectAll = (checked: boolean) => {
  if (checked) {
    selectedInstanceIds.value = instances.value.map((inst) => inst.instanceId)
  } else {
    selectedInstanceIds.value = []
  }
}

const handleSelectChange = (instanceId: string, checked: boolean) => {
  if (checked) {
    if (!selectedInstanceIds.value.includes(instanceId)) {
      selectedInstanceIds.value.push(instanceId)
    }
  } else {
    selectedInstanceIds.value = selectedInstanceIds.value.filter((id) => id !== instanceId)
  }
}

const getRuleNameForInstance = (instanceId: string) => {
  const inst = instances.value.find((i) => i.instanceId === instanceId)
  const rule = inst ? rules.value.find((r) => r.id === inst.ruleId) : null
  return rule ? getRuleName(rule) : inst ? String(inst.ruleId) : instanceId
}

const formatNamesList = (names: string[]) => {
  if (names.length === 0) return ''
  const quoteChar = locale.value === 'en' ? '"' : '“'
  const endQuoteChar = locale.value === 'en' ? '"' : '”'
  const comma = locale.value === 'en' ? ', ' : '、'

  if (names.length <= 3) {
    return names.map((n) => `${quoteChar}${n}${endQuoteChar}`).join(comma)
  } else {
    const subset = names.slice(0, 3)
    const extraCount = names.length - 3
    if (locale.value === 'en') {
      return subset.map((n) => `"${n}"`).join(', ') + ` and other ${extraCount} instances`
    } else {
      return subset.map((n) => `“${n}”`).join('、') + `等 ${extraCount} 个服务实例`
    }
  }
}

const handleBatchUndeploy = async () => {
  if (selectedInstanceIds.value.length === 0) return

  const confirmed = await modalStore.showConfirmation(
    t('app.simulation.deployments.batchUndeployConfirm', {
      count: selectedInstanceIds.value.length,
    }),
    t('app.simulation.deployments.btnBatchUndeploy'),
    t('app.simulation.deployments.btnBatchUndeploy'),
    t('confirmation.cancel'),
  )
  if (!confirmed) return

  isLoading.value = true
  const successNames: string[] = []
  const failNames: string[] = []

  try {
    const promises = selectedInstanceIds.value.map(async (id) => {
      const ruleName = getRuleNameForInstance(id)
      try {
        const res = await simulationApi.undeploySimulation(id)
        if (res.success) {
          successNames.push(ruleName)
        } else {
          failNames.push(ruleName)
        }
      } catch {
        failNames.push(ruleName)
      }
    })

    await Promise.all(promises)

    if (failNames.length === 0) {
      notificationStore.success(
        t('app.simulation.deployments.messages.batchUndeploySuccess', {
          names: formatNamesList(successNames),
        }),
      )
    } else if (successNames.length > 0) {
      notificationStore.warning(
        t('app.simulation.deployments.messages.batchUndeployPartial', {
          successNames: formatNamesList(successNames),
          failNames: formatNamesList(failNames),
        }),
      )
    } else {
      notificationStore.error(
        t('app.simulation.deployments.messages.batchUndeployFailed', {
          names: formatNamesList(failNames),
        }),
      )
    }

    selectedInstanceIds.value = []
    await loadNodeInstancesAndLogs()
  } catch {
    notificationStore.error(t('app.simulation.deployments.messages.batchUndeployError'))
  } finally {
    isLoading.value = false
  }
}

/**
 * 根据威胁等级/审计事件类型选择不同渲染色调的 Tag 标签
 * @param type 事件日志类别
 */
const getEventTypeTag = (type: string) => {
  if (type === 'exploit_attempt') return 'danger'
  if (type === 'http_request') return 'info'
  if (type === 'redis_command' || type === 'ftp_command' || type === 'rdp_negotiation')
    return 'warning'
  if (type === 'smtp_command' || type === 'pop3_command' || type === 'imap_command')
    return 'warning'
  if (type === 'auth_attempt') return 'danger'
  return 'success'
}

/**
 * 对仿真规则的漏洞分类标签执行本地多语言翻译
 * @param category 分类代码
 */
const getCategoryLabel = (category: string) => {
  const key = `app.simulation.rules.categories.${category}`
  const label = t(key)
  return label !== key ? label : category
}

/**
 * 根据规则类型选择标签颜色，和后端 category 枚举保持一致。
 * @param category 规则分类代码
 */
const getCategoryTagType = (category: string) => {
  if (category === 'cve_sim') return 'danger'
  if (category === 'vuln_sim') return 'warning'
  if (category === 'honeypot') return 'success'
  if (category === 'test_env') return 'info'
  return 'default'
}

/** 当前支持的仿真网络协议族源下拉框选项 */
const protocolOptions = [{ value: 'http', label: 'HTTP' }]

/** 自定义仿真规则触发路径中的支持匹配方法 */
const methodOptions = computed(() => [
  { value: 'ANY', label: t('app.simulation.rules.exploitPaths.anyMethod') },
  { value: 'GET', label: 'GET' },
  { value: 'POST', label: 'POST' },
  { value: 'PUT', label: 'PUT' },
  { value: 'PATCH', label: 'PATCH' },
  { value: 'DELETE', label: 'DELETE' },
  { value: 'HEAD', label: 'HEAD' },
  { value: 'OPTIONS', label: 'OPTIONS' },
])

/**
 * 根据所属节点名称、诱捕规则及原始哈希生成更具可读性的取证数据包物理下载文件名
 * 避免了浏览器下载出现冗长杂乱的哈希，并清晰体现所属环境（如：nodeName_ruleId_pcap.pcap）
 * @param row 目标仿真运行实例
 */
const getFriendlyPcapFilename = (row: SimInstance) => {
  if (!row.pcapFilePath) return 'capture.pcap'

  const node = nodes.value.find((n) => n.nodeId === row.nodeId)
  const nodeName = node ? node.name : row.nodeId
  const ruleId = row.ruleId

  let baseName = row.pcapFilePath.replace(/^pcap_/, '')

  const targetReplace = `${nodeName}_${ruleId}`
  if (baseName.includes(row.instanceId)) {
    baseName = baseName.replace(row.instanceId, targetReplace)
  } else {
    baseName = `${targetReplace}_${baseName}`
  }

  return baseName
}

/**
 * 流式下载指定仿真捕获的关联取证 PCAP 包数据，内置防目录穿透等安全机制。
 * 下载成功后动态建立临时文件锚点链接，由浏览器拉起标准下载。
 * @param row 目标仿真运行实例
 */
const handleDownloadPcap = async (row: SimInstance) => {
  if (!row.pcapFilePath) return
  isLoading.value = true
  try {
    const url = simulationApi.getPcapDownloadUrl(row.pcapFilePath)
    const res = await fetch(url)
    if (!res.ok) throw new Error('Download failed')

    const blob = await res.blob()
    const friendlyFilename = getFriendlyPcapFilename(row)

    const blobUrl = window.URL.createObjectURL(blob)
    const link = document.createElement('a')
    link.href = blobUrl
    link.download = friendlyFilename
    document.body.appendChild(link)
    link.click()
    document.body.removeChild(link)
    window.URL.revokeObjectURL(blobUrl)
    notificationStore.success(t('app.simulation.logs.messages.downloadSuccess'))
  } catch {
    notificationStore.error(t('app.simulation.logs.messages.downloadFailed'))
  } finally {
    isLoading.value = false
  }
}
</script>

<template>
  <div class="simulation-dashboard" data-seclab-app="simulation" data-page="simulation-dashboard">
    <!-- 头部横幅与 Tab 选项卡 -->
    <div class="dashboard-header" data-slot="header">
      <!-- Tab 切换 -->
      <div class="tab-box" data-ui="tab-box">
        <button
          v-for="(label, key) in {
            deployments: t('app.simulation.tabs.deployments'),
            rules: t('app.simulation.tabs.rules'),
            logs: t('app.simulation.tabs.logs'),
          }"
          :key="key"
          class="tab-btn"
          :class="{ active: activeTab === key }"
          @click="activeTab = key"
        >
          {{ label }}
        </button>
      </div>
    </div>

    <!-- 主交互面板 -->
    <div class="dashboard-body" data-slot="container">
      <!-- TAB 1: 运行实例 (Deployments) -->
      <div v-if="activeTab === 'deployments'" class="tab-content flex-column gap-layout">
        <!-- 顶部节点过滤栏 -->
        <div class="control-header card-bg flex-row justify-between align-center">
          <div class="filter-item">
            <span class="label">{{ t('app.simulation.deployments.selectNode') }}</span>
            <SecLabSelect
              v-model="selectedNodeId"
              :options="filterNodeOptions"
              class="node-select-filter"
            />
          </div>
          <div class="actions-group">
            <SecLabButton
              v-if="selectedInstanceIds.length > 0"
              type="danger"
              data-ui="sim-batch-undeploy-btn"
              @click="handleBatchUndeploy"
            >
              {{ t('app.simulation.deployments.btnBatchUndeploy') }} ({{
                selectedInstanceIds.length
              }})
            </SecLabButton>
          </div>
        </div>

        <div class="main-panel card-bg flex-column flex-1" data-slot="content">
          <div class="flex-1 overflow-auto">
            <SecLabTable
              :data="instances"
              :columns="instanceColumns"
              border
              data-ui="sim-deployments-table"
            >
              <template #selectionHeader>
                <SecLabCheckbox :model-value="isAllSelected" @change="handleSelectAll" />
              </template>
              <template #selection="{ row }">
                <SecLabCheckbox
                  :model-value="selectedInstanceIds.includes(row.instanceId)"
                  @change="(val) => handleSelectChange(row.instanceId, val)"
                />
              </template>
              <template #index="{ index }">
                {{ index + 1 }}
              </template>
              <template #nodeName="{ row }">
                <span data-ui="node-name-text">{{ getNodeName(row.nodeId) }}</span>
              </template>
              <template #rule="{ row }">
                <div class="rule-name-cell" data-ui="rule-name-cell">
                  <span
                    v-if="toInstance(row).pcapStatus === 'capturing'"
                    class="capturing-row-indicator"
                    :title="t('app.simulation.forensic.liveRecording')"
                  ></span>
                  <span class="rule-link-text mono" @click="handleShowInstanceRuleDetail(row)">
                    {{ getRuleName(rules.find((p) => p.id === row.ruleId)) || row.ruleId }}
                  </span>
                </div>
              </template>
              <template #status="{ row }">
                <SecLabTag :type="row.status === 'active' ? 'success' : 'danger'" size="small">
                  {{ row.status.toUpperCase() }}
                </SecLabTag>
              </template>
              <template #forensic="{ row }">
                <div class="forensic-cell">
                  <template v-if="toInstance(row).pcapStatus === 'capturing'">
                    <div class="capturing-wrapper">
                      <div
                        class="live-pulse-bars"
                        :title="t('app.simulation.forensic.intercepting')"
                      >
                        <span class="pulse-bar"></span>
                        <span class="pulse-bar"></span>
                        <span class="pulse-bar"></span>
                        <span class="pulse-bar"></span>
                      </div>
                      <SecLabButton
                        type="danger"
                        size="small"
                        data-ui="stop-forensic"
                        @click="handleStopCapture(toInstance(row))"
                      >
                        {{ t('app.simulation.forensic.stopBtn') }} ({{
                          getCaptureTimeStr(toInstance(row))
                        }})
                      </SecLabButton>
                    </div>
                  </template>
                  <template v-else-if="toInstance(row).pcapStatus === 'ready'">
                    <div class="ready-wrapper">
                      <a
                        href="#"
                        class="download-btn-link"
                        data-ui="download-pcap"
                        @click.prevent="handleDownloadPcap(toInstance(row))"
                      >
                        <SecLabButton type="primary" size="small" class="download-pulse-btn">
                          {{ t('app.simulation.forensic.downloadBtn') }}
                        </SecLabButton>
                      </a>
                      <SecLabButton
                        type="danger"
                        size="small"
                        data-ui="reset-forensic"
                        @click="handleResetPcap(toInstance(row))"
                        :title="t('app.simulation.forensic.resetTitle')"
                      >
                        {{ t('app.simulation.forensic.resetBtn') }}
                      </SecLabButton>
                    </div>
                  </template>
                  <template v-else>
                    <SecLabButton
                      type="primary"
                      size="small"
                      data-ui="start-forensic"
                      @click="handleStartCapture(toInstance(row))"
                    >
                      {{ t('app.simulation.forensic.startBtn') }}
                    </SecLabButton>
                  </template>
                </div>
              </template>
              <template #actions="{ row }">
                <SecLabButton type="danger" size="small" @click="handleUndeploy(row.instanceId)">
                  {{ t('app.simulation.deployments.btnUndeploy') }}
                </SecLabButton>
              </template>
              <template #empty>
                <div class="empty-placeholder">{{ t('app.simulation.deployments.emptyTip') }}</div>
              </template>
            </SecLabTable>
          </div>
        </div>
      </div>

      <!-- TAB 2: 规则整编 (Rules) -->
      <div v-if="activeTab === 'rules'" class="tab-content flex-column gap-layout">
        <!-- 筛选检索工具栏 -->
        <div class="control-header card-bg flex-layout gap-layout flex-align-center">
          <div class="filter-item">
            <span class="label">{{ t('app.simulation.rules.filters.name') }}</span>
            <SecLabInput
              v-model="rulesFilter.name"
              :placeholder="t('app.simulation.rules.filters.placeholderName')"
              style="width: 180px"
            />
          </div>
          <div class="filter-item">
            <span class="label">{{ t('app.simulation.rules.filters.cve') }}</span>
            <SecLabInput
              v-model="rulesFilter.cve"
              :placeholder="t('app.simulation.rules.filters.placeholderCve')"
              style="width: 160px"
            />
          </div>
          <div class="filter-item">
            <span class="label">{{ t('app.simulation.rules.filters.category') }}</span>
            <SecLabSelect
              v-model="rulesFilter.category"
              :options="categoryFilterOptions"
              style="width: 130px"
            />
          </div>
          <div class="filter-item">
            <span class="label">{{ t('app.simulation.rules.filters.protocol') }}</span>
            <SecLabSelect
              v-model="rulesFilter.protocol"
              :options="protocolFilterOptions"
              style="width: 110px"
            />
          </div>

          <!-- 右侧升级按钮 -->
          <div style="margin-left: auto">
            <SecLabButton
              type="secondary"
              size="small"
              :loading="isUploadingPackage"
              @click="triggerPackageUpload"
            >
              {{
                currentPackage
                  ? t('app.simulation.rules.package.upgradeBtn')
                  : t('app.simulation.rules.package.importBtn')
              }}
            </SecLabButton>
            <input
              type="file"
              ref="packageFileInput"
              accept=".slrp"
              @change="handleUploadPackage"
              style="display: none"
            />
          </div>
        </div>

        <div class="main-panel card-bg flex-column flex-1" data-slot="content">
          <div class="panel-header border-bottom" data-slot="header">
            <h3>{{ t('app.simulation.rules.management') }}</h3>
            <SecLabButton type="primary" size="small" @click="openNewRuleDialog">
              + {{ t('app.simulation.rules.newBtn') }}
            </SecLabButton>
          </div>

          <div class="flex-1 overflow-auto">
            <SecLabTable
              :data="pagedRules"
              :columns="extendedRuleColumns"
              border
              data-ui="sim-rules-table"
            >
              <template #index="{ index }">
                {{ (rulesCurrentPage - 1) * rulesPageSize + index + 1 }}
              </template>
              <template #name="{ row }">
                {{ getRuleName(row) }}
              </template>
              <template #cve="{ row }">
                <span class="mono" v-if="row.cve">{{ row.cve }}</span>
                <span class="disabled-text" v-else>{{ t('app.simulation.rules.noCve') }}</span>
              </template>
              <template #category="{ row }">
                <SecLabTag :type="getCategoryTagType(row.category)" size="small">
                  {{ getCategoryLabel(row.category) }}
                </SecLabTag>
              </template>
              <template #protocol="{ row }">
                <SecLabTag type="info" size="small">{{ row.protocol.toUpperCase() }}</SecLabTag>
              </template>
              <template #actions="{ row }">
                <div class="table-actions">
                  <SecLabButton type="primary" size="small" @click="openDeployDialog(row)">
                    {{ t('app.simulation.rules.deployBtn') }}
                  </SecLabButton>
                  <SecLabButton type="secondary" size="small" @click="openDetailDrawer(row)">
                    {{ t('common.details') }}
                  </SecLabButton>
                  <SecLabButton
                    type="danger"
                    size="small"
                    :disabled="row.id <= BUILTIN_RULE_MAX_ID"
                    @click="handleDeleteRule(row.id, row.name)"
                  >
                    {{ t('common.delete') }}
                  </SecLabButton>
                </div>
              </template>
              <template #empty>
                <div
                  v-if="!currentPackage && rules.length === 0"
                  class="empty-placeholder flex-column flex-align-center gap-layout"
                  style="
                    padding: 60px 0;
                    display: flex;
                    flex-direction: column;
                    align-items: center;
                    gap: 16px;
                  "
                >
                  <div
                    style="
                      font-size: 14px;
                      color: var(--text-color-secondary, #666);
                      text-align: center;
                      max-width: 480px;
                      line-height: 1.6;
                    "
                  >
                    {{ t('app.simulation.rules.package.emptyGuide') }}
                  </div>
                  <div
                    class="flex-layout gap-layout flex-justify-center"
                    style="display: flex; gap: 12px; justify-content: center; margin-top: 8px"
                  >
                    <SecLabButton
                      type="primary"
                      size="small"
                      :loading="isUploadingPackage"
                      @click="triggerPackageUpload"
                    >
                      {{ t('app.simulation.rules.package.importBtn') }}
                    </SecLabButton>
                    <SecLabButton type="secondary" size="small" @click="openNewRuleDialog">
                      {{ t('app.simulation.rules.newBtn') }}
                    </SecLabButton>
                  </div>
                </div>
                <div v-else class="empty-placeholder">
                  {{ t('app.simulation.rules.empty') }}
                </div>
              </template>
            </SecLabTable>
          </div>

          <!-- 分页器 -->
          <div
            class="pagination-bar border-top flex-layout flex-align-center flex-between"
            data-slot="footer"
          >
            <!-- 左侧规则库信息 -->
            <div
              v-if="currentPackage"
              class="flex-layout flex-align-center"
              style="font-size: 12px; color: #666; display: flex; align-items: center; gap: 16px"
            >
              <div
                class="flex-layout flex-align-center"
                style="display: flex; align-items: center; gap: 8px"
              >
                <span style="font-weight: bold; color: var(--text-color, #333)">
                  {{ t('app.simulation.rules.package.current') }}:
                </span>
                <span
                  class="mono"
                  style="color: var(--primary-color, #1890ff); font-family: monospace"
                >
                  v{{ currentPackage.version }}
                </span>
              </div>
              <span
                style="
                  border-left: 1px solid #e0e0e0;
                  padding-left: 12px;
                  height: 12px;
                  display: inline-flex;
                  align-items: center;
                "
              >
                {{ t('app.simulation.rules.package.ruleCount') }}: {{ currentPackage.ruleCount }}
              </span>
              <span
                style="
                  border-left: 1px solid #e0e0e0;
                  padding-left: 12px;
                  height: 12px;
                  display: inline-flex;
                  align-items: center;
                "
              >
                {{ t('app.simulation.rules.package.importedAt') }}:
                {{ formatDateTime(currentPackage.importedAt) }}
              </span>
            </div>
            <div v-else style="font-size: 12px; color: #999">
              {{ t('app.simulation.rules.package.notImported') }}
            </div>

            <SecLabPagination
              :current-page="rulesCurrentPage"
              :total-pages="rulesTotalPages"
              @page-change="(p) => (rulesCurrentPage = p)"
            />
          </div>
        </div>
      </div>

      <!-- TAB 3: 诱捕审计与抓包取证 (Logs) -->
      <div v-if="activeTab === 'logs'" class="tab-content flex-column gap-layout">
        <!-- 顶部节点过滤栏 -->
        <div class="control-header card-bg flex-layout flex-align-center flex-between">
          <div class="filter-item flex-layout flex-align-center gap-layout">
            <span class="label">{{ t('app.simulation.deployments.selectNode') }}</span>
            <SecLabSelect
              v-model="selectedNodeId"
              :options="filterNodeOptions"
              class="node-select-filter"
            />
          </div>
          <div class="flex-layout flex-align-center gap-layout" style="gap: 16px">
            <div
              class="flex-layout flex-align-center gap-layout"
              style="gap: 6px; cursor: pointer; user-select: none"
            >
              <SecLabCheckbox
                :model-value="isAutoRefreshLogs"
                @change="(val) => (isAutoRefreshLogs = val)"
              />
              <span
                style="font-size: 13px; color: var(--sdl-text-secondary)"
                @click="isAutoRefreshLogs = !isAutoRefreshLogs"
              >
                {{ t('app.simulation.logs.autoRefresh') }}
              </span>
            </div>
            <SecLabButton type="primary" size="small" @click="() => loadLogs()">
              {{ t('app.simulation.logs.refresh') }}
            </SecLabButton>
          </div>
        </div>

        <div class="main-panel card-bg flex-column flex-1" data-slot="content">
          <div class="flex-1 overflow-auto">
            <SecLabTable :data="logs" :columns="logColumns" border data-ui="sim-logs-table">
              <template #index="{ index }">
                {{ index + 1 }}
              </template>
              <template #time="{ row }">
                <span class="mono-time">{{ formatDateTime(row.timestamp) }}</span>
              </template>
              <template #nodeName="{ row }">
                <span data-ui="log-node-name-text">{{ getNodeName(row.nodeId) }}</span>
              </template>
              <template #type="{ row }">
                <SecLabTag :type="getEventTypeTag(row.eventType)" size="small">
                  {{ row.eventType.toUpperCase() }}
                </SecLabTag>
              </template>
              <template #summary="{ row }">
                <SecLabTooltip :text="row.detailSummary" position="top">
                  <div class="log-summary-cell" data-ui="log-summary-text">
                    {{ row.detailSummary }}
                  </div>
                </SecLabTooltip>
              </template>

              <template #empty>
                <div class="empty-placeholder">
                  {{ t('app.simulation.logs.empty') }}
                </div>
              </template>
            </SecLabTable>
          </div>

          <!-- 分页器 -->
          <div
            class="pagination-bar border-top flex-layout flex-align-center flex-end"
            data-slot="footer"
            style="padding: 10px 16px; display: flex; justify-content: flex-end"
          >
            <SecLabPagination
              :current-page="logPage"
              :total-pages="logTotalPages"
              @page-change="(p) => (logPage = p)"
            />
          </div>
        </div>
      </div>
    </div>

    <!-- 弹窗：部署仿真服务 -->
    <SecLabDialog
      :visible="isDeployDialogOpen"
      :title="t('app.simulation.deployments.deploySimulation')"
      width="500px"
      @close="isDeployDialogOpen = false"
    >
      <div class="dialog-detail-content flex-column gap-layout">
        <div
          class="detail-badge flex-layout gap-layout flex-align-center card-box"
          style="padding: 10px; margin-bottom: 12px"
        >
          <span style="font-weight: 600; font-size: 13px; color: var(--sdl-text-primary)">{{
            t('app.simulation.deployments.selectedRuleLabel')
          }}</span>
          <SecLabTag type="info">{{ getRuleName(ruleToDeploy) }}</SecLabTag>
        </div>

        <div class="form-group">
          <label>{{ t('app.simulation.deployments.targetNode') }}</label>
          <SecLabSelect v-model="deployForm.nodeId" :options="nodeOptions" />
        </div>

        <div class="form-group">
          <label>{{ t('app.simulation.deployments.port') }}</label>
          <SecLabInput
            v-model="deployForm.port"
            type="number"
            :placeholder="t('app.simulation.deployments.portPlaceholder')"
          />
        </div>
      </div>

      <template #footer>
        <SecLabButton type="secondary" @click="isDeployDialogOpen = false">
          {{ t('confirmation.cancel') }}
        </SecLabButton>
        <SecLabButton type="primary" :disabled="!canConfirmDeploy" @click="executeDeploy">
          {{ t('confirmation.confirm') }}
        </SecLabButton>
      </template>
    </SecLabDialog>

    <SimulationRuleDetailDialog
      :visible="isDetailDrawerOpen"
      :rule="ruleDetail"
      @close="isDetailDrawerOpen = false"
      @launch-browser="handleLaunchBrowser"
    />

    <!-- 弹窗：新建自定义规则 -->
    <SecLabDialog
      :visible="isRuleDialogOpen"
      :title="t('app.simulation.rules.newBtn')"
      width="800px"
      @close="isRuleDialogOpen = false"
    >
      <div class="dialog-detail-content flex-column gap-layout">
        <!-- 基础信息 -->
        <div class="section-card card-box">
          <h4>{{ t('app.simulation.rules.title') }}</h4>
          <div class="form-group">
            <label>{{ t('app.simulation.rules.fields.name') }}</label>
            <SecLabInput
              v-model="ruleForm.name"
              :placeholder="t('app.simulation.rules.fields.namePlaceholder')"
            />
          </div>
          <div class="form-group">
            <label>{{ t('app.simulation.rules.fields.protocol') }}</label>
            <SecLabSelect v-model="ruleForm.protocol" :options="protocolOptions" disabled />
          </div>
          <div class="form-group">
            <label>{{ t('app.simulation.rules.fields.serverHeader') }}</label>
            <SecLabInput
              v-model="ruleForm.serverHeader"
              :placeholder="t('app.simulation.rules.fields.serverHeaderPlaceholder')"
            />
          </div>
          <div class="form-group">
            <label>{{ t('app.simulation.rules.fields.html') }}</label>
            <textarea
              v-model="ruleForm.html"
              class="textarea-input"
              :placeholder="t('app.simulation.rules.fields.htmlPlaceholder')"
            ></textarea>
          </div>
        </div>

        <!-- 漏洞路由添加 -->
        <div class="section-card card-box">
          <div class="drawer-card-header" data-slot="header">
            <h4>{{ t('app.simulation.rules.exploitPaths.title') }}</h4>
            <SecLabButton type="secondary" size="small" @click="addExploitPath">
              + {{ t('app.simulation.rules.exploitPaths.addBtn') }}
            </SecLabButton>
          </div>

          <div class="exploit-paths-list">
            <div
              v-for="(ep, idx) in exploitPaths"
              :key="idx"
              class="exploit-path-card card-box border-dashed"
            >
              <div class="card-title flex-layout flex-between">
                <h5>{{ t('app.simulation.rules.exploitPaths.routeFeature') }} #{{ idx + 1 }}</h5>
                <SecLabButton type="danger" size="small" @click="removeExploitPath(idx)">
                  {{ t('common.delete') }}
                </SecLabButton>
              </div>
              <div class="form-group">
                <label>{{ t('app.simulation.rules.exploitPaths.path') }}</label>
                <SecLabInput
                  v-model="ep.path"
                  :placeholder="t('app.simulation.rules.exploitPaths.pathPlaceholder')"
                />
              </div>
              <div class="flex-layout gap-layout">
                <div class="form-group flex-1">
                  <label>{{ t('app.simulation.rules.exploitPaths.method') }}</label>
                  <SecLabSelect v-model="ep.triggerMethod" :options="methodOptions" />
                </div>
                <div class="form-group flex-1">
                  <label>{{ t('app.simulation.rules.exploitPaths.status') }}</label>
                  <SecLabInput
                    v-model="ep.responseStatus"
                    :placeholder="t('app.simulation.rules.exploitPaths.statusPlaceholder')"
                  />
                </div>
              </div>
              <div class="form-group">
                <label>{{ t('app.simulation.rules.exploitPaths.body') }}</label>
                <textarea
                  v-model="ep.responseBody"
                  class="textarea-input-sm"
                  :placeholder="t('app.simulation.rules.exploitPaths.bodyPlaceholder')"
                ></textarea>
              </div>
            </div>
          </div>
        </div>
      </div>

      <template #footer>
        <div class="flex-layout flex-end gap-layout">
          <SecLabButton type="secondary" @click="isRuleDialogOpen = false">
            {{ t('confirmation.cancel') }}
          </SecLabButton>
          <SecLabButton type="primary" @click="handleCreateRule">
            {{ t('common.create') }}
          </SecLabButton>
        </div>
      </template>
    </SecLabDialog>

    <!-- 加载等待遮罩 -->
    <SecLabLoading :loading="isLoading" cover />
  </div>
</template>

<style scoped>
.simulation-dashboard {
  height: 100%;
  padding: var(--sdl-space-4);
  background: var(--sdl-bg-canvas);
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-4);
  min-height: 0;
  overflow: hidden;
}

.dashboard-header {
  flex-shrink: 0;
  display: flex;
  justify-content: space-between;
  align-items: center;
  border-bottom: 1px solid rgba(148, 163, 184, 0.12);
  padding-bottom: var(--sdl-space-3);
}

/* Tabs */
.tab-box {
  display: flex;
  background: var(--sdl-bg-panel);
  border: 1px solid rgba(148, 163, 184, 0.12);
  border-radius: var(--sdl-radius-md);
  padding: 2px;
}

.tab-btn {
  border: none;
  background: transparent;
  color: var(--sdl-text-muted);
  font-size: 13px;
  font-weight: 500;
  padding: 8px 16px;
  border-radius: var(--sdl-radius-sm);
  cursor: pointer;
  transition: all 0.15s ease;
}

.tab-btn:hover {
  color: var(--sdl-text-primary);
  background: rgba(148, 163, 184, 0.05);
}

.tab-btn.active {
  color: var(--sdl-primary);
  background: var(--sdl-bg-card);
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.2);
}

/* Body */
.dashboard-body {
  flex: 1;
  min-height: 0;
}

.tab-content {
  height: 100%;
  min-height: 0;
}

/* 常用布局 */
.flex-layout {
  display: flex;
}

.flex-column {
  display: flex;
  flex-direction: column;
}

.flex-1 {
  flex: 1;
  min-height: 0;
}

.flex-2 {
  flex: 2;
  min-height: 0;
}

.flex-center {
  justify-content: center;
  align-items: center;
}

.flex-end {
  justify-content: flex-end;
}

.flex-between {
  justify-content: space-between;
}

.flex-align-center {
  align-items: center;
}

.gap-layout {
  gap: var(--sdl-space-4);
}

.gap-2 {
  gap: var(--sdl-space-2);
}

.border-bottom {
  border-bottom: 1px solid rgba(148, 163, 184, 0.12);
}

.border-top {
  border-top: 1px solid rgba(148, 163, 184, 0.12);
}

.overflow-auto {
  overflow: auto;
}

/* Sidebar & Main */
.card-bg {
  background: var(--sdl-bg-panel);
  border: 1px solid rgba(148, 163, 184, 0.12);
  border-radius: var(--sdl-radius-lg);
  box-sizing: border-box;
}

.main-panel {
  flex: 1;
  min-width: 0;
  overflow: hidden;
}

.panel-header {
  padding: var(--sdl-space-3) var(--sdl-space-4);
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-shrink: 0;
}

.panel-header h3 {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
  color: var(--sdl-text-primary);
}

.table-actions {
  display: flex;
  gap: var(--sdl-space-2);
  justify-content: center;
}

.card-box {
  background: var(--sdl-bg-card);
  border: 1px solid rgba(148, 163, 184, 0.08);
  border-radius: var(--sdl-radius-md);
  padding: var(--sdl-space-4);
}

.border-dashed {
  border-style: dashed;
}

.param-row {
  display: flex;
  margin-bottom: var(--sdl-space-2);
  font-size: 13px;
}

.param-row .label {
  color: var(--sdl-text-muted);
  width: 140px;
  flex-shrink: 0;
}

.param-row .val {
  color: var(--sdl-text-primary);
}

.mono {
  font-family: var(--sdl-font-mono);
}

.code-preview {
  margin: 0;
  background: rgba(8, 14, 24, 0.5);
  padding: var(--sdl-space-3);
  border-radius: var(--sdl-radius-sm);
  color: var(--sdl-text-secondary);
  font-family: var(--sdl-font-mono);
  font-size: 12px;
  max-height: 240px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-all;
}

.empty-placeholder {
  text-align: center;
  padding: 32px 16px;
  color: var(--sdl-text-subtle);
  font-size: 13px;
}

/* 顶部选择过滤栏 */
.control-header {
  padding: var(--sdl-space-3) var(--sdl-space-4);
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.filter-item {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
}

.filter-item .label {
  font-size: 13px;
  font-weight: 500;
  color: var(--sdl-text-secondary);
}

.node-select-filter {
  width: 260px;
}

/* 分页条 */
.pagination-bar {
  padding: var(--sdl-space-3) var(--sdl-space-4);
  flex-shrink: 0;
}

/* Logs and download pcap button */
.mono-time {
  font-family: var(--sdl-font-mono);
  font-size: 12px;
  color: var(--sdl-text-secondary);
}

.disabled-text {
  color: var(--sdl-text-subtle);
  font-size: 12px;
}

.log-actions {
  display: flex;
  justify-content: center;
}

.dialog-panel {
  width: 500px;
  box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
  border-radius: var(--sdl-radius-lg);
  overflow: hidden;
  animation: scaleUp 0.2s cubic-bezier(0.16, 1, 0.3, 1) forwards;
}

.table-scroll-container {
  max-height: 220px;
  overflow-y: auto;
  border-radius: var(--sdl-radius-md);
  border: 1px solid var(--sdl-border-subtle);
}

.code-preview-container {
  max-height: 180px;
  overflow: auto;
  border-radius: var(--sdl-radius-md);
  border: 1px solid var(--sdl-border-subtle);
  background-color: var(--sdl-bg-input);
}

.code-preview {
  margin: 0;
  padding: var(--sdl-space-3);
  font-family: var(--sdl-font-mono);
  font-size: var(--sdl-font-body-sm);
  color: var(--sdl-text-secondary);
  white-space: pre-wrap;
  word-break: break-all;
}

.section-header-row {
  margin-bottom: 14px;
}

.section-header-row h4 {
  margin: 0 !important;
}

.mini-tab-box {
  display: flex;
  background-color: rgba(8, 14, 24, 0.6);
  border: 1px solid var(--sdl-border-subtle);
  border-radius: var(--sdl-radius-md);
  padding: 2px;
  box-shadow: inset 0 1px 3px rgba(0, 0, 0, 0.3);
}

.mini-tab-btn {
  background: transparent;
  border: none;
  color: var(--sdl-text-muted);
  font-size: 12px;
  font-weight: 600;
  padding: 4px 12px;
  border-radius: var(--sdl-radius-sm);
  cursor: pointer;
  transition: all 0.15s cubic-bezier(0.16, 1, 0.3, 1);
}

.mini-tab-btn:hover {
  color: var(--sdl-text-primary);
  background-color: rgba(148, 163, 184, 0.05);
}

.mini-tab-btn.active {
  background-color: var(--sdl-bg-card);
  color: var(--sdl-primary);
  box-shadow: 0 2px 5px rgba(0, 0, 0, 0.3);
}

.browser-launch-btn {
  height: 24px;
  line-height: 1;
  padding: 0 var(--sdl-space-2);
  font-size: 11px;
}

.iframe-preview-container {
  height: 240px;
  border-radius: var(--sdl-radius-md);
  border: 1px solid var(--sdl-border-subtle);
  background-color: #ffffff;
  overflow: hidden;
}

.html-iframe-preview {
  width: 100%;
  height: 100%;
  border: none;
  background: #ffffff;
}

@keyframes scaleUp {
  from {
    transform: scale(0.9);
    opacity: 0;
  }
  to {
    transform: scale(1);
    opacity: 1;
  }
}

.section-card h4 {
  margin: 0 0 12px;
  font-size: 14px;
  color: var(--sdl-primary);
  font-weight: 600;
}

/* Form Styles inside Drawer */
.form-group {
  margin-bottom: var(--sdl-space-3);
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-2);
}

.form-group label {
  font-size: 12px;
  color: var(--sdl-text-secondary);
  font-weight: 500;
}

.textarea-input {
  width: 100%;
  height: 140px;
  padding: var(--sdl-space-2);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  background: var(--sdl-bg-input);
  color: var(--sdl-text-primary);
  box-sizing: border-box;
  font-family: var(--sdl-font-mono);
  font-size: 12px;
  resize: vertical;
}

.textarea-input-sm {
  width: 100%;
  height: 90px;
  padding: var(--sdl-space-2);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  background: var(--sdl-bg-input);
  color: var(--sdl-text-primary);
  box-sizing: border-box;
  font-family: var(--sdl-font-mono);
  font-size: 12px;
  resize: vertical;
}

.drawer-card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: var(--sdl-space-3);
}

.drawer-card-header h4 {
  margin: 0;
  font-size: 14px;
  color: var(--sdl-primary);
  font-weight: 600;
}

.exploit-paths-list {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-4);
}

.exploit-path-card {
  position: relative;
}

.exploit-path-card h5 {
  margin: 0 0 12px;
  font-size: 13px;
  color: var(--sdl-text-primary);
  font-weight: 600;
}

/* Forensic Capture Styles */
.forensic-cell {
  display: flex;
  justify-content: center;
  align-items: center;
}

.capturing-wrapper {
  display: flex;
  align-items: center;
  gap: 8px;
}

.ready-wrapper {
  display: flex;
  align-items: center;
  gap: 8px;
}

.download-btn-link {
  text-decoration: none;
  display: inline-flex;
}

.rule-name-cell {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--sdl-space-2);
}

.rule-link-text {
  color: var(--sdl-primary);
  text-decoration: none;
  cursor: pointer;
  transition: color 0.15s ease;
}

.rule-link-text:hover {
  text-decoration: underline;
  opacity: 0.85;
}

.log-summary-cell {
  max-width: 420px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.capturing-row-indicator {
  width: 7px;
  height: 7px;
  background-color: var(--sdl-danger);
  border-radius: var(--sdl-radius-pill);
  display: inline-block;
  position: relative;
  flex-shrink: 0;
  box-shadow: 0 0 0 0 rgba(255, 94, 122, 0.7);
  animation: pulse-recording 1.2s infinite cubic-bezier(0.4, 0, 0.6, 1);
}

@keyframes pulse-recording {
  0% {
    transform: scale(0.9);
    box-shadow: 0 0 0 0 rgba(255, 94, 122, 0.6);
  }
  60% {
    transform: scale(1.15);
    box-shadow: 0 0 0 5px rgba(255, 94, 122, 0);
  }
  100% {
    transform: scale(0.9);
    box-shadow: 0 0 0 0 rgba(255, 94, 122, 0);
  }
}

.live-pulse-bars {
  display: flex;
  align-items: flex-end;
  gap: 3px;
  height: 14px;
  padding-right: 4px;
  flex-shrink: 0;
}

.pulse-bar {
  width: 2px;
  height: 3px;
  background-color: var(--sdl-primary);
  border-radius: var(--sdl-radius-xs);
  animation: pulse-jump 0.8s infinite ease-in-out alternate;
}

.pulse-bar:nth-child(1) {
  animation-delay: 0.1s;
  height: 5px;
}
.pulse-bar:nth-child(2) {
  animation-delay: 0.3s;
  height: 9px;
}
.pulse-bar:nth-child(3) {
  animation-delay: 0.5s;
  height: 4px;
}
.pulse-bar:nth-child(4) {
  animation-delay: 0.2s;
  height: 7px;
}

@keyframes pulse-jump {
  0% {
    height: 3px;
    transform: scaleY(1);
    background-color: var(--sdl-primary);
  }
  100% {
    height: 14px;
    transform: scaleY(1.2);
    background-color: var(--sdl-secondary);
  }
}

.download-pulse-btn {
  position: relative;
  animation: button-breath 2s infinite ease-in-out;
  border-color: var(--sdl-primary) !important;
}

@keyframes button-breath {
  0% {
    box-shadow: 0 0 0 0 rgba(0, 200, 255, 0.4);
  }
  50% {
    box-shadow: 0 0 10px 3px rgba(0, 200, 255, 0.15);
  }
  100% {
    box-shadow: 0 0 0 0 rgba(0, 200, 255, 0);
  }
}
</style>

<style>
/* 优化超长审计文本在气泡提示中的折行显示 */
body .sl-tooltip-content {
  max-width: 450px;
  white-space: normal !important;
  word-break: break-all;
}
</style>
