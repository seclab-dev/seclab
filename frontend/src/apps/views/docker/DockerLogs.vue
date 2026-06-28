<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import {
  SecLabTable,
  SecLabTag,
  SecLabPagination,
  SecLabSelect,
  SecLabEmpty,
  SecLabLoading,
} from '@/components/ui'
import { dockerApi } from '@/api/modules/docker'
import type { PlatformLog } from '@/api/interface/logging'
import { useNotificationStore } from '@/stores/notification'
import { useI18n } from 'vue-i18n'
import { formatDateTime } from '@/utils/time'

/**
 * @file DockerLogs.vue
 * @description Docker 日志列表组件，严格遵循 SDL 设计规范。
 */

const logs = ref<PlatformLog[]>([])
const isLoading = ref(false)
const notificationStore = useNotificationStore()
const { t } = useI18n()

const currentPage = ref(1)
const pageSize = ref(10)
const totalLogs = ref(0)
const totalPages = computed(() => Math.max(1, Math.ceil(totalLogs.value / pageSize.value)))

/** 日志表格列定义 */
const logColumns = computed(() => [
  { label: t('app.platformLog.time'), width: 190, slot: 'time' },
  { label: t('app.platformLog.userOrSource'), width: 140, slot: 'user' },
  { prop: 'event', label: t('app.platformLog.eventType'), width: 160 },
  { label: t('app.platformLog.result'), width: 110, slot: 'result' },
  { label: t('app.platformLog.details'), minWidth: 280, slot: 'details' },
])

/** 每页条数选项 */
const pageSizeOptions = computed(() => [
  { label: t('common.pagination.perPage', { n: 10 }), value: 10 },
  { label: t('common.pagination.perPage', { n: 20 }), value: 20 },
  { label: t('common.pagination.perPage', { n: 50 }), value: 50 },
])

const getResultType = (result: string): 'success' | 'danger' | 'info' => {
  if (result === 'SUCCESS') {
    return 'success'
  }
  if (result === 'FAILED' || result === 'FAILURE') {
    return 'danger'
  }
  return 'info'
}

const formatDetails = (metadata: PlatformLog['metadata']): string => {
  const message = metadata?.message
  if (typeof message === 'string' && message.trim()) {
    return message
  }
  return ''
}

const fetchNodeLogs = async () => {
  isLoading.value = true

  const res = await dockerApi.fetchDockerLogs({
    page: currentPage.value,
    pageSize: pageSize.value,
  })

  if (res.success && res.data) {
    logs.value = res.data.logs ?? []
    totalLogs.value = res.data.total ?? 0

    const nextTotalPages = Math.max(1, Math.ceil(totalLogs.value / pageSize.value))
    if (currentPage.value > nextTotalPages) {
      currentPage.value = nextTotalPages
      isLoading.value = false
      return
    }
  } else {
    logs.value = []
    totalLogs.value = 0
    notificationStore.error(res.message || t('app.platformLog.fetchFailed'))
  }

  isLoading.value = false
}

watch(currentPage, fetchNodeLogs)
watch(pageSize, () => {
  currentPage.value = 1
  fetchNodeLogs()
})

onMounted(fetchNodeLogs)
</script>

<template>
  <div class="node-log-container">
    <div class="docker-card">
      <h2 class="card-title">
        {{ t('app.docker.menu.logs') }}
        <span class="total-count">{{ t('app.platformLog.total', { total: totalLogs }) }}</span>
      </h2>

      <div class="card-scroll-wrapper">
        <SecLabLoading :loading="isLoading" />

        <SecLabTable
          v-if="logs.length > 0 || isLoading"
          :data="logs"
          :columns="logColumns"
          border
          class="node-log-table"
        >
          <template #time="{ row }: { row: any }">
            {{ formatDateTime(row.timestamp) }}
          </template>

          <template #user="{ row }: { row: any }">
            {{ row?.username || row?.client_ip || 'System' }}
          </template>

          <template #result="{ row }: { row: any }">
            <SecLabTag :type="getResultType(row?.status || '')">
              {{ row?.status || '-' }}
            </SecLabTag>
          </template>

          <template #details="{ row }: { row: any }">
            <span class="log-message-cell">{{ formatDetails(row?.metadata) }}</span>
          </template>

          <template #empty>
            <SecLabEmpty :description="t('app.platformLog.noLogs')" />
          </template>
        </SecLabTable>

        <SecLabEmpty v-else :description="t('app.platformLog.noLogs')" />
      </div>

      <div class="pagination-controls">
        <div class="page-size-selector">
          <SecLabSelect
            v-model="pageSize"
            size="small"
            class="page-size-select"
            :options="pageSizeOptions"
          />
        </div>

        <SecLabPagination
          :current-page="currentPage"
          :total-pages="totalPages"
          @page-change="(p) => (currentPage = p)"
        />

        <span class="pagination-info">
          <template v-if="isLoading">{{ t('app.platformLog.loading') }}</template>
          <template v-else>
            {{ t('app.fileManager.pageStats', { currentPage, totalPages, total: totalLogs }) }}
          </template>
        </span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.node-log-container {
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

.card-title {
  margin: 0 0 var(--sdl-space-3) 0;
  font-size: var(--sdl-font-title);
  color: var(--sdl-text-primary);
}

.total-count {
  margin-left: var(--sdl-space-2);
  font-size: var(--sdl-font-body-sm);
  color: var(--sdl-text-muted);
  font-weight: normal;
}

.card-scroll-wrapper {
  flex: 1;
  min-height: 0;
  position: relative;
  overflow: hidden;
}

.log-message-cell {
  white-space: pre-wrap;
  word-break: break-all;
  font-family: var(--sdl-font-mono);
  font-size: var(--sdl-font-code);
  color: var(--sdl-text-secondary);
}

.pagination-controls {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-top: var(--sdl-space-3);
  padding-top: var(--sdl-space-3);
  border-top: 1px solid var(--sdl-border-subtle);
}

.page-size-select {
  width: 110px;
}

.pagination-info {
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-caption);
}

@media (max-width: 768px) {
  .pagination-controls {
    flex-direction: column;
    gap: var(--sdl-space-3);
  }
}
</style>
