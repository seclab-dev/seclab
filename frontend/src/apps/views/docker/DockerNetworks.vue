<script setup lang="ts">
import SecLabTable, { type SecLabTableColumn } from '@/components/ui/SecLabTable.vue'
import {
  SecLabButton,
  SecLabTag,
  SecLabSelect,
  SecLabInput,
  SecLabEmpty,
  SecLabFormItem,
  SecLabActionMenu,
} from '@/components/ui'
import { computed, ref, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDockerStore } from '@/stores/docker'
import { useConfirmationModalStore } from '@/stores/confirmation-modal'
import { formatIpamConfig, getConnectedContainerCount, formatKeyValue } from '@/utils/docker-format'
import { isSuiteManagedResource } from './docker-suite-labels'

/**
 * @file DockerNetworks.vue
 * @description Docker 网络列表与详情管理组件，严格遵循 SDL 设计规范，直接消费 Pinia Store。
 */

const { t } = useI18n()
const store = useDockerStore()
const modalStore = useConfirmationModalStore()

const networkRows = computed(() => store.networks ?? [])

/** 网络表格列定义 */
const columns = computed<SecLabTableColumn[]>(() => [
  { label: t('app.docker.networks.columns.name'), minWidth: 220, slot: 'name' },
  { label: t('app.docker.networks.columns.driver'), width: 120, slot: 'driver' },
  { label: t('app.docker.networks.columns.ipam'), minWidth: 260, slot: 'ipam' },
  {
    label: t('app.docker.networks.columns.connected'),
    width: 120,
    align: 'center',
    slot: 'connected',
  },
  { label: t('app.docker.networks.columns.actions'), width: 120, align: 'center', slot: 'actions' },
])

/** 驱动选项列表 */
const driverOptions = [
  { label: 'bridge', value: 'bridge' },
  { label: 'host', value: 'host' },
  { label: 'overlay', value: 'overlay' },
  { label: 'macvlan', value: 'macvlan' },
  { label: 'none', value: 'none' },
]

// ─── 网络连接管理 ───
const selectedContainerToConnect = ref('')
const isConnecting = ref(false)
const isDisconnecting = ref<Record<string, boolean>>({})

