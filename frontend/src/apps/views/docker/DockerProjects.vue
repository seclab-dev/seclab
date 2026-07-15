<script setup lang="ts">
/** Docker Compose 项目管理页：查询、配置、任务与节点变化均在模块内处理。 */
import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDockerStore } from '@/stores/docker'
import { useNodeStore } from '@/stores/node'
import { useConfirmationModalStore } from '@/stores/confirmation-modal'
import type * as dockerType from '@/api/interface/docker'
import MonacoEditor from '@/components/editor/MonacoEditor.vue'
import SecLabTable, { type SecLabTableColumn } from '@/components/ui/SecLabTable.vue'
import {
  SecLabActionMenu,
  SecLabAlert,
  SecLabButton,
  SecLabCheckbox,
  SecLabDialog,
  SecLabDrawer,
  SecLabEmpty,
  SecLabFormItem,
  SecLabInput,
  SecLabLoading,
  SecLabPagination,
  SecLabSelect,
  SecLabTag,
} from '@/components/ui'
import DockerProjectDetailDrawer from './DockerProjectDetailDrawer.vue'

const { t } = useI18n()
const store = useDockerStore()
const nodeStore = useNodeStore()
const confirmationStore = useConfirmationModalStore()

const keyword = ref('')
const managementKind = ref<dockerType.DockerProjectManagementKind | ''>('')
const runtimeState = ref<dockerType.DockerProjectRuntimeState | ''>('')
const detailVisible = ref(false)
const detailProjectName = ref('')
const createVisible = ref(false)
const createError = ref('')
const configurationVisible = ref(false)
const configurationProjectName = ref('')
const configurationYaml = ref('')
const configurationError = ref('')
const deploymentVisible = ref(false)
const deploymentProjectName = ref('')
const pullImages = ref(false)
const tasksVisible = ref(false)
const composeExample = 'services:\n  app:\n    image: nginx:latest\n'
const createForm = reactive({
  name: '',
  composeYaml: '',
})

const columns = computed<SecLabTableColumn[]>(() => [
  { label: t('app.docker.projects.columns.name'), minWidth: 220, slot: 'name' },
  { label: t('app.docker.projects.columns.management'), width: 140, slot: 'management' },
  { label: t('app.docker.projects.columns.status'), width: 130, align: 'center', slot: 'status' },
  {
    label: t('app.docker.projects.columns.services'),
    width: 100,
    align: 'center',
    prop: 'serviceCount',
  },
  {
    label: t('app.docker.projects.columns.runningTotal'),
    width: 140,
    align: 'center',
    slot: 'runningTotal',
  },
  { label: t('app.docker.projects.columns.actions'), width: 110, align: 'center', slot: 'actions' },
])
const taskColumns = computed<SecLabTableColumn[]>(() => [
  { label: t('app.docker.projects.tasks.project'), minWidth: 190, slot: 'project' },
  { label: t('app.docker.projects.tasks.operation'), width: 120, slot: 'operation' },
  { label: t('app.docker.projects.tasks.status'), width: 120, slot: 'taskStatus' },
  { label: t('app.docker.projects.tasks.stage'), width: 140, slot: 'stage' },
  { label: t('app.docker.projects.tasks.progress'), width: 90, slot: 'progress' },
  { label: t('app.docker.projects.columns.actions'), width: 90, slot: 'taskActions' },
])
const managementOptions = computed(() => [
  { label: t('app.docker.projects.filters.allManagement'), value: '' },
  { label: t('app.docker.projects.management.custom'), value: 'custom' },
  { label: t('app.docker.projects.management.suite'), value: 'suite' },
  { label: t('app.docker.projects.management.system'), value: 'system' },
])
const runtimeOptions = computed(() => [
  { label: t('app.docker.projects.filters.allStatus'), value: '' },
  { label: t('app.docker.projects.status.running'), value: 'running' },
  { label: t('app.docker.projects.status.partial'), value: 'partial' },
  { label: t('app.docker.projects.status.stopped'), value: 'stopped' },
  { label: t('app.docker.projects.status.unknown'), value: 'unknown' },
])
const totalPages = computed(() =>
  Math.max(1, Math.ceil(store.projectTotal / store.projectPageSize)),
)
const hasFilters = computed(() =>
  Boolean(keyword.value || managementKind.value || runtimeState.value),
)

