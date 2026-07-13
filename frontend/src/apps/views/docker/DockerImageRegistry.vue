<script setup lang="ts">
/**
 * @file DockerImageRegistry.vue
 * @description Docker 镜像仓库页面，负责搜索镜像、选择 tag 并拉取到当前节点。
 */

import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  dockerApi,
  type ImagePullProgress,
  type ImageSearchResult,
  type ImageTagInfo,
} from '@/api/modules/docker'
import { useDockerStore } from '@/stores/docker'
import { useNodeStore } from '@/stores/node'
import { useNotificationStore } from '@/stores/notification'
import {
  SecLabAlert,
  SecLabButton,
  SecLabDialog,
  SecLabInput,
  SecLabSelect,
  SecLabTag,
} from '@/components/ui'

const SEARCH_PAGE_SIZE = 20
const TAG_PAGE_SIZE = 10
const DEFAULT_TAG = 'latest'

const { t } = useI18n()
const dockerStore = useDockerStore()
const nodeStore = useNodeStore()
const notificationStore = useNotificationStore()

const keyword = ref('')
const searchResults = ref<ImageSearchResult[]>([])
const searchPage = ref(1)
const searchHasMore = ref(false)
const isSearching = ref(false)
const searchError = ref('')

const dialogVisible = ref(false)
const selectedImage = ref<ImageSearchResult | null>(null)
const tags = ref<ImageTagInfo[]>([])
const selectedTag = ref('')
const tagPage = ref(1)
const tagHasMore = ref(false)
const isLoadingTags = ref(false)
const tagError = ref('')

const pullProgress = ref<ImagePullProgress | null>(null)
const isStartingPull = ref(false)
const isCanceling = ref(false)
let pollTimer: number | null = null

const currentNodeName = computed(() => {
  const node = nodeStore.nodes.find((item) => item.id === nodeStore.currentNodeId)
  return node?.name || nodeStore.currentNodeId || 'local'
})

const canSearch = computed(() => keyword.value.trim().length > 0 && !isSearching.value)
const isPulling = computed(
  () => pullProgress.value?.status === 'pending' || pullProgress.value?.status === 'running',
)
const canPull = computed(
  () => Boolean(selectedImage.value) && !isPulling.value && !isStartingPull.value,
)
const pullActionDisabled = computed(() => (isPulling.value ? isCanceling.value : !canPull.value))
const pullStatusText = computed(() => {
  if (!pullProgress.value) return ''
  return (
    pullProgress.value.statusText ||
    t(`app.docker.images.registry.status.${pullProgress.value.status}`)
  )
})
const visiblePullProgress = computed(() => {
  if (!pullProgress.value || !['pending', 'running'].includes(pullProgress.value.status)) {
    return null
  }
  return pullProgress.value
})
const tagOptions = computed(() =>
  tags.value.map((tag) => ({
    label: tag.name,
    value: tag.name,
  })),
)

const formatCount = (value?: number) => {
  if (value == null) return '-'
  return new Intl.NumberFormat().format(value)
}

const stopPolling = () => {
  if (pollTimer !== null) {
    window.clearInterval(pollTimer)
    pollTimer = null
  }
}

const resetPullState = () => {
  stopPolling()
  pullProgress.value = null
  isStartingPull.value = false
  isCanceling.value = false
}

const resetDialogState = () => {
  resetPullState()
  selectedImage.value = null
  tags.value = []
  selectedTag.value = ''
  tagPage.value = 1
  tagHasMore.value = false
  isLoadingTags.value = false
  tagError.value = ''
  dialogVisible.value = false
}

