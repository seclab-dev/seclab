<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { systemApi } from '@/api/modules/system'
import type { DiskInfo, DiskPartitionInfo } from '@/api/interface/system'
import { useNotificationStore } from '@/stores/notification'
import { useConfirmationModalStore } from '@/stores/confirmation-modal'
import { useNodeStore } from '@/stores/node'
import { useWindowManagerStore } from '@/stores/window-manager'
import { formatBytes } from '@/utils/units'
import {
  SecLabButton,
  SecLabCard,
  SecLabTag,
  SecLabTable,
  SecLabLoading,
  SecLabInput,
  SecLabDialog,
  SecLabActionMenu,
} from '@/components/ui'
import type { SecLabTableColumn } from '@/components/ui/SecLabTable.vue'
import SecLabIcon from '@/components/icons/SecLabIcon.vue'

const props = defineProps<{
  isMaximized?: boolean
  windowId?: string
  payload?: Record<string, unknown>
}>()

const { t } = useI18n()
const notificationStore = useNotificationStore()
const confirmationModal = useConfirmationModalStore()
const nodeStore = useNodeStore()
const windowStore = useWindowManagerStore()
const systemClient = computed(() => systemApi.forNode(nodeStore.currentNodeId))

const loading = ref(false)
const actionLoading = ref(false)
const diskRows = ref<DiskInfo[]>([])
const diskMountRoot = '/mnt'
const mountDialogVisible = ref(false)
const mountpointInput = ref('')
const pendingMountTarget = ref<
  | { action: 'initialize'; disk: DiskInfo; partition?: undefined }
  | { action: 'mount'; disk: DiskInfo; partition: DiskPartitionInfo }
  | null
>(null)
const diskBusy = computed(() => loading.value || actionLoading.value)

watch(
  diskBusy,
  (busy) => {
    if (!props.windowId) return
    windowStore.updateWindowRuntimeState(props.windowId, {
      busy,
      allowsNodeSwitch: false,
      blockLevel: busy ? 'busy' : 'open',
      blockReason: busy ? t('app.diskManager.guardBusy') : t('app.diskManager.guardOpen'),
    })
  },
  { immediate: true },
)

/** 分区表格列配置 */
const partitionColumns = computed<SecLabTableColumn[]>(() => [
  { prop: 'name', label: t('app.diskManager.partition.columns.name'), minWidth: 120 },
  {
    prop: 'sizeBytes',
    label: t('app.diskManager.partition.columns.size'),
    minWidth: 110,
    slot: 'size',
  },
  {
    prop: 'usedBytes',
    label: t('app.diskManager.partition.columns.used'),
    minWidth: 110,
    slot: 'used',
  },
  {
    prop: 'availableBytes',
    label: t('app.diskManager.partition.columns.available'),
    minWidth: 110,
    slot: 'available',
  },
  {
    prop: 'usagePercent',
    label: t('app.diskManager.partition.columns.usage'),
    minWidth: 100,
    slot: 'usage',
  },
  { prop: 'mountpoint', label: t('app.diskManager.partition.columns.mountpoint'), minWidth: 140 },
  { prop: 'filesystem', label: t('app.diskManager.partition.columns.filesystem'), minWidth: 120 },
  {
    label: t('app.diskManager.partition.columns.action'),
    minWidth: 150,
    align: 'center',
    headerAlign: 'center',
    fixed: 'right',
    slot: 'action',
  },
])

const sortedDiskRows = computed(() => {
  return [...diskRows.value].sort((a, b) => Number(b.sizeBytes) - Number(a.sizeBytes))
})

const reloadDisks = async () => {
  loading.value = true
  try {
    const response = await systemClient.value.fetchDisks()
    if (!response.success) {
      throw new Error(response.message || t('app.diskManager.messages.loadFailed'))
    }
    diskRows.value = response.data ?? []
  } catch (error) {
    const message =
      error instanceof Error ? error.message : t('app.diskManager.messages.loadFailed')
    notificationStore.error(message)
  } finally {
    loading.value = false
  }
}

const handleInitPartition = async (disk: DiskInfo) => {
  if (!canInitializeDisk(disk) || actionLoading.value) return
  openMountDialog('initialize', disk)
}

