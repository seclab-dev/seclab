<script setup lang="ts">
/**
 * @file DockerImageDistribute.vue
 * @description 将主控已有镜像通过统一镜像任务分发到节点。
 */

import { computed, ref, onMounted, onUnmounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useNotificationStore } from '@/stores/notification'
import { nodesApi, type NodeSummaryResponse } from '@/api/modules/nodes'
import { dockerApi, type ImagePullProgress } from '@/api/modules/docker'
import type { ImageSummary } from '@/api/interface/docker'
import { SecLabButton, SecLabCheckbox, SecLabTag, SecLabEmpty, SecLabSelect } from '@/components/ui'

const { t } = useI18n()
const notificationStore = useNotificationStore()

const selectedLocalImage = ref<string | null>(null)
const controllerImages = ref<ImageSummary[]>([])

// ─── 节点选择与上传 ───
const nodeList = ref<NodeSummaryResponse[]>([])
const selectedNodeIds = ref<string[]>([])
const isSubmitting = ref(false)
const taskIds = ref<Record<string, string>>({})
const distributeProgress = ref<Record<string, ImagePullProgress>>({})

let timer: number | null = null

// ─── 镜像选项 ───
const localImageOptions = computed(() => {
  const options: { label: string; value: string }[] = []
  controllerImages.value.forEach((img) => {
    if (img.RepoTags && img.RepoTags.length > 0) {
      img.RepoTags.forEach((tag: string) => {
        options.push({ label: tag, value: tag })
      })
    } else if (img.Id) {
      const shortId = img.Id.substring(7, 19)
      options.push({ label: `<none> (${shortId})`, value: img.Id })
    }
  })
  return options
})

// ─── 节点获取与初始化 ───
const fetchNodes = async () => {
  try {
    const res = await nodesApi.list()
    nodeList.value = res.data || []
  } catch (e) {
    console.error('Failed to load nodes:', e)
  }
}

const fetchControllerImages = async () => {
  const response = await dockerApi.fetchLocalImages()
  controllerImages.value = response.data || []
}

// ─── 分发目标过滤 ───
const filteredNodeList = computed(() => {
  return nodeList.value.filter((node) => node.nodeId !== 'local')
})

// ─── 节点全选逻辑 ───
const isAllSelected = computed({
  get() {
    return (
      filteredNodeList.value.length > 0 &&
      selectedNodeIds.value.length === filteredNodeList.value.length
    )
  },
  set(val: boolean) {
    if (val) {
      selectedNodeIds.value = filteredNodeList.value.map((n) => n.nodeId)
    } else {
      selectedNodeIds.value = []
    }
  },
})

const toggleNodeSelect = (nodeId: string) => {
  const index = selectedNodeIds.value.indexOf(nodeId)
  if (index > -1) {
    selectedNodeIds.value.splice(index, 1)
  } else {
    selectedNodeIds.value.push(nodeId)
  }
}

// ─── 开始分发 ───
const startDistribute = async () => {
  if (!selectedLocalImage.value) return
  if (selectedNodeIds.value.length === 0) return

  isSubmitting.value = true
  taskIds.value = {}
  distributeProgress.value = {}

  selectedNodeIds.value.forEach((id) => {
    distributeProgress.value[id] = {
      taskId: '',
      nodeId: id,
      imageRef: selectedLocalImage.value!,
      progressPercent: 0,
      status: 'pending',
      stage: 'checking',
      statusText: '',
    }
  })

  try {
    for (const nodeId of selectedNodeIds.value) {
      const response = await dockerApi.startImageTask({
        nodeId,
        imageRef: selectedLocalImage.value,
        sourceMode: 'controller-first',
      })
      if (!response.success || !response.data?.taskId) throw new Error(response.message)
      taskIds.value[nodeId] = response.data.taskId
    }
    startPolling()
  } catch (e) {
    console.error('Distribute error:', e)
    const errMsg = e instanceof Error ? e.message : String(e)
    notificationStore.error(t('app.docker.images.distribute.actions.failed', { error: errMsg }))
    isSubmitting.value = false
  }
}