/** 可连接到当前网络（即目前尚未连接）的容器列表 */
const connectableContainers = computed(() => {
  const connectedIds = new Set(Object.keys(store.selectedNetworkDetail?.Containers || {}))
  return store.containers
    .filter((c) => c.Id && !connectedIds.has(c.Id))
    .map((c) => {
      const rawName = c.Names?.[0] || ''
      const name = rawName.replace(/^\//, '') || c.Id?.substring(0, 12) || ''
      return {
        label: name,
        value: c.Id!,
      }
    })
})

/** 已连接容器列表列定义 */
const containerColumns = computed<SecLabTableColumn[]>(() => [
  { label: t('app.docker.networks.detail.name'), slot: 'name' },
  { label: t('app.docker.networks.detail.ip'), width: 160, slot: 'ip' },
  { label: t('app.docker.networks.detail.actions'), width: 120, align: 'center', slot: 'actions' },
])

/** 已连接的容器列表格式化 */
const connectedContainersList = computed(() => {
  const containersMap = store.selectedNetworkDetail?.Containers
  if (!containersMap) return []
  return Object.entries(containersMap).map(([id, info]) => {
    const ip = info.IPv4Address || info.IPv6Address || '-'
    const rawName = info.Name || ''
    return {
      id,
      name: rawName.replace(/^\//, '') || id.substring(0, 12),
      ip,
    }
  })
})

/** 接入容器到网络 */
const handleConnect = async () => {
  if (!selectedContainerToConnect.value || !store.selectedNetworkDetail?.Id) return
  isConnecting.value = true
  try {
    await store.handleConnectNetwork(
      store.selectedNetworkDetail.Id,
      selectedContainerToConnect.value,
    )
    selectedContainerToConnect.value = ''
  } finally {
    isConnecting.value = false
  }
}

/** 从网络断开容器 */
const handleDisconnect = async (containerId: string, containerName: string) => {
  if (!store.selectedNetworkDetail?.Id) return
  const confirmed = await modalStore.showConfirmation(
    t('app.docker.networks.detail.disconnectConfirm', {
      containerName,
      networkName:
        store.selectedNetworkDetail.Name || store.selectedNetworkDetail.Id.substring(0, 12),
    }),
    t('app.docker.messages.deleteConfirmTitle'),
    t('app.docker.networks.detail.disconnectContainer'),
    t('confirmation.cancel'),
  )
  if (!confirmed) return
  isDisconnecting.value[containerId] = true
  try {
    await store.handleDisconnectNetwork(store.selectedNetworkDetail.Id, containerId)
  } finally {
    isDisconnecting.value[containerId] = false
  }
}

/** 挂载时如果容器列表为空，预先加载容器，保证接入容器下拉框有数据 */
onMounted(() => {
  if (store.containers.length === 0) {
    void store.fetchContainers()
  }
})
</script>

<template>
  <div class="docker-networks" data-page="docker-networks">
    <div class="docker-card">
      <div v-if="!store.isNetworkCreateActive" class="network-actions">
        <SecLabButton type="primary" @click="store.handleCreateNetwork">
          {{ t('app.docker.networks.actions.create') }}
        </SecLabButton>
      </div>

      <!-- 网络创建面板 -->
      <div v-if="store.isNetworkCreateActive" class="network-create-panel">
        <div class="create-panel-header">
          <h3>{{ t('app.docker.networks.create.title') }}</h3>
        </div>

        <div class="create-panel-body sl-form">
          <SecLabFormItem :label="t('app.docker.networks.create.name')" required>
            <SecLabInput
              v-model="store.networkForm.name"
              :placeholder="t('app.docker.networks.create.namePlaceholder')"
            />
          </SecLabFormItem>

          <SecLabFormItem :label="t('app.docker.networks.create.driver')">
            <SecLabSelect v-model="store.networkForm.driver" :options="driverOptions" />
          </SecLabFormItem>

          <div class="form-row">
            <SecLabFormItem :label="t('app.docker.networks.create.subnet')" class="form-row-item">
              <SecLabInput
                v-model="store.networkForm.subnet"
                :placeholder="t('app.docker.networks.create.subnetPlaceholder')"
              />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.docker.networks.create.gateway')" class="form-row-item">
              <SecLabInput
                v-model="store.networkForm.gateway"
                :placeholder="t('app.docker.networks.create.gatewayPlaceholder')"
              />
            </SecLabFormItem>
          </div>

          <SecLabFormItem :label="t('app.docker.networks.create.labels')">
            <SecLabInput
              v-model="store.networkForm.labels"
              type="textarea"
              :rows="4"
              placeholder="e.g. key=value"
            />
          </SecLabFormItem>
        </div>

        <div class="create-panel-footer">
          <SecLabButton @click="store.cancelNetworkCreate">
            {{ t('app.docker.networks.actions.cancel') }}
          </SecLabButton>
          <SecLabButton type="primary" @click="store.submitNetworkForm">
            {{ t('app.docker.networks.actions.submit') }}
          </SecLabButton>
        </div>
      </div>

      <!-- 网络列表 -->
      <div v-if="!store.isNetworkCreateActive" class="card-scroll-wrapper">
        <SecLabTable v-if="networkRows.length > 0" :data="networkRows" :columns="columns" border>
          <template #name="{ row }: { row: any }">
            <div class="resource-name-cell">
              <span class="network-name-cell" @click="store.handleViewNetwork(row?.Id)">{{
                row?.Name || '-'
              }}</span>
              <SecLabTag v-if="isSuiteManagedResource(row?.Labels)" type="primary" size="small">
                {{ t('app.docker.suiteManaged') }}
              </SecLabTag>
            </div>
          </template>

          <template #driver="{ row }: { row: any }">
            <SecLabTag type="info">{{ row?.Driver || '-' }}</SecLabTag>
          </template>

          <template #ipam="{ row }: { row: any }">
            <span class="ipam-text">{{ formatIpamConfig(row?.IPAM) }}</span>
          </template>

          <template #connected="{ row }: { row: any }">
            <SecLabTag type="info">
              {{ getConnectedContainerCount(row?.Containers) }}
            </SecLabTag>
          </template>

          <template #actions="{ row }: { row: any }">
            <SecLabActionMenu
              :label="t('app.docker.networks.actions.menu')"
              :actions="[
                {
                  label: t('app.docker.networks.actions.view'),
                  handler: () => store.handleViewNetwork(row?.Id),
                },
                {
                  label: t('app.docker.networks.actions.delete'),
                  handler: () => store.handleDeleteNetwork(row?.Id),
                  class: 'btn-delete',
                },
              ]"
            />
          </template>

          <template #empty>
            <SecLabEmpty :description="t('app.docker.networks.empty')" />
          </template>
        </SecLabTable>

        <SecLabEmpty v-else :description="t('app.docker.networks.empty')" />
      </div>
    </div>

    <!-- 网络详情 Modal -->
    <div v-if="store.isNetworkModalVisible && store.selectedNetworkDetail" class="modal-overlay">
      <div class="modal">
        <div class="modal-header">
          <h3>{{ t('app.docker.networks.detail.title') }}</h3>
          <button class="modal-close" @click="store.closeNetworkModal">×</button>
        </div>
        <div class="modal-body">
          <div class="detail-grid">
            <div class="detail-item">
              <span class="detail-label">{{ t('app.docker.networks.detail.name') }}</span>
              <span class="detail-value highlight-value">{{
                store.selectedNetworkDetail.Name || '-'
              }}</span>
            </div>
            <div class="detail-item">
              <span class="detail-label">{{ t('app.docker.networks.detail.id') }}</span>
              <span class="detail-value mono-text">{{
                store.selectedNetworkDetail.Id || '-'
              }}</span>
            </div>
            <div class="detail-item">
              <span class="detail-label">{{ t('app.docker.networks.detail.scope') }}</span>
              <span class="detail-value">{{ store.selectedNetworkDetail.Scope || '-' }}</span>
            </div>
            <div class="detail-item">
              <span class="detail-label">{{ t('app.docker.networks.detail.driver') }}</span>
              <span class="detail-value">{{ store.selectedNetworkDetail.Driver || '-' }}</span>
            </div>
            <div class="detail-item">
              <span class="detail-label">{{ t('app.docker.networks.detail.internal') }}</span>
              <span class="detail-value">{{
                store.selectedNetworkDetail.Internal
                  ? t('app.docker.networks.detail.yes')
                  : t('app.docker.networks.detail.no')
              }}</span>
            </div>
            <div class="detail-item">
              <span class="detail-label">{{ t('app.docker.networks.detail.attachable') }}</span>
              <span class="detail-value">{{
                store.selectedNetworkDetail.Attachable
                  ? t('app.docker.networks.detail.yes')
                  : t('app.docker.networks.detail.no')
              }}</span>
            </div>
            <div class="detail-item">
              <span class="detail-label">{{ t('app.docker.networks.detail.ingress') }}</span>
              <span class="detail-value">{{
                store.selectedNetworkDetail.Ingress
                  ? t('app.docker.networks.detail.yes')
                  : t('app.docker.networks.detail.no')
              }}</span>
            </div>
            <div class="detail-item">
              <span class="detail-label">{{ t('app.docker.networks.detail.createdAt') }}</span>
              <span class="detail-value">{{ store.selectedNetworkDetail.Created || '-' }}</span>
            </div>
            <div class="detail-item detail-full">
              <span class="detail-label">{{ t('app.docker.networks.detail.ipam') }}</span>
              <span class="detail-value mono-text">
                {{ formatIpamConfig(store.selectedNetworkDetail.IPAM) }}
              </span>
            </div>
            <div class="detail-item detail-full">
              <span class="detail-label">{{ t('app.docker.networks.detail.options') }}</span>
              <span class="detail-value">{{
                formatKeyValue(store.selectedNetworkDetail.Options)
              }}</span>
            </div>
            <div class="detail-item detail-full">
              <span class="detail-label">{{ t('app.docker.networks.detail.labels') }}</span>
              <span class="detail-value">{{
                formatKeyValue(store.selectedNetworkDetail.Labels)
              }}</span>
            </div>
          </div>

          <!-- 已连接容器列表 -->
          <div class="containers-section">
            <h4 class="section-title">{{ t('app.docker.networks.detail.containers') }}</h4>
            <SecLabTable
              v-if="connectedContainersList.length > 0"
              :data="connectedContainersList"
              :columns="containerColumns"
              border
              size="small"
              class="connected-table"
            >
              <template #name="{ row }">
                <span class="container-name-text">{{ row.name }}</span>
              </template>
              <template #ip="{ row }">
                <span class="mono-text">{{ row.ip }}</span>
              </template>
              <template #actions="{ row }">
                <SecLabButton
                  type="danger"
                  size="small"
                  :loading="isDisconnecting[row.id]"
                  @click="handleDisconnect(row.id, row.name)"
                >
                  {{ t('app.docker.networks.detail.disconnectContainer') }}
                </SecLabButton>
              </template>
            </SecLabTable>
            <SecLabEmpty v-else :description="t('app.docker.networks.detail.noContainers')" />
          </div>

          <!-- 接入容器操作区 -->
          <div class="connect-section">
            <h4 class="section-title">{{ t('app.docker.networks.detail.connectContainer') }}</h4>
            <div class="connect-form-row">
              <SecLabSelect
                v-model="selectedContainerToConnect"
                :options="connectableContainers"
                :placeholder="t('app.docker.networks.detail.selectContainerPlaceholder')"
                class="container-select"
              />
              <SecLabButton
                type="primary"
                :disabled="!selectedContainerToConnect"
                :loading="isConnecting"
                @click="handleConnect"
              >
                {{ t('app.docker.networks.detail.connectAction') }}
              </SecLabButton>
            </div>
          </div>
        </div>
        <div class="modal-footer">
          <SecLabButton @click="store.closeNetworkModal">
            {{ t('common.close') }}
          </SecLabButton>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.docker-networks {
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

.network-actions {
  display: flex;
  justify-content: flex-start;
  margin-bottom: var(--sdl-space-4);
}

.network-create-panel {
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-lg);
  padding: var(--sdl-space-5);
  margin-bottom: var(--sdl-space-4);
  background: var(--sdl-bg-card);
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-4);
  flex: 1;
  min-height: 0;
}

