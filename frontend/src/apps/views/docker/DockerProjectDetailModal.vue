<script setup lang="ts">
import { ref, computed, onMounted, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDockerStore } from '@/stores/docker'
import { getStateIcon, formatPorts, getContainerIP } from '@/utils/docker-format'
import SecLabTable, { type SecLabTableColumn } from '@/components/ui/SecLabTable.vue'
import { SecLabButton, SecLabTag, SecLabEmpty, SecLabLoading } from '@/components/ui'
import SecLabIcon from '@/components/icons/SecLabIcon.vue'

/**
 * @file DockerProjectDetailModal.vue
 * @description Docker Compose 项目详情弹窗，包含服务列表与聚合日志，严格遵循 SDL 风格。
 */

const props = defineProps<{
  projectName: string
}>()

const emit = defineEmits<{
  (e: 'close'): void
}>()

const { t } = useI18n()
const store = useDockerStore()

const activeTab = ref<'services' | 'logs'>('services')
const isLogsLoading = ref(false)
const isDetailLoading = ref(false)
const logsViewerRef = ref<HTMLDivElement | null>(null)

const projectContainers = computed(() => {
  return store.composeContainers
    .filter((c) => c.Labels?.['com.docker.compose.project'] === props.projectName)
    .map((c) => {
      const serviceName = c.Labels?.['com.docker.compose.service'] || '-'
      const rawName = c.Names?.[0] || ''
      const containerName = rawName.replace(/^\//, '') || c.Id?.substring(0, 12) || ''
      const ip = getContainerIP(c.NetworkSettings)
      const ports = formatPorts(c.Ports)
      return {
        id: c.Id!,
        serviceName,
        containerName,
        state: c.State || 'unknown',
        ip,
        ports,
      }
    })
})

const columns = computed<SecLabTableColumn[]>(() => [
  { label: t('app.docker.projects.detail.serviceName'), minWidth: 120, slot: 'serviceName' },
  { label: t('app.docker.projects.detail.containerName'), minWidth: 150, slot: 'containerName' },
  { label: t('app.docker.projects.detail.status'), width: 120, align: 'center', slot: 'status' },
  { label: t('app.docker.projects.detail.ip'), width: 130, slot: 'ip' },
  { label: t('app.docker.projects.detail.ports'), minWidth: 160, slot: 'ports' },
  { label: t('app.docker.projects.detail.actions'), width: 150, align: 'center', slot: 'actions' },
])

const resolveStatusType = (
  status: string | undefined,
): 'success' | 'warning' | 'danger' | 'info' | 'primary' => {
  const normalized = (status || '').toLowerCase()
  if (normalized === 'running') return 'success'
  if (normalized === 'exited' || normalized === 'stopped') return 'danger'
  if (normalized === 'paused') return 'warning'
  if (normalized === 'created') return 'info'
  return 'primary'
}

const handleContainerOp = async (
  id: string,
  name: string,
  action: 'start' | 'stop' | 'restart',
) => {
  await store.handleContainerAction(id, name, action)
  // 重新拉取服务列表以更新状态
  await store.fetchComposeContainers()
}

const handleScale = async (serviceName: string) => {
  const res = window.prompt(
    t('app.docker.projects.detail.scalePrompt', { service: serviceName }),
    '1',
  )
  if (res === null) return
  const replicas = parseInt(res.trim(), 10)
  if (isNaN(replicas) || replicas < 0) {
    alert(t('app.docker.projects.detail.invalidReplicas'))
    return
  }
  isDetailLoading.value = true
  try {
    await store.handleScaleComposeProject(props.projectName, serviceName, replicas)
    await store.fetchComposeContainers()
  } finally {
    isDetailLoading.value = false
  }
}

const loadLogs = async () => {
  isLogsLoading.value = true
  try {
    await store.fetchProjectLogs(props.projectName)
    await nextTick()
    if (logsViewerRef.value) {
      logsViewerRef.value.scrollTop = logsViewerRef.value.scrollHeight
    }
  } finally {
    isLogsLoading.value = false
  }
}

const switchTab = (tab: 'services' | 'logs') => {
  activeTab.value = tab
  if (tab === 'logs') {
    void loadLogs()
  }
}

onMounted(async () => {
  isDetailLoading.value = true
  try {
    await store.fetchComposeContainers()
  } finally {
    isDetailLoading.value = false
  }
})
</script>

<template>
  <div class="modal-overlay" data-ui="project-detail-modal">
    <div class="modal">
      <div class="modal-header">
        <h3>{{ t('app.docker.projects.detail.title', { name: props.projectName }) }}</h3>
        <button class="modal-close" @click="emit('close')">×</button>
      </div>
      <div class="modal-body">
        <SecLabLoading :loading="isDetailLoading" cover />
        <div class="tabs-header">
          <div
            :class="['tab-item', { 'tab-item-active': activeTab === 'services' }]"
            @click="switchTab('services')"
          >
            {{ t('app.docker.projects.detail.tabServices') }}
          </div>
          <div
            :class="['tab-item', { 'tab-item-active': activeTab === 'logs' }]"
            @click="switchTab('logs')"
          >
            {{ t('app.docker.projects.detail.tabLogs') }}
          </div>
        </div>

        <div class="tab-content">
          <!-- 服务列表 -->
          <div v-if="activeTab === 'services'" class="services-tab-content">
            <SecLabTable
              v-if="projectContainers.length > 0"
              :data="projectContainers"
              :columns="columns"
              border
              size="small"
              class="services-table"
            >
              <template #serviceName="{ row }">
                <span class="service-name-text">{{ row.serviceName }}</span>
              </template>
              <template #containerName="{ row }">
                <span class="container-name-text">{{ row.containerName }}</span>
              </template>
              <template #status="{ row }">
                <SecLabTag :type="resolveStatusType(row.state)">
                  <SecLabIcon :name="getStateIcon(row.state)" :size="12" />
                  <span class="status-text">{{ row.state }}</span>
                </SecLabTag>
              </template>
              <template #ip="{ row }">
                <span class="mono-text">{{ row.ip }}</span>
              </template>
              <template #ports="{ row }">
                <span class="mono-text ports-text">{{ row.ports }}</span>
              </template>
              <template #actions="{ row }">
                <div class="row-actions">
                  <SecLabButton
                    v-if="row.state !== 'running'"
                    size="small"
                    type="primary"
                    @click="handleContainerOp(row.id, row.containerName, 'start')"
                  >
                    {{ t('common.start') }}
                  </SecLabButton>
                  <SecLabButton
                    v-if="row.state === 'running'"
                    size="small"
                    type="danger"
                    @click="handleContainerOp(row.id, row.containerName, 'stop')"
                  >
                    {{ t('common.stop') }}
                  </SecLabButton>
                  <SecLabButton
                    size="small"
                    @click="handleContainerOp(row.id, row.containerName, 'restart')"
                  >
                    {{ t('common.restart') }}
                  </SecLabButton>
                  <SecLabButton size="small" @click="handleScale(row.serviceName)">
                    {{ t('app.docker.projects.detail.scale') }}
                  </SecLabButton>
                </div>
              </template>
            </SecLabTable>
            <SecLabEmpty v-else :description="t('app.docker.projects.detail.emptyServices')" />
          </div>

          <!-- 项目日志 -->
          <div v-else class="logs-tab-content">
            <div class="logs-toolbar">
              <SecLabButton size="small" type="primary" :loading="isLogsLoading" @click="loadLogs">
                {{ t('app.docker.projects.detail.logsRefresh') }}
              </SecLabButton>
            </div>
            <div class="logs-viewer" ref="logsViewerRef">
              <pre v-if="store.projectLogs.length > 0" class="logs-pre">{{
                store.projectLogs.join('\n')
              }}</pre>
              <SecLabEmpty v-else :description="t('app.docker.projects.empty')" />
            </div>
          </div>
        </div>
      </div>
      <div class="modal-footer">
        <SecLabButton @click="emit('close')">
          {{ t('common.close') }}
        </SecLabButton>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* ─── 弹窗通用 ─── */
.modal-overlay {
  position: fixed;
  inset: 0;
  background-color: var(--sdl-bg-backdrop);
  display: flex;
  justify-content: center;
  align-items: center;
  z-index: var(--sdl-z-index-modal);
  backdrop-filter: blur(4px);
}

.modal {
  background: var(--sdl-bg-panel);
  border: 1px solid var(--sdl-border-strong);
  border-radius: var(--sdl-radius-lg);
  box-shadow: var(--sdl-shadow-window);
  width: 900px;
  max-width: 95%;
  max-height: 85%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  animation: sl-modal-pop 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
}

.modal-header {
  padding: var(--sdl-space-4) var(--sdl-space-5);
  border-bottom: 1px solid var(--sdl-border-subtle);
  background: var(--sdl-bg-muted);
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-shrink: 0;
}

.modal-header h3 {
  margin: 0;
  font-size: var(--sdl-font-subtitle);
  color: var(--sdl-text-primary);
  font-weight: 600;
}

.modal-close {
  background: none;
  border: none;
  font-size: 20px;
  cursor: pointer;
  color: var(--sdl-text-muted);
  transition: color 0.2s;
}

.modal-close:hover {
  color: var(--sdl-text-primary);
}

.modal-body {
  position: relative;
  padding: var(--sdl-space-5);
  overflow-y: hidden;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-4);
  flex: 1;
  min-height: 0;
}