// ─── 状态轮询 ───
const startPolling = () => {
  if (timer) clearInterval(timer)

  void fetchDistributeProgress()
  timer = window.setInterval(fetchDistributeProgress, 1000)
}

const fetchDistributeProgress = async () => {
  try {
    for (const [nodeId, currentTaskId] of Object.entries(taskIds.value)) {
      const response = await dockerApi.fetchImageTaskProgress(currentTaskId)
      if (response.success && response.data) distributeProgress.value[nodeId] = response.data
    }
    const allFinished = Object.values(distributeProgress.value).every((item) =>
      ['success', 'failed', 'cancelled'].includes(item.status),
    )
    if (allFinished) {
      if (timer) clearInterval(timer)
      timer = null
      isSubmitting.value = false
    }
  } catch (e) {
    console.error('Polling status failed:', e)
  }
}

onMounted(() => {
  void fetchNodes()
  void fetchControllerImages()
})

onUnmounted(() => {
  if (timer) {
    clearInterval(timer)
  }
})

// 监听本地镜像的列表变化，自动选择第一个
watch(
  controllerImages,
  (next) => {
    if (next.length > 0 && !selectedLocalImage.value) {
      const firstImg = next.find((img) => img.RepoTags && img.RepoTags.length > 0)
      if (firstImg && firstImg.RepoTags && firstImg.RepoTags.length > 0) {
        selectedLocalImage.value = firstImg.RepoTags[0]
      } else if (next[0]?.Id) {
        selectedLocalImage.value = next[0].Id
      }
    }
  },
  { immediate: true },
)
</script>

