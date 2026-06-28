<script setup lang="ts">
/**
 * @file DockerImageList.vue
 * @description Docker 本地镜像列表组件，支持搜索、删除镜像及拉取新镜像，符合 SDL 设计规范。
 */

import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDockerStore } from '@/stores/docker'
import { useNotificationStore } from '@/stores/notification'
import { dockerApi } from '@/api/modules/docker'
import { formatImageTags, formatBytes } from '@/utils/docker-format'
import { SecLabTable, SecLabTag, SecLabButton, SecLabEmpty, SecLabInput } from '@/components/ui'
import { isSuiteManagedResource } from './docker-suite-labels'
import type { SecLabTableColumn } from '@/components/ui/SecLabTable.vue'
import type * as dockerType from '@/api/interface/docker'

const { t, locale } = useI18n()
const store = useDockerStore()
const notificationStore = useNotificationStore()

// ─── 搜索过滤 ───
const searchQuery = ref('')

const filteredImages = computed(() => {
  const query = searchQuery.value.trim().toLowerCase()
  if (!query) return store.imagesList
  return store.imagesList.filter((img) => {
    const id = img.Id?.toLowerCase() || ''
    const shortId = img.Id?.substring(7, 19).toLowerCase() || ''
    const tags = img.RepoTags?.join(', ').toLowerCase() || ''
    return id.includes(query) || shortId.includes(query) || tags.includes(query)
  })
})

// ─── 镜像拉取 ───
const newImageToPull = ref('')
const isPulling = ref(false)

const pullImage = async () => {
  const imageName = newImageToPull.value.trim()
  if (!imageName) return
  isPulling.value = true
  try {
    const res = await dockerApi.pullLocalImage(imageName)
    if (res.success) {
      notificationStore.success(t('app.docker.images.distribute.pull.success'))
      newImageToPull.value = ''
      await store.fetchImagesList()
    } else {
      notificationStore.error(
        t('app.docker.images.distribute.pull.failed', {
          error: res.message || t('common.unknownError'),
        }),
      )
    }
  } catch (e) {
    console.error('Failed to pull image:', e)
    const errMsg = e instanceof Error ? e.message : String(e)
    notificationStore.error(t('app.docker.images.distribute.pull.failed', { error: errMsg }))
  } finally {
    isPulling.value = false
  }
}

// ─── 本地镜像表格列定义 ───
const columns = computed<SecLabTableColumn[]>(() => [
  { label: t('app.docker.images.columns.tags'), minWidth: 220, slot: 'tags' },
  { label: t('app.docker.images.columns.id'), width: 140, slot: 'id' },
  { label: t('app.docker.images.columns.size'), width: 110, slot: 'size' },
  { label: t('app.docker.images.columns.virtualSize'), width: 120, slot: 'virtualSize' },
  {
    label: t('app.docker.images.columns.containers'),
    width: 90,
    align: 'center',
    slot: 'containers',
  },
  { label: t('app.docker.images.columns.createdAt'), minWidth: 180, slot: 'createdAt' },
  { label: t('app.docker.images.columns.actions'), width: 100, align: 'center', slot: 'actions' },
])

// ─── 格式化工具 ───
const formatCreatedAt = (created: number | undefined) => {
  if (!created) return '-'
  return new Date(created * 1000).toLocaleString(locale.value === 'zh' ? 'zh-CN' : 'en-US')
}

const formatImageShortId = (id: string | undefined) => {
  if (!id) return '-'
  return id.substring(7, 19)
}

// ─── 删除镜像 ───
const handleDelete = async (row: dockerType.ImageSummary) => {
  if (row.Id) {
    await store.handleDeleteImage({ id: row.Id, containers: row.Containers ?? 0 })
  }
}
</script>

<template>
  <div class="image-list-wrapper" data-ui="docker-image-list">
    <!-- 操作栏（搜索与拉取） -->
    <div class="card-actions">
      <div class="search-wrapper">
        <SecLabInput
          v-model="searchQuery"
          :placeholder="t('common.search')"
          clearable
          data-ui="image-search-input"
          class="search-input"
        />
      </div>

      <div class="pull-wrapper">
        <SecLabInput
          v-model="newImageToPull"
          :placeholder="t('app.docker.images.distribute.pull.placeholder')"
          :disabled="isPulling"
          class="input-pull"
          data-ui="image-pull-input"
        />
        <SecLabButton
          type="primary"
          :disabled="!newImageToPull || isPulling"
          @click="pullImage"
          data-ui="image-pull-btn"
        >
          <span v-if="isPulling" class="spinner">⏳</span>
          <span>{{
            isPulling
              ? t('app.docker.images.distribute.pull.btnPulling')
              : t('app.docker.images.distribute.pull.btnPull')
          }}</span>
        </SecLabButton>
      </div>
    </div>

    <!-- 表格 -->
    <SecLabTable v-if="filteredImages.length > 0" :data="filteredImages" :columns="columns" border>
      <!-- 镜像 Tags -->
      <template #tags="{ row }: { row: dockerType.ImageSummary }">
        <div class="resource-name-cell">
          <span>{{ formatImageTags(row?.RepoTags) }}</span>
          <SecLabTag v-if="isSuiteManagedResource(row?.Labels)" type="primary" size="small">
            {{ t('app.docker.suiteManaged') }}
          </SecLabTag>
        </div>
      </template>

      <!-- 镜像 ID -->
      <template #id="{ row }: { row: dockerType.ImageSummary }">
        <span class="sl-text-secondary" :title="row?.Id">
          {{ formatImageShortId(row?.Id) }}
        </span>
      </template>

      <!-- 镜像 大小 -->
      <template #size="{ row }: { row: dockerType.ImageSummary }">
        {{ formatBytes(row?.Size) }}
      </template>

      <!-- 镜像 虚拟大小 -->
      <template #virtualSize="{ row }: { row: dockerType.ImageSummary }">
        {{ formatBytes(row?.VirtualSize) }}
      </template>

      <!-- 被容器引用数 -->
      <template #containers="{ row }: { row: dockerType.ImageSummary }">
        <SecLabTag type="info">{{ row?.Containers ?? 0 }}</SecLabTag>
      </template>

      <!-- 创建时间 -->
      <template #created="{ row }: { row: dockerType.ImageSummary }">
        {{ formatCreatedAt(row?.Created) }}
      </template>

      <!-- 操作 -->
      <template #actions="{ row }: { row: dockerType.ImageSummary }">
        <SecLabButton
          type="danger"
          size="small"
          @click="handleDelete(row)"
          data-ui="image-delete-btn"
        >
          {{ t('app.docker.images.actions.delete') }}
        </SecLabButton>
      </template>

      <template #empty>
        <SecLabEmpty :description="t('app.docker.images.emptyLocal')" />
      </template>
    </SecLabTable>

    <SecLabEmpty v-else :description="t('app.docker.images.emptyLocal')" />
  </div>
</template>

<style scoped>
.image-list-wrapper {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.card-actions {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  margin-bottom: var(--sdl-space-4);
  gap: var(--sdl-space-4);
}

.search-wrapper {
  margin-right: auto;
  width: 280px;
}

.pull-wrapper {
  display: flex;
  gap: var(--sdl-space-2);
  width: 420px;
}

.input-pull {
  flex: 1;
}

.resource-name-cell {
  min-width: 0;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.spinner {
  display: inline-block;
  animation: spin 1s linear infinite;
  margin-right: 8px;
}

@keyframes spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}
</style>