const loadProjects = (page = store.projectPage) =>
  store.fetchComposeProjects({
    keyword: keyword.value.trim() || undefined,
    managementKind: managementKind.value || undefined,
    runtimeState: runtimeState.value || undefined,
    page,
    pageSize: store.projectPageSize,
  })

const openDetail = async (name: string) => {
  detailProjectName.value = name
  detailVisible.value = true
  await store.fetchComposeProjectDetail(name)
}
const closeDetail = () => {
  detailVisible.value = false
  detailProjectName.value = ''
  store.clearComposeProjectDetail()
}
const openCreate = () => {
  createError.value = ''
  Object.assign(createForm, {
    name: '',
    composeYaml: '',
  })
  createVisible.value = true
}
const fillComposeExample = async () => {
  if (createForm.composeYaml.trim()) {
    const confirmed = await confirmationStore.showConfirmation(
      t('app.docker.projects.create.exampleConfirm'),
      t('app.docker.projects.create.exampleConfirmTitle'),
      t('app.docker.projects.create.fillExample'),
      t('common.cancel'),
    )
    if (!confirmed) return
  }
  createForm.composeYaml = composeExample
}
const submitCreate = async () => {
  createError.value = ''
  const validation = await store.validateComposeYaml(createForm.composeYaml)
  if (!validation) {
    createError.value = store.projectConfigurationError || t('common.unknownError')
    return
  }
  if (!validation.valid) {
    createError.value = validation.error || t('app.docker.projects.configuration.invalid')
    return
  }
  if (
    await store.createComposeProject({
      name: createForm.name.trim(),
      composeYaml: createForm.composeYaml,
    })
  ) {
    createVisible.value = false
    tasksVisible.value = true
  }
}

const openConfiguration = async (name: string) => {
  store.clearComposeProjectConfiguration()
  configurationProjectName.value = name
  configurationYaml.value = ''
  configurationError.value = ''
  configurationVisible.value = true
  if (await store.fetchComposeProjectConfiguration(name)) {
    configurationYaml.value = store.projectConfiguration?.composeYaml || ''
  }
}
const closeConfiguration = () => {
  configurationVisible.value = false
  configurationProjectName.value = ''
  configurationYaml.value = ''
  configurationError.value = ''
  store.clearComposeProjectConfiguration()
}
const saveConfiguration = async () => {
  if (!store.projectConfiguration) return
  configurationError.value = ''
  const validation = await store.validateComposeYaml(configurationYaml.value)
  if (!validation?.valid) {
    configurationError.value =
      validation?.error ||
      store.projectConfigurationError ||
      t('app.docker.projects.configuration.invalid')
    return
  }
  const saved = await store.saveComposeProjectConfiguration(
    configurationProjectName.value,
    configurationYaml.value,
    store.projectConfiguration.revision,
  )
  if (saved) {
    closeConfiguration()
    await loadProjects()
  } else {
    configurationError.value = store.projectConfigurationError || t('common.unknownError')
  }
}

const openDeployment = (name: string) => {
  deploymentProjectName.value = name
  pullImages.value = false
  deploymentVisible.value = true
}
const submitDeployment = async () => {
  if (await store.redeployComposeProject(deploymentProjectName.value, pullImages.value)) {
    deploymentVisible.value = false
    tasksVisible.value = true
  }
}
const removeProject = async (name: string) => {
  const confirmed = await confirmationStore.showConfirmation(
    t('app.docker.projects.remove.confirm', { name }),
    t('app.docker.projects.remove.title'),
    t('common.delete'),
    t('common.cancel'),
  )
  if (confirmed && (await store.removeComposeProject(name))) tasksVisible.value = true
}