const searchImages = async (append = false) => {
  const normalizedKeyword = keyword.value.trim()
  if (!normalizedKeyword || isSearching.value) return

  isSearching.value = true
  searchError.value = ''
  const nextPage = append ? searchPage.value + 1 : 1
  try {
    const response = await dockerApi.forNode(nodeStore.currentNodeId).searchImages({
      keyword: normalizedKeyword,
      page: nextPage,
      pageSize: SEARCH_PAGE_SIZE,
    })
    if (!response.success || !response.data) {
      searchError.value = response.message || t('common.unknownError')
      if (!append) searchResults.value = []
      return
    }
    searchResults.value = append
      ? [...searchResults.value, ...response.data.results]
      : response.data.results
    searchPage.value = response.data.page
    searchHasMore.value = response.data.hasMore
  } catch (error) {
    console.error('Failed to search images:', error)
    searchError.value = error instanceof Error ? error.message : String(error)
    if (!append) searchResults.value = []
  } finally {
    isSearching.value = false
  }
}

const openDownloadDialog = async (image: ImageSearchResult) => {
  resetPullState()
  selectedImage.value = image
  tags.value = []
  selectedTag.value = ''
  tagPage.value = 1
  tagHasMore.value = false
  tagError.value = ''
  dialogVisible.value = true
  await loadTags(false)
}

const closeDialog = () => {
  if (isPulling.value || isStartingPull.value || isCanceling.value) return
  resetDialogState()
}

const loadTags = async (append = false) => {
  if (!selectedImage.value || isLoadingTags.value) return
  if (append && !tagHasMore.value) return

  isLoadingTags.value = true
  tagError.value = ''
  const nextPage = append ? tagPage.value + 1 : 1
  try {
    const response = await dockerApi.forNode(nodeStore.currentNodeId).fetchImageTags({
      repository: selectedImage.value.repository,
      page: nextPage,
      pageSize: TAG_PAGE_SIZE,
    })
    if (!response.success || !response.data) {
      tagError.value = response.message || t('common.unknownError')
      if (!append) tags.value = []
      return
    }
    tags.value = append ? [...tags.value, ...response.data.tags] : response.data.tags
    tagPage.value = response.data.page
    tagHasMore.value = response.data.hasMore
  } catch (error) {
    console.error('Failed to load image tags:', error)
    tagError.value = error instanceof Error ? error.message : String(error)
    if (!append) tags.value = []
  } finally {
    isLoadingTags.value = false
  }
}

const fetchPullProgress = async (taskId: string) => {
  try {
    const response = await dockerApi.fetchImageTaskProgress(taskId)
    if (!response.success || !response.data) return

    pullProgress.value = response.data
    if (['success', 'failed', 'cancelled'].includes(response.data.status)) {
      stopPolling()
      isCanceling.value = false
      if (response.data.status === 'success') {
        notificationStore.success(t('app.docker.images.registry.success'))
        await dockerStore.fetchImagesList()
      } else if (response.data.status === 'failed') {
        notificationStore.error(
          t('app.docker.images.registry.failed', {
            error:
              response.data.registryError ||
              response.data.controllerError ||
              response.data.statusText,
          }),
        )
      } else if (response.data.status === 'cancelled') {
        notificationStore.success(t('app.docker.images.registry.cancelled'))
      }
    }
  } catch (error) {
    console.error('Failed to fetch image pull progress:', error)
    stopPolling()
    const errMsg = error instanceof Error ? error.message : String(error)
    pullProgress.value = {
      taskId,
      nodeId: nodeStore.currentNodeId,
      imageRef: `${selectedImage.value?.repository || ''}:${selectedTag.value}`,
      status: 'failed',
      stage: 'pulling',
      progressPercent: pullProgress.value?.progressPercent ?? 0,
      statusText: t('app.docker.images.registry.progressFailed'),
      registryError: errMsg,
    }
    isCanceling.value = false
    notificationStore.error(t('app.docker.images.registry.failed', { error: errMsg }))
  }
}

const startPolling = (taskId: string) => {
  stopPolling()
  void fetchPullProgress(taskId)
  pollTimer = window.setInterval(() => {
    void fetchPullProgress(taskId)
  }, 1000)
}

