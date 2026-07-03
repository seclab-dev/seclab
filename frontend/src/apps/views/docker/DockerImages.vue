<script setup lang="ts">
/**
 * @file DockerImages.vue
 * @description Docker 镜像模块外壳组件，通过 Tab 切换本地镜像列表及镜像分发管理。
 */

import { ref, computed, defineAsyncComponent } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDockerStore } from '@/stores/docker'
import { useNodeStore } from '@/stores/node'
import { useNotificationStore } from '@/stores/notification'
import { dockerApi } from '@/api/modules/docker'
import SecLabIcon from '@/components/icons/SecLabIcon.vue'
import {
  SecLabAlert,
  SecLabButton,
  SecLabDialog,
  SecLabFormItem,
  SecLabInput,
  SecLabLoading,
  SecLabSwitch,
  SecLabTabs,
} from '@/components/ui'
import type * as dockerType from '@/api/interface/docker'

// 声明原有属性以确保与主视图组件通信时的 TypeScript 类型兼容性
defineProps<{
  imagesList: dockerType.ImageSummary[]
  utils?: {
    formatImageTags: (tags: dockerType.ImageSummary['RepoTags'] | undefined) => string
    formatBytes: (bytes: number | undefined) => string
  }
}>()

defineEmits<{
  (e: 'deleteImage', payload: { id: string; containers: number }): void
}>()

const { t } = useI18n()
const store = useDockerStore()
const nodeStore = useNodeStore()
const notificationStore = useNotificationStore()
const activeTab = ref<'local' | 'registry' | 'distribute'>('local')
const settingsVisible = ref(false)
const settingsLoading = ref(false)
const settingsSaving = ref(false)
const settingsError = ref('')
const registryMirrorsText = ref('')
const proxy = ref('')
const proxyEnabled = ref(false)
const settingsTab = ref<'registry' | 'proxy'>('registry')

// ─── Tab 配置 ───
const tabOptions = computed(() => [
  { label: t('app.docker.images.tabs.local', { count: store.imagesList.length }), name: 'local' },
  { label: t('app.docker.images.tabs.registry'), name: 'registry' },
  { label: t('app.docker.images.tabs.distribute'), name: 'distribute' },
])
const settingsTabOptions = computed(() => [
  { label: t('app.docker.images.settings.tabs.registry'), name: 'registry' },
  { label: t('app.docker.images.settings.tabs.proxy'), name: 'proxy' },
])

// ─── 动态异步导入子组件 ───
const DockerImageList = defineAsyncComponent(() => import('./DockerImageList.vue'))
const DockerImageRegistry = defineAsyncComponent(() => import('./DockerImageRegistry.vue'))
const DockerImageDistribute = defineAsyncComponent(() => import('./DockerImageDistribute.vue'))

/** 打开并读取当前节点的 Docker daemon 设置。 */
async function openSettings() {
  settingsVisible.value = true
  settingsLoading.value = true
  settingsError.value = ''
  settingsTab.value = 'registry'
  try {
    const response = await dockerApi.forNode(nodeStore.currentNodeId).fetchDaemonSettings()
    if (response.success && response.data) {
      registryMirrorsText.value = response.data.registryMirrors.join('\n')
      proxy.value = response.data.proxy
      proxyEnabled.value = response.data.proxyEnabled === true
    } else {
      settingsError.value = response.message || t('app.docker.images.settings.loadFailed')
    }
  } catch (error) {
    console.error('Failed to load Docker daemon settings', error)
    settingsError.value = t('app.docker.images.settings.loadFailed')
  } finally {
    settingsLoading.value = false
  }
}

/** 保存设置并由当前节点 Agent 重启 Docker 使其生效。 */
async function saveSettings() {
  settingsError.value = ''
  const normalizedProxy = proxy.value.trim()
  if (proxyEnabled.value && !normalizedProxy) {
    settingsTab.value = 'proxy'
    settingsError.value = t('app.docker.images.settings.proxyRequired')
    return
  }
  if (proxyEnabled.value && !isValidProxyUrl(normalizedProxy)) {
    settingsTab.value = 'proxy'
    settingsError.value = t('app.docker.images.settings.proxyInvalid')
    return
  }

  settingsSaving.value = true
  try {
    const registryMirrors = Array.from(
      new Set(
        registryMirrorsText.value
          .split('\n')
          .map((value) => value.trim())
          .filter(Boolean),
      ),
    )
    const response = await dockerApi.forNode(nodeStore.currentNodeId).updateDaemonSettings({
      registryMirrors,
      proxy: normalizedProxy,
      proxyEnabled: proxyEnabled.value,
    })
    if (response.success) {
      settingsVisible.value = false
      notificationStore.success(t('app.docker.images.settings.saveSuccess'))
    } else {
      settingsError.value = response.message || t('app.docker.images.settings.saveFailed')
    }
  } catch (error) {
    console.error('Failed to save Docker daemon settings', error)
    settingsError.value = t('app.docker.images.settings.saveFailed')
  } finally {
    settingsSaving.value = false
  }
}

