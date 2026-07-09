<script setup lang="ts">
/**
 * @file SuiteCenterView.vue
 * @description Compose 套件中心，负责套件包导入、安装与生命周期操作。
 */

import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { suitesApi } from '@/api/modules/suites'
import type {
  SuiteCatalogItem,
  SuiteInstallProgress,
  SuiteInstanceSummary,
} from '@/api/interface/suites'
import {
  SecLabButton,
  SecLabCheckbox,
  SecLabDialog,
  SecLabEmpty,
  SecLabLoading,
} from '@/components/ui'
import AppIcon from '@/components/icons/AppIcon.vue'
import SecLabIcon from '@/components/icons/SecLabIcon.vue'
import { useNodeStore } from '@/stores/node'
import { useNotificationStore } from '@/stores/notification'
import { useWindowManagerStore } from '@/stores/window-manager'

type SuiteCategory = 'all' | 'tools' | 'other'

interface SuiteCard {
  suite: SuiteCatalogItem
  instance?: SuiteInstanceSummary
  category: SuiteCategory
  icon: string
  status: string
  installed: boolean
}

const { t, locale } = useI18n()
const notificationStore = useNotificationStore()
const windowStore = useWindowManagerStore()
const nodeStore = useNodeStore()

const loading = ref(false)
const operatingId = ref('')
const activeCategory = ref<SuiteCategory>('all')
const selectedSuiteId = ref('')
const catalog = ref<SuiteCatalogItem[]>([])
const instances = ref<SuiteInstanceSummary[]>([])
const fileInput = ref<HTMLInputElement | null>(null)
const importDialogVisible = ref(false)
const selectedImportFile = ref<File | null>(null)
const uninstallDialogVisible = ref(false)
const selectedUninstallSuite = ref<SuiteCard | null>(null)
const removeSuiteData = ref(false)
const deleteDialogVisible = ref(false)
const selectedDeleteSuite = ref<SuiteCard | null>(null)
const installProgress = ref<SuiteInstallProgress | null>(null)
const cancelingInstall = ref(false)
let installProgressTimer: ReturnType<typeof window.setInterval> | null = null

const hasSuites = computed(() => catalog.value.length > 0)
const currentNodeId = computed(() => nodeStore.currentNodeId || 'local')
const currentNodeUnavailable = computed(() => nodeStore.currentNodeUnavailable)
const selectedSuite = computed(
  () => suiteCards.value.find((item) => item.suite.suiteId === selectedSuiteId.value) ?? null,
)
const visibleInstallProgress = computed(() => {
  if (!installProgress.value) return null
  return installProgress.value.nodeId === currentNodeId.value ? installProgress.value : null
})

const suiteCards = computed<SuiteCard[]>(() => {
  return catalog.value.map((suite) => {
    const instance = instances.value.find((item) => item.suiteId === suite.suiteId)
    return {
      suite,
      instance,
      category: resolveSuiteCategory(suite),
      icon: suite.icon,
      status: instance?.status ?? suite.status,
      installed: !!instance,
    }
  })
})

const filteredSuiteCards = computed(() => {
  if (activeCategory.value === 'all') return suiteCards.value
  return suiteCards.value.filter((item) => item.category === activeCategory.value)
})

const categoryItems = computed(() => {
  const categories: SuiteCategory[] = ['all', 'tools', 'other']
  return categories.map((category) => ({
    key: category,
    label: t(`app.suiteCenter.categories.${category}`),
    count:
      category === 'all'
        ? suiteCards.value.length
        : suiteCards.value.filter((item) => item.category === category).length,
  }))
})

function resolveSuiteCategory(suite: SuiteCatalogItem): SuiteCategory {
  if (suite.category) {
    const cat = suite.category.toLowerCase() as SuiteCategory
    const validCategories: SuiteCategory[] = ['tools', 'other']
    if (validCategories.includes(cat)) {
      return cat
    }
  }
  return 'other'
}

function statusLabel(status: string) {
  const key = `app.suiteCenter.status.${status}`
  const translated = t(key)
  return translated === key ? status : translated
}

