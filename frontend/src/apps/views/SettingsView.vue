<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { formatDateTime } from '@/utils/time'
import { seclabApi } from '@/api/modules/seclab'
import { securityApi } from '@/api/modules/security'
import { systemApi } from '@/api/modules/system'
import {
  upgradesApi,
  type UpgradePlanDetail,
  type UpgradeReleaseRecord,
} from '@/api/modules/upgrades'
import type { SystemAboutInfo } from '@/api/interface/system'
import { useNodeStore } from '@/stores/node'
import { useNotificationStore } from '@/stores/notification'
import { useThemeStore } from '@/stores/theme'
import { useWindowManagerStore } from '@/stores/window-manager'
import { useConfirmationModalStore } from '@/stores/confirmation-modal'
import { forgetLoginEntry, rememberLoginEntry } from '@/utils/login-entry'
import {
  SecLabButton,
  SecLabCard,
  SecLabSelect,
  SecLabAlert,
  SecLabFormItem,
  SecLabDescriptions,
  SecLabMenu,
  SecLabInput,
  SecLabEmpty,
  SecLabLoading,
  SecLabTable,
  SecLabTag,
} from '@/components/ui'
import type { SecLabTableColumn } from '@/components/ui/SecLabTable.vue'
import SystemMonitoringSettings from './settings/SystemMonitoringSettings.vue'

const props = defineProps<{
  isMaximized?: boolean
  windowId?: string
  payload?: Record<string, unknown>
}>()

const router = useRouter()
const { t, te, locale } = useI18n()
const nodeStore = useNodeStore()
const notificationStore = useNotificationStore()
const themeStore = useThemeStore()
const windowStore = useWindowManagerStore()
const confirmationStore = useConfirmationModalStore()
const systemClient = computed(() => systemApi.forNode(nodeStore.currentNodeId))

const appVersion = import.meta.env.VITE_APP_VERSION ?? 'unknown'
const gitHash = import.meta.env.VITE_GIT_HASH ?? 'unknown'

const networkLoading = ref(false)
const monitoringBusy = ref(false)
const networkSaving = ref(false)
const networkForm = ref({
  host: '',
  port: 0,
  publicHost: '',
})
const securityLoading = ref(false)
const securitySaving = ref(false)
const credentialSaving = ref(false)
const securityForm = ref({
  safeEntry: '',
  passwordComplexity: false,
})
const usernameForm = ref('')
const passwordForm = ref({
  newPassword: '',
  confirmPassword: '',
})

interface InterfaceOption {
  value: string
  label: string
}

const fetchedInterfaces = ref<InterfaceOption[]>([])
const hasLoadedInterfaces = ref(false)

const interfaceOptions = computed<InterfaceOption[]>(() => {
  const items = hasLoadedInterfaces.value
    ? fetchedInterfaces.value
    : [
        { value: '::', label: '' },
        { value: '0.0.0.0', label: '' },
      ]

  return items.map((item) => {
    if (item.value === '::') {
      return { value: '::', label: t('app.settings.network.interfaceOptions.dualStack') }
    }
    if (item.value === '0.0.0.0') {
      return { value: '0.0.0.0', label: t('app.settings.network.interfaceOptions.ipv4Only') }
    }
    return item
  })
})

const loadInterfaces = async () => {
  try {
    const response = await seclabApi.fetchInterfaces()
    if (response.success && response.data) {
      fetchedInterfaces.value = response.data.map((item) => ({
        value: item.ip,
        label: item.label,
      }))
      hasLoadedInterfaces.value = true
    }
  } catch (error) {
    console.error('Failed to load system network interfaces', error)
  }
}

const aboutLoading = ref(false)
const aboutCopying = ref(false)
const aboutInfo = ref<SystemAboutInfo | null>(null)
const upgradeLoading = ref(false)
const upgradeSyncing = ref(false)
const upgradeStarting = ref(false)
const upgradeUploading = ref(false)
const upgradeReleases = ref<UpgradeReleaseRecord[]>([])
const selectedUpgradeVersion = ref('')
const upgradePlan = ref<UpgradePlanDetail | null>(null)

// 上传与回显相关变量
const uploadFile = ref<File | null>(null)
const uploadProgress = ref(0)
const fileInputRef = ref<HTMLInputElement | null>(null)
const uploadedRelease = ref<UpgradeReleaseRecord | null>(null)
const isValidating = ref(false)

const uploadStatusText = computed(() => {
  if (uploadProgress.value >= 100 || isValidating.value) {
    return t('app.settings.upgrade.validating')
  }
  return t('app.settings.upgrade.uploadProgress', { percent: uploadProgress.value })
})

const normalizeDisplayVersion = (version: string) => version.trim().replace(/^v/i, '')

const isSameAsCurrentVersion = (version: string) =>
  normalizeDisplayVersion(version) === normalizeDisplayVersion(appVersion)

// 升级进度百分比计算
const upgradeProgressPercent = computed(() => {
  if (!upgradePlan.value || upgradePlan.value.targets.length === 0) return 0
  const finished = upgradePlan.value.targets.filter(
    (t) => t.status === 'succeeded' || t.status === 'failed',
  ).length
  return Math.round((finished * 100) / upgradePlan.value.targets.length)
})

const settingsBusy = computed(
  () =>
    networkLoading.value ||
    monitoringBusy.value ||
    networkSaving.value ||
    securityLoading.value ||
    securitySaving.value ||
    credentialSaving.value ||
    aboutLoading.value ||
    aboutCopying.value ||
    upgradeLoading.value ||
    upgradeSyncing.value ||
    upgradeUploading.value ||
    isValidating.value ||
    upgradeStarting.value,
)

watch(
  settingsBusy,
  (busy) => {
    if (!props.windowId) return
    windowStore.updateWindowRuntimeState(props.windowId, {
      busy,
      allowsNodeSwitch: false,
      blockLevel: busy ? 'busy' : 'open',
      blockReason: busy ? t('app.settings.guardBusy') : t('app.settings.guardOpen'),
    })
  },
  { immediate: true },
)