/** 校验 Docker 代理地址是否满足协议、主机和显式端口要求。 */
function isValidProxyUrl(value: string): boolean {
  try {
    const url = new URL(value)
    return (
      (url.protocol === 'http:' || url.protocol === 'https:') &&
      Boolean(url.hostname) &&
      Boolean(url.port) &&
      (url.pathname === '' || url.pathname === '/') &&
      !url.search &&
      !url.hash
    )
  } catch {
    return false
  }
}

function closeSettings() {
  if (settingsSaving.value) return
  settingsVisible.value = false
}
</script>

<template>
  <div class="docker-images" data-page="docker-images">
    <div class="docker-card">
      <div class="images-toolbar" data-ui="images-toolbar">
        <SecLabTabs
          v-model="activeTab"
          class="images-tabs"
          :tabs="tabOptions"
          data-ui="images-tabs"
        />
        <SecLabButton
          class="settings-button"
          :aria-label="t('app.docker.images.settings.action')"
          data-ui="daemon-settings-button"
          @click="openSettings"
        >
          <SecLabIcon name="settings" :size="16" />
        </SecLabButton>
      </div>

      <div class="card-scroll-wrapper">
        <DockerImageList v-if="activeTab === 'local'" />
        <DockerImageRegistry v-else-if="activeTab === 'registry'" />
        <DockerImageDistribute v-else-if="activeTab === 'distribute'" />
      </div>
    </div>

    <SecLabDialog
      :visible="settingsVisible"
      :title="t('app.docker.images.settings.title')"
      width="620px"
      :close-on-click-overlay="false"
      data-ui="daemon-settings-dialog"
      @close="closeSettings"
    >
      <div class="settings-dialog-body" data-slot="body">
        <SecLabAlert
          type="warning"
          show-icon
          :description="t('app.docker.images.settings.restartWarning')"
        />
        <SecLabAlert
          v-if="settingsError"
          type="error"
          show-icon
          :description="settingsError"
          data-ui="daemon-settings-error"
        />
        <SecLabLoading :loading="settingsLoading" cover />
        <div class="settings-form" data-ui="daemon-settings-form">
          <SecLabTabs
            v-model="settingsTab"
            :tabs="settingsTabOptions"
            data-ui="daemon-settings-tabs"
          />
          <div v-if="settingsTab === 'registry'" data-slot="registry-settings">
            <SecLabFormItem :label="t('app.docker.images.settings.registryMirrors')">
              <SecLabInput
                v-model="registryMirrorsText"
                type="textarea"
                :rows="4"
                :disabled="settingsLoading || settingsSaving"
                :placeholder="t('app.docker.images.settings.registryMirrorsPlaceholder')"
                data-ui="registry-mirrors-input"
              />
            </SecLabFormItem>
          </div>
          <div v-else class="proxy-settings" data-slot="proxy-settings">
            <SecLabFormItem :label="t('app.docker.images.settings.proxyStatus')">
              <SecLabSwitch
                v-model="proxyEnabled"
                :disabled="settingsLoading || settingsSaving"
                :active-text="
                  proxyEnabled
                    ? t('app.docker.images.settings.proxyEnabled')
                    : t('app.docker.images.settings.proxyDisabled')
                "
                data-ui="daemon-proxy-switch"
              />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.docker.images.settings.proxy')">
              <SecLabInput
                v-model="proxy"
                :disabled="!proxyEnabled || settingsLoading || settingsSaving"
                :placeholder="t('app.docker.images.settings.proxyPlaceholder')"
                autocomplete="off"
                data-ui="daemon-proxy-input"
              />
            </SecLabFormItem>
          </div>
        </div>
      </div>
      <template #footer>
        <SecLabButton :disabled="settingsSaving" @click="closeSettings">
          {{ t('common.cancel') }}
        </SecLabButton>
        <SecLabButton
          type="primary"
          :loading="settingsSaving"
          :disabled="settingsLoading"
          data-ui="daemon-settings-save"
          @click="saveSettings"
        >
          {{ t('common.save') }}
        </SecLabButton>
      </template>
    </SecLabDialog>
  </div>
</template>

<style scoped>
.docker-images {
  flex-grow: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.docker-card {
  display: flex;
  flex-direction: column;
  min-height: 0;
  margin-top: var(--sdl-space-3);
  background-color: var(--sdl-bg-panel);
  padding: var(--sdl-space-5);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-lg);
  box-sizing: border-box;
  flex-grow: 1;
}

.images-toolbar {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--sdl-space-3);
  margin-bottom: var(--sdl-space-2);
}

.images-tabs {
  flex: 1;
  min-width: 0;
}

.settings-button {
  width: 32px;
  min-width: 32px;
  padding: 0;
}

.settings-dialog-body {
  position: relative;
  min-height: 260px;
}

.settings-form {
  display: grid;
  gap: var(--sdl-space-4);
}

.proxy-settings {
  display: grid;
  gap: var(--sdl-space-4);
}

.card-scroll-wrapper {
  flex: 1;
  min-height: 0;
  overflow: auto;
}
</style>
