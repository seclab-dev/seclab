<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { notificationsApi } from '@/api/modules/notifications'
import type { NotificationRecord, NotificationLevel } from '@/api/generated'
import { useNotificationStore } from '@/stores/notification'
import { useConfirmationModalStore } from '@/stores/confirmation-modal'
import { useI18n } from 'vue-i18n'
import { formatDateTime } from '@/utils/time'
import {
  SecLabButton,
  SecLabCard,
  SecLabTable,
  SecLabTag,
  SecLabPagination,
  SecLabLoading,
  SecLabSelect,
  SecLabInput,
  SecLabCheckbox,
} from '@/components/ui'
import type { SecLabTableColumn } from '@/components/ui/SecLabTable.vue'

defineProps<{
  isMaximized?: boolean
  payload?: Record<string, unknown>
}>()

const { t } = useI18n()
const notificationStore = useNotificationStore()
const modalStore = useConfirmationModalStore()

const currentPage = ref(1)
const pageSize = ref(20)
const totalRecords = ref(0)
const records = ref<NotificationRecord[]>([])
const isLoading = ref(false)
const selectedIds = ref<number[]>([])
const keyword = ref('')
const level = ref('all')

const levelOptions = computed(() => [
  { label: t('app.notificationHistory.level.all'), value: 'all' },
  { label: t('app.notificationHistory.level.info'), value: 'info' },
  { label: t('app.notificationHistory.level.success'), value: 'success' },
  { label: t('app.notificationHistory.level.warning'), value: 'warning' },
  { label: t('app.notificationHistory.level.error'), value: 'error' },
])

const pageSizeOptions = [
  { label: '20', value: 20 },
  { label: '50', value: 50 },
  { label: '100', value: 100 },
]

const totalPages = computed(() => Math.max(1, Math.ceil(totalRecords.value / pageSize.value)))

const tableColumns = computed<SecLabTableColumn[]>(() => [
  { label: '', width: 50, align: 'center', slot: 'selection' },
  { label: t('app.notificationHistory.columns.id'), width: 80, align: 'center', slot: 'index' },
  { label: t('app.notificationHistory.columns.time'), width: 180, align: 'center', slot: 'time' },
  { label: t('app.notificationHistory.columns.type'), width: 100, align: 'center', slot: 'type' },
  {
    prop: 'message',
    label: t('app.notificationHistory.columns.message'),
    minWidth: 300,
    slot: 'message',
  },
])

const getTagType = (levelValue: string): 'success' | 'danger' | 'warning' | 'info' => {
  const val = (levelValue || '').toLowerCase()
  if (val === 'success') return 'success'
  if (val === 'error' || val === 'danger') return 'danger'
  if (val === 'warning') return 'warning'
  return 'info'
}

const fetchHistoryLogs = async () => {
  isLoading.value = true
  try {
    const response = await notificationsApi.list({
      page: currentPage.value,
      pageSize: pageSize.value,
      keyword: keyword.value || undefined,
      level: level.value !== 'all' ? (level.value as NotificationLevel) : undefined,
    })
    if (!response.success || !response.data) {
      records.value = []
      totalRecords.value = 0
      notificationStore.error(response.message || t('app.notificationHistory.fetchFailed'))
      return
    }
    records.value = response.data.records ?? []
    totalRecords.value = response.data.total ?? 0
  } catch (error) {
    records.value = []
    totalRecords.value = 0
    const msg = error instanceof Error ? error.message : t('app.notificationHistory.fetchFailed')
    notificationStore.error(msg)
  } finally {
    isLoading.value = false
  }
}

const handleSearch = () => {
  currentPage.value = 1
  void fetchHistoryLogs()
}

const handleRefresh = () => {
  void fetchHistoryLogs()
}

const toggleSelection = (id: number) => {
  const index = selectedIds.value.indexOf(id)
  if (index === -1) {
    selectedIds.value.push(id)
  } else {
    selectedIds.value.splice(index, 1)
  }
}

const handleBatchDelete = async () => {
  if (selectedIds.value.length === 0) return

  const confirmed = await modalStore.showConfirmation(
    t('app.notificationHistory.batchDeleteConfirm', { count: selectedIds.value.length }),
    t('app.notificationHistory.title'),
    t('app.notificationHistory.batchDeleteAction'),
    t('confirmation.cancel'),
  )

  if (!confirmed) return

  const response = await notificationsApi.batchRemove({ ids: selectedIds.value })
  if (!response.success) {
    notificationStore.error(response.message || t('app.notificationHistory.batchDeleteFailed'))
    return
  }

  notificationStore.success(t('app.notificationHistory.batchDeleteSuccess'))
  selectedIds.value = []
  void fetchHistoryLogs()
}

const handleClearLogs = async () => {
  const confirmed = await modalStore.showConfirmation(
    t('app.notificationHistory.clearConfirm'),
    t('app.notificationHistory.title'),
    t('app.notificationHistory.clearAction'),
    t('confirmation.cancel'),
  )

  if (!confirmed) return

  const response = await notificationsApi.clear()
  if (!response.success) {
    notificationStore.error(response.message || t('app.notificationHistory.clearFailed'))
    return
  }

  notificationStore.success(t('app.notificationHistory.clearSuccess'))
  records.value = []
  totalRecords.value = 0
  currentPage.value = 1
  selectedIds.value = []
}