// --- 菜单定义 ---
const menuItems = computed(() => [
  {
    key: 'system',
    label: t('app.settings.menu.system'),
    children: [
      { key: 'about', label: t('app.settings.menu.about') },
      { key: 'monitoring', label: t('app.settings.menu.monitoring') },
      { key: 'network', label: t('app.settings.menu.network') },
      { key: 'security', label: t('app.settings.menu.security') },
      { key: 'upgrade', label: t('app.settings.menu.upgrade') },
    ],
  },
  {
    key: 'personalization',
    label: t('app.settings.menu.personalization'),
    children: [
      { key: 'language', label: t('app.settings.menu.language') },
      { key: 'theme', label: t('app.settings.menu.theme') },
    ],
  },
])

const activeMenu = ref('about')

const releaseColumns = computed<SecLabTableColumn[]>(() => [
  { prop: 'version', label: t('app.settings.upgrade.version'), minWidth: 120, align: 'center' },
  {
    prop: 'channel',
    label: t('app.settings.upgrade.channel'),
    width: 110,
    slot: 'channel',
    align: 'center',
  },
  {
    prop: 'checksumStatus',
    label: t('app.settings.upgrade.checksum'),
    width: 120,
    slot: 'checksum',
    align: 'center',
  },
  {
    prop: 'signatureStatus',
    label: t('app.settings.upgrade.signature'),
    width: 120,
    slot: 'signature',
    align: 'center',
  },
  {
    prop: 'publishedAt',
    label: t('app.settings.upgrade.publishedAt'),
    minWidth: 160,
    slot: 'publishedAt',
    align: 'center',
  },
  {
    prop: 'version',
    label: t('app.settings.upgrade.actions'),
    width: 180,
    slot: 'actions',
    align: 'center',
  },
])

const targetColumns = computed<SecLabTableColumn[]>(() => [
  { prop: 'targetType', label: t('app.settings.upgrade.targetType'), width: 120 },
  { prop: 'nodeId', label: t('app.settings.upgrade.nodeId'), minWidth: 160, slot: 'nodeId' },
  { prop: 'targetVersion', label: t('app.settings.upgrade.targetVersion'), width: 120 },
  { prop: 'status', label: t('app.settings.upgrade.status'), width: 120, slot: 'status' },
  { prop: 'errorDetail', label: t('app.settings.upgrade.error'), minWidth: 180, slot: 'error' },
])

// --- 信息项定义 ---
const softwareInfoItems = computed(() => [
  { label: t('app.settings.about.version'), value: appVersion },
  { label: t('app.settings.about.hash'), value: gitHash },
])

const hardwareInfoItems = computed(() => [
  { label: t('app.settings.about.hostname'), value: textOrPlaceholder(aboutInfo.value?.hostname) },
  { label: t('app.settings.about.osName'), value: textOrPlaceholder(aboutInfo.value?.osName) },
  {
    label: t('app.settings.about.osVersion'),
    value: textOrPlaceholder(aboutInfo.value?.osVersion),
  },
  {
    label: t('app.settings.about.kernelVersion'),
    value: textOrPlaceholder(aboutInfo.value?.kernelVersion),
  },
  {
    label: t('app.settings.about.architecture'),
    value: textOrPlaceholder(aboutInfo.value?.architecture),
  },
  { label: t('app.settings.about.cpu'), value: cpuText() },
  {
    label: t('app.settings.about.cpuVendor'),
    value: textOrPlaceholder(aboutInfo.value?.cpuVendor),
  },
  {
    label: t('app.settings.about.cpuPhysicalCores'),
    value: aboutInfo.value?.cpuPhysicalCores || '--',
  },
  {
    label: t('app.settings.about.cpuLogicalCores'),
    value: aboutInfo.value?.cpuLogicalCores || '--',
  },
  {
    label: t('app.settings.about.cpuFrequency'),
    value: formatCpuFreq(aboutInfo.value?.cpuFrequencyMhz),
  },
  {
    label: t('app.settings.about.cpuCacheL3'),
    value: textOrPlaceholder(aboutInfo.value?.cpuCacheL3),
  },
  { label: t('app.settings.about.memory'), value: formatBytes(aboutInfo.value?.memoryTotalBytes) },
  {
    label: t('app.settings.about.memoryAvailable'),
    value: formatBytes(aboutInfo.value?.memoryAvailableBytes),
  },
  { label: t('app.settings.about.swapTotal'), value: formatBytes(aboutInfo.value?.swapTotalBytes) },
  { label: t('app.settings.about.swapUsed'), value: formatBytes(aboutInfo.value?.swapUsedBytes) },
  { label: t('app.settings.about.uptime'), value: formatUptime(aboutInfo.value?.uptimeSeconds) },
  {
    label: t('app.settings.about.bootTime'),
    value: formatDateTime(aboutInfo.value?.bootTimeEpoch),
  },
  { label: t('app.settings.about.timezone'), value: textOrPlaceholder(aboutInfo.value?.timezone) },
  { label: t('app.settings.about.locale'), value: textOrPlaceholder(aboutInfo.value?.locale) },
  {
    label: t('app.settings.about.virtualization'),
    value: textOrPlaceholder(aboutInfo.value?.virtualization),
  },
  {
    label: t('app.settings.about.primaryInterface'),
    value: textOrPlaceholder(aboutInfo.value?.primaryInterface),
  },
  {
    label: t('app.settings.about.macAddress'),
    value: textOrPlaceholder(aboutInfo.value?.macAddress),
  },
  {
    label: t('app.settings.about.ipv4Addresses'),
    value: formatIpList(aboutInfo.value?.ipv4Addresses),
  },
  {
    label: t('app.settings.about.ipv6Addresses'),
    value: formatIpList(aboutInfo.value?.ipv6Addresses),
  },
  {
    label: t('app.settings.about.defaultGateway'),
    value: textOrPlaceholder(aboutInfo.value?.defaultGateway),
  },
  { label: t('app.settings.about.dnsServers'), value: formatDnsList(aboutInfo.value?.dnsServers) },
  { label: t('app.settings.about.disk'), value: formatBytes(aboutInfo.value?.diskTotalBytes) },
])

const handleLocaleChange = (value: string | number | boolean | null) => {
  if (value === 'en' || value === 'zh') {
    locale.value = value
    localStorage.setItem('seclab_locale', value)
  }
}

const handleThemeChange = (value: string | number | boolean | null) => {
  if (value === 'light' || value === 'dark') {
    themeStore.setTheme(value)
  }
}