const rowActions = (project: dockerType.DockerProjectSummary) => {
  const actions: Array<{
    label: string
    handler: () => void
    disabled?: boolean
    className?: string
  }> = [{ label: t('app.docker.projects.actions.detail'), handler: () => openDetail(project.name) }]
  if (project.management.readOnly) return actions
  actions.push(
    {
      label: t('common.start'),
      handler: async () => {
        await store.runComposeProjectLifecycle(project.name, 'start')
      },
      disabled: !project.capabilities.canStart,
    },
    {
      label: t('common.stop'),
      handler: async () => {
        await store.runComposeProjectLifecycle(project.name, 'stop')
      },
      disabled: !project.capabilities.canStop,
    },
    {
      label: t('common.restart'),
      handler: async () => {
        await store.runComposeProjectLifecycle(project.name, 'restart')
      },
      disabled: !project.capabilities.canRestart,
    },
    {
      label: t('app.docker.projects.actions.editCompose'),
      handler: () => openConfiguration(project.name),
      disabled: !project.capabilities.canEditConfiguration,
    },
    {
      label: t('app.docker.projects.actions.redeploy'),
      handler: () => openDeployment(project.name),
      disabled: !project.capabilities.canRedeploy,
    },
    {
      label: t('common.delete'),
      handler: () => removeProject(project.name),
      disabled: !project.capabilities.canRemove,
      className: 'btn-delete',
    },
  )
  return actions
}

const statusTagType = (state: dockerType.DockerProjectRuntimeState) => {
  if (state === 'running') return 'success'
  if (state === 'partial') return 'warning'
  if (state === 'stopped') return 'info'
  return 'default'
}
const taskStatusType = (status: dockerType.DockerProjectTaskStatus) => {
  if (status === 'succeeded') return 'success'
  if (status === 'failed') return 'danger'
  if (status === 'cancelled') return 'warning'
  return 'info'
}
const taskStageLabel = (task: dockerType.DockerProjectTask) => {
  if (task.status === 'succeeded' || task.status === 'failed' || task.status === 'cancelled') {
    return t(`app.docker.projects.tasks.statuses.${task.status}`)
  }
  return t(`app.docker.projects.tasks.stages.${task.stage}`)
}
const openTasks = async () => {
  tasksVisible.value = true
  await store.fetchComposeProjectTasks()
  if (store.projectTasks.some((task) => task.status === 'queued' || task.status === 'running')) {
    store.startProjectTaskPolling()
  }
}
const handleVisibility = () => {
  if (document.hidden) {
    store.stopProjectTaskPolling()
  } else {
    void store.fetchComposeProjectTasks().then(() => store.startProjectTaskPolling())
  }
}

watch([keyword, managementKind, runtimeState], () => void loadProjects(1))
watch(
  () => nodeStore.currentNodeId,
  () => {
    closeDetail()
    closeConfiguration()
    tasksVisible.value = false
    void Promise.all([loadProjects(1), store.fetchComposeProjectTasks()])
  },
)
onMounted(() => {
  void Promise.all([loadProjects(1), store.fetchComposeProjectTasks()]).then(() => {
    if (store.projectTasks.some((task) => task.status === 'queued' || task.status === 'running')) {
      store.startProjectTaskPolling()
    }
  })
  document.addEventListener('visibilitychange', handleVisibility)
})
onUnmounted(() => {
  document.removeEventListener('visibilitychange', handleVisibility)
  store.stopProjectTaskPolling()
})
</script>

