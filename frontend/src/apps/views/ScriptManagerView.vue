<script setup lang="ts">
/**
 * @file ScriptManagerView.vue
 * @description 全局自定义 Shell 脚本资产、修订与异步运行界面。
 */

import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ScriptDetail, ScriptRun, ScriptSummary } from '@/api/modules/scripts'
import { useScriptLibrary } from '@/composables/useScriptLibrary'
import { useConfirmationModalStore } from '@/stores/confirmation-modal'
import { useNotificationStore } from '@/stores/notification'
import { useWindowManagerStore } from '@/stores/window-manager'
import MonacoEditor from '@/components/editor/MonacoEditor.vue'
import SecLabIcon from '@/components/icons/SecLabIcon.vue'
import {
  SecLabActionMenu,
  SecLabAlert,
  SecLabButton,
  SecLabDialog,
  SecLabDrawer,
  SecLabEmpty,
  SecLabFormItem,
  SecLabInput,
  SecLabLoading,
  SecLabPagination,
  SecLabSelect,
  SecLabTable,
  SecLabTag,
} from '@/components/ui'
import type { SecLabTableColumn } from '@/components/ui/SecLabTable.vue'

const props = defineProps<{ windowId?: string }>()
const { t } = useI18n()
const library = useScriptLibrary()
const confirmation = useConfirmationModalStore()
const notifications = useNotificationStore()
const windowStore = useWindowManagerStore()

type EditorMode = 'view' | 'create' | 'edit' | 'clone'
interface ScriptMenuAction {
  label: string
  icon: string
  className?: string
  handler: () => void
}

const selectedId = ref('')
const mode = ref<EditorMode>('view')
const baseline = ref('')
const form = ref({ name: '', description: '', content: '', timeoutSeconds: 300 })
const runDialogVisible = ref(false)
const runNodeId = ref('')
const runTimeoutSeconds = ref(300)
const runsVisible = ref(false)
const outputVisible = ref(false)
const selectedRun = ref<ScriptRun | null>(null)

const selectedSummary = computed(
  () => library.scripts.value.find((script) => script.scriptId === selectedId.value) ?? null,
)
const isEditing = computed(() => mode.value !== 'view')
const formSnapshot = computed(() => JSON.stringify(form.value))
const isDirty = computed(() => isEditing.value && formSnapshot.value !== baseline.value)
const totalPages = computed(() =>
  Math.max(1, Math.ceil(library.total.value / library.filters.pageSize)),
)
const onlineNodeOptions = computed(() =>
  library.nodes.value
    .filter((node) => node.status === 'online')
    .map((node) => ({
      value: node.nodeId,
      label: node.nodeId === 'local' ? t('app.nodes.master') : node.name || node.nodeId,
    })),
)
const currentTrackedRun = computed(() =>
  selectedRun.value
    ? (library.trackedRuns.value[selectedRun.value.runId] ?? selectedRun.value)
    : null,
)
const outputText = computed(() =>
  (library.output.value?.items ?? [])
    .map((chunk) => `[${chunk.stream}]\n${chunk.content}`)
    .join('\n'),
)

const runColumns = computed<SecLabTableColumn[]>(() => [
  { prop: 'scriptName', label: t('app.scriptManager.runs.script'), minWidth: 150 },
  { prop: 'nodeName', label: t('app.scriptManager.runs.node'), minWidth: 120 },
  { prop: 'status', label: t('app.scriptManager.runs.status'), width: 130, slot: 'status' },
  {
    prop: 'queuedAt',
    label: t('app.scriptManager.runs.queuedAt'),
    minWidth: 170,
    slot: 'queuedAt',
  },
  { prop: 'actions', label: t('common.actions'), width: 170, align: 'right', slot: 'actions' },
])

const formatTime = (value?: string) => (value ? new Date(value).toLocaleString() : '')
const statusLabel = (status: string) => t(`app.scriptManager.status.${status}`)
const statusTag = (status: string) => {
  if (status === 'succeeded') return 'success'
  if (status === 'failed' || status === 'timedOut') return 'danger'
  if (status === 'cancelled') return 'warning'
  return 'info'
}

/** 用详情填充编辑表单并记录脏状态基线。 */
const fillForm = (detail: ScriptDetail) => {
  form.value = {
    name: detail.name,
    description: detail.description ?? '',
    content: detail.source.content,
    timeoutSeconds: detail.executionDefaults.timeoutSeconds,
  }
  baseline.value = JSON.stringify(form.value)
}

