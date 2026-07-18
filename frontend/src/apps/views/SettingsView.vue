<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { formatDateTime } from '@/utils/time'
import { securityApi } from '@/api/modules/security'
import {
  upgradesApi,
  type UpgradePlanDetail,
  type UpgradeReleaseRecord,
} from '@/api/modules/upgrades'
import { useToastStore } from '@/stores/toast'
import { useWindowManagerStore } from '@/stores/window-manager'
import { useConfirmationModalStore } from '@/stores/confirmation-modal'
import { forgetLoginEntry, rememberLoginEntry } from '@/utils/login-entry'
import {
  SecLabButton,
  SecLabCard,
  SecLabFormItem,
  SecLabMenu,
  SecLabInput,
  SecLabEmpty,
  SecLabLoading,
  SecLabTable,
  SecLabTag,
} from '@/components/ui'
import type { SecLabTableColumn } from '@/components/ui/SecLabTable.vue'
import AboutSettings from './settings/AboutSettings.vue'
import NetworkSettings from './settings/NetworkSettings.vue'
import PersonalizationSettings from './settings/PersonalizationSettings.vue'
import SystemMonitoringSettings from './settings/SystemMonitoringSettings.vue'

const props = defineProps<{
  isMaximized?: boolean
  windowId?: string
  payload?: Record<string, unknown>
}>()

const router = useRouter()
const { t, te } = useI18n()
const toastStore = useToastStore()
const windowStore = useWindowManagerStore()
const confirmationStore = useConfirmationModalStore()
const appVersion = import.meta.env.VITE_APP_VERSION ?? 'unknown'

const networkBusy = ref(false)
const networkDirty = ref(false)
const monitoringBusy = ref(false)
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

const aboutBusy = ref(false)
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
    networkBusy.value ||
    monitoringBusy.value ||
    securityLoading.value ||
    securitySaving.value ||
    credentialSaving.value ||
    aboutBusy.value ||
    upgradeLoading.value ||
    upgradeSyncing.value ||
    upgradeUploading.value ||
    isValidating.value ||
    upgradeStarting.value,
)
const settingsDirty = computed(() => networkDirty.value)

watch(
  [settingsBusy, settingsDirty],
  ([busy, dirty]) => {
    if (!props.windowId) return
    windowStore.updateWindowRuntimeState(props.windowId, {
      busy,
      dirty,
      allowsNodeSwitch: false,
      blockLevel: busy ? 'busy' : dirty ? 'dirty' : 'open',
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
      toastStore.error(response.message || t('app.settings.security.loadFailed'))
      return
    }
    securityForm.value.safeEntry = response.data.safeEntry
    securityForm.value.passwordComplexity = response.data.passwordComplexity
    usernameForm.value = ''
  } catch (error) {
    console.error('Failed to load security settings', error)
    toastStore.error(t('app.settings.security.loadFailed'))
  } finally {
    securityLoading.value = false
  }
}

const saveSecuritySettings = async () => {
  const safeEntry = securityForm.value.safeEntry.trim()
  if (safeEntry && !isValidSafeEntry(safeEntry)) {
    toastStore.error(t('app.settings.security.safeEntryInvalid'))
    return
  }
  securitySaving.value = true
  try {
    const response = await securityApi.updateSettings({
      safeEntry,
      passwordComplexity: securityForm.value.passwordComplexity,
    })
    if (!response.success) {
      toastStore.error(response.message || t('app.settings.security.saveFailed'))
      return
    }
    securityForm.value.safeEntry = safeEntry
    if (safeEntry) {
      rememberLoginEntry(`/${safeEntry}`)
    } else {
      forgetLoginEntry()
    }
    toastStore.success(t('app.settings.security.saveSuccess'))
  } catch (error) {
    console.error('Failed to save security settings', error)
    toastStore.error(t('app.settings.security.saveFailed'))
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
    toastStore.error(t('app.settings.security.usernameInvalid'))
    return
  }
  credentialSaving.value = true
  try {
    const response = await securityApi.updateUsername(username)
    if (!response.success) {
      toastStore.error(response.message || t('app.settings.security.usernameFailed'))
      return
    }
    usernameForm.value = ''
    toastStore.success(t('app.settings.security.usernameSuccess'))
  } catch (error) {
    console.error('Failed to update username', error)
    toastStore.error(t('app.settings.security.usernameFailed'))
  } finally {
    credentialSaving.value = false
  }
}

const savePassword = async () => {
  if (!passwordForm.value.newPassword) {
    toastStore.error(t('app.settings.security.passwordRequired'))
    return
  }
  if (passwordForm.value.newPassword.length < 5) {
    toastStore.error(t('app.settings.security.passwordTooShort'))
    return
  }
  if (passwordForm.value.newPassword !== passwordForm.value.confirmPassword) {
    toastStore.error(t('app.settings.security.passwordMismatch'))
    return
  }
  credentialSaving.value = true
  try {
    const response = await securityApi.updatePassword(passwordForm.value.newPassword)
    if (!response.success) {
      toastStore.error(response.message || t('app.settings.security.passwordFailed'))
      return
    }
    passwordForm.value.newPassword = ''
    passwordForm.value.confirmPassword = ''
    toastStore.success(t('app.settings.security.passwordSuccess'))
  } catch (error) {
    console.error('Failed to update password', error)
    toastStore.error(t('app.settings.security.passwordFailed'))
  } finally {
    credentialSaving.value = false
  }
}

const loadUpgradeReleases = async () => {
  upgradeLoading.value = true
  try {
    const response = await upgradesApi.listReleases()
    if (!response.success) {
      toastStore.error(response.message || t('app.settings.upgrade.loadFailed'))
      return
    }
    upgradeReleases.value = response.data ?? []
    selectedUpgradeVersion.value ||= upgradeReleases.value[0]?.version || ''
  } catch (error) {
    console.error('Failed to load upgrade releases', error)
    toastStore.error(t('app.settings.upgrade.loadFailed'))
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
      toastStore.error(t('app.settings.upgrade.invalidFormat'))
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
    toastStore.error(t('app.settings.upgrade.selectFile'))
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
      toastStore.error(te(msg) ? t(msg) : msg)
      return
    }

    toastStore.success(t('app.settings.upgrade.uploadSuccess'))
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
    toastStore.error(t('app.settings.upgrade.uploadFailed'))
  } finally {
    upgradeUploading.value = false
    isValidating.value = false
    uploadProgress.value = 0
  }
}