const confirmInitPartition = async (disk: DiskInfo, mountpoint: string) => {
  const confirmed = await confirmationModal.showConfirmation(
    t('app.diskManager.confirm.initMessage', { disk: disk.name, mountpoint }),
    t('app.diskManager.confirm.title'),
    t('app.diskManager.actions.initPartition'),
    t('confirmation.cancel'),
  )
  if (!confirmed) return

  const dangerConfirmed = await confirmationModal.showConfirmation(
    t('app.diskManager.confirm.initDangerMessage', { disk: disk.name }),
    t('app.diskManager.confirm.dangerTitle'),
    t('app.diskManager.confirm.initDangerConfirm'),
    t('confirmation.cancel'),
  )
  if (!dangerConfirmed) return

  actionLoading.value = true
  try {
    const response = await systemClient.value.initializeDisk(disk.name, 'ext4', mountpoint)
    if (!response.success) {
      throw new Error(response.message || t('app.diskManager.messages.partitionFailed'))
    }
    notificationStore.success(t('app.diskManager.messages.partitionSuccess', { disk: disk.name }))
    await reloadDisks()
  } catch (error) {
    const message =
      error instanceof Error ? error.message : t('app.diskManager.messages.partitionFailed')
    notificationStore.error(message)
  } finally {
    actionLoading.value = false
  }
}

const textOrPlaceholder = (value?: string) => {
  if (!value) return '--'
  const text = value.trim()
  return text.length > 0 ? text : '--'
}

const formatUsagePercent = (value: number | null) => {
  if (value == null || Number.isNaN(value)) return '--'
  return `${value.toFixed(2)}%`
}

const isUnformattedDisk = (disk: DiskInfo) => {
  if (!disk.partitions.length) return true
  return disk.partitions.every((partition) => {
    const fs = (partition.filesystem || '').trim()
    return !fs || fs === '--'
  })
}

const isMountedPartition = (partition: DiskPartitionInfo) => {
  return getMountpoints(partition).length > 0
}

const getMountpoints = (partition: DiskPartitionInfo) => {
  return (partition.mountpoint || '')
    .split(',')
    .map((value) => value.trim())
    .filter((value) => value.length > 0 && value !== '--')
}

const hasFilesystem = (partition: DiskPartitionInfo) => {
  const fs = (partition.filesystem || '').trim()
  return fs.length > 0 && fs !== '--'
}

const isManagedMountpoint = (mountpoint: string) => {
  const normalized = mountpoint.length > 1 ? mountpoint.replace(/\/+$/, '') : mountpoint
  return normalized.startsWith(`${diskMountRoot}/`)
}

const isManagedMountedPartition = (partition: DiskPartitionInfo) => {
  const mountpoints = getMountpoints(partition)
  return mountpoints.length === 1 && isManagedMountpoint(mountpoints[0])
}

const hasExternalMountpoint = (partition: DiskPartitionInfo) => {
  const mountpoints = getMountpoints(partition)
  return mountpoints.some((mountpoint) => !isManagedMountpoint(mountpoint))
}

const canInitializeDisk = (disk: DiskInfo) => {
  if (disk.isSystemDisk) return false
  return !disk.partitions.some(isMountedPartition)
}

const canRepartitionDisk = (disk: DiskInfo) => {
  return (
    !disk.isSystemDisk && disk.partitions.length > 0 && !disk.partitions.some(isMountedPartition)
  )
}

const canMountPartition = (disk: DiskInfo, partition: DiskPartitionInfo) => {
  return !disk.isSystemDisk && !isMountedPartition(partition) && hasFilesystem(partition)
}

const canUnmountPartition = (disk: DiskInfo, partition: DiskPartitionInfo) => {
  return !disk.isSystemDisk && isManagedMountedPartition(partition)
}

const canFormatPartition = (disk: DiskInfo, partition: DiskPartitionInfo) => {
  return !disk.isSystemDisk && !isMountedPartition(partition)
}

const partitionActions = (disk: DiskInfo, partition: DiskPartitionInfo) => {
  const actions: Array<{
    label: string
    class?: string
    disabled?: boolean
    handler: () => void
  }> = []

  if (canMountPartition(disk, partition)) {
    actions.push({
      label: t('app.diskManager.actions.mount'),
      class: 'app-btn-start',
      handler: () => {
        void runPartitionAction('mount', disk, partition)
      },
    })
  }
  if (canUnmountPartition(disk, partition)) {
    actions.push({
      label: t('app.diskManager.actions.unmount'),
      class: 'app-btn-stop',
      handler: () => {
        void runPartitionAction('unmount', disk, partition)
      },
    })
  }
  if (canFormatPartition(disk, partition)) {
    actions.push({
      label: t('app.diskManager.actions.format'),
      class: 'app-btn-delete',
      handler: () => {
        void runPartitionAction('format', disk, partition)
      },
    })
  }
  if (canRepartitionDisk(disk) && partition.name === disk.partitions[0]?.name) {
    actions.push({
      label: t('app.diskManager.actions.repartition'),
      class: 'app-btn-delete',
      handler: () => {
        void handleInitPartition(disk)
      },
    })
  }

  if (actions.length === 0 || disk.isSystemDisk || hasExternalMountpoint(partition)) {
    return [
      {
        label: t('app.diskManager.actions.unavailable'),
        disabled: true,
        handler: () => {},
      },
    ]
  }

  return actions
}