<template>
  <div class="project-page" data-page="docker-projects">
    <div class="project-toolbar" data-ui="toolbar">
      <div class="toolbar-filters">
        <SecLabInput
          id="docker-project-search"
          v-model="keyword"
          name="dockerProjectSearch"
          :placeholder="t('app.docker.projects.filters.searchPlaceholder')"
        />
        <SecLabSelect
          id="docker-project-management-filter"
          v-model="managementKind"
          name="dockerProjectManagementFilter"
          :options="managementOptions"
        />
        <SecLabSelect
          id="docker-project-status-filter"
          v-model="runtimeState"
          name="dockerProjectStatusFilter"
          :options="runtimeOptions"
        />
      </div>
      <div class="toolbar-actions">
        <SecLabButton
          type="secondary"
          :loading="store.projectListLoading"
          @click="loadProjects()"
          >{{ t('common.refresh') }}</SecLabButton
        >
        <SecLabButton type="secondary" @click="openTasks">{{
          t('app.docker.projects.tasks.title')
        }}</SecLabButton>
        <SecLabButton type="primary" @click="openCreate">{{
          t('app.docker.projects.actions.create')
        }}</SecLabButton>
      </div>
    </div>

    <SecLabAlert
      v-if="store.projectListError && store.composeProjects.length"
      type="warning"
      :title="t('app.docker.projects.refreshFailed')"
      :description="store.projectListError"
      show-icon
      data-ui="refresh-warning"
    />
    <div class="project-content" data-slot="content">
      <SecLabAlert
        v-if="store.projectListError && !store.composeProjects.length && !store.projectListLoading"
        type="error"
        :title="t('app.docker.projects.loadFailed')"
        :description="store.projectListError"
        show-icon
      />
      <SecLabTable
        v-if="store.composeProjects.length"
        :data="store.composeProjects"
        :columns="columns"
        border
        data-ui="table"
      >
        <template #name="{ row }: { row: dockerType.DockerProjectSummary }">
          <button class="project-name" type="button" @click="openDetail(row.name)">
            {{ row.name }}
          </button>
        </template>
        <template #management="{ row }: { row: dockerType.DockerProjectSummary }">
          <SecLabTag :type="row.management.kind === 'custom' ? 'default' : 'primary'">{{
            t(`app.docker.projects.management.${row.management.kind}`)
          }}</SecLabTag>
        </template>
        <template #status="{ row }: { row: dockerType.DockerProjectSummary }">
          <SecLabTag :type="statusTagType(row.runtimeState)">{{
            t(`app.docker.projects.status.${row.runtimeState}`)
          }}</SecLabTag>
        </template>
        <template #runningTotal="{ row }: { row: dockerType.DockerProjectSummary }"
          >{{ row.containerStates.running }}/{{ row.containerStates.total }}</template
        >
        <template #actions="{ row }: { row: dockerType.DockerProjectSummary }">
          <SecLabActionMenu
            :label="t('app.docker.projects.actions.menu')"
            :actions="rowActions(row)"
          />
        </template>
      </SecLabTable>
      <SecLabEmpty
        v-else-if="!store.projectListLoading && !store.projectListError"
        :description="
          hasFilters ? t('app.docker.projects.filteredEmpty') : t('app.docker.projects.empty')
        "
      />
      <SecLabLoading :loading="store.projectListLoading && !store.composeProjects.length" cover />
    </div>
    <div
      v-if="store.projectTotal > store.projectPageSize"
      class="project-pagination"
      data-ui="pagination"
    >
      <SecLabPagination
        :current-page="store.projectPage"
        :total-pages="totalPages"
        @page-change="loadProjects"
      />
    </div>

    <SecLabDialog
      :visible="createVisible"
      :title="t('app.docker.projects.create.title')"
      width="820px"
      :close-on-click-overlay="!store.projectMutationLoading"
      data-ui="project-create-dialog"
      @close="createVisible = false"
    >
      <div class="project-form" data-slot="body">
        <SecLabAlert v-if="createError" type="error" :title="createError" show-icon />
        <SecLabFormItem
          :label="t('app.docker.projects.create.name')"
          for="docker-project-name"
          required
        >
          <SecLabInput id="docker-project-name" v-model="createForm.name" name="projectName" />
        </SecLabFormItem>
        <div class="compose-field" data-ui="compose-configuration-field">
          <div class="compose-field-header">
            <div id="docker-project-compose-label" class="compose-field-label">
              <span class="required-mark" aria-hidden="true">*</span>
              {{ t('app.docker.projects.create.compose') }}
            </div>
            <SecLabButton
              type="secondary"
              size="small"
              data-ui="fill-compose-example"
              @click="fillComposeExample"
            >
              {{ t('app.docker.projects.create.fillExample') }}
            </SecLabButton>
          </div>
          <div class="compose-editor" data-ui="compose-editor">
            <MonacoEditor
              id="docker-project-compose"
              v-model="createForm.composeYaml"
              name="composeYaml"
              language="yaml"
              :minimap="false"
              aria-labelledby="docker-project-compose-label"
            />
          </div>
        </div>
      </div>
      <template #footer>
        <SecLabButton :disabled="store.projectMutationLoading" @click="createVisible = false">{{
          t('common.cancel')
        }}</SecLabButton>
        <SecLabButton
          type="primary"
          :loading="store.projectMutationLoading"
          @click="submitCreate"
          >{{ t('app.docker.projects.actions.submit') }}</SecLabButton
        >
      </template>
    </SecLabDialog>

    <DockerProjectDetailDrawer
      v-model="detailVisible"
      :project-name="detailProjectName"
      @close="closeDetail"
      @configure="openConfiguration"
      @redeploy="openDeployment"
      @show-tasks="tasksVisible = true"
    />

    <SecLabDrawer
      v-model="configurationVisible"
      :title="t('app.docker.projects.configuration.title', { name: configurationProjectName })"
      width="820px"
      data-ui="project-configuration-drawer"
      @close="closeConfiguration"
    >
      <div class="configuration-content" data-slot="body">
        <SecLabAlert
          v-if="configurationError || store.projectConfigurationError"
          type="error"
          :title="configurationError || store.projectConfigurationError || ''"
          show-icon
        />
        <SecLabAlert
          type="info"
          :title="t('app.docker.projects.configuration.saveOnlyTitle')"
          :description="t('app.docker.projects.configuration.saveOnlyDescription')"
          show-icon
        />
        <div
          v-if="store.projectConfiguration"
          class="compose-editor configuration-editor"
          data-ui="compose-editor"
        >
          <MonacoEditor
            v-model="configurationYaml"
            language="yaml"
            :minimap="false"
            :aria-label="t('app.docker.projects.configuration.editorLabel')"
          />
        </div>
        <SecLabLoading
          :loading="store.projectConfigurationLoading && !store.projectConfiguration"
          cover
        />
      </div>
      <template #footer>
        <SecLabButton @click="closeConfiguration">{{ t('common.cancel') }}</SecLabButton>
        <SecLabButton
          type="primary"
          :loading="store.projectMutationLoading"
          :disabled="!store.projectConfiguration"
          @click="saveConfiguration"
          >{{ t('common.save') }}</SecLabButton
        >
      </template>
    </SecLabDrawer>

    <SecLabDialog
      :visible="deploymentVisible"
      :title="t('app.docker.projects.deployment.title')"
      width="520px"
      data-ui="project-deployment-dialog"
      @close="deploymentVisible = false"
    >
      <div class="dialog-content" data-slot="body">
        <SecLabAlert
          type="warning"
          :title="t('app.docker.projects.deployment.recreateTitle')"
          :description="t('app.docker.projects.deployment.recreateDescription')"
          show-icon
        />
        <SecLabCheckbox
          id="docker-project-pull-images"
          v-model="pullImages"
          name="pullImages"
          :label="t('app.docker.projects.deployment.pullImages')"
        />
      </div>
      <template #footer>
        <SecLabButton @click="deploymentVisible = false">{{ t('common.cancel') }}</SecLabButton>
        <SecLabButton
          type="warning"
          :loading="store.projectMutationLoading"
          @click="submitDeployment"
          >{{ t('app.docker.projects.actions.redeploy') }}</SecLabButton
        >
      </template>
    </SecLabDialog>

    <SecLabDrawer
      v-model="tasksVisible"
      :title="t('app.docker.projects.tasks.title')"
      width="860px"
      data-ui="project-task-drawer"
    >
      <div class="task-content" data-slot="body">
        <SecLabAlert
          v-if="store.projectTasksError"
          type="warning"
          :title="t('app.docker.projects.tasks.refreshFailed')"
          :description="store.projectTasksError"
          show-icon
        />
        <SecLabTable
          v-if="store.projectTasks.length"
          :data="store.projectTasks"
          :columns="taskColumns"
          border
        >
          <template #project="{ row }: { row: dockerType.DockerProjectTask }">
            <div class="task-project">{{ row.projectName }}</div>
            <div v-if="row.errorSummary" class="task-message task-error">
              {{ row.errorSummary }}
            </div>
            <div v-if="row.cleanupWarning" class="task-message">{{ row.cleanupWarning }}</div>
          </template>
          <template #operation="{ row }: { row: dockerType.DockerProjectTask }">{{
            t(`app.docker.projects.tasks.operations.${row.operation}`)
          }}</template>
          <template #taskStatus="{ row }: { row: dockerType.DockerProjectTask }"
            ><SecLabTag :type="taskStatusType(row.status)">{{
              t(`app.docker.projects.tasks.statuses.${row.status}`)
            }}</SecLabTag></template
          >
          <template #stage="{ row }: { row: dockerType.DockerProjectTask }">{{
            taskStageLabel(row)
          }}</template>
          <template #progress="{ row }: { row: dockerType.DockerProjectTask }"
            >{{ row.progressPercent }}%</template
          >
          <template #taskActions="{ row }: { row: dockerType.DockerProjectTask }">
            <SecLabButton
              v-if="row.status === 'queued' || row.status === 'running'"
              size="small"
              type="danger"
              :loading="store.projectTaskCancelingIds.includes(row.id)"
              @click="store.cancelComposeProjectTask(row.id)"
              >{{ t('common.cancel') }}</SecLabButton
            >
          </template>
        </SecLabTable>
        <SecLabEmpty
          v-else-if="!store.projectTasksLoading"
          :description="t('app.docker.projects.tasks.empty')"
        />
        <SecLabLoading :loading="store.projectTasksLoading && !store.projectTasks.length" cover />
      </div>
    </SecLabDrawer>
  </div>