const loadNetworkConfig = async () => {
  networkLoading.value = true
  try {
    await loadInterfaces()
    const response = await seclabApi.fetchNetwork()
    if (!response.success) {
      notificationStore.error(response.message || t('app.settings.network.loadFailed'))
      return
    }
    if (response.data) {
      networkForm.value.host = response.data.host
      networkForm.value.port = response.data.port
      networkForm.value.publicHost = response.data.publicHost || ''
    }
  } catch (error) {
    console.error('Failed to load SecLab network config', error)
    notificationStore.error(t('app.settings.network.loadFailed'))
  } finally {
    networkLoading.value = false
  }
}

const saveNetworkConfig = async () => {
  if (!networkForm.value.host.trim()) {
    notificationStore.error(t('app.settings.network.validationHost'))
    return
  }
  if (
    !Number.isInteger(networkForm.value.port) ||
    networkForm.value.port < 1 ||
    networkForm.value.port > 65535
  ) {
    notificationStore.error(t('app.settings.network.validationPort'))
    return
  }

  networkSaving.value = true
  try {
    const response = await seclabApi.updateNetwork({
      host: networkForm.value.host.trim(),
      port: networkForm.value.port,
      publicHost: networkForm.value.publicHost.trim() || null,
    })
    if (!response.success || !response.data) {
      notificationStore.error(response.message || t('app.settings.network.saveFailed'))
      return
    }
    notificationStore.success(t('app.settings.network.saveSuccess'))
    const target = response.data.nextUrl
    setTimeout(() => {
      window.location.replace(target)
    }, 400)
  } catch (error) {
    console.error('Failed to save SecLab network config', error)
    notificationStore.error(t('app.settings.network.saveFailed'))
  } finally {
    networkSaving.value = false
  }
}

const generateSafeEntryValue = () => {
  const chars = 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789'
  const cryptoApi = window.crypto
  const bytes = new Uint8Array(16)
  cryptoApi.getRandomValues(bytes)
  return Array.from(bytes, (byte) => chars[byte % chars.length]).join('')
}

const reservedSafeEntryPrefixes = [
  'api',
  'assets',
  'images',
  'favicon',
  'static',
  'public',
  'health',
  'metrics',
  'ws',
  'wss',
  'robots',
]

const isValidSafeEntry = (value: string) => {
  if (!/^[A-Za-z0-9]{8,32}$/.test(value)) return false
  const lower = value.toLowerCase()
  return !reservedSafeEntryPrefixes.some((prefix) => lower.startsWith(prefix))
}

const loadSecuritySettings = async () => {
  securityLoading.value = true
  try {
    const response = await securityApi.fetchSettings()
    if (!response.success || !response.data) {
      notificationStore.error(response.message || t('app.settings.security.loadFailed'))
      return
    }
    securityForm.value.safeEntry = response.data.safeEntry
    securityForm.value.passwordComplexity = response.data.passwordComplexity
    usernameForm.value = ''
  } catch (error) {
    console.error('Failed to load security settings', error)
    notificationStore.error(t('app.settings.security.loadFailed'))
  } finally {
    securityLoading.value = false
  }
}

const saveSecuritySettings = async () => {
  const safeEntry = securityForm.value.safeEntry.trim()
  if (safeEntry && !isValidSafeEntry(safeEntry)) {
    notificationStore.error(t('app.settings.security.safeEntryInvalid'))
    return
  }
  securitySaving.value = true
  try {
    const response = await securityApi.updateSettings({
      safeEntry,
      passwordComplexity: securityForm.value.passwordComplexity,
    })
    if (!response.success) {
      notificationStore.error(response.message || t('app.settings.security.saveFailed'))
      return
    }
    securityForm.value.safeEntry = safeEntry
    if (safeEntry) {
      rememberLoginEntry(`/${safeEntry}`)
    } else {
      forgetLoginEntry()
    }
    notificationStore.success(t('app.settings.security.saveSuccess'))
  } catch (error) {
    console.error('Failed to save security settings', error)
    notificationStore.error(t('app.settings.security.saveFailed'))
  } finally {
    securitySaving.value = false
  }
}

const regenerateSafeEntry = () => {
  securityForm.value.safeEntry = generateSafeEntryValue()
}

const clearSafeEntry = () => {
  securityForm.value.safeEntry = ''
}

const saveUsername = async () => {
  const username = usernameForm.value.trim()
  if (!/^[A-Za-z0-9_][A-Za-z0-9_-]{0,63}$/.test(username)) {
    notificationStore.error(t('app.settings.security.usernameInvalid'))
    return
  }
  credentialSaving.value = true
  try {
    const response = await securityApi.updateUsername(username)
    if (!response.success) {
      notificationStore.error(response.message || t('app.settings.security.usernameFailed'))
      return
    }
    usernameForm.value = ''
    notificationStore.success(t('app.settings.security.usernameSuccess'))
  } catch (error) {
    console.error('Failed to update username', error)
    notificationStore.error(t('app.settings.security.usernameFailed'))
  } finally {
    credentialSaving.value = false
  }
}

const savePassword = async () => {
  if (!passwordForm.value.newPassword) {
    notificationStore.error(t('app.settings.security.passwordRequired'))
    return
  }
  if (passwordForm.value.newPassword.length < 5) {
    notificationStore.error(t('app.settings.security.passwordTooShort'))
    return
  }
  if (passwordForm.value.newPassword !== passwordForm.value.confirmPassword) {
    notificationStore.error(t('app.settings.security.passwordMismatch'))
    return
  }
  credentialSaving.value = true
  try {
    const response = await securityApi.updatePassword(passwordForm.value.newPassword)
    if (!response.success) {
      notificationStore.error(response.message || t('app.settings.security.passwordFailed'))
      return
    }
    passwordForm.value.newPassword = ''
    passwordForm.value.confirmPassword = ''
    notificationStore.success(t('app.settings.security.passwordSuccess'))
  } catch (error) {
    console.error('Failed to update password', error)
    notificationStore.error(t('app.settings.security.passwordFailed'))
  } finally {
    credentialSaving.value = false
  }
}