.create-panel-header h3 {
  margin: 0;
  color: var(--sdl-primary);
  font-size: var(--sdl-font-title);
}

.create-panel-body {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-2);
  flex: 1;
  min-height: 0;
  overflow: auto;
}

.form-row {
  display: flex;
  gap: var(--sdl-space-4);
}

.form-row-item {
  flex: 1;
  min-width: 0;
}

.create-panel-footer {
  display: flex;
  gap: var(--sdl-space-3);
  justify-content: flex-end;
}

.card-scroll-wrapper {
  flex: 1;
  min-height: 0;
  overflow: auto;
}

.resource-name-cell {
  min-width: 0;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.network-name-cell {
  color: var(--sdl-primary);
  cursor: pointer;
  font-weight: 500;
}

.network-name-cell:hover {
  text-decoration: underline;
}

.ipam-text {
  display: inline-block;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: var(--sdl-font-mono);
  font-size: var(--sdl-font-caption);
  color: var(--sdl-text-secondary);
}

/* ─── 弹窗通用 ─── */
.modal-overlay {
  position: fixed;
  inset: 0;
  background-color: var(--sdl-bg-backdrop);
  display: flex;
  justify-content: center;
  align-items: center;
  z-index: var(--sdl-z-index-modal);
  backdrop-filter: blur(4px);
}

.modal {
  background: var(--sdl-bg-panel);
  border: 1px solid var(--sdl-border-strong);
  border-radius: var(--sdl-radius-lg);
  box-shadow: var(--sdl-shadow-window);
  width: 550px;
  max-width: 90%;
  max-height: 85%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  animation: sl-modal-pop 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
}

.modal-header {
  padding: var(--sdl-space-4) var(--sdl-space-5);
  border-bottom: 1px solid var(--sdl-border-subtle);
  background: var(--sdl-bg-muted);
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.modal-header h3 {
  margin: 0;
  font-size: var(--sdl-font-subtitle);
  color: var(--sdl-text-primary);
  font-weight: 600;
}

.modal-close {
  background: none;
  border: none;
  font-size: 20px;
  cursor: pointer;
  color: var(--sdl-text-muted);
  transition: color 0.2s;
}

.modal-close:hover {
  color: var(--sdl-text-primary);
}

.modal-body {
  padding: var(--sdl-space-5);
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-5);
}

.modal-footer {
  padding: var(--sdl-space-3) var(--sdl-space-5);
  border-top: 1px solid var(--sdl-border-subtle);
  display: flex;
  justify-content: flex-end;
}

/* ─── 网络详情 ─── */
.detail-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--sdl-space-3);
  border-bottom: 1px solid var(--sdl-border-subtle);
  padding-bottom: var(--sdl-space-4);
}

.detail-item {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-1);
}

.detail-full {
  grid-column: span 2;
}

.detail-label {
  font-weight: 600;
  color: var(--sdl-text-secondary);
  font-size: var(--sdl-font-caption);
}

.detail-value {
  color: var(--sdl-text-primary);
  word-break: break-all;
  font-size: var(--sdl-font-body-sm);
}

.highlight-value {
  color: var(--sdl-primary);
  font-weight: 600;
}

.mono-text {
  font-family: var(--sdl-font-mono);
}

/* ─── 容器列表与连接 ─── */
.containers-section,
.connect-section {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
}

.section-title {
  margin: 0;
  font-size: var(--sdl-font-body);
  color: var(--sdl-text-primary);
  border-left: 3px solid var(--sdl-primary);
  padding-left: var(--sdl-space-2);
}

.connected-table {
  max-height: 180px;
  overflow-y: auto;
}

.container-name-text {
  color: var(--sdl-text-primary);
  font-weight: 500;
}

.connect-form-row {
  display: flex;
  gap: var(--sdl-space-3);
  align-items: center;
}

.container-select {
  flex: 1;
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
