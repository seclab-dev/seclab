<script setup lang="ts">
/**
 * @file DockerImageList.vue
 * @description 当前节点 Docker 镜像列表，提供独立加载、刷新和安全删除状态。
 */

import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDockerStore } from '@/stores/docker'
import { useNodeStore } from '@/stores/node'
import { formatImageTags, formatBytes } from '@/utils/docker-format'
import {
  SecLabAlert,
  SecLabButton,
  SecLabEmpty,
  SecLabInput,
  SecLabLoading,
  SecLabTable,
  SecLabTag,
} from '@/components/ui'
import type { SecLabTableColumn } from '@/components/ui/SecLabTable.vue'
import type { DockerImageSummary } from '@/api/interface/docker'

const { t, locale } = useI18n()
const store = useDockerStore()
const nodeStore = useNodeStore()
const searchQuery = ref('')

const filteredImages = computed(() => {
  const query = searchQuery.value.trim().toLowerCase()
  if (!query) return store.imagesList
  return store.imagesList.filter((image) => {
    const shortId = image.id.replace(/^sha256:/, '').substring(0, 12)
    return (
      image.id.toLowerCase().includes(query) ||
      shortId.toLowerCase().includes(query) ||
      image.tags.some((tag) => tag.toLowerCase().includes(query)) ||
      image.digests.some((digest) => digest.toLowerCase().includes(query))
    )
  })
})

const columns = computed<SecLabTableColumn[]>(() => [
  { label: t('app.docker.images.columns.tags'), minWidth: 240, slot: 'tags' },
  { label: t('app.docker.images.columns.id'), width: 140, slot: 'id', align: 'center' },
  { label: t('app.docker.images.columns.size'), width: 120, slot: 'size', align: 'center' },
  {
    label: t('app.docker.images.columns.containers'),
    width: 100,
    align: 'center',
    slot: 'containers',
  },
  {
    label: t('app.docker.images.columns.createdAt'),
    minWidth: 180,
    slot: 'createdAt',
    align: 'center',
  },
  { label: t('app.docker.images.columns.actions'), width: 100, align: 'center', slot: 'actions' },
])

const formatCreatedAt = (createdAt: number) =>
  new Date(createdAt * 1000).toLocaleString(locale.value === 'zh' ? 'zh-CN' : 'en-US')

const shortImageId = (id: string) => id.replace(/^sha256:/, '').substring(0, 12)

watch(
  () => nodeStore.currentNodeId,
  () => {
    searchQuery.value = ''
    void store.fetchImagesList()
  },
)

onMounted(() => {
  void store.fetchImagesList()
})
</script>

<template>
  <div class="image-list-page" data-page="docker-image-list">
    <div class="image-toolbar" data-ui="toolbar">
      <SecLabInput
        id="docker-local-image-search"
        v-model="searchQuery"
        name="docker-local-image-search"
        :placeholder="t('app.docker.images.searchPlaceholder')"
        clearable
        class="search-input"
        data-ui="image-search-input"
      />
      <SecLabButton
        size="small"
        :loading="store.imageListLoading"
        data-ui="image-refresh-button"
        @click="store.fetchImagesList"
      >
        {{ t('common.refresh') }}
      </SecLabButton>
    </div>

    <SecLabAlert
      v-if="store.imageListError"
      type="warning"
      :title="t('app.docker.images.refreshFailed')"
      :description="store.imageListError"
      show-icon
      data-ui="image-list-error"
    />

    <div class="image-table-shell" data-ui="table">
      <SecLabTable v-if="filteredImages.length" :data="filteredImages" :columns="columns" border>
        <template #tags="{ row }: { row: DockerImageSummary }">
          <div class="resource-name-cell">
            <span :title="formatImageTags(row.tags)">{{ formatImageTags(row.tags) }}</span>
            <SecLabTag v-if="row.dangling" type="warning" size="small">
              {{ t('app.docker.images.dangling') }}
            </SecLabTag>
          </div>
        </template>
        <template #id="{ row }: { row: DockerImageSummary }">
          <span class="image-id" :title="row.id">{{ shortImageId(row.id) }}</span>
        </template>
        <template #size="{ row }: { row: DockerImageSummary }">
          {{ formatBytes(row.sizeBytes) }}
        </template>
        <template #containers="{ row }: { row: DockerImageSummary }">
          <SecLabTag type="info">{{ row.containerCount }}</SecLabTag>
        </template>
        <template #createdAt="{ row }: { row: DockerImageSummary }">
          {{ formatCreatedAt(row.createdAt) }}
        </template>
        <template #actions="{ row }: { row: DockerImageSummary }">
          <SecLabButton
            type="danger"
            size="small"
            :loading="store.imageDeleteLoadingId === row.id"
            :disabled="row.containerCount > 0 || Boolean(store.imageDeleteLoadingId)"
            :title="
              row.containerCount > 0
                ? t('app.docker.images.actions.inUse', { count: row.containerCount })
                : undefined
            "
            data-ui="image-delete-button"
            @click="store.handleDeleteImage(row.id)"
          >
            {{ t('app.docker.images.actions.delete') }}
          </SecLabButton>
        </template>
      </SecLabTable>
      <SecLabEmpty
        v-else-if="!store.imageListLoading"
        :description="
          searchQuery ? t('app.docker.images.filteredEmpty') : t('app.docker.images.emptyLocal')
        "
      />
      <SecLabLoading :loading="store.imageListLoading && !store.imagesList.length" cover />
    </div>
  </div>
</template>

<style scoped>
.image-list-page {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
  height: 100%;
  min-height: 0;
}

.image-toolbar {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-3);
}

.search-input {
  width: min(320px, 100%);
}

.image-table-shell {
  position: relative;
  flex: 1;
  min-height: 160px;
  overflow: auto;
}

.resource-name-cell {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: var(--sdl-space-2);
}

.resource-name-cell > span:first-child {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.image-id {
  color: var(--sdl-text-secondary);
  font-family: var(--sdl-font-mono);
}

@media (max-width: 640px) {
  .image-toolbar {
    align-items: stretch;
    flex-direction: column;
  }

  .search-input {
    width: 100%;
  }
}
</style>
