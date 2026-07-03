<script setup lang="ts">
/**
 * @file DockerImageDistribute.vue
 * @description Docker 镜像分发组件，支持通过拉取/选择本地镜像或上传离线 Tar 镜像文件，分发到各集群节点，支持分发进度跟踪。
 */

import { computed, ref, onMounted, onUnmounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDockerStore } from '@/stores/docker'
import { useNodeStore } from '@/stores/node'
import { useNotificationStore } from '@/stores/notification'
import { nodesApi, type NodeSummaryResponse } from '@/api/modules/nodes'
import { dockerApi, type DistributeNodeStatus } from '@/api/modules/docker'
import { formatBytes } from '@/utils/docker-format'
import { SecLabButton, SecLabCheckbox, SecLabTag, SecLabEmpty, SecLabSelect } from '@/components/ui'

const { t } = useI18n()
const store = useDockerStore()
const nodeStore = useNodeStore()
const notificationStore = useNotificationStore()

// ─── 分发模式选择与当前节点镜像 ───
const distMode = ref<'file' | 'local'>('local')
const selectedLocalImage = ref<string | null>(null)

// ─── 节点选择与上传 ───
const nodeList = ref<NodeSummaryResponse[]>([])
const selectedNodeIds = ref<string[]>([])
const uploadFile = ref<File | null>(null)
const isDragging = ref(false)
const isSubmitting = ref(false)
const taskId = ref<string | null>(null)
const distributeProgress = ref<Record<string, DistributeNodeStatus>>({})

let timer: number | null = null
const sourceNodeId = computed(() => nodeStore.currentNodeId || 'local')