function statusClass(status: string) {
  if (status === 'enabled') return 'is-running'
  if (status === 'error') return 'is-error'
  if (status === 'installed' || status === 'disabled') return 'is-stopped'
  return 'is-working'
}

function nodeDisplayName(nodeId: string) {
  return nodeStore.nodes.find((node) => node.id === nodeId)?.name || nodeId
}

function isInstallingSuite(card: SuiteCard | null) {
  return (
    !!card &&
    operatingId.value === `install:${card.suite.suiteId}` &&
    installProgress.value?.nodeId === currentNodeId.value
  )
}

function installProgressStepLabel(step: string) {
  const key = `app.suiteCenter.installProgress.steps.${step}`
  const translated = t(key)
  return translated === key ? step : translated
}

function clearInstallProgressTimer() {
  if (installProgressTimer) {
    window.clearInterval(installProgressTimer)
    installProgressTimer = null
  }
}

function closeDetail() {
  selectedSuiteId.value = ''
}

async function refreshSuites() {
  loading.value = true
  try {
    const response = await suitesApi.fetchSuites(currentNodeId.value)
    if (!response.success || !response.data) {
      notificationStore.error(response.message || t('app.suiteCenter.messages.loadFailed'))
      return
    }
    catalog.value = response.data.catalog
    instances.value = response.data.instances
    if (
      selectedSuiteId.value &&
      !catalog.value.some((item) => item.suiteId === selectedSuiteId.value)
    ) {
      closeDetail()
    }
  } catch (error) {
    console.error('Failed to load suites', error)
    notificationStore.error(t('app.suiteCenter.messages.loadFailed'))
  } finally {
    loading.value = false
  }
}

function openImportDialog() {
  importDialogVisible.value = true
}

function resetImportSelection() {
  selectedImportFile.value = null
  if (fileInput.value) {
    fileInput.value.value = ''
  }
}

function closeImportDialog() {
  if (operatingId.value === 'import') return
  importDialogVisible.value = false
  resetImportSelection()
}

function openFilePicker() {
  fileInput.value?.click()
}

function handleFileSelected(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  selectedImportFile.value = file
}

async function confirmImportSuite() {
  const file = selectedImportFile.value
  if (!file) {
    notificationStore.error(t('app.suiteCenter.messages.selectPackageFirst'))
    return
  }
  operatingId.value = 'import'
  try {
    const response = await suitesApi.importSuite(file)
    if (!response.success) {
      notificationStore.error(response.message || t('app.suiteCenter.messages.importFailed'))
      return
    }
    notificationStore.success(t('app.suiteCenter.messages.importSuccess'))
    await refreshSuites()
    importDialogVisible.value = false
    resetImportSelection()
  } catch (error) {
    console.error('Failed to import suite', error)
    notificationStore.error(t('app.suiteCenter.messages.importFailed'))
  } finally {
    operatingId.value = ''
  }
}

async function installSuite(card: SuiteCard) {
  if (currentNodeUnavailable.value) {
    notificationStore.error(t('app.suiteCenter.messages.loadFailed'))
    return
  }
  operatingId.value = `install:${card.suite.suiteId}`
  clearInstallProgressTimer()
  const targetNodeId = currentNodeId.value
  installProgress.value = {
    taskId: '',
    instanceId: '',
    nodeId: targetNodeId,
    progressPercent: 1,
    status: 'queued',
    currentStep: 'queued',
    isFinished: false,
    cancelRequested: false,
  }
  try {
    const response = await suitesApi.installSuite(card.suite.suiteId, targetNodeId)
    if (!response.success || !response.data) {
      notificationStore.error(response.message || t('app.suiteCenter.messages.installFailed'))
      operatingId.value = ''
      installProgress.value = null
      return
    }
    installProgress.value = {
      taskId: response.data.taskId,
      instanceId: response.data.instanceId,
      nodeId: targetNodeId,
      progressPercent: 1,
      status: 'queued',
      currentStep: 'queued',
      isFinished: false,
      cancelRequested: false,
    }
    startInstallProgressPolling(response.data.taskId)
  } catch (error) {
    console.error('Failed to install suite', error)
    notificationStore.error(t('app.suiteCenter.messages.installFailed'))
    operatingId.value = ''
    installProgress.value = null
    cancelingInstall.value = false
  }
}