function formatBytes(bytes?: number) {
  if (!bytes || bytes <= 0) return '--'
  const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB']
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  const value = bytes / Math.pow(1024, index)
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[index]}`
}

function textOrPlaceholder(value?: string) {
  if (!value) return '--'
  const normalized = value.trim()
  return normalized ? normalized : '--'
}

function cpuText() {
  if (!aboutInfo.value) return '--'
  const model = textOrPlaceholder(aboutInfo.value.cpuModel)
  const cores =
    aboutInfo.value.cpuLogicalCores > 0
      ? ` (${t('app.settings.about.logicalCores', { count: aboutInfo.value.cpuLogicalCores })})`
      : ''
  return `${model}${cores}`
}

function formatUptime(seconds?: number) {
  if (!seconds || seconds <= 0) return '--'
  const days = Math.floor(seconds / 86400)
  const hours = Math.floor((seconds % 86400) / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)
  if (days > 0) return `${days}d ${hours}h ${minutes}m`
  if (hours > 0) return `${hours}h ${minutes}m`
  return `${minutes}m`
}

function formatCpuFreq(freqMhz?: number) {
  if (!freqMhz || freqMhz <= 0) return '--'
  if (freqMhz >= 1000) return `${(freqMhz / 1000).toFixed(2)} GHz`
  return `${freqMhz} MHz`
}

function formatIpList(values?: string[]) {
  if (!values || values.length === 0) return '--'
  return values.join(', ')
}

function formatDnsList(values?: string[]) {
  if (!values || values.length === 0) return '--'
  return values.join(', ')
}

function buildAboutExport() {
  return {
    exportedAt: new Date().toISOString(),
    app: {
      version: appVersion,
      gitHash,
    },
    system: aboutInfo.value,
  }
}

const copyAboutInfo = async () => {
  const payload = buildAboutExport()
  const text = JSON.stringify(payload, null, 2)
  aboutCopying.value = true
  try {
    if (navigator.clipboard && typeof navigator.clipboard.writeText === 'function') {
      await navigator.clipboard.writeText(text)
    } else {
      const textarea = document.createElement('textarea')
      textarea.value = text
      textarea.style.position = 'fixed'
      textarea.style.opacity = '0'
      document.body.appendChild(textarea)
      textarea.focus()
      textarea.select()
      const copied = document.execCommand('copy')
      document.body.removeChild(textarea)
      if (!copied) throw new Error('copy failed')
    }
    notificationStore.success(t('app.settings.about.copySuccess'))
  } catch (error) {
    console.error('Failed to copy system about info', error)
    notificationStore.error(t('app.settings.about.copyFailed'))
  } finally {
    aboutCopying.value = false
  }
}

const loadAboutInfo = async () => {
  aboutLoading.value = true
  try {
    const response = await systemClient.value.fetchAbout()
    if (!response.success) {
      notificationStore.error(response.message || t('app.settings.about.loadFailed'))
      return
    }
    aboutInfo.value = response.data ?? null
  } catch (error) {
    console.error('Failed to load system about info', error)
    notificationStore.error(t('app.settings.about.loadFailed'))
  } finally {
    aboutLoading.value = false
  }
}

const loadUpgradeReleases = async () => {
  upgradeLoading.value = true
  try {
    const response = await upgradesApi.listReleases()
    if (!response.success) {
      notificationStore.error(response.message || t('app.settings.upgrade.loadFailed'))
      return
    }
    upgradeReleases.value = response.data ?? []
    selectedUpgradeVersion.value ||= upgradeReleases.value[0]?.version || ''
  } catch (error) {
    console.error('Failed to load upgrade releases', error)
    notificationStore.error(t('app.settings.upgrade.loadFailed'))
  } finally {
    upgradeLoading.value = false
  }
}

const triggerFileSelect = () => {
  fileInputRef.value?.click()
}

const onUpgradeFileChange = (event: Event) => {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0] ?? null
  if (file) {
    const fileName = file.name.toLowerCase()
    if (!fileName.endsWith('.tar.gz') && !fileName.endsWith('.tgz')) {
      notificationStore.error(t('app.settings.upgrade.invalidFormat'))
      uploadFile.value = null
      input.value = ''
      return
    }
  }
  uploadFile.value = file
  // 选择新文件时，清空之前上传成功的回显数据
  uploadedRelease.value = null
}

const uploadUpgradeRelease = async () => {
  if (!uploadFile.value) {
    notificationStore.error(t('app.settings.upgrade.selectFile'))
    return
  }
  upgradeUploading.value = true
  isValidating.value = false
  uploadProgress.value = 0
  try {
    const formData = new FormData()
    formData.append('file', uploadFile.value)

    const response = await upgradesApi.uploadRelease(formData, (progressEvent) => {
      if (progressEvent.total) {
        uploadProgress.value = Math.round((progressEvent.loaded * 100) / progressEvent.total)
        if (uploadProgress.value >= 100) {
          isValidating.value = true
        }
      }
    })

    if (!response.success) {
      const msg = response.message || 'app.settings.upgrade.uploadFailed'
      notificationStore.error(te(msg) ? t(msg) : msg)
      return
    }

    notificationStore.success(t('app.settings.upgrade.uploadSuccess'))
    uploadedRelease.value = response.data ?? null

    await loadUpgradeReleases()
    if (response.data?.version) {
      selectedUpgradeVersion.value = response.data.version
    }
    // 上传成功后清空选择的文件
    uploadFile.value = null
    if (fileInputRef.value) {
      fileInputRef.value.value = ''
    }
  } catch (error) {
    console.error('Failed to upload upgrade release', error)
    notificationStore.error(t('app.settings.upgrade.uploadFailed'))
  } finally {
    upgradeUploading.value = false
    isValidating.value = false
    uploadProgress.value = 0
  }
}

const startClusterUpgrade = async (overwriteSameVersion = false) => {
  if (!selectedUpgradeVersion.value) {
    notificationStore.error(t('app.settings.upgrade.selectVersion'))
    return
  }
  upgradeStarting.value = true
  try {
    const createResponse = await upgradesApi.createPlan({
      targetVersion: selectedUpgradeVersion.value,
      component: 'cluster',
      includeOffline: true,
      overwriteSameVersion,
    })
    if (!createResponse.success || !createResponse.data) {
      notificationStore.error(createResponse.message || t('app.settings.upgrade.startFailed'))
      return
    }
    const startResponse = await upgradesApi.startPlan(createResponse.data.plan.planId)
    if (!startResponse.success || !startResponse.data) {
      notificationStore.error(startResponse.message || t('app.settings.upgrade.startFailed'))
      return
    }
    upgradePlan.value = startResponse.data
    window.sessionStorage.setItem('seclab.activeUpgradePlanId', startResponse.data.plan.planId)
    window.sessionStorage.setItem(
      'seclab.activeUpgradePlanDetail',
      JSON.stringify(startResponse.data),
    )
    notificationStore.success(t('app.settings.upgrade.startSuccess'))
    router.push({ path: '/upgrade-progress', query: { planId: startResponse.data.plan.planId } })
  } catch (error) {
    console.error('Failed to start upgrade plan', error)
    notificationStore.error(t('app.settings.upgrade.startFailed'))
  } finally {
    upgradeStarting.value = false
  }
}

const goToNodeUpgradeGuide = () => {
  windowStore.openWindowWithPayload('node-manager', { highlightUpgrade: true })
}

const refreshUpgradePlan = async () => {
  const planId = upgradePlan.value?.plan.planId
  if (!planId) return
  try {
    const response = await upgradesApi.detail(planId)
    if (response.success && response.data) {
      upgradePlan.value = response.data
    }
  } catch (error) {
    console.error('Failed to refresh upgrade plan', error)
  }
}

function statusTagType(status?: string) {
  if (status === 'succeeded' || status === 'verified') return 'success'
  if (status === 'failed' || status === 'missing') return 'danger'
  if (status === 'running' || status === 'pending') return 'info'
  if (status === 'deferred' || status === 'unknown') return 'warning'
  return 'default'
}

const isTargetVersionEligible = (targetVersion: string) => {
  const release = upgradeReleases.value.find((item) => item.version === targetVersion)
  return release?.upgradeEligible ?? false
}

const startUpgradeForVersion = async (version: string) => {
  const overwriteSameVersion = isSameAsCurrentVersion(version)
  const confirmed = await confirmationStore.showConfirmation(
    overwriteSameVersion
      ? t('app.settings.upgrade.overwriteSameVersionConfirm', { version })
      : t('app.settings.upgrade.startConfirm', { version }),
    t('app.settings.upgrade.start'),
    t('common.confirm'),
    t('common.cancel'),
  )
  if (!confirmed) return
  selectedUpgradeVersion.value = version
  await startClusterUpgrade(overwriteSameVersion)
}

const isUpgradeFinished = (status?: string) => {
  if (!status) return false
  return status === 'succeeded' || status === 'failed' || status === 'unknown'
}

const resetUpgradeFlow = () => {
  upgradePlan.value = null
}

const deleteReleaseForVersion = async (version: string) => {
  const confirmed = await confirmationStore.showConfirmation(
    t('app.settings.upgrade.deleteConfirm', { version }),
    t('app.settings.upgrade.delete'),
    t('common.confirm'),
    t('common.cancel'),
  )
  if (!confirmed) return
  upgradeLoading.value = true
  try {
    const response = await upgradesApi.deleteRelease(version)
    if (response.success) {
      notificationStore.success(t('app.settings.upgrade.deleteSuccess'))
      await loadUpgradeReleases()
    } else {
      notificationStore.error(response.message || t('app.settings.upgrade.deleteFailed'))
    }
  } catch (error) {
    console.error('Failed to delete release', error)
    notificationStore.error(t('app.settings.upgrade.deleteFailed'))
  } finally {
    upgradeLoading.value = false
  }
}

onMounted(() => {
  void loadNetworkConfig()
  void loadSecuritySettings()
  void loadAboutInfo()
  void loadUpgradeReleases()
  window.setInterval(() => {
    void refreshUpgradePlan()
  }, 10000)
})
</script>

<template>
  <div class="settings" data-seclab-app="settings">
    <div class="sidebar">
      <SecLabCard class="nav-card" shadow="never" full-height>
        <SecLabMenu v-model="activeMenu" :items="menuItems" />
      </SecLabCard>
    </div>

    <div class="content">
      <div v-if="activeMenu === 'monitoring'" class="section">
        <SystemMonitoringSettings @busy-change="monitoringBusy = $event" />
      </div>

      <div v-else-if="activeMenu === 'network'" class="section">
        <SecLabCard class="card" shadow="never">
          <template #header>
            <h2 class="section-title">{{ t('app.settings.network.label') }}</h2>
          </template>

          <div class="form-wrapper">
            <p class="description">{{ t('app.settings.network.description') }}</p>

            <div v-if="!networkLoading" class="sl-form network-form">
              <SecLabFormItem :label="t('app.settings.network.host')">
                <SecLabSelect
                  v-model="networkForm.host"
                  :options="interfaceOptions"
                  class="select-control"
                />
              </SecLabFormItem>
              <SecLabFormItem :label="t('app.settings.network.port')">
                <SecLabInput
                  v-model.number="networkForm.port"
                  type="number"
                  :min="1"
                  :max="65535"
                />
              </SecLabFormItem>
              <SecLabFormItem :label="t('app.settings.network.publicHost')">
                <SecLabInput
                  v-model="networkForm.publicHost"
                  :placeholder="t('app.settings.network.publicHostPlaceholder')"
                />
              </SecLabFormItem>

              <SecLabAlert type="warning" :title="t('app.settings.network.warning')" show-icon />

              <div class="form-actions">
                <SecLabButton type="primary" :loading="networkSaving" @click="saveNetworkConfig">
                  {{ t('app.settings.network.save') }}
                </SecLabButton>
              </div>
            </div>
            <SecLabLoading
              :loading="networkLoading"
              :text="t('app.settings.network.loading')"
              cover
            />
          </div>
        </SecLabCard>
      </div>

      <div v-else-if="activeMenu === 'security'" class="section">
        <SecLabCard class="card" shadow="never">
          <template #header>
            <h2 class="section-title">{{ t('app.settings.security.label') }}</h2>
          </template>

          <div class="form-wrapper">
            <p class="description">{{ t('app.settings.security.description') }}</p>

            <div
              v-if="!securityLoading"
              class="sl-form security-form"
              data-page="settings-security"
            >
              <SecLabFormItem :label="t('app.settings.security.safeEntry')">
                <div class="inline-row">
                  <SecLabInput
                    v-model="securityForm.safeEntry"
                    :placeholder="t('app.settings.security.safeEntryPlaceholder')"
                  />
                  <SecLabButton @click="regenerateSafeEntry">
                    {{ t('app.settings.security.regenerate') }}
                  </SecLabButton>
                  <SecLabButton @click="clearSafeEntry">
                    {{ t('app.settings.security.disable') }}
                  </SecLabButton>
                </div>
              </SecLabFormItem>

              <SecLabFormItem :label="t('app.settings.security.passwordComplexity')">
                <label class="checkbox-line">
                  <input v-model="securityForm.passwordComplexity" type="checkbox" />
                  <span>{{ t('app.settings.security.passwordComplexityHint') }}</span>
                </label>
              </SecLabFormItem>

              <div class="form-actions">
                <SecLabButton
                  type="primary"
                  :loading="securitySaving"
                  @click="saveSecuritySettings"
                >
                  {{ t('app.settings.security.save') }}
                </SecLabButton>
              </div>

              <div class="settings-divider" />

              <SecLabFormItem :label="t('app.settings.security.newUsername')">
                <div class="inline-row">
                  <SecLabInput
                    v-model="usernameForm"
                    :placeholder="t('app.settings.security.usernamePlaceholder')"
                  />
                  <SecLabButton :loading="credentialSaving" @click="saveUsername">
                    {{ t('app.settings.security.changeUsername') }}
                  </SecLabButton>
                </div>
              </SecLabFormItem>

              <form class="credential-password-form" @submit.prevent>
                <input
                  v-model="usernameForm"
                  type="text"
                  name="username"
                  autocomplete="username"
                  class="sr-only"
                  tabindex="-1"
                />
                <SecLabFormItem :label="t('app.settings.security.newPassword')">
                  <SecLabInput
                    v-model="passwordForm.newPassword"
                    type="password"
                    autocomplete="new-password"
                    :placeholder="t('app.settings.security.newPasswordPlaceholder')"
                  />
                </SecLabFormItem>
                <SecLabFormItem :label="t('app.settings.security.confirmPassword')">
                  <div class="inline-row">
                    <SecLabInput
                      v-model="passwordForm.confirmPassword"
                      type="password"
                      autocomplete="new-password"
                      :placeholder="t('app.settings.security.confirmPasswordPlaceholder')"
                    />
                    <SecLabButton :loading="credentialSaving" @click="savePassword">
                      {{ t('app.settings.security.changePassword') }}
                    </SecLabButton>
                  </div>
                </SecLabFormItem>
              </form>
            </div>
            <SecLabLoading
              :loading="securityLoading"
              :text="t('app.settings.security.loading')"
              cover
            />
          </div>
        </SecLabCard>
      </div>

      <div v-else-if="activeMenu === 'language'" class="section">
        <SecLabCard class="card" shadow="never">
          <template #header>
            <h2 class="section-title">{{ t('app.settings.language.label') }}</h2>
          </template>
          <p class="description">{{ t('app.settings.language.description') }}</p>
          <SecLabSelect
            class="select-control"
            :model-value="locale"
            :options="[
              { value: 'zh', label: t('app.settings.language.zh') },
              { value: 'en', label: t('app.settings.language.en') },
            ]"
            @update:model-value="handleLocaleChange"
          />
        </SecLabCard>
      </div>

      <div v-else-if="activeMenu === 'theme'" class="section">
        <SecLabCard class="card" shadow="never">
          <template #header>
            <h2 class="section-title">{{ t('app.settings.theme.label') }}</h2>
          </template>
          <p class="description">{{ t('app.settings.theme.description') }}</p>
          <SecLabSelect
            class="select-control"
            :model-value="themeStore.currentTheme"
            :options="[
              { value: 'light', label: t('app.settings.theme.light') },
              { value: 'dark', label: t('app.settings.theme.dark') },
            ]"
            @update:model-value="handleThemeChange"
          />
        </SecLabCard>
      </div>

      <div v-else-if="activeMenu === 'upgrade'" class="section">
        <SecLabCard class="card" shadow="never">
          <template #header>
            <h2 class="section-title">{{ t('app.settings.upgrade.label') }}</h2>
          </template>

          <div class="upgrade-wrapper" data-page="settings-upgrade">
            <!-- 状态 A：没有正在执行的升级计划 -->
            <div v-if="!upgradePlan" class="upgrade-main-flow">
              <p class="description">{{ t('app.settings.upgrade.description') }}</p>

              <div class="fine-grained-upgrade-link" style="margin-bottom: 16px">
                <a
                  href="javascript:void(0)"
                  style="
                    color: var(--sdl-primary);
                    font-size: 13px;
                    text-decoration: none;
                    display: inline-flex;
                    align-items: center;
                    gap: 4px;
                  "
                  @click="goToNodeUpgradeGuide"
                >
                  {{ t('app.settings.upgrade.fineGrainedLink') }}
                </a>
              </div>

              <div
                class="upgrade-upload-area"
                style="position: relative"
                data-ui="upgrade-upload-panel"
              >
                <input
                  ref="fileInputRef"
                  type="file"
                  accept=".gz,.tgz,.tar.gz"
                  style="display: none"
                  @change="onUpgradeFileChange"
                />

                <div class="upload-control-row">
                  <!-- 长条文本框：显示选中文件名 -->
                  <SecLabInput
                    :model-value="uploadFile ? uploadFile.name : ''"
                    readonly
                    :placeholder="t('app.settings.upgrade.placeholder')"
                    class="upload-path-input"
                    data-slot="upload-path-input"
                  />
                  <!-- 选择文件按钮 -->
                  <SecLabButton
                    type="secondary"
                    data-ui="upgrade-file-select"
                    @click="triggerFileSelect"
                  >
                    {{ t('app.settings.upgrade.selectFileButton') }}
                  </SecLabButton>
                  <!-- 上传按钮 -->
                  <SecLabButton
                    type="primary"
                    :loading="upgradeUploading && !isValidating"
                    :disabled="!uploadFile"
                    data-ui="upgrade-upload-submit"
                    @click="uploadUpgradeRelease"
                  >
                    {{ t('app.settings.upgrade.upload') }}
                  </SecLabButton>
                </div>

                <!-- 上传进度条 -->
                <div v-if="upgradeUploading" class="progress-container upload-progress-bar">
                  <div class="progress-text">
                    {{ uploadStatusText }}
                  </div>
                  <div class="progress-bar-bg">
                    <div
                      class="progress-bar-fill animated-glow"
                      :style="{ width: uploadProgress + '%' }"
                    ></div>
                  </div>
                </div>

                <SecLabLoading
                  :loading="isValidating"
                  :text="t('app.settings.upgrade.validating')"
                  cover
                />
              </div>

              <!-- 下方的版本表格 -->
              <div class="releases-section" data-slot="upgrade-releases-list">
                <h3 class="sub-title">{{ t('app.settings.upgrade.releases') }}</h3>
                <SecLabTable
                  v-if="upgradeReleases.length > 0"
                  :data="upgradeReleases"
                  :columns="releaseColumns"
                  border
                >
                  <template #channel="{ row }">
                    <SecLabTag :type="row.channel === 'stable' ? 'success' : 'warning'">
                      {{ row.channel }}
                    </SecLabTag>
                  </template>
                  <template #checksum="{ row }">
                    <SecLabTag :type="statusTagType(row.checksumStatus)">
                      {{ row.checksumStatus }}
                    </SecLabTag>
                  </template>
                  <template #signature="{ row }">
                    <SecLabTag :type="statusTagType(row.signatureStatus)">
                      {{ row.signatureStatus }}
                    </SecLabTag>
                  </template>
                  <template #publishedAt="{ row }">
                    {{ row.publishedAt ? formatDateTime(row.publishedAt) : '--' }}
                  </template>
                  <template #actions="{ row }">
                    <div class="table-actions-row">
                      <SecLabButton
                        size="small"
                        type="primary"
                        :loading="upgradeStarting && selectedUpgradeVersion === row.version"
                        :disabled="!isTargetVersionEligible(row.version)"
                        @click="startUpgradeForVersion(row.version)"
                      >
                        {{ t('app.settings.upgrade.startUpgrade') }}
                      </SecLabButton>
                      <SecLabButton
                        size="small"
                        type="danger"
                        @click="deleteReleaseForVersion(row.version)"
                      >
                        {{ t('app.settings.upgrade.delete') }}
                      </SecLabButton>
                    </div>
                  </template>
                </SecLabTable>

                <SecLabEmpty
                  v-else
                  class="releases-empty"
                  :description="t('app.settings.upgrade.noReleases')"
                />
              </div>
            </div>

            <!-- 状态 B：正在进行升级或者升级结束 -->
            <div v-else class="upgrade-execution-flow" data-ui="upgrade-execution-panel">
              <div class="execution-header-panel">
                <div class="execution-title-row">
                  <h3 class="execution-title">
                    {{ t('app.settings.upgrade.plan') }}
                    <SecLabTag :type="statusTagType(upgradePlan.plan.status)" class="status-tag">
                      {{ upgradePlan.plan.status }}
                    </SecLabTag>
                  </h3>
                  <SecLabButton
                    v-if="isUpgradeFinished(upgradePlan.plan.status)"
                    size="small"
                    type="secondary"
                    data-ui="upgrade-back-button"
                    @click="resetUpgradeFlow"
                  >
                    {{ t('app.settings.upgrade.back') }}
                  </SecLabButton>
                </div>

                <div class="execution-metrics-row">
                  <div class="metric-block">
                    <span class="m-label">{{ t('app.settings.upgrade.targetVersion') }}</span>
                    <span class="m-value code-text">{{ upgradePlan.plan.targetVersion }}</span>
                  </div>
                  <div class="metric-block">
                    <span class="m-label">{{ t('app.settings.upgrade.planId') }}</span>
                    <span class="m-value code-text">{{ upgradePlan.plan.planId }}</span>
                  </div>
                </div>

                <!-- 进度条 -->
                <div class="progress-container upgrade-progress-bar">
                  <div class="progress-header">
                    <span class="progress-label">{{
                      t('app.settings.upgrade.upgradeProgress')
                    }}</span>
                    <span class="progress-percent font-bold">{{ upgradeProgressPercent }}%</span>
                  </div>
                  <div class="progress-bar-bg">
                    <div
                      class="progress-bar-fill animated-glow"
                      :style="{ width: upgradeProgressPercent + '%' }"
                    ></div>
                  </div>
                </div>
              </div>

              <!-- 节点与事件列表 -->
              <div class="upgrade-detail" data-slot="upgrade-plan-detail">
                <div class="upgrade-panel">
                  <h3 class="sub-title">{{ t('app.settings.upgrade.targets') }}</h3>
                  <SecLabTable :data="upgradePlan.targets" :columns="targetColumns" border>
                    <template #nodeId="{ row }">
                      {{ row.nodeId || 'local' }}
                    </template>
                    <template #status="{ row }">
                      <SecLabTag :type="statusTagType(row.status)">{{ row.status }}</SecLabTag>
                    </template>
                    <template #error="{ row }">
                      <span class="error-text">{{ row.errorDetail || '--' }}</span>
                    </template>
                  </SecLabTable>
                </div>
              </div>
            </div>

            <SecLabLoading
              :loading="upgradeLoading"
              :text="t('app.settings.upgrade.loading')"
              cover
            />
          </div>
        </SecLabCard>
      </div>

      <div v-else-if="activeMenu === 'about'" class="section">
        <SecLabCard class="card" shadow="never">
          <template #header>
            <h2 class="section-title">{{ t('app.settings.about.label') }}</h2>
          </template>

          <div class="about-wrapper">
            <div class="about-toolbar">
              <p class="description">{{ t('app.settings.about.description') }}</p>
              <SecLabButton size="small" :loading="aboutCopying" @click="copyAboutInfo">
                {{ t('app.settings.about.copyButton') }}
              </SecLabButton>
            </div>

            <div class="about-body">
              <h3 class="sub-title">{{ t('app.settings.about.software') }}</h3>
              <SecLabDescriptions :items="softwareInfoItems" border class="desc-block" />

              <h3 class="sub-title">{{ t('app.settings.about.hardware') }}</h3>
              <SecLabDescriptions :items="hardwareInfoItems" border class="desc-block" />
            </div>
            <SecLabLoading :loading="aboutLoading" :text="t('app.settings.about.loading')" cover />
          </div>
        </SecLabCard>
      </div>

      <div v-else class="section">
        <SecLabCard class="card" shadow="never">
          <SecLabEmpty :description="t('app.settings.wip')" />
        </SecLabCard>
      </div>
    </div>
  </div>
</template>

<style scoped>
.settings {
  display: flex;
  height: 100%;
  background: var(--sdl-bg-canvas);
  gap: var(--sdl-space-3);
  padding: var(--sdl-space-3);
  overflow: hidden;
  box-sizing: border-box;
}

.sidebar {
  width: 240px;
  flex-shrink: 0;
  min-height: 0;
}

.nav-card :deep(.sl-card-content) {
  padding: var(--sdl-space-4) var(--sdl-space-3);
  overflow-y: auto;
}

.content {
  flex: 1;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.section {
  width: 100%;
  height: 100%;
  min-height: 0;
}

.card {
  height: 100%;
}

.card :deep(.sl-card-content) {
  display: flex;
  flex-direction: column;
  min-height: 0;
  padding: var(--sdl-space-5) var(--sdl-space-6);
}

.form-wrapper,
.about-wrapper,
.upgrade-wrapper {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
  position: relative;
}

.form-wrapper {
  overflow-y: auto;
  padding-right: var(--sdl-space-2);
}

.section-title {
  margin: 0;
  color: var(--sdl-text-primary);
  font-size: var(--sdl-font-subtitle);
  font-weight: 700;
}

.description {
  font-size: var(--sdl-font-body-sm);
  color: var(--sdl-text-muted);
  margin-top: 0;
  margin-bottom: var(--sdl-space-4);
}

.select-control {
  width: 240px;
}

.network-form,
.security-form {
  max-width: 520px;
}

.form-actions {
  margin-top: var(--sdl-space-4);
}

.inline-row {
  display: flex;
  gap: var(--sdl-space-2);
  align-items: center;
}

.inline-row > :deep(.sl-input) {
  flex: 1;
  min-width: 0;
}

.checkbox-line {
  display: inline-flex;
  align-items: center;
  gap: var(--sdl-space-2);
  color: var(--sdl-text-secondary);
  font-size: var(--sdl-font-body-sm);
}

.settings-divider {
  height: 1px;
  background: var(--sdl-border-subtle);
  margin: var(--sdl-space-6) 0 var(--sdl-space-4);
}

.about-toolbar {
  margin-bottom: var(--sdl-space-3);
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--sdl-space-3);
}

.about-toolbar .description {
  margin-bottom: 0;
  flex: 1;
}

.about-body {
  overflow-y: auto;
  flex: 1;
  min-height: 0;
  padding-bottom: var(--sdl-space-4);
  padding-right: var(--sdl-space-2);
}

.sub-title {
  margin: 0 0 var(--sdl-space-2);
  font-size: var(--sdl-font-body-sm);
  color: var(--sdl-text-primary);
  font-weight: 600;
}

.desc-block {
  margin-bottom: var(--sdl-space-4);
}

.upgrade-wrapper {
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.upgrade-main-flow,
.upgrade-execution-flow {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-4);
  min-height: 0;
}

.upgrade-upload-area {
  padding: var(--sdl-space-4);
  border: 1px dashed var(--sdl-border-subtle);
  border-radius: var(--sdl-radius-md);
  background: var(--sdl-bg-sunken);
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
  flex-shrink: 0;
}

.upload-control-row {
  display: flex;
  gap: var(--sdl-space-3);
  align-items: center;
}

.upload-path-input {
  flex: 1;
}

.releases-section {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-2);
  min-height: 0;
}

.releases-empty {
  min-height: 160px;
}

.table-actions-row {
  display: flex;
  gap: var(--sdl-space-2);
  justify-content: center;
  align-items: center;
}

/* 进度条通用样式 */
.progress-container {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-1);
  margin-top: var(--sdl-space-2);
  width: 100%;
}

.progress-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: var(--sdl-font-caption);
  color: var(--sdl-text-secondary);
}

.progress-text {
  font-size: var(--sdl-font-caption);
  color: var(--sdl-text-secondary);
  text-align: right;
  margin-bottom: 2px;
}

.progress-bar-bg {
  width: 100%;
  height: 8px;
  background: var(--sdl-bg-sunken);
  border-radius: var(--sdl-radius-full);
  overflow: hidden;
  border: 1px solid var(--sdl-border-subtle);
}

.progress-bar-fill {
  height: 100%;
  border-radius: var(--sdl-radius-full);
  background: linear-gradient(90deg, var(--sdl-primary) 0%, #a855f7 100%);
  transition: width 0.3s cubic-bezier(0.4, 0, 0.2, 1);
  box-shadow: 0 0 8px rgba(var(--sdl-primary-rgb), 0.5);
}

/* 进度流动炫光动画 */
.animated-glow {
  background-size: 200% 200%;
  animation: progressGlow 2s linear infinite;
}

@keyframes progressGlow {
  0% {
    background-position: 0% 50%;
  }
  50% {
    background-position: 100% 50%;
  }
  100% {
    background-position: 0% 50%;
  }
}

/* 状态 B 的 execution 头部面板 */
.execution-header-panel {
  background: var(--sdl-bg-card);
  border: 1px solid var(--sdl-border-subtle);
  border-radius: var(--sdl-radius-md);
  padding: var(--sdl-space-5);
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-4);
}

.execution-title-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.execution-title {
  margin: 0;
  font-size: var(--sdl-font-body-md);
  font-weight: 700;
  color: var(--sdl-text-primary);
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
}

.execution-metrics-row {
  display: flex;
  gap: var(--sdl-space-6);
}

.metric-block {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-1);
}

.metric-block .m-label {
  font-size: var(--sdl-font-caption);
  color: var(--sdl-text-secondary);
}

.metric-block .m-value {
  font-size: var(--sdl-font-body-sm);
  color: var(--sdl-text-primary);
  font-weight: 600;
}

.upgrade-panel {
  min-width: 0;
  background: var(--sdl-bg-card);
  border: 1px solid var(--sdl-border-subtle);
  border-radius: var(--sdl-radius-md);
  padding: var(--sdl-space-4);
}

.plan-summary {
  display: grid;
  gap: var(--sdl-space-3);
}

.metric-item {
  display: grid;
  gap: var(--sdl-space-1);
}

.metric-label {
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-caption);
}

.metric-value {
  color: var(--sdl-text-primary);
  font-size: var(--sdl-font-body-sm);
  word-break: break-all;
}

.plan-empty {
  min-height: 128px;
}

.upgrade-detail {
  display: grid;
  gap: var(--sdl-space-4);
  padding-bottom: var(--sdl-space-4);
}

.error-text {
  color: var(--sdl-danger);
  word-break: break-word;
}

.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}

@media (max-width: 1024px) {
  .settings {
    flex-direction: column;
    overflow-y: auto;
  }

  .sidebar {
    width: 100%;
    height: auto;
  }

  .content {
    flex: none;
    height: auto;
  }
}
</style>