const pullImage = async () => {
  if (!selectedImage.value || isStartingPull.value) return

  isStartingPull.value = true
  pullProgress.value = null
  try {
    const response = await dockerApi.startImageTask({
      nodeId: nodeStore.currentNodeId,
      imageRef: `${selectedImage.value.repository}:${selectedTag.value || DEFAULT_TAG}`,
      sourceMode: 'controller-first',
    })
    if (response.success && response.data?.taskId) {
      startPolling(response.data.taskId)
      return
    }
    notificationStore.error(
      t('app.docker.images.registry.failed', {
        error: response.message || t('common.unknownError'),
      }),
    )
  } catch (error) {
    console.error('Failed to start image pull:', error)
    const errMsg = error instanceof Error ? error.message : String(error)
    notificationStore.error(t('app.docker.images.registry.failed', { error: errMsg }))
  } finally {
    isStartingPull.value = false
  }
}

const cancelPull = async () => {
  const taskId = pullProgress.value?.taskId
  if (!taskId || isCanceling.value) return

  isCanceling.value = true
  try {
    const response = await dockerApi.cancelImageTask(taskId)
    if (response.success && response.data) {
      pullProgress.value = response.data
      if (['success', 'failed', 'cancelled'].includes(response.data.status)) {
        isCanceling.value = false
      }
    } else {
      isCanceling.value = false
    }
  } catch (error) {
    console.error('Failed to cancel image pull:', error)
    const errMsg = error instanceof Error ? error.message : String(error)
    isCanceling.value = false
    notificationStore.error(t('app.docker.images.registry.cancelFailed', { error: errMsg }))
  }
}

watch(keyword, () => {
  searchResults.value = []
  searchError.value = ''
  searchPage.value = 1
  searchHasMore.value = false
})
watch(
  () => nodeStore.currentNodeId,
  () => {
    keyword.value = ''
    searchResults.value = []
    searchError.value = ''
    searchPage.value = 1
    searchHasMore.value = false
    resetDialogState()
  },
)

onBeforeUnmount(() => {
  stopPolling()
})
</script>