async function cancelInstallSuite() {
  const taskId = installProgress.value?.taskId
  if (!taskId || cancelingInstall.value) return
  cancelingInstall.value = true
  try {
    const response = await suitesApi.cancelInstall(taskId)
    if (response.success && response.data) {
      installProgress.value = response.data
    } else {
      notificationStore.error(response.message || t('app.suiteCenter.messages.cancelInstallFailed'))
    }
  } catch (error) {
    console.error('Failed to cancel suite install', error)
    notificationStore.error(t('app.suiteCenter.messages.cancelInstallFailed'))
  } finally {
    cancelingInstall.value = false
  }
}

function startInstallProgressPolling(taskId: string) {
  clearInstallProgressTimer()
  void pollInstallProgress(taskId)
  installProgressTimer = window.setInterval(() => {
    void pollInstallProgress(taskId)
  }, 1000)
}

async function pollInstallProgress(taskId: string) {
  try {
    const response = await suitesApi.fetchInstallProgress(taskId)
    if (!response.success || !response.data) {
      return
    }
    installProgress.value = response.data
    if (!response.data.isFinished) return

    clearInstallProgressTimer()
    if (response.data.status === 'success') {
      notificationStore.success(t('app.suiteCenter.messages.installSuccess'))
      await refreshSuites()
    } else if (response.data.status === 'canceled') {
      notificationStore.success(t('app.suiteCenter.messages.cancelInstallSuccess'))
      await refreshSuites()
    } else {
      notificationStore.error(response.data.error || t('app.suiteCenter.messages.installFailed'))
      await refreshSuites()
    }
    operatingId.value = ''
  } catch (error) {
    console.error('Failed to fetch suite install progress', error)
  }
}

function requestDeleteSuite(card: SuiteCard) {
  selectedDeleteSuite.value = card
  deleteDialogVisible.value = true
}

function closeDeleteDialog() {
  const suiteId = selectedDeleteSuite.value?.suite.suiteId
  if (suiteId && operatingId.value === `delete:${suiteId}`) return
  deleteDialogVisible.value = false
  selectedDeleteSuite.value = null
}

async function confirmDeleteSuite() {
  const suite = selectedDeleteSuite.value?.suite
  if (!suite) return
  operatingId.value = `delete:${suite.suiteId}`
  try {
    const response = await suitesApi.deleteSuite(suite.suiteId)
    if (!response.success) {
      notificationStore.error(response.message || t('app.suiteCenter.messages.deleteFailed'))
      return
    }
    notificationStore.success(t('app.suiteCenter.messages.deleteSuccess'))
    deleteDialogVisible.value = false
    selectedDeleteSuite.value = null
    closeDetail()
    await refreshSuites()
  } catch (error) {
    console.error('Failed to delete suite package', error)
    notificationStore.error(t('app.suiteCenter.messages.deleteFailed'))
  } finally {
    operatingId.value = ''
  }
}

async function enableInstance(instance: SuiteInstanceSummary) {
  operatingId.value = `enable:${instance.instanceId}`
  try {
    const response = await suitesApi.enableInstance(instance.instanceId)
    if (!response.success) {
      notificationStore.error(response.message || t('app.suiteCenter.messages.enableFailed'))
      return
    }
    notificationStore.success(t('app.suiteCenter.messages.enableSuccess'))
    await refreshSuites()
    await windowStore.refreshDesktopState()
  } catch (error) {
    console.error('Failed to enable suite', error)
    notificationStore.error(t('app.suiteCenter.messages.enableFailed'))
  } finally {
    operatingId.value = ''
  }
}