const startClusterUpgrade = async (overwriteSameVersion = false) => {
  if (!selectedUpgradeVersion.value) {
    toastStore.error(t('app.settings.upgrade.selectVersion'))
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
      toastStore.error(createResponse.message || t('app.settings.upgrade.startFailed'))
      return
    }
    const startResponse = await upgradesApi.startPlan(createResponse.data.plan.planId)
    if (!startResponse.success || !startResponse.data) {
      toastStore.error(startResponse.message || t('app.settings.upgrade.startFailed'))
      return
    }
    upgradePlan.value = startResponse.data
    window.sessionStorage.setItem('seclab.activeUpgradePlanId', startResponse.data.plan.planId)
    window.sessionStorage.setItem(
      'seclab.activeUpgradePlanDetail',
      JSON.stringify(startResponse.data),
    )
    toastStore.success(t('app.settings.upgrade.startSuccess'))
    router.push({ path: '/upgrade-progress', query: { planId: startResponse.data.plan.planId } })
  } catch (error) {
    console.error('Failed to start upgrade plan', error)
    toastStore.error(t('app.settings.upgrade.startFailed'))
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
      toastStore.success(t('app.settings.upgrade.deleteSuccess'))
      await loadUpgradeReleases()
    } else {
      toastStore.error(response.message || t('app.settings.upgrade.deleteFailed'))
    }
  } catch (error) {
    console.error('Failed to delete release', error)
    toastStore.error(t('app.settings.upgrade.deleteFailed'))
  } finally {
    upgradeLoading.value = false
  }
}

const loadedSections = new Set<string>()

/** 按需加载当前设置分区，避免打开应用时请求全部配置。 */
const loadActiveSection = async (section: string) => {
  if (loadedSections.has(section)) return
  loadedSections.add(section)

  if (section === 'security') {
    await loadSecuritySettings()
  } else if (section === 'upgrade') {
    await loadUpgradeReleases()
  }
}

watch(activeMenu, (section) => void loadActiveSection(section), { immediate: true })

let upgradeRefreshTimer: number | undefined

/** 停止升级计划轮询。 */
const stopUpgradeRefreshTimer = () => {
  if (upgradeRefreshTimer !== undefined) {
    window.clearInterval(upgradeRefreshTimer)
    upgradeRefreshTimer = undefined
  }
}

/** 根据升级计划状态启停轮询，离开分区后不保留后台任务。 */
const syncUpgradeRefreshTimer = () => {
  stopUpgradeRefreshTimer()

  if (
    activeMenu.value !== 'upgrade' ||
    !upgradePlan.value ||
    isUpgradeFinished(upgradePlan.value.plan.status)
  ) {
    return
  }

  upgradeRefreshTimer = window.setInterval(() => void refreshUpgradePlan(), 10000)
}

watch([activeMenu, () => upgradePlan.value?.plan.status], syncUpgradeRefreshTimer)
onBeforeUnmount(stopUpgradeRefreshTimer)
</script>

<template>
  <div class="settings" data-page="settings" data-seclab-app="settings">
    <div class="sidebar" data-slot="navigation">
      <SecLabCard class="nav-card" shadow="never" full-height>
        <SecLabMenu v-model="activeMenu" :items="menuItems" />
      </SecLabCard>
    </div>

    <div class="content" data-slot="content">
      <div v-if="activeMenu === 'monitoring'" class="section">
        <SystemMonitoringSettings @busy-change="monitoringBusy = $event" />
      </div>

      <div v-else-if="activeMenu === 'network'" class="section">
        <NetworkSettings
          @busy-change="networkBusy = $event"
          @dirty-change="networkDirty = $event"
        />
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
        <PersonalizationSettings kind="language" />
      </div>

      <div v-else-if="activeMenu === 'theme'" class="section">
        <PersonalizationSettings kind="theme" />
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

              <div class="fine-grained-upgrade-link" data-ui="node-upgrade-guide">
                <SecLabButton type="secondary" size="small" @click="goToNodeUpgradeGuide">
                  {{ t('app.settings.upgrade.fineGrainedLink') }}
                </SecLabButton>
              </div>

              <div class="upgrade-upload-area" data-ui="upgrade-upload-panel">
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
                    <div class="progress-bar-fill" :style="{ width: uploadProgress + '%' }"></div>
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
                      class="progress-bar-fill"
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
        <AboutSettings @busy-change="aboutBusy = $event" />
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
  background: var(--sdl-bg-muted);
  position: relative;
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
  background: var(--sdl-bg-muted);
  border-radius: var(--sdl-radius-pill);
  overflow: hidden;
  border: 1px solid var(--sdl-border-subtle);
}

.progress-bar-fill {
  height: 100%;
  border-radius: var(--sdl-radius-pill);
  background: var(--sdl-primary);
  transition: width 0.3s cubic-bezier(0.4, 0, 0.2, 1);
}

.fine-grained-upgrade-link {
  margin-bottom: var(--sdl-space-4);
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
  font-size: var(--sdl-font-body);
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