// ─── 镜像选项 ───
const localImageOptions = computed(() => {
  const options: { label: string; value: string }[] = []
  store.imagesList.forEach((img) => {
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

// ─── 分发目标过滤 ───
const filteredNodeList = computed(() => {
  if (distMode.value === 'local') {
    return nodeList.value.filter((n) => n.nodeId !== sourceNodeId.value)
  }
  return nodeList.value
})

watch(distMode, (newMode) => {
  selectedNodeIds.value = []
  if (newMode === 'local') {
    void store.fetchImagesList()
  }
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

// ─── 文件拖拽与选择 ───
const handleFileChange = (e: Event) => {
  const target = e.target as HTMLInputElement
  if (target.files && target.files.length > 0) {
    uploadFile.value = target.files[0]
  }
}

const handleDragOver = (e: DragEvent) => {
  e.preventDefault()
  isDragging.value = true
}

const handleDragLeave = () => {
  isDragging.value = false
}

const handleDrop = (e: DragEvent) => {
  e.preventDefault()
  isDragging.value = false
  if (e.dataTransfer?.files && e.dataTransfer.files.length > 0) {
    uploadFile.value = e.dataTransfer.files[0]
  }
}

const triggerSelectFile = () => {
  const input = document.createElement('input')
  input.type = 'file'
  input.accept = [
    '.tar',
    '.tar.gz',
    '.tgz',
    '.tar.bz2',
    '.tbz',
    '.tbz2',
    '.tar.xz',
    '.txz',
    '.tar.zst',
    '.tar.zstd',
    'application/x-tar',
    'application/gzip',
    'application/x-bzip2',
    'application/x-xz',
    'application/zstd',
  ].join(',')
  input.onchange = handleFileChange
  input.click()
}

const clearFile = () => {
  uploadFile.value = null
}

// ─── 开始分发 ───
const startDistribute = async () => {
  if (distMode.value === 'file') {
    if (!uploadFile.value) return
  } else {
    if (!selectedLocalImage.value) return
  }
  if (selectedNodeIds.value.length === 0) return

  isSubmitting.value = true
  taskId.value = null
  distributeProgress.value = {}

  selectedNodeIds.value.forEach((id) => {
    distributeProgress.value[id] = {
      progressPercent: 0,
      status: 'waiting',
    }
  })

  try {
    let resData = ''
    if (distMode.value === 'file') {
      const formData = new FormData()
      formData.append('file', uploadFile.value!)
      formData.append('nodeIds', JSON.stringify(selectedNodeIds.value))
      const res = await dockerApi.distributeImage(formData)
      resData = res.data ?? ''
    } else {
      const res = await dockerApi.distributeLocalImage({
        imageName: String(selectedLocalImage.value),
        nodeIds: selectedNodeIds.value,
        sourceNodeId: sourceNodeId.value,
      })
      resData = res.data ?? ''
    }

    if (resData) {
      taskId.value = resData
      startPolling()
    } else {
      throw new Error('未获取到任务ID')
    }
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
  if (!taskId.value) return

  try {
    const res = await dockerApi.fetchDistributeStatus(taskId.value)
    if (res && res.success && res.data && res.data.nodeStatuses) {
      distributeProgress.value = res.data.nodeStatuses

      const allFinished = Object.values(res.data.nodeStatuses).every(
        (node) => node.status === 'success' || node.status === 'failed',
      )

      if (allFinished) {
        if (timer) {
          clearInterval(timer)
          timer = null
        }
        isSubmitting.value = false
      }
    }
  } catch (e) {
    console.error('Polling status failed:', e)
  }
}

onMounted(() => {
  void fetchNodes()
  void store.fetchImagesList()
})

onUnmounted(() => {
  if (timer) {
    clearInterval(timer)
  }
})

// 监听本地镜像的列表变化，自动选择第一个
watch(
  () => store.imagesList,
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

watch(sourceNodeId, () => {
  selectedNodeIds.value = []
  selectedLocalImage.value = null
})
</script>

<template>
  <div class="distribute-container" data-ui="docker-image-distribute">
    <!-- 左侧：文件上传与配置 -->
    <div class="distribute-panel-left">
      <!-- 分发模式选择 -->
      <div class="distribute-mode-tabs" data-ui="distribute-mode-tabs">
        <button
          class="mode-tab-btn"
          :class="{ 'is-active': distMode === 'local' }"
          @click="distMode = 'local'"
        >
          {{ t('app.docker.images.distribute.modes.local') }}
        </button>
        <button
          class="mode-tab-btn"
          :class="{ 'is-active': distMode === 'file' }"
          @click="distMode = 'file'"
        >
          {{ t('app.docker.images.distribute.modes.file') }}
        </button>
      </div>

      <!-- 离线文件模式 -->
      <div
        v-if="distMode === 'file'"
        class="upload-dropzone"
        :class="{ 'is-dragging': isDragging, 'has-file': uploadFile }"
        @dragover="handleDragOver"
        @dragleave="handleDragLeave"
        @drop="handleDrop"
        @click="triggerSelectFile"
        data-ui="distribute-upload-dropzone"
      >
        <div v-if="!uploadFile" class="upload-placeholder">
          <div class="upload-icon">📤</div>
          <div class="upload-title">
            {{ t('app.docker.images.distribute.upload.placeholder') }}
          </div>
        </div>
        <div v-else class="upload-file-info" @click.stop>
          <div class="file-icon">📦</div>
          <div class="file-details">
            <div class="file-name" :title="uploadFile.name">{{ uploadFile.name }}</div>
            <div class="file-size">{{ formatBytes(uploadFile.size) }}</div>
          </div>
          <SecLabButton
            type="danger"
            size="small"
            @click="clearFile"
            data-ui="distribute-clear-file"
          >
            {{ t('app.docker.images.distribute.upload.clear') }}
          </SecLabButton>
        </div>
      </div>

      <!-- 本地镜像分发模式 -->
      <div v-else class="local-distribute-panel" data-ui="distribute-local-panel">
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
          :disabled="
            (distMode === 'file' ? !uploadFile : !selectedLocalImage) ||
            selectedNodeIds.length === 0 ||
            isSubmitting
          "
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
              <SecLabTag v-if="distributeProgress[node.nodeId].status === 'waiting'" type="default">
                {{ t('app.docker.images.distribute.nodes.waiting') }}
              </SecLabTag>
              <SecLabTag
                v-else-if="distributeProgress[node.nodeId].status === 'exporting'"
                type="warning"
              >
                {{ t('app.docker.images.distribute.nodes.exporting') }}
              </SecLabTag>
              <SecLabTag
                v-else-if="distributeProgress[node.nodeId].status === 'uploading'"
                type="primary"
              >
                {{
                  t('app.docker.images.distribute.nodes.uploading', {
                    percent: distributeProgress[node.nodeId].progressPercent,
                  })
                }}
              </SecLabTag>
              <SecLabTag
                v-else-if="distributeProgress[node.nodeId].status === 'loading'"
                type="warning"
              >
                {{ t('app.docker.images.distribute.nodes.loading') }}
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
                :title="distributeProgress[node.nodeId].error"
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

.distribute-mode-tabs {
  display: flex;
  background-color: rgba(255, 255, 255, 0.04);
  border-radius: var(--sdl-radius-md);
  padding: 4px;
  border: 1px solid var(--sdl-border-default);
}

.mode-tab-btn {
  flex: 1;
  border: none;
  background: transparent;
  color: var(--sdl-text-secondary);
  padding: var(--sdl-space-2) var(--sdl-space-3);
  font-size: var(--sdl-font-body-sm);
  font-weight: 500;
  border-radius: var(--sdl-radius-sm);
  cursor: pointer;
  transition: all 0.2s;
}

.mode-tab-btn:hover {
  color: var(--sdl-text-primary);
  background-color: rgba(255, 255, 255, 0.02);
}

.mode-tab-btn.is-active {
  color: var(--sdl-primary);
  background-color: rgba(var(--sdl-primary-rgb), 0.08);
  font-weight: 600;
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

.upload-dropzone {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 2px dashed var(--sdl-border-default);
  border-radius: var(--sdl-radius-lg);
  cursor: pointer;
  background-color: rgba(255, 255, 255, 0.02);
  transition: all 0.3s cubic-bezier(0.25, 0.8, 0.25, 1);
  padding: var(--sdl-space-6);
  text-align: center;
}

.upload-dropzone:hover {
  border-color: var(--sdl-primary);
  background-color: rgba(var(--sdl-primary-rgb), 0.04);
}

.upload-dropzone.is-dragging {
  border-color: var(--sdl-primary);
  background-color: rgba(var(--sdl-primary-rgb), 0.08);
  transform: scale(0.995);
}

.upload-dropzone.has-file {
  border-style: solid;
  border-color: var(--sdl-border-default);
  background-color: rgba(255, 255, 255, 0.04);
  cursor: default;
}

.upload-placeholder {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--sdl-space-3);
}

.upload-icon {
  font-size: 40px;
  filter: drop-shadow(0 2px 8px rgba(var(--sdl-primary-rgb), 0.2));
}

.upload-title {
  font-size: var(--sdl-font-body-sm);
  font-weight: 500;
  color: var(--sdl-text-primary);
}

.upload-hint {
  font-size: var(--sdl-font-body-xs);
  color: var(--sdl-text-muted);
}

.upload-file-info {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-4);
  width: 100%;
  padding: var(--sdl-space-4);
  border-radius: var(--sdl-radius-md);
  background-color: rgba(0, 0, 0, 0.2);
}

.file-icon {
  font-size: 36px;
}

.file-details {
  flex: 1;
  text-align: left;
  min-width: 0;
}

.file-name {
  font-size: var(--sdl-font-body-sm);
  font-weight: 600;
  color: var(--sdl-text-primary);
  word-break: break-all;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.file-size {
  font-size: var(--sdl-font-body-xs);
  color: var(--sdl-text-muted);
  margin-top: var(--sdl-space-1);
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
