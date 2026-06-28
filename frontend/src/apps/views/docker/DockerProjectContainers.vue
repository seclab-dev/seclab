<script setup lang="ts">
import SecLabTable, { type SecLabTableColumn } from '@/components/ui/SecLabTable.vue'
import {
  SecLabButton,
  SecLabTag,
  SecLabEmpty,
  SecLabInput,
  SecLabFormItem,
  SecLabActionMenu,
} from '@/components/ui'
import SecLabIcon from '@/components/icons/SecLabIcon.vue'
import MonacoEditor from '@/components/editor/MonacoEditor.vue'
import * as dockerType from '@/api/interface/docker'
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDockerStore } from '@/stores/docker'
import { getStateIcon } from '@/utils/docker-format'
import DockerProjectDetailModal from './DockerProjectDetailModal.vue'

/**
 * @file DockerProjectContainers.vue
 * @description Docker Compose 项目列表组件，严格遵循 SDL 设计规范，直接消费 Pinia Store。
 */

const { t } = useI18n()
const store = useDockerStore()

const composeMissingHint = computed(() => t('app.docker.projects.actions.composeMissing'))
const deleteRunningBlockedHint = computed(() =>
  t('app.docker.projects.actions.deleteRunningBlocked'),
)

// ─── 项目详情 Modal 控制 ───
const showDetailModal = ref(false)
const currentProjectForDetail = ref('')

const openDetailModal = (projectName: string) => {
  currentProjectForDetail.value = projectName
  showDetailModal.value = true
}

const closeDetailModal = () => {
  showDetailModal.value = false
  currentProjectForDetail.value = ''
}

/** 项目列表表格列定义 */
const columns = computed<SecLabTableColumn[]>(() => [
  { label: t('app.docker.projects.columns.name'), minWidth: 220, slot: 'name' },
  { label: t('app.docker.projects.columns.status'), width: 180, align: 'center', slot: 'status' },
  {
    label: t('app.docker.projects.columns.runningTotal'),
    width: 140,
    align: 'center',
    slot: 'runningTotal',
  },
  { label: t('app.docker.projects.columns.actions'), width: 120, align: 'center', slot: 'actions' },
])

const resolveStatusType = (
  status: string | undefined,
): 'success' | 'warning' | 'danger' | 'info' | 'primary' => {
  const normalized = (status || '').toLowerCase()
  if (normalized === 'running') return 'success'
  if (normalized === 'stopped') return 'warning'
  if (normalized === 'exited') return 'danger'
  if (normalized === 'created') return 'info'
  return 'primary'
}

const statusLabel = (status: string | undefined) => {
  const normalized = (status || '').toLowerCase()
  switch (normalized) {
    case 'running':
      return t('app.docker.projects.status.running')
    case 'stopped':
      return t('app.docker.projects.status.stopped')
    case 'exited':
      return t('app.docker.projects.status.exited')
    case 'created':
      return t('app.docker.projects.status.created')
    default:
      return t('app.docker.projects.status.unknown')
  }
}
</script>