async function disableInstance(instance: SuiteInstanceSummary) {
  operatingId.value = `disable:${instance.instanceId}`
  try {
    const response = await suitesApi.disableInstance(instance.instanceId)
    if (!response.success) {
      notificationStore.error(response.message || t('app.suiteCenter.messages.disableFailed'))
      return
    }
    notificationStore.success(t('app.suiteCenter.messages.disableSuccess'))
    windowStore.closeWindowsBySuiteInstanceId(instance.instanceId)
    await refreshSuites()
    await windowStore.refreshDesktopState()
  } catch (error) {
    console.error('Failed to disable suite', error)
    notificationStore.error(t('app.suiteCenter.messages.disableFailed'))
  } finally {
    operatingId.value = ''
  }
}

function requestUninstallInstance(card: SuiteCard) {
  selectedUninstallSuite.value = card
  removeSuiteData.value = false
  uninstallDialogVisible.value = true
}

function closeUninstallDialog() {
  if (selectedUninstallSuite.value?.instance) {
    const instanceId = selectedUninstallSuite.value.instance.instanceId
    if (operatingId.value === `uninstall:${instanceId}`) return
  }
  uninstallDialogVisible.value = false
  selectedUninstallSuite.value = null
  removeSuiteData.value = false
}

async function confirmUninstallInstance() {
  const instance = selectedUninstallSuite.value?.instance
  if (!instance) return
  operatingId.value = `uninstall:${instance.instanceId}`
  try {
    const response = await suitesApi.uninstallInstance(instance.instanceId, {
      removeData: removeSuiteData.value,
    })
    if (!response.success) {
      notificationStore.error(response.message || t('app.suiteCenter.messages.uninstallFailed'))
      return
    }
    notificationStore.success(t('app.suiteCenter.messages.uninstallSuccess'))
    uninstallDialogVisible.value = false
    selectedUninstallSuite.value = null
    removeSuiteData.value = false
    closeDetail()
    windowStore.closeWindowsBySuiteInstanceId(instance.instanceId)
    await refreshSuites()
    await windowStore.refreshDesktopState()
  } catch (error) {
    console.error('Failed to uninstall suite', error)
    notificationStore.error(t('app.suiteCenter.messages.uninstallFailed'))
  } finally {
    operatingId.value = ''
  }
}

onMounted(() => {
  void refreshSuites()
})

onBeforeUnmount(() => {
  clearInstallProgressTimer()
})

watch(
  () => locale.value,
  () => {
    void refreshSuites()
  },
)

watch(
  () => currentNodeId.value,
  () => {
    void refreshSuites()
  },
)
</script>

