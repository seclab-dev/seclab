<script setup lang="ts">
import SecLabTable, { type SecLabTableColumn } from '@/components/ui/SecLabTable.vue'
import {
  SecLabButton,
  SecLabTag,
  SecLabInput,
  SecLabEmpty,
  SecLabFormItem,
  SecLabSelect,
  SecLabActionMenu,
} from '@/components/ui'
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDockerStore } from '@/stores/docker'
import { useNotificationStore } from '@/stores/notification'
import { isSuiteManagedResource } from './docker-suite-labels'

/**
 * @file DockerVolumes.vue
 * @description Docker 卷管理组件，支持列表展示、新建卷与删除卷，严格遵循 SDL 设计规范。
 */

const { t, locale } = useI18n()
const dockerStore = useDockerStore()
const notificationStore = useNotificationStore()

/** 搜索关键词 */
const searchQuery = ref('')

/** 新建卷相关状态 */
const isCreateActive = ref(false)
const isSubmitting = ref(false)
const volumeForm = ref({
  name: '',
  driver: 'local',
  labels: '',
})

const driverOptions = [{ label: 'local', value: 'local' }]

/**
 * 获取数据源
 */
const volumeSource = computed(() => dockerStore.volumes ?? [])

/**
 * 根据搜索关键词过滤卷列表
 */
const filteredVolumes = computed(() => {
  const query = searchQuery.value.trim().toLowerCase()
  if (!query) return volumeSource.value
  return volumeSource.value.filter((v) => {
    const name: string = v?.Name || ''
    return name.toLowerCase().includes(query)
  })
})

/** 卷表格列定义 */
const columns = computed<SecLabTableColumn[]>(() => [
  { label: t('app.docker.volumes.name'), minWidth: 220, slot: 'name' },
  { label: t('app.docker.volumes.driver'), width: 120, slot: 'driver' },
  { label: t('app.docker.volumes.mountpoint'), minWidth: 260, slot: 'mountpoint' },
  { label: t('app.docker.volumes.scope'), width: 100, align: 'center', slot: 'scope' },
  { label: t('app.docker.volumes.createdAt'), width: 200, slot: 'createdAt' },
  { label: t('app.docker.volumes.actions.menu'), width: 120, align: 'center', slot: 'actions' },
])

/**
 * 格式化卷的创建时间
 */
const formatCreatedAt = (createdAt: string | undefined): string => {
  if (!createdAt) return '-'
  const date = new Date(createdAt)
  if (isNaN(date.getTime())) return '-'
  return date.toLocaleString(locale.value === 'zh' ? 'zh-CN' : 'en-US')
}

/** 处理搜索输入 */
const handleSearchInput = (value: string | number | boolean | undefined) => {
  searchQuery.value = typeof value === 'string' ? value : value == null ? '' : String(value)
}

/** 取消创建 */
const cancelCreate = () => {
  isCreateActive.value = false
  volumeForm.value = {
    name: '',
    driver: 'local',
    labels: '',
  }
}

/** 提交创建 */
const submitCreate = async () => {
  if (!volumeForm.value.name.trim()) {
    notificationStore.error(t('app.docker.messages.volumeNameRequired'))
    return
  }
  isSubmitting.value = true
  try {
    await dockerStore.handleCreateVolume({
      name: volumeForm.value.name.trim(),
      driver: volumeForm.value.driver,
      labels: volumeForm.value.labels.trim() || undefined,
    })
    cancelCreate()
  } finally {
    isSubmitting.value = false
  }
}

/** 删除数据卷 */
const handleDelete = async (name: string) => {
  if (!name) return
  await dockerStore.handleDeleteVolume(name)
}
</script>