const confirmDiscard = async () => {
  if (!isDirty.value) return true
  return confirmation.showConfirmation(
    t('app.scriptManager.unsaved.message'),
    t('app.scriptManager.unsaved.title'),
    t('app.scriptManager.unsaved.discard'),
    t('common.cancel'),
  )
}

/** 选择摘要后按需加载正文，未保存内容必须先明确放弃。 */
const selectScript = async (script: ScriptSummary) => {
  if (script.scriptId === selectedId.value && mode.value === 'view') return
  if (!(await confirmDiscard())) return
  selectedId.value = script.scriptId
  mode.value = 'view'
  const detail = await library.loadDetail(script.scriptId)
  if (detail) fillForm(detail)
}

const startCreate = async () => {
  if (!(await confirmDiscard())) return
  selectedId.value = ''
  mode.value = 'create'
  form.value = {
    name: '',
    description: '',
    content: '#!/bin/bash\n\nset -euo pipefail\n',
    timeoutSeconds: 300,
  }
  baseline.value = JSON.stringify(form.value)
}

const startEdit = () => {
  if (!library.detail.value?.capabilities.canUpdate) return
  fillForm(library.detail.value)
  mode.value = 'edit'
}

const startClone = async () => {
  const detail = library.detail.value
  if (!detail?.capabilities.canClone || !(await confirmDiscard())) return
  selectedId.value = ''
  mode.value = 'clone'
  form.value = {
    name: t('app.scriptManager.cloneName', { name: detail.name }),
    description: detail.description ?? '',
    content: detail.source.content,
    timeoutSeconds: detail.executionDefaults.timeoutSeconds,
  }
  baseline.value = JSON.stringify(form.value)
}

const cancelEdit = async () => {
  if (!(await confirmDiscard())) return
  mode.value = 'view'
  const detail = library.detail.value
  if (detail) {
    selectedId.value = detail.scriptId
    fillForm(detail)
  } else if (library.scripts.value[0]) {
    await selectScript(library.scripts.value[0])
  }
}

const save = async () => {
  if (!form.value.name.trim())
    return notifications.warning(t('app.scriptManager.messages.nameRequired'))
  if (!form.value.content.trim())
    return notifications.warning(t('app.scriptManager.messages.contentRequired'))
  try {
    const detail =
      mode.value === 'edit' && library.detail.value
        ? await library.updateScript(library.detail.value.scriptId, {
            expectedRevision: library.detail.value.revision,
            name: form.value.name,
            description: form.value.description || undefined,
            content: form.value.content,
            timeoutSeconds: Number(form.value.timeoutSeconds),
          })
        : await library.createScript({
            name: form.value.name,
            description: form.value.description || undefined,
            content: form.value.content,
            timeoutSeconds: Number(form.value.timeoutSeconds),
          })
    selectedId.value = detail.scriptId
    library.detail.value = detail
    fillForm(detail)
    mode.value = 'view'
    notifications.success(t('app.scriptManager.messages.saved'))
  } catch (error) {
    notifications.error(
      error instanceof Error ? error.message : t('app.scriptManager.messages.saveFailed'),
    )
  }
}

const remove = async () => {
  const script = selectedSummary.value
  if (!script?.capabilities.canRemove) return
  const confirmed = await confirmation.showConfirmation(
    t('app.scriptManager.delete.message', { name: script.name }),
    t('app.scriptManager.delete.title'),
    t('app.scriptManager.delete.confirm'),
    t('common.cancel'),
  )
  if (!confirmed) return
  try {
    await library.removeScript(script.scriptId)
    selectedId.value = ''
    mode.value = 'view'
    notifications.success(t('app.scriptManager.messages.removed'))
  } catch (error) {
    notifications.error(
      error instanceof Error ? error.message : t('app.scriptManager.messages.deleteFailed'),
    )
  }
}

const openRun = () => {
  const detail = library.detail.value
  if (!detail?.capabilities.canRun) return
  runNodeId.value = onlineNodeOptions.value[0]?.value ?? ''
  runTimeoutSeconds.value = detail.executionDefaults.timeoutSeconds
  runDialogVisible.value = true
}

const submitRun = async () => {
  const detail = library.detail.value
  if (!detail || !runNodeId.value) return
  try {
    const run = await library.startRun(
      detail.scriptId,
      runNodeId.value,
      Number(runTimeoutSeconds.value),
    )
    runDialogVisible.value = false
    selectedRun.value = run
    notifications.success(t('app.scriptManager.messages.runAccepted'))
  } catch (error) {
    notifications.error(
      error instanceof Error ? error.message : t('app.scriptManager.messages.runFailed'),
    )
  }
}