<template>
  <div class="docker-containers" data-page="docker-projects">
    <div class="docker-card">
      <div v-if="store.isProjectCreateActive" class="app-project-create-panel">
        <div class="create-panel-body sl-form">
          <SecLabFormItem :label="t('app.docker.projects.create.name')" required>
            <SecLabInput
              id="project-name"
              v-model="store.projectFormName"
              :placeholder="t('app.docker.projects.create.namePlaceholder')"
            />
          </SecLabFormItem>

          <!-- TODO: 暂时隐藏自定义项目目录功能 -->
          <!-- <SecLabFormItem :label="t('app.docker.projects.create.dir')">
            <SecLabInput
              id="project-dir"
              v-model="store.projectFormDir"
              :placeholder="t('app.docker.projects.create.dirPlaceholder')"
            />
          </SecLabFormItem> -->

          <SecLabFormItem
            class="app-compose-group"
            :label="t('app.docker.projects.create.compose')"
            required
          >
            <div class="app-compose-editor" data-ui="docker-compose-editor">
              <MonacoEditor
                v-model="store.projectFormCompose"
                language="yaml"
                @change="store.validateComposeYaml"
              />
            </div>
            <p v-if="store.composeYamlError" class="app-form-error">
              {{ t('app.docker.projects.create.yamlError', { error: store.composeYamlError }) }}
            </p>
          </SecLabFormItem>
        </div>

        <div class="create-panel-footer">
          <SecLabButton @click="store.cancelProjectCreate">
            {{ t('app.docker.projects.actions.cancel') }}
          </SecLabButton>
          <SecLabButton type="primary" @click="store.submitProjectForm">
            {{ t('app.docker.projects.actions.submit') }}
          </SecLabButton>
        </div>
      </div>

      <div v-else class="card-scroll-wrapper">
        <div class="app-card-actions app-card-actions-left">
          <SecLabButton type="primary" @click="store.isProjectCreateActive = true">
            {{ t('app.docker.projects.actions.create') }}
          </SecLabButton>
        </div>

        <SecLabTable
          v-if="store.composeProjects.length > 0"
          :data="store.composeProjects"
          :columns="columns"
          border
        >
          <template #name="{ row }: { row: dockerType.ComposeProjectSummary }">
            <span class="project-name-cell" @click="openDetailModal(row?.name)">{{
              row?.name
            }}</span>
          </template>

          <template #status="{ row }: { row: dockerType.ComposeProjectSummary }">
            <SecLabTag :type="resolveStatusType(row?.status)">
              <SecLabIcon :name="getStateIcon(row?.status)" :size="14" />
              <span class="status-text">{{ statusLabel(row?.status) }}</span>
            </SecLabTag>
          </template>

          <template #runningTotal="{ row }: { row: dockerType.ComposeProjectSummary }">
            {{ row?.runningContainers ?? 0 }}/{{ row?.totalContainers ?? 0 }}
          </template>

          <template #actions="{ row }: { row: dockerType.ComposeProjectSummary }">
            <SecLabActionMenu
              :label="t('app.docker.projects.actions.menu')"
              :actions="[
                {
                  label: t('app.docker.projects.actions.detail'),
                  handler: () => openDetailModal(row?.name),
                },
                {
                  label: t('common.start'),
                  handler: () => store.handleComposeProjectAction(row?.name, 'start'),
                  disabled: !row?.hasComposeFile || row?.status === 'running',
                  tooltip: !row?.hasComposeFile ? composeMissingHint : undefined,
                },
                {
                  label: t('common.stop'),
                  handler: () => store.handleComposeProjectAction(row?.name, 'stop'),
                  disabled: !row?.hasComposeFile || row?.status !== 'running',
                  tooltip: !row?.hasComposeFile ? composeMissingHint : undefined,
                },
                {
                  label: t('common.restart'),
                  handler: () => store.handleComposeProjectAction(row?.name, 'restart'),
                  disabled: !row?.hasComposeFile,
                  tooltip: !row?.hasComposeFile ? composeMissingHint : undefined,
                },
                {
                  label: t('app.docker.projects.actions.update'),
                  handler: () => store.handleComposeProjectAction(row?.name, 'update'),
                  disabled: !row?.hasComposeFile,
                  tooltip: !row?.hasComposeFile ? composeMissingHint : undefined,
                },
                {
                  label: t('app.docker.projects.actions.editCompose'),
                  handler: () => store.handleEditComposeConfig(row?.name),
                  disabled: !row?.hasComposeFile,
                  tooltip: !row?.hasComposeFile ? composeMissingHint : undefined,
                },
                {
                  label: t('common.delete'),
                  handler: () => store.handleComposeProjectAction(row?.name, 'delete'),
                  disabled: row?.status === 'running',
                  tooltip:
                    row?.status === 'running'
                      ? deleteRunningBlockedHint
                      : row?.projectType === 'docker'
                        ? t('app.docker.projects.actions.deleteDockerHint')
                        : t('app.docker.projects.actions.deleteSystemHint'),
                  class: 'btn-delete',
                },
              ]"
            />
          </template>

          <template #empty>
            <SecLabEmpty :description="t('app.docker.projects.empty')" />
          </template>
        </SecLabTable>

        <SecLabEmpty v-else :description="t('app.docker.projects.empty')" />
      </div>
    </div>

    <!-- 项目详情与日志 Modal -->
    <DockerProjectDetailModal
      v-if="showDetailModal"
      :project-name="currentProjectForDetail"
      @close="closeDetailModal"
    />
  </div>
</template>

<style scoped>
.docker-containers {
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

.app-project-create-panel {
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-lg);
  padding: var(--sdl-space-5);
  background: var(--sdl-bg-card);
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-4);
  flex: 1;
  min-height: 0;
}

.create-panel-body {
  flex: 1;
  min-height: 0;
  overflow: auto;
}

.app-compose-group {
  flex: 1;
  min-height: 0;
}

.app-compose-editor {
  display: flex;
  align-items: stretch;
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-sm);
  background: var(--sdl-bg-input);
  overflow: hidden;
  width: 100%;
  height: 380px;
  min-height: 280px;
}

.app-form-error {
  color: var(--sdl-danger);
  font-size: var(--sdl-font-caption);
  margin: var(--sdl-space-2) 0 0;
}

.create-panel-footer {
  display: flex;
  gap: var(--sdl-space-3);
  flex-shrink: 0;
}

.card-scroll-wrapper {
  flex-grow: 1;
  min-height: 0;
  overflow-y: auto;
}

.app-card-actions {
  display: flex;
  justify-content: flex-end;
  margin-bottom: var(--sdl-space-4);
}

.app-card-actions-left {
  justify-content: flex-start;
}

.project-name-cell {
  color: var(--sdl-primary);
  cursor: pointer;
  font-weight: 500;
}

.project-name-cell:hover {
  text-decoration: underline;
}

.status-text {
  margin-left: var(--sdl-space-1);
}
</style>