<template>
  <div class="registry-page" data-ui="docker-image-registry">
    <SecLabAlert
      type="info"
      show-icon
      :description="t('app.docker.images.registry.targetHint', { node: currentNodeName })"
      data-ui="registry-target-hint"
    />

    <div class="registry-form" data-ui="registry-search-form">
      <SecLabInput
        v-model="keyword"
        :placeholder="t('app.docker.images.registry.placeholder')"
        :disabled="isSearching"
        data-ui="registry-image-input"
        @keyup.enter="searchImages(false)"
      />
      <SecLabButton
        type="primary"
        :disabled="!canSearch"
        class="registry-action-button"
        data-ui="registry-search-button"
        @click="searchImages(false)"
      >
        {{
          isSearching
            ? t('app.docker.images.registry.searching')
            : t('app.docker.images.registry.search')
        }}
      </SecLabButton>
    </div>

    <SecLabAlert
      v-if="searchError"
      type="error"
      show-icon
      :description="t('app.docker.images.registry.searchFailed', { error: searchError })"
      data-ui="registry-search-error"
    />

    <section class="image-results" data-ui="registry-search-results">
      <div v-if="searchResults.length" class="image-list">
        <article
          v-for="image in searchResults"
          :key="image.repository"
          class="image-row"
          data-ui="registry-image-result"
        >
          <div class="image-main">
            <div class="image-title">
              <span>{{ image.displayName }}</span>
              <SecLabTag v-if="image.isOfficial" type="success">
                {{ t('app.docker.images.registry.official') }}
              </SecLabTag>
              <SecLabTag v-else-if="image.isAutomated" type="warning">
                {{ t('app.docker.images.registry.automated') }}
              </SecLabTag>
            </div>
            <p class="image-description">
              {{ image.description || t('app.docker.images.registry.noDescription') }}
            </p>
            <div class="image-meta">
              <span>{{
                t('app.docker.images.registry.stars', { count: formatCount(image.starCount) })
              }}</span>
              <span>{{
                t('app.docker.images.registry.pulls', { count: formatCount(image.pullCount) })
              }}</span>
            </div>
          </div>
          <SecLabButton
            type="primary"
            size="small"
            data-ui="registry-download-button"
            @click="openDownloadDialog(image)"
          >
            {{ t('app.docker.images.registry.download') }}
          </SecLabButton>
        </article>
      </div>
      <SecLabButton
        v-if="searchHasMore"
        :disabled="isSearching"
        class="load-more-button"
        data-ui="registry-load-more"
        @click="searchImages(true)"
      >
        {{
          isSearching
            ? t('app.docker.images.registry.loadingMore')
            : t('app.docker.images.registry.loadMore')
        }}
      </SecLabButton>
    </section>

    <SecLabDialog
      :visible="dialogVisible"
      :title="t('app.docker.images.registry.downloadTitle')"
      width="520px"
      :close-on-click-overlay="false"
      data-ui="registry-download-dialog"
      @close="closeDialog"
    >
      <div v-if="selectedImage" class="download-dialog-body" data-slot="body">
        <div class="download-field" data-ui="registry-download-image-name">
          <div class="download-field-label">{{ t('app.docker.images.registry.imageName') }}</div>
          <div class="download-field-value">{{ selectedImage.displayName }}</div>
        </div>
        <div class="download-field" data-ui="registry-tag-select">
          <div class="download-field-label">{{ t('app.docker.images.registry.version') }}</div>
          <div class="tag-select-row">
            <SecLabSelect
              v-model="selectedTag"
              :options="tagOptions"
              :disabled="isPulling || isLoadingTags"
              :placeholder="t('app.docker.images.registry.selectTag')"
              data-ui="registry-tag-select-control"
              @dropdown-reach-bottom="loadTags(true)"
            >
              <template v-if="isLoadingTags" #dropdown-footer>
                <div class="tag-loading-state" data-ui="registry-tag-loading">
                  <span class="tag-loading-dot" />
                  <span>{{ t('app.docker.images.registry.loadingTags') }}</span>
                </div>
              </template>
            </SecLabSelect>
          </div>
        </div>

        <SecLabAlert
          v-if="tagError"
          type="error"
          show-icon
          :description="t('app.docker.images.registry.tagsFailed', { error: tagError })"
          data-ui="registry-tags-error"
        />

        <div
          v-if="visiblePullProgress"
          class="inline-pull-progress"
          data-ui="registry-pull-progress"
        >
          <div class="progress-track">
            <div
              class="progress-fill"
              :class="visiblePullProgress.status"
              :style="{ width: `${visiblePullProgress.progressPercent}%` }"
            />
          </div>
          <div class="progress-inline-meta">
            <span>{{ visiblePullProgress.progressPercent }}%</span>
            <span>{{ pullStatusText }}</span>
          </div>
        </div>
      </div>

      <template #footer>
        <SecLabButton :disabled="isPulling || isStartingPull || isCanceling" @click="closeDialog">
          {{ t('common.cancel') }}
        </SecLabButton>
        <SecLabButton
          :type="isPulling ? 'danger' : 'primary'"
          :disabled="pullActionDisabled"
          data-ui="registry-pull-button"
          @click="isPulling ? cancelPull() : pullImage()"
        >
          {{
            isCanceling
              ? t('app.docker.images.registry.canceling')
              : isPulling
                ? t('app.docker.images.registry.cancel')
                : isStartingPull
                  ? t('app.docker.images.registry.pulling')
                  : t('app.docker.images.registry.pull')
          }}
        </SecLabButton>
      </template>
    </SecLabDialog>
  </div>
</template>

<style scoped>
.registry-page {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-4);
  height: 100%;
  min-height: 0;
  padding: var(--sdl-space-1);
}