const openRuns = async () => {
  runsVisible.value = true
  await library.loadRuns()
}
const openOutput = async (run: ScriptRun) => {
  selectedRun.value = run
  outputVisible.value = true
  await library.loadOutput(run.runId)
}
const cancelRun = async (run: ScriptRun) => {
  try {
    selectedRun.value = await library.cancelRun(run.runId)
    notifications.success(t('app.scriptManager.messages.cancelAccepted'))
  } catch (error) {
    notifications.error(
      error instanceof Error ? error.message : t('app.scriptManager.messages.cancelFailed'),
    )
  }
}

const refresh = async () => {
  if (!(await confirmDiscard())) return
  await library.refreshScripts()
  if (selectedId.value) {
    const detail = await library.loadDetail(selectedId.value)
    if (detail) fillForm(detail)
  }
}

const rowActions = (script: ScriptSummary): ScriptMenuAction[] => {
  const actions: ScriptMenuAction[] = [
    {
      label: t('app.scriptManager.actions.open'),
      icon: 'info',
      handler: () => void selectScript(script),
    },
  ]
  if (script.capabilities.canRun)
    actions.push({
      label: t('app.scriptManager.actions.run'),
      icon: 'play',
      handler: () => void selectScript(script).then(openRun),
    })
  if (script.capabilities.canClone)
    actions.push({
      label: t('app.scriptManager.actions.clone'),
      icon: 'copy',
      handler: () => void selectScript(script).then(startClone),
    })
  if (script.capabilities.canRemove)
    actions.push({
      label: t('app.scriptManager.actions.remove'),
      icon: 'trash',
      className: 'danger',
      handler: () => void selectScript(script).then(remove),
    })
  return actions
}

watch(
  () => library.scripts.value,
  (scripts) => {
    if (!selectedId.value && mode.value === 'view' && scripts[0]) void selectScript(scripts[0])
  },
)
watch(
  [isDirty, () => library.saveState.value.refreshing],
  ([dirty, saving]) => {
    if (!props.windowId) return
    windowStore.updateWindowRuntimeState(props.windowId, {
      dirty,
      busy: saving,
      allowsNodeSwitch: !dirty && !saving,
      blockLevel: saving ? 'busy' : dirty ? 'dirty' : 'open',
      blockReason: saving
        ? t('app.scriptManager.guardSaving')
        : dirty
          ? t('app.scriptManager.guardDirty')
          : t('app.scriptManager.guardOpen'),
    })
  },
  { immediate: true },
)

const beforeUnload = (event: BeforeUnloadEvent) => {
  if (!isDirty.value) return
  event.preventDefault()
}
window.addEventListener('beforeunload', beforeUnload)
onBeforeUnmount(() => window.removeEventListener('beforeunload', beforeUnload))
</script>