<template>
  <div class="distribute-container" data-ui="docker-image-distribute">
    <!-- 左侧：文件上传与配置 -->
    <div class="distribute-panel-left">
      <div class="local-distribute-panel" data-ui="distribute-local-panel">
        <div class="panel-section">
          <label class="section-label">{{
            t('app.docker.images.distribute.selectLocal.title')
          }}</label>
          <SecLabSelect
            v-if="localImageOptions.length > 0"
            v-model="selectedLocalImage"
            :options="localImageOptions"
            :placeholder="t('app.docker.images.distribute.selectLocal.placeholder')"
            class="select-local"
            data-ui="distribute-select-image"
          />
          <div v-else class="select-empty-state" data-ui="distribute-select-image-empty">
            {{ t('app.docker.images.distribute.selectLocal.empty') }}
          </div>
        </div>
      </div>

      <div class="action-bar">
        <SecLabButton
          type="primary"
          :disabled="!selectedLocalImage || selectedNodeIds.length === 0 || isSubmitting"
          class="btn-start"
          @click="startDistribute"
          data-ui="distribute-submit-btn"
        >
          <span v-if="isSubmitting" class="spinner">⏳</span>
          <span>{{
            isSubmitting
              ? t('app.docker.images.distribute.actions.submitting')
              : t('app.docker.images.distribute.actions.submit')
          }}</span>
        </SecLabButton>
      </div>
    </div>

    <!-- 右侧：节点选择与状态追踪 -->
    <div class="distribute-panel-right">
      <div class="panel-header">
        <div class="panel-title">{{ t('app.docker.images.distribute.nodes.title') }}</div>
        <div class="select-all-wrapper">
          <SecLabCheckbox v-model="isAllSelected" data-ui="distribute-select-all-nodes">{{
            t('app.docker.images.distribute.nodes.selectAll')
          }}</SecLabCheckbox>
        </div>
      </div>

      <div class="node-list-wrapper">
        <div
          v-for="node in filteredNodeList"
          :key="node.nodeId"
          class="node-item"
          :class="{
            'is-checked': selectedNodeIds.includes(node.nodeId),
            'is-active-task': distributeProgress[node.nodeId],
          }"
          @click="toggleNodeSelect(node.nodeId)"
          data-ui="node-item"
        >
          <div class="node-checkbox-area" @click.stop>
            <SecLabCheckbox
              :model-value="selectedNodeIds.includes(node.nodeId)"
              @change="toggleNodeSelect(node.nodeId)"
            />
          </div>

          <div class="node-info-area">
            <div class="node-name-row">
              <span class="node-name">{{ node.name }}</span>
              <SecLabTag v-if="node.status === 'online'" type="success" size="small">{{
                t('app.docker.images.distribute.nodes.online')
              }}</SecLabTag>
              <SecLabTag v-else type="default" size="small">{{
                t('app.docker.images.distribute.nodes.offline')
              }}</SecLabTag>
            </div>
            <div class="node-addr">{{ node.address || '-' }}</div>
          </div>

          <!-- 进度跟踪部分 -->
          <div v-if="distributeProgress[node.nodeId]" class="node-status-area" @click.stop>
            <div class="status-tags-row">
              <SecLabTag v-if="distributeProgress[node.nodeId].status === 'pending'" type="default">
                {{ t('app.docker.images.distribute.nodes.waiting') }}
              </SecLabTag>
              <SecLabTag
                v-else-if="distributeProgress[node.nodeId].stage === 'exporting'"
                type="warning"
              >
                {{ t('app.docker.images.distribute.nodes.exporting') }}
              </SecLabTag>
              <SecLabTag
                v-else-if="distributeProgress[node.nodeId].stage === 'uploading'"
                type="primary"
              >
                {{
                  t('app.docker.images.distribute.nodes.uploading', {
                    percent: distributeProgress[node.nodeId].progressPercent,
                  })
                }}
              </SecLabTag>
              <SecLabTag
                v-else-if="distributeProgress[node.nodeId].stage === 'loading'"
                type="warning"
              >
                {{ t('app.docker.images.distribute.nodes.loading') }}
              </SecLabTag>
              <SecLabTag
                v-else-if="distributeProgress[node.nodeId].status === 'running'"
                type="primary"
                :title="distributeProgress[node.nodeId].statusText"
              >
                {{ distributeProgress[node.nodeId].statusText }}
              </SecLabTag>
              <SecLabTag
                v-else-if="distributeProgress[node.nodeId].status === 'success'"
                type="success"
              >
                {{ t('app.docker.images.distribute.nodes.success') }}
              </SecLabTag>
              <SecLabTag
                v-else-if="distributeProgress[node.nodeId].status === 'failed'"
                type="danger"
                :title="
                  distributeProgress[node.nodeId].registryError ||
                  distributeProgress[node.nodeId].controllerError
                "
              >
                {{ t('app.docker.images.distribute.nodes.failed') }}
              </SecLabTag>
            </div>

            <!-- 进度条 -->
            <div class="progress-bar-container">
              <div
                class="progress-bar-fill"
                :class="distributeProgress[node.nodeId].status"
                :style="{ width: `${distributeProgress[node.nodeId].progressPercent}%` }"
              ></div>
            </div>
          </div>
        </div>

        <SecLabEmpty
          v-if="filteredNodeList.length === 0"
          :description="t('app.docker.images.distribute.nodes.empty')"
        />
      </div>
    </div>
  </div>
</template>

<style scoped>
.distribute-container {
  display: flex;
  gap: var(--sdl-space-5);
  flex: 1;
  min-height: 0;
  height: 100%;
}

.distribute-panel-left {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-4);
  max-width: 50%;
}

.distribute-panel-right {
  flex: 1;
  display: flex;
  flex-direction: column;
  background-color: var(--sdl-bg-input);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  padding: var(--sdl-space-4);
  min-height: 0;
}

.local-distribute-panel {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
  background-color: rgba(255, 255, 255, 0.02);
  border: 1px dashed var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  padding: var(--sdl-space-4);
}

