<script setup lang="ts">
/**
 * @file DockerContainerList.vue
 * @description Docker 容器列表组件，展示所有容器，支持搜索、批量操作和单容器管理。
 */

import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDockerStore } from '@/stores/docker'
import { useNotificationStore } from '@/stores/notification'
import { useContainerResourceStats } from './composables/useContainerResourceStats'
import { getStateIcon, formatPercent, formatBytes } from '@/utils/docker-format'
import { isSuiteManagedResource } from './docker-suite-labels'
import {
  SecLabButton,
  SecLabTag,
  SecLabCheckbox,
  SecLabEmpty,
  SecLabInput,
  SecLabActionMenu,
} from '@/components/ui'
import SecLabTable, { type SecLabTableColumn } from '@/components/ui/SecLabTable.vue'
import SecLabIcon from '@/components/icons/SecLabIcon.vue'
import type * as dockerType from '@/api/interface/docker'

const emit = defineEmits<{
  /** 触发打开详情页事件 */
  (e: 'open-detail', id: string): void
}>()

const { t } = useI18n()
const store = useDockerStore()
const notificationStore = useNotificationStore()

// ─── 搜索过滤 ───
const searchQuery = ref('')

const filteredContainers = computed(() => {
  const query = searchQuery.value.trim().toLowerCase()
  if (!query) return store.containers
  return store.containers.filter((c) => {
    const name = c.Names?.[0]?.replace(/^\//, '').toLowerCase() || ''
    const id = c.Id?.toLowerCase() || ''
    const image = c.Image?.toLowerCase() || ''
    return name.includes(query) || id.includes(query) || image.includes(query)
  })
})

// ─── 多选状态管理 ───
const selectedIds = ref<Set<string>>(new Set())

const isAllSelected = computed(() => {
  const list = filteredContainers.value
  return list.length > 0 && selectedIds.value.size === list.length
})

const isIndeterminate = computed(() => {
  const list = filteredContainers.value
  return selectedIds.value.size > 0 && selectedIds.value.size < list.length
})

const handleSelectAll = (val: boolean) => {
  if (val) {
    selectedIds.value = new Set(filteredContainers.value.map((c) => c.Id as string))
  } else {
    selectedIds.value.clear()
  }
}

const handleSelectRow = (id: string, val: boolean) => {
  if (val) {
    selectedIds.value.add(id)
  } else {
    selectedIds.value.delete(id)
  }
}

// ─── 性能指标 Hook ───
const {
  getContainerStats,
  requestContainerStats,
  handleContainerRowMouseEnter,
  formatNetworkUsage,
} = useContainerResourceStats({
  containerResourceStats: computed(() => store.containerResourceStats),
  onFetchContainerStats: (id) => store.fetchContainerResourceStats(id),
  formatBytes,
})

// ─── 表格列配置 ───
const columns = computed<SecLabTableColumn[]>(() => [
  { label: '', width: 48, align: 'center', slot: 'selection', headerSlot: 'selectionHeader' },
  { label: t('app.docker.containers.name'), minWidth: 240, slot: 'name' },
  { label: t('app.docker.containers.status'), width: 100, align: 'center', slot: 'status' },
  { label: t('app.docker.containers.resources'), minWidth: 320, slot: 'resource' },
  { label: t('app.docker.containers.actions'), width: 120, align: 'center', slot: 'actions' },
])

// ─── 单容器操作 ───
const handleAction = async (
  containerId: string | null,
  containerName: string | null,
  action: 'start' | 'stop' | 'restart' | 'remove' | 'pause' | 'unpause' | 'kill',
) => {
  await store.handleContainerAction(containerId, containerName, action)
}

const handleDelete = async (
  containerId: string | null,
  containerName: string | null,
  containerState: string | undefined,
) => {
  if (containerState === 'running') {
    notificationStore.error(
      t('app.docker.containers.messages.deleteStateBlocked', { state: containerState }),
    )
    return
  }
  await store.handleContainerAction(containerId, containerName, 'remove')
}

// ─── 批量操作 ───
const handleBatchAction = async (
  action: 'start' | 'stop' | 'restart' | 'remove' | 'pause' | 'unpause' | 'kill',
) => {
  const ids = Array.from(selectedIds.value)
  if (ids.length === 0) return
  const names = ids.map(
    (id) => store.containers.find((c) => c.Id === id)?.Names?.[0]?.replace(/^\//, '') || id,
  )

  // 删除操作特殊检查：禁止删除运行中的容器
  if (action === 'remove') {
    const hasRunning = ids.some((id) => {
      const state = store.containers.find((c) => c.Id === id)?.State
      return state === 'running'
    })
    if (hasRunning) {
      notificationStore.error(t('app.docker.containers.messages.deleteRunningBlocked'))
      return
    }
  }

  await store.handleContainerAction(ids, names, action)
  selectedIds.value.clear()
}

// ─── 创建容器 ───
const triggerCreate = async () => {
  await store.startContainerCreateFlow()
}

// ─── 查看详情 ───
const openContainerDetail = (containerId: string | undefined, state?: string) => {
  if (!containerId) return
  emit('open-detail', containerId)
  requestContainerStats(containerId, state)
}
</script>

<template>
  <div class="container-list" data-ui="docker-container-list">
    <div class="card-actions" data-ui="docker-container-actions">
      <!-- 搜索框 -->
      <div class="search-wrapper">
        <SecLabInput
          v-model="searchQuery"
          :placeholder="t('app.docker.containers.containerNamePlaceholder')"
          clearable
          data-ui="container-search-input"
          class="search-input"
        />
      </div>

      <!-- 批量操作 -->
      <div class="batch-actions" v-if="selectedIds.size > 0">
        <SecLabButton @click="handleBatchAction('start')" data-ui="batch-start-btn" size="small">
          {{ t('app.docker.containers.batchStart') }}
        </SecLabButton>
        <SecLabButton @click="handleBatchAction('stop')" data-ui="batch-stop-btn" size="small">
          {{ t('app.docker.containers.batchStop') }}
        </SecLabButton>
        <SecLabButton
          @click="handleBatchAction('restart')"
          data-ui="batch-restart-btn"
          size="small"
        >
          {{ t('app.docker.containers.batchRestart') }}
        </SecLabButton>
        <SecLabButton @click="handleBatchAction('pause')" data-ui="batch-pause-btn" size="small">
          {{ t('app.docker.containers.batchPause') }}
        </SecLabButton>
        <SecLabButton
          @click="handleBatchAction('unpause')"
          data-ui="batch-unpause-btn"
          size="small"
        >
          {{ t('app.docker.containers.batchUnpause') }}
        </SecLabButton>
        <SecLabButton
          @click="handleBatchAction('kill')"
          type="danger"
          data-ui="batch-kill-btn"
          size="small"
        >
          {{ t('app.docker.containers.batchKill') }}
        </SecLabButton>
        <SecLabButton
          type="danger"
          @click="handleBatchAction('remove')"
          data-ui="batch-remove-btn"
          size="small"
        >
          {{ t('app.docker.containers.batchDelete') }}
        </SecLabButton>
      </div>

      <SecLabButton type="primary" @click="triggerCreate" data-ui="create-container-btn">
        {{ t('app.docker.containers.create') }}
      </SecLabButton>
    </div>

    <!-- 容器表格 -->
    <div class="container-table-wrapper" data-ui="docker-container-table">
      <SecLabEmpty
        v-if="filteredContainers.length === 0"
        :description="t('app.docker.containers.noContainers')"
      />
      <SecLabTable
        v-else
        :data="filteredContainers"
        :columns="columns"
        border
        @row-mouseenter="handleContainerRowMouseEnter"
      >
        <!-- 表头全选 -->
        <template #selectionHeader>
          <SecLabCheckbox
            :model-value="isAllSelected"
            :indeterminate="isIndeterminate"
            @update:model-value="handleSelectAll"
          />
        </template>

        <!-- 行选择 -->
        <template #selection="{ row: container }: { row: dockerType.ContainerSummary }">
          <SecLabCheckbox
            v-if="container.Id"
            :model-value="selectedIds.has(container.Id)"
            @update:model-value="(val) => handleSelectRow(container.Id!, val)"
          />
        </template>

        <!-- 容器名称 -->
        <template #name="{ row: container }: { row: dockerType.ContainerSummary }">
          <div class="resource-name-cell">
            <SecLabButton
              type="secondary"
              size="small"
              @click="openContainerDetail(container?.Id, container?.State)"
            >
              {{ container?.Names?.[0]?.replace(/^\//, '') || 'N/A' }}
            </SecLabButton>
            <SecLabTag v-if="isSuiteManagedResource(container?.Labels)" type="primary" size="small">
              {{ t('app.docker.suiteManaged') }}
            </SecLabTag>
          </div>
        </template>

        <!-- 运行状态 -->
        <template #status="{ row: container }: { row: dockerType.ContainerSummary }">
          <SecLabTag :type="container?.State === 'running' ? 'success' : 'info'">
            <SecLabIcon :name="getStateIcon(container?.State)" :size="14" />
          </SecLabTag>
        </template>

        <!-- 资源占用 -->
        <template #resource="{ row: container }: { row: dockerType.ContainerSummary }">
          <template v-if="container.Id && getContainerStats(container.Id)">
            <div class="resource-line">
              CPU {{ formatPercent(getContainerStats(container?.Id)?.cpuPercent) }}
            </div>
            <div class="resource-line">
              MEM {{ formatPercent(getContainerStats(container?.Id)?.memoryPercent) }}
            </div>
            <div class="resource-line">NET {{ formatNetworkUsage(container?.Id) }}</div>
          </template>
          <span v-else class="resource-muted">-</span>
        </template>

        <!-- 单行操作 -->
        <template #actions="{ row: container }: { row: dockerType.ContainerSummary }">
          <SecLabActionMenu
            :label="t('common.actions')"
            :actions="[
              {
                label: t('app.docker.containers.startContainer'),
                handler: () =>
                  handleAction(
                    container?.Id ?? null,
                    container?.Names?.[0]?.replace(/^\//, '') ?? null,
                    'start',
                  ),
                disabled: container?.State === 'running',
              },
              {
                label: t('app.docker.containers.stopContainer'),
                handler: () =>
                  handleAction(
                    container?.Id ?? null,
                    container?.Names?.[0]?.replace(/^\//, '') ?? null,
                    'stop',
                  ),
                disabled: container?.State !== 'running',
              },
              {
                label: t('app.docker.containers.restartContainer'),
                handler: () =>
                  handleAction(
                    container?.Id ?? null,
                    container?.Names?.[0]?.replace(/^\//, '') ?? null,
                    'restart',
                  ),
              },
              {
                label: t('app.docker.containers.removeContainer'),
                handler: () =>
                  handleDelete(
                    container?.Id ?? null,
                    container?.Names?.[0]?.replace(/^\//, '') ?? null,
                    container?.State,
                  ),
                disabled: container?.State === 'running',
                tooltip:
                  container?.State === 'running'
                    ? t('app.docker.containers.messages.deleteRunningBlocked')
                    : undefined,
                class: 'btn-delete',
              },
            ]"
          />
        </template>

        <template #empty>
          <SecLabEmpty :description="t('app.docker.containers.noContainers')" />
        </template>
      </SecLabTable>
    </div>
  </div>
</template>

<style scoped>
.container-list {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}

.card-actions {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  margin-bottom: var(--sdl-space-4);
  gap: var(--sdl-space-3);
}

.search-wrapper {
  margin-right: auto;
  width: 280px;
}

.batch-actions {
  display: flex;
  gap: var(--sdl-space-2);
  flex-wrap: wrap;
}

.container-table-wrapper {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.resource-line {
  line-height: 1.4;
  font-family: var(--sdl-font-mono);
  font-size: var(--sdl-font-caption);
  color: var(--sdl-text-secondary);
}

.resource-muted {
  color: var(--sdl-text-subtle);
  font-style: italic;
}

.resource-name-cell {
  min-width: 0;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
</style>