<template>
  <div class="suite-center" data-page="suite-center">
    <div class="suite-sidebar" data-slot="category-menu">
      <div class="suite-sidebar-title">{{ t('app.suiteCenter.appName') }}</div>
      <div class="suite-category-list" data-ui="suite-category-list">
        <button
          v-for="category in categoryItems"
          :key="category.key"
          type="button"
          class="suite-category-item"
          :class="{ active: activeCategory === category.key }"
          @click="activeCategory = category.key"
        >
          <span>{{ category.label }}</span>
          <span class="suite-category-count">{{ category.count }}</span>
        </button>
      </div>
      <div class="suite-import-area">
        <SecLabButton
          type="primary"
          :loading="operatingId === 'import'"
          data-ui="suite-import-button"
          @click="openImportDialog"
        >
          <SecLabIcon name="plus" :size="14" />
          {{ t('app.suiteCenter.import') }}
        </SecLabButton>
      </div>
    </div>

    <SecLabDialog
      :visible="importDialogVisible"
      :title="t('app.suiteCenter.importTitle')"
      width="520px"
      @close="closeImportDialog"
    >
      <div class="suite-import-dialog" data-ui="suite-import-dialog">
        <div class="suite-package-picker">
          <input
            ref="fileInput"
            type="file"
            accept=".slsp"
            class="file-input"
            @change="handleFileSelected"
          />
          <div class="suite-package-info">
            <div class="suite-package-name">
              {{ selectedImportFile?.name || t('app.suiteCenter.noPackageSelected') }}
            </div>
            <div class="suite-package-hint">
              {{ t('app.suiteCenter.importFormatHint') }}
            </div>
          </div>
          <SecLabButton data-ui="suite-select-package-button" @click="openFilePicker">
            <SecLabIcon name="package" :size="14" />
            {{ t('app.suiteCenter.selectPackage') }}
          </SecLabButton>
        </div>
      </div>
      <template #footer>
        <SecLabButton type="secondary" @click="closeImportDialog">
          {{ t('common.cancel') }}
        </SecLabButton>
        <SecLabButton
          type="primary"
          :disabled="!selectedImportFile"
          :loading="operatingId === 'import'"
          @click="confirmImportSuite"
        >
          {{ t('app.suiteCenter.confirmImport') }}
        </SecLabButton>
      </template>
    </SecLabDialog>

    <div class="suite-content" data-slot="suite-cards">
      <SecLabLoading
        v-if="loading && !hasSuites"
        :loading="true"
        :text="t('app.suiteCenter.loading')"
      />
      <SecLabEmpty
        v-else-if="!hasSuites"
        :description="t('app.suiteCenter.empty')"
        icon="package"
      />
      <SecLabEmpty
        v-else-if="filteredSuiteCards.length === 0"
        :description="t('app.suiteCenter.emptyCategory')"
        icon="package"
      />
      <div v-else class="suite-card-grid">
        <button
          v-for="card in filteredSuiteCards"
          :key="card.suite.suiteId"
          type="button"
          class="suite-card"
          :data-slot="card.suite.suiteId"
          @click="selectedSuiteId = card.suite.suiteId"
        >
          <div class="suite-card-head">
            <AppIcon class="suite-logo" :name="card.icon" :size="44" :label="card.suite.name" />
            <div class="suite-card-title-group">
              <div class="suite-card-title">{{ card.suite.name }}</div>
              <div class="suite-card-id">{{ card.suite.suiteId }}</div>
            </div>
            <span class="status-pill" :class="statusClass(card.status)">
              {{ statusLabel(card.status) }}
            </span>
          </div>
          <div class="suite-card-summary">{{ card.suite.summary || t('common.none') }}</div>
          <div v-if="card.instance?.lastError" class="suite-error">
            {{ card.instance.lastError }}
          </div>
        </button>
      </div>
    </div>

    <SecLabDialog
      :visible="!!selectedSuite"
      :title="selectedSuite?.suite.name || ''"
      width="620px"
      @close="closeDetail"
    >
      <div v-if="selectedSuite" class="suite-detail">
        <div class="suite-detail-head">
          <AppIcon
            class="suite-logo"
            :name="selectedSuite.icon"
            :size="52"
            :label="selectedSuite.suite.name"
          />
          <div class="suite-detail-title-group">
            <div class="suite-detail-title">{{ selectedSuite.suite.name }}</div>
            <div class="suite-detail-id">{{ selectedSuite.suite.suiteId }}</div>
          </div>
          <span class="status-pill" :class="statusClass(selectedSuite.status)">
            {{ statusLabel(selectedSuite.status) }}
          </span>
        </div>
        <div class="suite-detail-summary">
          {{ selectedSuite.suite.summary || t('common.none') }}
        </div>
        <div class="suite-detail-grid">
          <div>
            <span>{{ t('app.suiteCenter.fields.version') }}</span>
            <strong>{{ selectedSuite.suite.version }}</strong>
          </div>
          <div>
            <span>{{ t('app.suiteCenter.fields.category') }}</span>
            <strong>{{ t(`app.suiteCenter.categories.${selectedSuite.category}`) }}</strong>
          </div>
          <div v-if="selectedSuite.instance">
            <span>{{ t('app.suiteCenter.fields.node') }}</span>
            <strong>{{ nodeDisplayName(selectedSuite.instance.nodeId) }}</strong>
          </div>
          <div v-if="selectedSuite.instance">
            <span>{{ t('app.suiteCenter.fields.project') }}</span>
            <strong>{{ selectedSuite.instance.composeProjectName }}</strong>
          </div>
        </div>
        <div v-if="selectedSuite.instance?.lastError" class="suite-error">
          {{ selectedSuite.instance.lastError }}
        </div>
      </div>
      <template #footer>
        <div class="suite-dialog-footer">
          <div
            v-if="isInstallingSuite(selectedSuite) && visibleInstallProgress"
            class="suite-install-progress"
            data-ui="suite-install-progress"
          >
            <div class="suite-install-progress-line">
              <span>{{ installProgressStepLabel(visibleInstallProgress.currentStep) }}</span>
            </div>
            <div class="suite-install-progress-track">
              <div class="suite-install-progress-bar" aria-hidden="true">
                <div :style="{ width: `${visibleInstallProgress.progressPercent}%` }"></div>
              </div>
              <strong>{{ visibleInstallProgress.progressPercent }}%</strong>
            </div>
          </div>
          <div class="suite-dialog-actions">
            <template v-if="selectedSuite && isInstallingSuite(selectedSuite)">
              <SecLabButton
                type="danger"
                :loading="cancelingInstall || installProgress?.status === 'canceling'"
                @click="cancelInstallSuite"
              >
                {{ t('app.suiteCenter.cancelInstall') }}
              </SecLabButton>
              <SecLabButton type="primary" :loading="true" disabled>
                {{ t('app.suiteCenter.install') }}
              </SecLabButton>
            </template>
            <template v-else-if="selectedSuite && !selectedSuite.instance">
              <SecLabButton
                type="danger"
                :loading="operatingId === `delete:${selectedSuite.suite.suiteId}`"
                @click="requestDeleteSuite(selectedSuite)"
              >
                {{ t('app.suiteCenter.delete') }}
              </SecLabButton>
              <SecLabButton
                type="primary"
                :loading="isInstallingSuite(selectedSuite)"
                :disabled="currentNodeUnavailable"
                @click="installSuite(selectedSuite)"
              >
                {{ t('app.suiteCenter.install') }}
              </SecLabButton>
            </template>
            <template v-else-if="selectedSuite?.instance">
              <SecLabButton
                type="primary"
                :disabled="selectedSuite.instance.status === 'enabled'"
                :loading="operatingId === `enable:${selectedSuite.instance.instanceId}`"
                @click="enableInstance(selectedSuite.instance)"
              >
                {{ t('app.suiteCenter.enable') }}
              </SecLabButton>
              <SecLabButton
                :disabled="selectedSuite.instance.status !== 'enabled'"
                :loading="operatingId === `disable:${selectedSuite.instance.instanceId}`"
                @click="disableInstance(selectedSuite.instance)"
              >
                {{ t('app.suiteCenter.disable') }}
              </SecLabButton>
              <SecLabButton
                type="danger"
                :loading="operatingId === `uninstall:${selectedSuite.instance.instanceId}`"
                @click="requestUninstallInstance(selectedSuite)"
              >
                {{ t('app.suiteCenter.uninstall') }}
              </SecLabButton>
            </template>
          </div>
        </div>
      </template>
    </SecLabDialog>

    <SecLabDialog
      :visible="uninstallDialogVisible"
      :title="t('app.suiteCenter.uninstallTitle')"
      width="520px"
      @close="closeUninstallDialog"
    >
      <div v-if="selectedUninstallSuite" class="suite-uninstall-dialog">
        <p class="suite-uninstall-text">
          {{
            t('app.suiteCenter.uninstallMessage', {
              name: selectedUninstallSuite.suite.name,
            })
          }}
        </p>
        <SecLabCheckbox v-model="removeSuiteData" data-ui="suite-remove-data-checkbox">
          {{ t('app.suiteCenter.removeData') }}
        </SecLabCheckbox>
        <div v-if="removeSuiteData" class="suite-uninstall-danger">
          {{ t('app.suiteCenter.removeDataWarning') }}
        </div>
      </div>
      <template #footer>
        <SecLabButton type="secondary" @click="closeUninstallDialog">
          {{ t('common.cancel') }}
        </SecLabButton>
        <SecLabButton
          type="danger"
          :loading="
            !!selectedUninstallSuite?.instance &&
            operatingId === `uninstall:${selectedUninstallSuite.instance.instanceId}`
          "
          @click="confirmUninstallInstance"
        >
          {{
            removeSuiteData
              ? t('app.suiteCenter.uninstallAndRemoveData')
              : t('app.suiteCenter.uninstall')
          }}
        </SecLabButton>
      </template>
    </SecLabDialog>

    <SecLabDialog
      :visible="deleteDialogVisible"
      :title="t('app.suiteCenter.deleteTitle')"
      width="520px"
      @close="closeDeleteDialog"
    >
      <div v-if="selectedDeleteSuite" class="suite-delete-dialog">
        <p class="suite-uninstall-text">
          {{
            t('app.suiteCenter.deleteMessage', {
              name: selectedDeleteSuite.suite.name,
            })
          }}
        </p>
      </div>
      <template #footer>
        <SecLabButton type="secondary" @click="closeDeleteDialog">
          {{ t('common.cancel') }}
        </SecLabButton>
        <SecLabButton
          type="danger"
          :loading="
            !!selectedDeleteSuite && operatingId === `delete:${selectedDeleteSuite.suite.suiteId}`
          "
          @click="confirmDeleteSuite"
        >
          {{ t('app.suiteCenter.delete') }}
        </SecLabButton>
      </template>
    </SecLabDialog>
  </div>