</template>

<style scoped>
.project-page {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
  padding-top: var(--sdl-space-3);
}
.project-toolbar,
.toolbar-filters,
.toolbar-actions {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-3);
}
.project-toolbar {
  justify-content: space-between;
  flex-wrap: wrap;
  padding: var(--sdl-space-3) var(--sdl-space-4);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  background: var(--sdl-bg-panel);
}
.toolbar-filters {
  flex: 1;
  min-width: 0;
}
.project-content,
.configuration-content,
.task-content {
  position: relative;
  flex: 1;
  min-height: 0;
  overflow: auto;
}
.project-name {
  display: block;
  padding: 0;
  border: 0;
  color: var(--sdl-text-link);
  background: transparent;
  cursor: pointer;
  font: inherit;
  font-weight: var(--sdl-font-weight-medium);
}
.task-project {
  font-weight: var(--sdl-font-weight-medium);
}
.task-message {
  margin-top: var(--sdl-space-1);
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-size-xs);
  overflow-wrap: anywhere;
}
.task-error {
  color: var(--sdl-color-danger);
}
.project-pagination {
  display: flex;
  justify-content: flex-end;
  flex-shrink: 0;
}
.project-form,
.configuration-content,
.task-content,
.dialog-content {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-4);
}
.compose-editor {
  height: 360px;
  min-height: 240px;
  overflow: hidden;
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
}
.compose-field,
.compose-field-header {
  display: flex;
}
.compose-field {
  flex-direction: column;
  gap: var(--sdl-space-2);
}
.compose-field-header {
  align-items: center;
  gap: var(--sdl-space-2);
}
.compose-field-label {
  color: var(--sdl-text-primary);
  font-size: var(--sdl-font-size-sm);
  font-weight: var(--sdl-font-weight-medium);
}
.required-mark {
  color: var(--sdl-color-danger);
}
@media (max-width: 860px) {
  .project-toolbar,
  .toolbar-filters {
    align-items: stretch;
    flex-direction: column;
  }
  .toolbar-actions {
    justify-content: flex-end;
  }
}
</style>