watch([currentPage, pageSize], () => {
  void fetchHistoryLogs()
})

onMounted(() => {
  void fetchHistoryLogs()
})
</script>

<template>
  <div class="notification-history" data-seclab-app="notification-history">
    <SecLabCard shadow="never" class="toolbar-card" data-ui="notification-toolbar">
      <div class="toolbar">
        <div class="toolbar-left">
          <div class="title-block">
            <h2>{{ t('app.notificationHistory.title') }}</h2>
            <p class="stats">
              {{
                t('app.notificationHistory.pageStats', {
                  currentPage,
                  totalPages,
                  total: totalRecords,
                })
              }}
            </p>
          </div>
        </div>
        <div class="toolbar-right">
          <SecLabButton v-if="selectedIds.length > 0" type="danger" @click="handleBatchDelete">
            {{ t('app.notificationHistory.batchDeleteAction') }} ({{ selectedIds.length }})
          </SecLabButton>
          <div class="filter-group">
            <div style="width: 140px">
              <SecLabSelect v-model="level" :options="levelOptions" />
            </div>
            <div style="width: 200px">
              <SecLabInput
                v-model="keyword"
                :placeholder="t('app.notificationHistory.searchPlaceholder')"
                @change="handleSearch"
                @keyup.enter="handleSearch"
              />
            </div>
            <SecLabButton @click="handleRefresh">
              {{ t('app.notificationHistory.refresh') }}
            </SecLabButton>
            <SecLabButton
              type="danger"
              plain
              :disabled="totalRecords === 0"
              @click="handleClearLogs"
            >
              {{ t('app.notificationHistory.clearAction') }}
            </SecLabButton>
          </div>
        </div>
      </div>
    </SecLabCard>

    <SecLabCard shadow="never" class="table-card" full-height>
      <SecLabTable :data="records" :columns="tableColumns" border data-ui="notification-table">
        <template #selection="{ row }: { row: NotificationRecord }">
          <div style="display: flex; justify-content: center">
            <SecLabCheckbox
              :model-value="selectedIds.includes(row.id)"
              @change="toggleSelection(row.id)"
            />
          </div>
        </template>
        <template #index="{ index }">
          {{ (currentPage - 1) * pageSize + index + 1 }}
        </template>
        <template #time="{ row }: { row: NotificationRecord }">
          <span class="time-cell">{{ formatDateTime(row.createdAt * 1000) }}</span>
        </template>
        <template #type="{ row }: { row: NotificationRecord }">
          <SecLabTag :type="getTagType(row.level)" size="small">
            {{ row.level.toUpperCase() }}
          </SecLabTag>
        </template>
        <template #message="{ row }: { row: NotificationRecord }">
          <div class="message-cell">{{ row.message }}</div>
        </template>
        <template #empty>
          <div class="empty-placeholder">
            {{ t('app.notificationHistory.empty') }}
          </div>
        </template>
      </SecLabTable>
    </SecLabCard>

    <SecLabCard shadow="never" class="pagination-card">
      <div class="pagination-wrapper" data-ui="notification-pagination">
        <SecLabPagination
          :current-page="currentPage"
          :total-pages="totalPages"
          @page-change="(p) => (currentPage = p)"
        />
        <div style="width: 80px; margin-left: var(--sdl-space-4)">
          <SecLabSelect v-model="pageSize" :options="pageSizeOptions" />
        </div>
      </div>
    </SecLabCard>

    <SecLabLoading :loading="isLoading && records.length === 0" cover />
  </div>
</template>

<style scoped>
.notification-history {
  height: 100%;
  padding: var(--sdl-space-3);
  background: var(--sdl-bg-canvas);
  overflow: hidden;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
  min-height: 0;
  box-sizing: border-box;
}

.toolbar-card {
  flex-shrink: 0;
}

.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--sdl-space-3);
}

.toolbar-left {
  display: flex;
  align-items: center;
}

.toolbar-right {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-3);
  flex-wrap: wrap;
}

.filter-group {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
}

.title-block h2 {
  margin: 0;
  font-size: var(--sdl-font-title);
  color: var(--sdl-text-primary);
  font-weight: 700;
}

.title-block .stats {
  margin: var(--sdl-space-1) 0 0;
  font-size: var(--sdl-font-caption);
  color: var(--sdl-text-muted);
}

.table-card {
  flex: 1;
  min-height: 0;
}

.time-cell {
  font-family: var(--sdl-font-mono);
  font-size: 12px;
  color: var(--sdl-text-secondary);
}

.message-cell {
  font-size: var(--sdl-font-body-sm);
  color: var(--sdl-text-primary);
  line-height: 1.5;
  word-break: break-all;
}

.empty-placeholder {
  padding: var(--sdl-space-10) 0;
  text-align: center;
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-body);
}

.pagination-card {
  flex-shrink: 0;
}

.pagination-wrapper {
  display: flex;
  justify-content: center;
  align-items: center;
}
</style>