<template>
  <div class="script-library" data-page="script-library" data-seclab-app="script-library">
    <div class="toolbar" data-ui="toolbar" data-slot="content">
      <SecLabInput
        id="script-library-search"
        v-model="library.filters.keyword"
        name="scriptLibrarySearch"
        class="search-input"
        :aria-label="t('app.scriptManager.search')"
        :placeholder="t('app.scriptManager.searchPlaceholder')"
      />
      <div class="toolbar-actions">
        <SecLabButton @click="openRuns">
          <SecLabIcon name="history" :size="14" />
          {{ t('app.scriptManager.actions.runs') }}
        </SecLabButton>
        <SecLabButton
          :loading="library.listState.value.refreshing"
          :disabled="library.listState.value.initialLoading"
          @click="refresh"
        >
          <SecLabIcon name="refresh" :size="14" />
          {{ t('common.refresh') }}
        </SecLabButton>
        <SecLabButton type="primary" @click="startCreate">
          <SecLabIcon name="plus" :size="14" />
          {{ t('app.scriptManager.actions.create') }}
        </SecLabButton>
      </div>
    </div>

    <SecLabAlert
      v-if="library.listState.value.warning"
      data-ui="alert"
      type="warning"
      :title="t('app.scriptManager.messages.refreshWarning')"
      :description="library.listState.value.warning"
      show-icon
    />
    <SecLabAlert
      v-if="library.activeRuns.value.length"
      data-ui="alert"
      type="info"
      :title="t('app.scriptManager.activeRuns', { count: library.activeRuns.value.length })"
      show-icon
    />

    <div class="workspace" data-ui="workspace" data-slot="content">
      <div class="script-list" data-ui="script-list">
        <div class="list-scroll" role="listbox" :aria-label="t('app.scriptManager.listLabel')">
          <div
            v-for="script in library.scripts.value"
            :key="script.scriptId"
            class="script-row"
            :class="{ selected: script.scriptId === selectedId }"
          >
            <button
              type="button"
              class="script-select"
              role="option"
              :aria-selected="script.scriptId === selectedId"
              @click="selectScript(script)"
            >
              <span class="script-name">{{ script.name }}</span>
              <span v-if="script.description" class="script-description">{{
                script.description
              }}</span>
              <span class="script-meta">
                {{ t('app.scriptManager.revision', { revision: script.revision }) }}
                <SecLabTag v-if="script.lastRun" :type="statusTag(script.lastRun.status)">
                  {{ statusLabel(script.lastRun.status) }}
                </SecLabTag>
              </span>
            </button>
            <SecLabActionMenu :label="t('common.actions')" :actions="rowActions(script)" />
          </div>
          <SecLabEmpty
            v-if="!library.listState.value.initialLoading && !library.scripts.value.length"
            :description="
              library.filters.keyword.trim()
                ? t('app.scriptManager.empty.filtered')
                : t('app.scriptManager.empty.library')
            "
          />
        </div>
        <SecLabLoading :loading="library.listState.value.initialLoading" cover />
        <SecLabPagination
          v-if="library.total.value > library.filters.pageSize"
          :current-page="library.filters.page"
          :total-pages="totalPages"
          @page-change="(page) => (library.filters.page = page)"
        />
      </div>

      <div class="editor-pane" data-ui="editor" data-slot="detail">
        <template v-if="isEditing || library.detail.value">
          <div class="editor-toolbar">
            <div class="editor-identity">
              <strong>{{
                isEditing
                  ? form.name || t('app.scriptManager.untitled')
                  : library.detail.value?.name
              }}</strong>
              <SecLabTag v-if="library.detail.value && !isEditing" type="info">
                {{ t('app.scriptManager.revision', { revision: library.detail.value.revision }) }}
              </SecLabTag>
            </div>
            <div class="editor-actions">
              <template v-if="isEditing">
                <SecLabButton @click="cancelEdit">{{ t('common.cancel') }}</SecLabButton>
                <SecLabButton
                  type="primary"
                  :loading="library.isActionPending('save')"
                  @click="save"
                >
                  {{ t('common.save') }}
                </SecLabButton>
              </template>
              <template v-else-if="library.detail.value">
                <SecLabButton v-if="library.detail.value.capabilities.canRun" @click="openRun">
                  <SecLabIcon name="play" :size="14" />{{ t('app.scriptManager.actions.run') }}
                </SecLabButton>
                <SecLabButton v-if="library.detail.value.capabilities.canClone" @click="startClone">
                  {{ t('app.scriptManager.actions.clone') }}
                </SecLabButton>
                <SecLabButton v-if="library.detail.value.capabilities.canUpdate" @click="startEdit">
                  {{ t('common.edit') }}
                </SecLabButton>
                <SecLabButton
                  v-if="library.detail.value.capabilities.canRemove"
                  type="danger"
                  :loading="library.isActionPending(`remove:${library.detail.value.scriptId}`)"
                  @click="remove"
                >
                  {{ t('common.delete') }}
                </SecLabButton>
              </template>
            </div>
          </div>

          <div v-if="isEditing" class="metadata-form" data-ui="form">
            <SecLabFormItem
              :label="t('app.scriptManager.fields.name')"
              for="script-library-name"
              required
            >
              <SecLabInput
                id="script-library-name"
                v-model="form.name"
                name="scriptName"
                :maxlength="80"
              />
            </SecLabFormItem>
            <SecLabFormItem
              :label="t('app.scriptManager.fields.timeout')"
              for="script-library-timeout"
              required
            >
              <SecLabInput
                id="script-library-timeout"
                v-model="form.timeoutSeconds"
                name="scriptTimeoutSeconds"
                type="number"
                :min="1"
                :max="86400"
              />
            </SecLabFormItem>
            <SecLabFormItem
              class="description-field"
              :label="t('app.scriptManager.fields.description')"
              for="script-library-description"
            >
              <SecLabInput
                id="script-library-description"
                v-model="form.description"
                name="scriptDescription"
                type="textarea"
                :rows="2"
                :maxlength="500"
              />
            </SecLabFormItem>
          </div>
          <div v-else-if="library.detail.value?.description" class="detail-description">
            {{ library.detail.value.description }}
          </div>

          <div class="editor-shell">
            <label id="script-source-label" for="script-library-source" class="editor-label">
              {{ t('app.scriptManager.fields.source') }}
            </label>
            <MonacoEditor
              id="script-library-source"
              v-model="form.content"
              name="scriptSource"
              language="shell"
              file-path="script.sh"
              :document-key="selectedId || mode"
              :read-only="!isEditing"
              :aria-labelledby="'script-source-label'"
              :minimap="false"
              @save="save"
            />
          </div>
        </template>
        <SecLabEmpty
          v-else-if="library.detailState.value.error"
          :description="library.detailState.value.error"
        />
        <SecLabEmpty v-else :description="t('app.scriptManager.empty.selection')" />
        <SecLabLoading :loading="library.detailState.value.initialLoading" cover />
      </div>
    </div>

    <SecLabDialog
      :visible="runDialogVisible"
      data-ui="run-dialog"
      :title="t('app.scriptManager.runDialog.title')"
      width="480px"
      @close="runDialogVisible = false"
    >
      <div class="dialog-form" data-slot="content">
        <SecLabAlert type="warning" :title="t('app.scriptManager.runDialog.notice')" show-icon />
        <SecLabFormItem :label="t('app.scriptManager.fields.node')" for="script-run-node" required>
          <SecLabSelect
            id="script-run-node"
            v-model="runNodeId"
            name="scriptRunNode"
            :options="onlineNodeOptions"
          />
        </SecLabFormItem>
        <SecLabFormItem
          :label="t('app.scriptManager.fields.timeout')"
          for="script-run-timeout"
          required
        >
          <SecLabInput
            id="script-run-timeout"
            v-model="runTimeoutSeconds"
            name="scriptRunTimeout"
            type="number"
            :min="1"
            :max="86400"
          />
        </SecLabFormItem>
      </div>
      <template #footer>
        <SecLabButton @click="runDialogVisible = false">{{ t('common.cancel') }}</SecLabButton>
        <SecLabButton
          type="primary"
          :disabled="!runNodeId"
          :loading="
            library.detail.value
              ? library.isActionPending(`run:${library.detail.value.scriptId}`)
              : false
          "
          @click="submitRun"
        >
          {{ t('app.scriptManager.actions.run') }}
        </SecLabButton>
      </template>
    </SecLabDialog>

    <SecLabDrawer
      v-model="runsVisible"
      data-ui="runs"
      :title="t('app.scriptManager.runs.title')"
      width="900px"
    >
      <div class="runs-shell" data-slot="content">
        <SecLabTable :data="library.runs.value" :columns="runColumns" row-key="runId" border>
          <template #status="{ row }: { row: ScriptRun }">
            <SecLabTag :type="statusTag(row.status)">{{ statusLabel(row.status) }}</SecLabTag>
            <span v-if="row.phase && !row.finishedAt" class="run-phase">{{ row.phase }}</span>
          </template>
          <template #queuedAt="{ row }: { row: ScriptRun }">{{
            formatTime(row.queuedAt)
          }}</template>
          <template #actions="{ row }: { row: ScriptRun }">
            <div class="run-actions">
              <SecLabButton v-if="row.output.available" size="small" @click="openOutput(row)">
                {{ t('app.scriptManager.runs.output') }}
              </SecLabButton>
              <SecLabButton
                v-if="row.capabilities.canCancel"
                size="small"
                type="danger"
                :loading="library.isActionPending(`cancel:${row.runId}`)"
                @click="cancelRun(row)"
              >
                {{ t('common.cancel') }}
              </SecLabButton>
            </div>
          </template>
          <template #empty
            ><SecLabEmpty :description="t('app.scriptManager.runs.empty')"
          /></template>
        </SecLabTable>
        <SecLabLoading :loading="library.runsState.value.initialLoading" cover />
      </div>
    </SecLabDrawer>

    <SecLabDrawer
      v-model="outputVisible"
      data-ui="run-output"
      :title="t('app.scriptManager.runs.outputTitle')"
      width="760px"
    >
      <div class="output-shell" data-slot="content">
        <SecLabAlert
          v-if="library.output.value?.truncated"
          type="warning"
          :title="t('app.scriptManager.runs.truncated')"
          show-icon
        />
        <SecLabAlert
          v-if="currentTrackedRun?.errorSummary"
          type="error"
          :title="statusLabel(currentTrackedRun.status)"
          :description="currentTrackedRun.errorSummary"
          show-icon
        />
        <pre v-if="outputText">{{ outputText }}</pre>
        <SecLabEmpty
          v-else-if="!library.outputState.value.initialLoading"
          :description="t('app.scriptManager.runs.noOutput')"
        />
        <SecLabLoading :loading="library.outputState.value.initialLoading" cover />
      </div>
    </SecLabDrawer>
  </div>