.registry-form {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-3);
  max-width: 760px;
}

.registry-form :deep(.sdl-input-wrapper) {
  flex: 1;
}

.registry-action-button {
  min-width: 108px;
  height: 38px;
  flex-shrink: 0;
}

.image-results {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
  max-width: 860px;
  min-height: 0;
}

.image-list {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-2);
  min-height: 0;
  overflow: auto;
}

.image-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sdl-space-4);
  padding: var(--sdl-space-3);
  border: 1px solid var(--sdl-color-border);
  border-radius: 8px;
  background: var(--sdl-color-bg-container);
}

.image-main {
  min-width: 0;
}

.image-title {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
  color: var(--sdl-color-text);
  font-size: 14px;
  font-weight: 600;
}

.image-description {
  margin: var(--sdl-space-1) 0;
  overflow: hidden;
  color: var(--sdl-color-text-secondary);
  font-size: 12px;
  line-height: 1.5;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.image-meta {
  display: flex;
  gap: var(--sdl-space-3);
  color: var(--sdl-color-text-secondary);
  font-size: 12px;
}

.load-more-button {
  align-self: center;
}

.download-dialog-body {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
}

.download-field {
  display: grid;
  grid-template-columns: 72px minmax(0, 1fr);
  align-items: center;
  gap: var(--sdl-space-3);
}

.download-field-label {
  color: var(--sdl-color-text-secondary);
  font-size: 13px;
  text-align: right;
}

.download-field-value {
  min-width: 0;
  overflow: hidden;
  padding: var(--sdl-space-2) var(--sdl-space-3);
  color: var(--sdl-color-text);
  font-size: 14px;
  font-weight: 600;
  border: 1px solid var(--sdl-color-border);
  border-radius: 6px;
  background: var(--sdl-color-fill-tertiary);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tag-select-row {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
  min-width: 0;
}

.tag-select-row :deep(.sdl-select) {
  flex: 1;
  min-width: 0;
}

.tag-loading-state {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--sdl-space-2);
}

.tag-loading-dot {
  width: 8px;
  height: 8px;
  border-radius: 999px;
  background: var(--sdl-color-primary);
  animation: tag-loading-pulse 0.9s ease-in-out infinite;
}

@keyframes tag-loading-pulse {
  0%,
  100% {
    opacity: 0.35;
    transform: scale(0.85);
  }

  50% {
    opacity: 1;
    transform: scale(1);
  }
}

.inline-pull-progress {
  display: grid;
  grid-template-columns: minmax(160px, 1fr) auto;
  align-items: center;
  gap: var(--sdl-space-3);
}

.progress-inline-meta {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sdl-space-3);
  min-width: 180px;
  color: var(--sdl-color-text-secondary);
  font-size: 12px;
}

.progress-track {
  width: 100%;
  height: 8px;
  overflow: hidden;
  border-radius: 999px;
  background: var(--sdl-color-fill-secondary);
}

.progress-fill {
  height: 100%;
  min-width: 2px;
  border-radius: inherit;
  background: var(--sdl-color-primary);
  transition: width 0.2s ease;
}

.progress-fill.success {
  background: var(--sdl-color-success);
}

.progress-fill.failed {
  background: var(--sdl-color-danger);
}

.progress-fill.cancelled {
  background: var(--sdl-color-warning);
}

@media (max-width: 720px) {
  .registry-form,
  .image-row,
  .progress-inline-meta {
    align-items: stretch;
    flex-direction: column;
  }

  .registry-action-button,
  .image-row :deep(.sdl-button) {
    width: 100%;
  }

  .download-field {
    grid-template-columns: 1fr;
  }

  .download-field-label {
    text-align: left;
  }

  .tag-select-row {
    align-items: stretch;
    flex-direction: column;
  }

  .inline-pull-progress {
    grid-template-columns: 1fr;
  }
}
</style>