const normalizeMountpointInput = (value: string) => {
  const trimmed = value.trim().replace(/\/+$/, '')
  if (!trimmed) return ''
  const relative = trimmed.startsWith('/mnt/')
    ? trimmed.slice('/mnt/'.length)
    : trimmed.replace(/^\/+/, '')
  if (!relative || relative.split('/').some((part) => !part || part === '.' || part === '..')) {
    return ''
  }
  return `${diskMountRoot}/${relative}`
}

const openMountDialog = (
  action: 'initialize' | 'mount',
  disk: DiskInfo,
  partition?: DiskPartitionInfo,
) => {
  const name = partition?.name || disk.partitions[0]?.name || disk.name
  mountpointInput.value = name
  pendingMountTarget.value =
    action === 'mount' && partition ? { action, disk, partition } : { action: 'initialize', disk }
  mountDialogVisible.value = true
}

const closeMountDialog = () => {
  mountDialogVisible.value = false
  pendingMountTarget.value = null
  mountpointInput.value = ''
}

const confirmMountDialog = async () => {
  const target = pendingMountTarget.value
  if (!target) return
  const mountpoint = normalizeMountpointInput(mountpointInput.value)
  if (!mountpoint) {
    notificationStore.error(t('app.diskManager.messages.invalidMountpoint'))
    return
  }
  closeMountDialog()
  if (target.action === 'initialize') {
    await confirmInitPartition(target.disk, mountpoint)
    return
  }
  await runPartitionAction('mount', target.disk, target.partition, mountpoint)
}

const runPartitionAction = async (
  action: 'mount' | 'unmount' | 'format',
  disk: DiskInfo,
  partition: DiskPartitionInfo,
  mountpoint?: string,
) => {
  if (disk.isSystemDisk || actionLoading.value) return

  const isMounted = isMountedPartition(partition)
  if (action === 'mount' && !canMountPartition(disk, partition)) return
  if (action === 'unmount' && !canUnmountPartition(disk, partition)) return
  if (action === 'format' && !canFormatPartition(disk, partition)) return
  let confirmed = false
  if (action === 'mount') {
    if (!mountpoint) {
      openMountDialog('mount', disk, partition)
      return
    }
    confirmed = await confirmationModal.showConfirmation(
      t('app.diskManager.confirm.mountMessage', {
        disk: disk.name,
        partition: partition.name,
        mountpoint,
      }),
      t('app.diskManager.confirm.title'),
      t('app.diskManager.actions.mount'),
      t('confirmation.cancel'),
    )
  } else if (action === 'unmount') {
    confirmed = await confirmationModal.showConfirmation(
      t('app.diskManager.confirm.unmountMessage', { disk: disk.name, partition: partition.name }),
      t('app.diskManager.confirm.title'),
      t('app.diskManager.actions.unmount'),
      t('confirmation.cancel'),
    )
  } else {
    confirmed = await confirmationModal.showConfirmation(
      t('app.diskManager.confirm.formatMessage', {
        disk: disk.name,
        partition: partition.name,
      }),
      t('app.diskManager.confirm.title'),
      t('app.diskManager.actions.format'),
      t('confirmation.cancel'),
    )
    if (confirmed) {
      confirmed = await confirmationModal.showConfirmation(
        t('app.diskManager.confirm.formatDangerMessage', { partition: partition.name }),
        t('app.diskManager.confirm.dangerTitle'),
        t('app.diskManager.confirm.formatDangerConfirm'),
        t('confirmation.cancel'),
      )
    }
  }
  if (!confirmed) return

  actionLoading.value = true
  try {
    let response
    if (action === 'mount') {
      response = await systemClient.value.mountPartition(disk.name, partition.name, mountpoint)
    } else if (action === 'unmount') {
      response = await systemClient.value.unmountPartition(disk.name, partition.name)
    } else {
      if (isMounted) {
        throw new Error(t('app.diskManager.messages.formatNeedUnmount'))
      }
      response = await systemClient.value.formatPartition(disk.name, partition.name, 'ext4')
    }
    if (!response.success) {
      throw new Error(response.message || t('app.diskManager.messages.operationFailed'))
    }
    const successKey =
      action === 'mount'
        ? 'app.diskManager.messages.mountSuccess'
        : action === 'unmount'
          ? 'app.diskManager.messages.unmountSuccess'
          : 'app.diskManager.messages.formatSuccess'
    notificationStore.success(t(successKey, { partition: partition.name }))
    await reloadDisks()
  } catch (error) {
    const message =
      error instanceof Error ? error.message : t('app.diskManager.messages.operationFailed')
    notificationStore.error(message)
  } finally {
    actionLoading.value = false
  }
}