</template>

<style scoped>
.suite-center {
  height: 100%;
  display: grid;
  grid-template-columns: 220px minmax(0, 1fr);
  background: var(--sdl-bg-primary);
  color: var(--sdl-text-primary);
}

.suite-sidebar {
  min-width: 0;
  border-right: 1px solid var(--sdl-border-subtle);
  background: var(--sdl-bg-panel);
  display: flex;
  flex-direction: column;
}

.suite-sidebar-title {
  padding: 18px 16px 12px;
  font-size: 15px;
  font-weight: 700;
}

.suite-category-list {
  min-height: 0;
  flex: 1;
  padding: 4px 10px;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.suite-category-item {
  width: 100%;
  height: 34px;
  padding: 0 10px;
  border: 1px solid transparent;
  border-radius: 6px;
  background: transparent;
  color: var(--sdl-text-secondary);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 13px;
}

.suite-category-item:hover,
.suite-category-item.active {
  color: var(--sdl-text-primary);
  background: var(--sdl-bg-hover);
  border-color: var(--sdl-border-subtle);
}

.suite-category-count {
  color: var(--sdl-text-muted);
  font-family: var(--sdl-font-mono);
  font-size: 12px;
}

.suite-import-area {
  padding: 12px;
  border-top: 1px solid var(--sdl-border-subtle);
}

.suite-import-area :deep(.seclab-button) {
  width: 100%;
}

.suite-import-area :deep(.sl-button-content) {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

.file-input {
  display: none;
}

.suite-import-dialog {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.suite-uninstall-dialog,
.suite-delete-dialog {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.suite-uninstall-text {
  margin: 0;
  color: var(--sdl-text-secondary);
  font-size: 13px;
  line-height: 1.6;
}

.suite-uninstall-danger {
  padding: 10px 12px;
  border: 1px solid color-mix(in srgb, var(--sdl-danger) 35%, transparent);
  border-radius: 6px;
  color: var(--sdl-danger);
  background: color-mix(in srgb, var(--sdl-danger) 8%, transparent);
  font-size: 12px;
  line-height: 1.5;
}

.suite-package-picker {
  min-width: 0;
  padding: 12px;
  border: 1px solid var(--sdl-border-subtle);
  border-radius: 8px;
  display: flex;
  align-items: center;
  gap: 12px;
}

.suite-package-info {
  min-width: 0;
  flex: 1;
}

.suite-package-name {
  color: var(--sdl-text-primary);
  font-size: 13px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.suite-package-hint {
  margin-top: 4px;
  color: var(--sdl-text-muted);
  font-size: 12px;
}

.suite-content {
  min-width: 0;
  min-height: 0;
  padding: 18px;
  overflow: auto;
  position: relative;
}

.suite-card-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 14px;
}

.suite-card {
  min-width: 0;
  padding: 14px;
  border: 1px solid var(--sdl-border-subtle);
  border-radius: 8px;
  background: var(--sdl-bg-muted);
  color: var(--sdl-text-primary);
  cursor: pointer;
  display: flex;
  flex-direction: column;
  gap: 12px;
  text-align: left;
  transition:
    border-color 0.2s,
    background-color 0.2s;
}

.suite-card:hover {
  border-color: var(--sdl-primary);
  background: var(--sdl-bg-hover);
}

.suite-card-head,
.suite-detail-head {
  display: flex;
  align-items: center;
}

.suite-card-head,
.suite-detail-head {
  gap: 10px;
}

.suite-logo {
  flex: 0 0 auto;
  color: var(--sdl-text-primary);
}

.suite-card-title-group,
.suite-detail-title-group {
  min-width: 0;
  flex: 1;
}

.suite-card-title,
.suite-detail-title {
  font-weight: 700;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.suite-card-id,
.suite-detail-id {
  margin-top: 3px;
  color: var(--sdl-text-muted);
  font-family: var(--sdl-font-mono);
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.suite-card-summary,
.suite-detail-summary,
.suite-error {
  font-size: 12px;
  line-height: 1.5;
}

.suite-card-summary {
  color: var(--sdl-text-secondary);
  overflow: hidden;
  display: -webkit-box;
  line-clamp: 2;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
}

.suite-detail {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.suite-dialog-footer {
  width: 100%;
  min-width: 0;
  display: flex;
  align-items: center;
  justify-content: flex-start;
  gap: 14px;
}

.suite-dialog-actions {
  margin-left: auto;
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
}

.suite-install-progress {
  min-width: 0;
  max-width: 360px;
  flex: 1 1 auto;
  color: var(--sdl-text-secondary);
}

.suite-install-progress-line {
  display: flex;
  align-items: center;
  font-size: 12px;
  line-height: 1.4;
}

.suite-install-progress-line span {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.suite-install-progress-track {
  margin-top: 5px;
  display: flex;
  align-items: center;
  gap: 8px;
}

.suite-install-progress-track strong {
  flex: 0 0 auto;
  min-width: 34px;
  text-align: right;
  color: var(--sdl-text-primary);
  font-family: var(--sdl-font-mono);
  font-size: 12px;
}

.suite-install-progress-bar {
  flex: 1 1 auto;
  min-width: 0;
  height: 4px;
  overflow: hidden;
  border-radius: 999px;
  background: var(--sdl-bg-muted);
}

.suite-install-progress-bar div {
  height: 100%;
  border-radius: inherit;
  background: var(--sdl-primary);
  transition: width 0.2s ease;
}

.suite-detail-summary {
  color: var(--sdl-text-secondary);
}

.suite-detail-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}

.suite-detail-grid div {
  min-width: 0;
  padding: 10px;
  border: 1px solid var(--sdl-border-subtle);
  border-radius: 6px;
  background: var(--sdl-bg-muted);
}

.suite-detail-grid span,
.suite-detail-grid strong {
  display: block;
}

.suite-detail-grid span {
  color: var(--sdl-text-muted);
  font-size: 12px;
}

.suite-detail-grid strong {
  margin-top: 4px;
  overflow-wrap: anywhere;
  font-size: 13px;
}

.suite-error {
  color: var(--sdl-danger);
  overflow-wrap: anywhere;
}

.status-pill {
  flex: 0 0 auto;
  border-radius: 999px;
  padding: 2px 8px;
  font-size: 11px;
  font-weight: 600;
  border: 1px solid var(--sdl-border-subtle);
}

.status-pill.is-running {
  color: var(--sdl-success);
}

.status-pill.is-error {
  color: var(--sdl-danger);
}

.status-pill.is-stopped {
  color: var(--sdl-text-muted);
}

.status-pill.is-working {
  color: var(--sdl-warning);
}

@media (max-width: 680px) {
  .suite-dialog-footer {
    align-items: stretch;
    flex-direction: column;
  }

  .suite-install-progress {
    max-width: none;
    flex: 0 1 auto;
  }

  .suite-dialog-actions {
    margin-left: 0;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
}
</style>