</template>

<style scoped>
.script-library {
  height: 100%;
  min-height: 0;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
  padding: var(--sdl-space-3);
  overflow: hidden;
  background: var(--sdl-bg-canvas);
}
.toolbar,
.editor-toolbar,
.toolbar-actions,
.editor-actions,
.editor-identity,
.run-actions,
.script-meta {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
}
.toolbar {
  justify-content: space-between;
  flex-wrap: wrap;
}
.search-input {
  width: min(360px, 100%);
}
.workspace {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(240px, 30%) minmax(0, 1fr);
  overflow: hidden;
  border: 1px solid var(--sdl-border-subtle);
  border-radius: var(--sdl-radius-lg);
  background: var(--sdl-bg-panel);
}
.script-list,
.editor-pane,
.runs-shell,
.output-shell {
  position: relative;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.script-list {
  border-right: 1px solid var(--sdl-border-subtle);
}
.list-scroll {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: var(--sdl-space-2);
}
.script-row {
  display: flex;
  align-items: center;
  border-radius: var(--sdl-radius-md);
  border: 1px solid transparent;
}
.script-row:hover,
.script-row.selected {
  background: var(--sdl-bg-hover);
  border-color: var(--sdl-border-subtle);
}
.script-select {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: var(--sdl-space-1);
  padding: var(--sdl-space-3);
  border: 0;
  color: inherit;
  background: transparent;
  text-align: left;
  cursor: pointer;
}
.script-select:focus-visible {
  outline: 2px solid var(--sdl-focus-ring);
  outline-offset: -2px;
}
.script-name {
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  font-weight: 600;
}
.script-description {
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--sdl-text-secondary);
  font-size: var(--sdl-font-body-sm);
}
.script-meta {
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-caption);
}
.editor-pane {
  overflow: hidden;
}
.editor-toolbar {
  min-height: 48px;
  justify-content: space-between;
  padding: var(--sdl-space-2) var(--sdl-space-3);
  border-bottom: 1px solid var(--sdl-border-subtle);
}
.metadata-form {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 180px;
  gap: var(--sdl-space-3);
  padding: var(--sdl-space-3);
  border-bottom: 1px solid var(--sdl-border-subtle);
}
.description-field {
  grid-column: 1 / -1;
}
.detail-description {
  padding: var(--sdl-space-3);
  color: var(--sdl-text-secondary);
  border-bottom: 1px solid var(--sdl-border-subtle);
}
.editor-shell {
  flex: 1;
  min-height: 260px;
  display: flex;
  flex-direction: column;
}
.editor-label {
  padding: var(--sdl-space-2) var(--sdl-space-3);
  color: var(--sdl-text-secondary);
  font-size: var(--sdl-font-body-sm);
}
.editor-shell :deep(.monaco-editor-wrapper) {
  flex: 1;
  min-height: 0;
}
.dialog-form {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
}
.runs-shell {
  min-height: 420px;
}
.run-actions {
  justify-content: flex-end;
}
.run-phase {
  margin-left: var(--sdl-space-2);
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-caption);
}
.output-shell {
  gap: var(--sdl-space-3);
}
.output-shell pre {
  min-height: 320px;
  max-height: calc(100vh - 220px);
  margin: 0;
  padding: var(--sdl-space-3);
  overflow: auto;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  color: var(--sdl-text-primary);
  background: var(--sdl-bg-base);
  border: 1px solid var(--sdl-border-subtle);
  border-radius: var(--sdl-radius-md);
  font-family: var(--sdl-font-mono);
}
@media (max-width: 760px) {
  .workspace {
    grid-template-columns: 1fr;
    grid-template-rows: minmax(180px, 34%) minmax(0, 1fr);
  }
  .script-list {
    border-right: 0;
    border-bottom: 1px solid var(--sdl-border-subtle);
  }
  .metadata-form {
    grid-template-columns: 1fr;
  }
  .description-field {
    grid-column: auto;
  }
  .editor-toolbar {
    align-items: flex-start;
    flex-direction: column;
  }
  .editor-actions {
    width: 100%;
    overflow-x: auto;
  }
}
</style>