onMounted(async () => {
  await reloadDisks()
})
</script>

<template>
  <div class="disk-manager" data-seclab-app="disk-manager">
    <div v-if="sortedDiskRows.length === 0 && !loading" class="empty-state">
      <div class="empty-content">
        <SecLabIcon class="empty-icon" name="disk" :size="48" />
        <p>{{ t('app.diskManager.messages.empty') }}</p>
      </div>
    </div>

    <template v-else>
      <div class="grid">
        <SecLabCard
          v-for="disk in sortedDiskRows"
          :key="disk.name"
          class="card"
          shadow="hover"
          :class="{ 'is-system-disk': disk.isSystemDisk }"
        >
          <template #header>
            <div class="card-header">
              <span class="disk-name">{{ disk.name }}</span>
              <SecLabTag v-if="disk.isSystemDisk" type="danger">
                {{ t('app.diskManager.labels.systemDisk') }}
              </SecLabTag>
            </div>
          </template>

          <div class="info-row">
            <div class="item">
              <span class="label">{{ t('app.diskManager.fields.size') }}</span>
              <span class="value">{{ formatBytes(disk.sizeBytes) }}</span>
            </div>
            <div class="divider"></div>
            <div class="item">
              <span class="label">{{ t('app.diskManager.fields.partitionCount') }}</span>
              <span class="value">{{ disk.partitionCount }}</span>
            </div>
            <div class="divider"></div>
            <div class="item">
              <span class="label">{{ t('app.diskManager.fields.diskType') }}</span>
              <span class="value">{{ textOrPlaceholder(disk.diskType) }}</span>
            </div>
            <div class="divider"></div>
            <div class="item">
              <span class="label">{{ t('app.diskManager.fields.model') }}</span>
              <span class="value">{{ textOrPlaceholder(disk.model) }}</span>
            </div>
            <div class="divider"></div>
            <div class="item">
              <span class="label">{{ t('app.diskManager.fields.serial') }}</span>
              <span class="value">{{ textOrPlaceholder(disk.serial) }}</span>
            </div>
            <div class="divider"></div>
            <div class="item">
              <span class="label">{{ t('app.diskManager.fields.partitionTable') }}</span>
              <span class="value">{{ textOrPlaceholder(disk.partitionTable) }}</span>
            </div>
          </div>

          <div v-if="isUnformattedDisk(disk)" class="unformatted-section">
            <div class="unformatted-hint">
              <SecLabIcon class="hint-icon" name="info" :size="14" />
              <p>{{ t('app.diskManager.messages.unformattedDisk') }}</p>
            </div>
            <div class="unformatted-actions">
              <SecLabButton
                v-if="canInitializeDisk(disk)"
                type="primary"
                :loading="actionLoading"
                @click="handleInitPartition(disk)"
              >
                {{ t('app.diskManager.actions.initPartition') }}
              </SecLabButton>
              <SecLabButton v-if="!canInitializeDisk(disk)" type="secondary" disabled>
                {{ t('app.diskManager.actions.unavailable') }}
              </SecLabButton>
            </div>
          </div>

          <div v-else class="partition-section">
            <div class="section-header">
              <h4 class="section-title">{{ t('app.diskManager.partition.title') }}</h4>
            </div>
            <SecLabTable :data="disk.partitions" :columns="partitionColumns" border>
              <template #size="{ row }: { row: any }">
                {{ formatBytes(row.sizeBytes) }}
              </template>
              <template #used="{ row }: { row: any }">
                {{ row.usedBytes == null ? '--' : formatBytes(row.usedBytes) }}
              </template>
              <template #available="{ row }: { row: any }">
                {{ row.availableBytes == null ? '--' : formatBytes(row.availableBytes) }}
              </template>
              <template #usage="{ row }: { row: any }">
                {{ formatUsagePercent(row.usagePercent) }}
              </template>
              <template #action="{ row }: { row: any }">
                <div class="action-cell">
                  <SecLabActionMenu
                    :actions="partitionActions(disk, row)"
                    :disabled="disk.isSystemDisk || actionLoading"
                    :label="t('app.diskManager.partition.columns.action')"
                  />
                </div>
              </template>
            </SecLabTable>
          </div>

          <div v-if="disk.isSystemDisk" class="system-disk-hint">
            <SecLabIcon class="hint-icon" name="lock" :size="14" />
            <span>{{ t('app.diskManager.messages.systemDiskBlocked') }}</span>
          </div>
        </SecLabCard>
      </div>
    </template>

    <SecLabLoading :loading="loading" cover />

    <SecLabDialog
      :visible="mountDialogVisible"
      :title="t('app.diskManager.mountDialog.title')"
      width="420px"
      @close="closeMountDialog"
    >
      <div class="mount-dialog-content">
        <label class="mount-label">{{ t('app.diskManager.mountDialog.label') }}</label>
        <SecLabInput
          v-model="mountpointInput"
          :placeholder="t('app.diskManager.mountDialog.placeholder')"
        />
        <p class="mount-hint">
          {{
            t('app.diskManager.mountDialog.hint', {
              mountpoint: normalizeMountpointInput(mountpointInput) || '--',
            })
          }}
        </p>
      </div>
      <template #footer>
        <SecLabButton type="secondary" @click="closeMountDialog">
          {{ t('confirmation.cancel') }}
        </SecLabButton>
        <SecLabButton type="primary" @click="confirmMountDialog">
          {{
            pendingMountTarget?.action === 'initialize'
              ? t('app.diskManager.actions.startPartition')
              : t('app.diskManager.actions.mount')
          }}
        </SecLabButton>
      </template>
    </SecLabDialog>
  </div>