<template>
  <div class="docker-volumes" data-page="docker-volumes">
    <div class="docker-card">
      <!-- 顶部工具栏 -->
      <div v-if="!isCreateActive" class="volume-toolbar">
        <SecLabInput
          :model-value="searchQuery"
          :placeholder="t('app.docker.volumes.searchPlaceholder')"
          class="volume-search"
          @update:model-value="handleSearchInput"
        />
        <SecLabButton type="primary" class="btn-create" @click="isCreateActive = true">
          {{ t('app.docker.volumes.actions.create') }}
        </SecLabButton>
      </div>

      <!-- 新建卷面板 -->
      <div v-if="isCreateActive" class="volume-create-panel">
        <div class="create-panel-header">
          <h3>{{ t('app.docker.volumes.create.title') }}</h3>
        </div>

        <div class="create-panel-body sl-form">
          <SecLabFormItem :label="t('app.docker.volumes.create.name')" required>
            <SecLabInput
              v-model="volumeForm.name"
              :placeholder="t('app.docker.volumes.create.namePlaceholder')"
            />
          </SecLabFormItem>

          <SecLabFormItem :label="t('app.docker.volumes.create.driver')">
            <SecLabSelect v-model="volumeForm.driver" :options="driverOptions" />
          </SecLabFormItem>

          <SecLabFormItem :label="t('app.docker.volumes.create.labels')">
            <SecLabInput
              v-model="volumeForm.labels"
              type="textarea"
              :rows="4"
              placeholder="e.g. key=value"
            />
          </SecLabFormItem>
        </div>

        <div class="create-panel-footer">
          <SecLabButton @click="cancelCreate">
            {{ t('app.docker.volumes.actions.cancel') }}
          </SecLabButton>
          <SecLabButton type="primary" :loading="isSubmitting" @click="submitCreate">
            {{ t('app.docker.volumes.actions.submit') }}
          </SecLabButton>
        </div>
      </div>

      <!-- 卷列表 -->
      <div v-if="!isCreateActive" class="card-scroll-wrapper">
        <SecLabTable
          v-if="filteredVolumes.length > 0"
          :data="filteredVolumes"
          :columns="columns"
          border
        >
          <template #name="{ row }: { row: any }">
            <div class="resource-name-cell">
              <span class="volume-name">{{ row?.Name || '-' }}</span>
              <SecLabTag v-if="isSuiteManagedResource(row?.Labels)" type="primary" size="small">
                {{ t('app.docker.suiteManaged') }}
              </SecLabTag>
            </div>
          </template>

          <template #driver="{ row }: { row: any }">
            <SecLabTag type="info">{{ row?.Driver || '-' }}</SecLabTag>
          </template>

          <template #mountpoint="{ row }: { row: any }">
            <span class="mountpoint-text">{{ row?.Mountpoint || '-' }}</span>
          </template>

          <template #scope="{ row }: { row: any }">
            <SecLabTag type="info">{{ row?.Scope || '-' }}</SecLabTag>
          </template>

          <template #createdAt="{ row }: { row: any }">
            <span>{{ formatCreatedAt(row?.CreatedAt) }}</span>
          </template>

          <template #actions="{ row }: { row: any }">
            <SecLabActionMenu
              :label="t('app.docker.volumes.actions.menu')"
              :actions="[
                {
                  label: t('app.docker.volumes.actions.delete'),
                  handler: () => handleDelete(row?.Name),
                  class: 'btn-delete',
                },
              ]"
            />
          </template>

          <template #empty>
            <SecLabEmpty :description="t('app.docker.volumes.empty')" />
          </template>
        </SecLabTable>

        <SecLabEmpty v-else :description="t('app.docker.volumes.empty')" />
      </div>
    </div>
  </div>
</template>

<style scoped>
.docker-volumes {
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

.volume-toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: var(--sdl-space-4);
}

.volume-search {
  width: 280px;
}

.volume-create-panel {
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

.volume-name {
  font-family: var(--sdl-font-mono);
  font-size: var(--sdl-font-caption);
  color: var(--sdl-text-primary);
  word-break: break-all;
}

.mountpoint-text {
  display: inline-block;
  white-space: pre-wrap;
  word-break: break-all;
  font-family: var(--sdl-font-mono);
  font-size: var(--sdl-font-caption);
  color: var(--sdl-text-secondary);
}

.resource-name-cell {
  min-width: 0;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
</style>