.panel-section {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-2);
}

.section-label {
  font-size: var(--sdl-font-body-xs);
  font-weight: 600;
  color: var(--sdl-text-secondary);
}

.select-empty-state {
  min-height: 38px;
  display: flex;
  align-items: center;
  padding: 0 var(--sdl-space-3);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-sm);
  background-color: rgba(255, 255, 255, 0.02);
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-body-sm);
}

.action-bar {
  margin-top: auto;
}

.btn-start {
  width: 100%;
  height: 44px;
  font-size: var(--sdl-font-body-sm);
  font-weight: 600;
}

.panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: var(--sdl-space-3);
  padding-bottom: var(--sdl-space-2);
  border-bottom: 1px solid var(--sdl-border-default);
}

.panel-title {
  font-size: var(--sdl-font-body-md);
  font-weight: 600;
  color: var(--sdl-text-primary);
}

.node-list-wrapper {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-2);
  padding-right: var(--sdl-space-1);
}

.node-item {
  display: flex;
  align-items: center;
  padding: var(--sdl-space-3) var(--sdl-space-4);
  background-color: rgba(255, 255, 255, 0.02);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  cursor: pointer;
  transition: all 0.2s ease;
  gap: var(--sdl-space-4);
}

.node-item:hover {
  background-color: rgba(255, 255, 255, 0.05);
  border-color: var(--sdl-border-hover);
}

.node-item.is-checked {
  border-color: rgba(var(--sdl-primary-rgb), 0.5);
  background-color: rgba(var(--sdl-primary-rgb), 0.03);
  box-shadow: 0 0 10px rgba(var(--sdl-primary-rgb), 0.05);
}

.node-checkbox-area {
  display: flex;
  align-items: center;
}

.node-info-area {
  flex: 1;
  min-width: 0;
}

.node-name-row {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
}

.node-name {
  font-weight: 500;
  color: var(--sdl-text-primary);
}

.node-addr {
  font-size: var(--sdl-font-body-xs);
  color: var(--sdl-text-muted);
  margin-top: var(--sdl-space-1);
}

.node-status-area {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: var(--sdl-space-2);
  width: 140px;
}

.status-tags-row {
  display: flex;
  gap: var(--sdl-space-2);
}

.progress-bar-container {
  width: 100%;
  height: 4px;
  background-color: rgba(255, 255, 255, 0.08);
  border-radius: 2px;
  overflow: hidden;
}

.progress-bar-fill {
  height: 100%;
  width: 0;
  transition: width 0.3s ease;
  border-radius: 2px;
}

.progress-bar-fill.exporting {
  background-color: var(--sdl-warning);
}

.progress-bar-fill.uploading {
  background-color: var(--sdl-primary);
  background-image: linear-gradient(
    45deg,
    rgba(255, 255, 255, 0.15) 25%,
    transparent 25%,
    transparent 50%,
    rgba(255, 255, 255, 0.15) 50%,
    rgba(255, 255, 255, 0.15) 75%,
    transparent 75%,
    transparent
  );
  background-size: 1rem 1rem;
  animation: progress-bar-stripes 1s linear infinite;
}

.progress-bar-fill.loading {
  background-color: var(--sdl-warning);
  background-image: linear-gradient(
    45deg,
    rgba(255, 255, 255, 0.15) 25%,
    transparent 25%,
    transparent 50%,
    rgba(255, 255, 255, 0.15) 50%,
    rgba(255, 255, 255, 0.15) 75%,
    transparent 75%,
    transparent
  );
  background-size: 1rem 1rem;
  animation: progress-bar-stripes 1s linear infinite;
}

.progress-bar-fill.success {
  background-color: var(--sdl-success);
}

.progress-bar-fill.failed {
  background-color: var(--sdl-danger);
}

@keyframes progress-bar-stripes {
  from {
    background-position: 1rem 0;
  }
  to {
    background-position: 0 0;
  }
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