.modal-footer {
  padding: var(--sdl-space-3) var(--sdl-space-5);
  border-top: 1px solid var(--sdl-border-subtle);
  display: flex;
  justify-content: flex-end;
  flex-shrink: 0;
}

/* ─── Tabs ─── */
.tabs-header {
  display: flex;
  border-bottom: 1px solid var(--sdl-border-subtle);
  gap: var(--sdl-space-6);
  flex-shrink: 0;
}

.tab-item {
  padding: var(--sdl-space-2) 0;
  font-size: var(--sdl-font-body-sm);
  color: var(--sdl-text-secondary);
  cursor: pointer;
  border-bottom: 2px solid transparent;
  transition: all 0.2s;
  font-weight: 500;
}

.tab-item:hover {
  color: var(--sdl-text-primary);
}

.tab-item-active {
  color: var(--sdl-primary);
  border-bottom-color: var(--sdl-primary);
}

.tab-content {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

/* ─── 服务列表 ─── */
.services-tab-content {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
}

.services-table {
  flex: 1;
}

.service-name-text {
  font-weight: 600;
  color: var(--sdl-primary);
}

.container-name-text {
  color: var(--sdl-text-primary);
}

.status-text {
  margin-left: var(--sdl-space-1);
}

.mono-text {
  font-family: var(--sdl-font-mono);
  font-size: var(--sdl-font-caption);
}

.ports-text {
  word-break: break-all;
  white-space: pre-wrap;
}

.row-actions {
  display: flex;
  gap: var(--sdl-space-2);
  justify-content: center;
}

/* ─── 聚合日志 ─── */
.logs-tab-content {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
  overflow: hidden;
}

.logs-toolbar {
  display: flex;
  justify-content: flex-end;
  flex-shrink: 0;
}

.logs-viewer {
  flex: 1;
  background-color: var(--sdl-bg-input);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  padding: var(--sdl-space-4);
  overflow-y: auto;
  font-family: var(--sdl-font-mono);
  font-size: var(--sdl-font-caption);
  color: var(--sdl-text-secondary);
}

.logs-pre {
  margin: 0;
  white-space: pre-wrap;
  word-break: break-all;
  line-height: 1.5;
}

@keyframes sl-modal-pop {
  from {
    transform: scale(0.95);
    opacity: 0;
  }
  to {
    transform: scale(1);
    opacity: 1;
  }
}
</style>