</template>

<style scoped>
.disk-manager {
  height: 100%;
  padding: var(--sdl-space-3);
  background: var(--sdl-bg-canvas);
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
  box-sizing: border-box;
}

.empty-state {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--sdl-bg-panel);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-lg);
}

.empty-content {
  text-align: center;
}

.empty-icon {
  display: block;
  margin-bottom: var(--sdl-space-3);
  opacity: 0.5;
}

.empty-content p {
  color: var(--sdl-text-secondary);
  font-size: var(--sdl-font-body);
}

.grid {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
}

.card.is-system-disk {
  border-color: var(--sdl-danger-soft);
}

.card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.disk-name {
  font-size: var(--sdl-font-subtitle);
  font-weight: 700;
  color: var(--sdl-text-primary);
}

.info-row {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-4);
  background: var(--sdl-bg-panel);
  padding: var(--sdl-space-2) var(--sdl-space-4);
  border-radius: var(--sdl-radius-md);
  overflow-x: auto;
  scrollbar-width: none;
}

.info-row .item {
  display: flex;
  flex-direction: column;
  flex-shrink: 0;
}

.info-row .label {
  font-size: 10px;
  color: var(--sdl-text-muted);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.info-row .value {
  font-size: var(--sdl-font-body-sm);
  color: var(--sdl-text-primary);
  font-weight: 600;
}

.info-row .divider {
  width: 1px;
  height: 20px;
  background: var(--sdl-border-subtle);
}

.unformatted-section {
  margin-top: var(--sdl-space-4);
  background: var(--sdl-bg-panel);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  padding: var(--sdl-space-3) var(--sdl-space-4);
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.unformatted-hint {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
}

.unformatted-hint p {
  margin: 0;
  color: var(--sdl-text-secondary);
  font-weight: 500;
}

.unformatted-actions {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
  flex-shrink: 0;
}

.partition-section {
  margin-top: var(--sdl-space-4);
}

.section-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sdl-space-3);
  margin-bottom: var(--sdl-space-2);
}

.section-title {
  margin: 0;
  font-size: var(--sdl-font-body-sm);
  color: var(--sdl-text-secondary);
  font-weight: 600;
}

.action-cell {
  display: flex;
  justify-content: center;
}

.system-disk-hint {
  margin-top: var(--sdl-space-3);
  padding: var(--sdl-space-2) var(--sdl-space-3);
  background: var(--sdl-danger-soft);
  border-radius: var(--sdl-radius-sm);
  color: var(--sdl-danger);
  font-size: var(--sdl-font-caption);
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
}

.hint-icon {
  flex-shrink: 0;
}

.mount-dialog-content {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-2);
}

.mount-label {
  color: var(--sdl-text-secondary);
  font-size: var(--sdl-font-body-sm);
  font-weight: 600;
}

.mount-hint {
  margin: 0;
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-caption);
  line-height: 1.5;
}

@media (max-width: 1024px) {
  .info-row {
    gap: var(--sdl-space-3);
  }
}
</style>
