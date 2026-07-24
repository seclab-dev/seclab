<script setup lang="ts">
/**
 * @file FileEditorView.vue
 * @description 固定节点上的可靠多标签文件编辑器。
 */
import { computed, nextTick, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import MonacoEditor from '@/components/editor/MonacoEditor.vue'
import { useFileEditor, type EditorTab } from '@/composables/useFileEditor'
import { useWindowManagerStore } from '@/stores/window-manager'
import { useNodeStore } from '@/stores/node'
import { SecLabButton, SecLabTag, SecLabSwitch, SecLabDialog } from '@/components/ui'
import SecLabIcon from '@/components/icons/SecLabIcon.vue'

const props = defineProps<{
  windowId?: string
  payload?: {
    path?: string
    nodeId?: string
  }
}>()

const { t } = useI18n()
const windowStore = useWindowManagerStore()
const nodeStore = useNodeStore()
const targetNodeId = ref(props.payload?.nodeId || nodeStore.currentNodeId)
const { loadFileContent, saveFileContent } = useFileEditor(targetNodeId)

const tabs = ref<EditorTab[]>([])
const activePath = ref('')
const isWrap = ref(false)
const fontSize = ref(14)
const editorRef = ref<{ disposeDocument: (documentKey: string) => void } | null>(null)

const confirmDialogVisible = ref(false)
const pendingPath = ref('')
const confirmationKind = ref<'close' | 'reload'>('close')

const activeTab = computed(() => tabs.value.find((t) => t.path === activePath.value))
const dirtyTabs = computed(() => tabs.value.filter((tab) => tab.isDirty))
const editorScopeId = computed(() => props.windowId || 'default')
const editorPanelId = computed(() => `file-editor-${editorScopeId.value}-panel`)
const editorInputId = computed(() => `file-editor-${editorScopeId.value}-content`)
const confirmationTitle = computed(() =>
  confirmationKind.value === 'close'
    ? t('app.fileEditor.closeUnsavedTitle')
    : t('app.fileEditor.reloadUnsavedTitle'),
)
const confirmationMessage = computed(() =>
  confirmationKind.value === 'close'
    ? t('app.fileEditor.closeUnsavedMessage')
    : t('app.fileEditor.reloadUnsavedMessage'),
)
const confirmationLabel = computed(() =>
  confirmationKind.value === 'close'
    ? t('app.fileEditor.closeDirectly')
    : t('app.fileEditor.reloadDirectly'),
)

const tabDomId = (tab: EditorTab) =>
  `file-editor-${editorScopeId.value}-tab-${Math.max(0, tabs.value.indexOf(tab))}`

const isLoadBusy = (tab: EditorTab) =>
  tab.loadState === 'initialLoading' || tab.loadState === 'refreshing'

const isSaveBusy = (tab: EditorTab) => tab.saveState === 'saving' || tab.saveState === 'reconciling'

const statusType = (tab: EditorTab) => {
  if (
    tab.saveState === 'conflict' ||
    tab.saveState === 'failed' ||
    tab.loadState === 'initialError'
  ) {
    return 'danger' as const
  }
  if (tab.isDirty || tab.loadState === 'stale' || tab.durability === 'uncertain') {
    return 'warning' as const
  }
  if (isLoadBusy(tab) || isSaveBusy(tab)) return 'info' as const
  return 'success' as const
}

const statusLabel = (tab: EditorTab) => {
  if (tab.saveState === 'saving') return t('app.fileEditor.saving')
  if (tab.saveState === 'reconciling') return t('app.fileEditor.reconciling')
  if (tab.saveState === 'conflict') return t('app.fileEditor.conflict')
  if (tab.saveState === 'failed') return t('app.fileEditor.saveFailedShort')
  if (tab.loadState === 'initialLoading') return t('app.fileEditor.loading')
  if (tab.loadState === 'refreshing') return t('app.fileEditor.refreshing')
  if (tab.loadState === 'initialError') return t('app.fileEditor.loadFailedShort')
  if (tab.loadState === 'stale') return t('app.fileEditor.stale')
  if (tab.durability === 'uncertain') return t('app.fileEditor.durabilityWarning')
  return tab.isDirty ? t('app.fileEditor.unsaved') : t('app.fileEditor.saved')
}

const openTab = async (path: string) => {
  if (!path) return
  const existingTab = tabs.value.find((t) => t.path === path)
  if (existingTab) {
    activePath.value = path
    return
  }

  const name = path.split('/').filter(Boolean).pop() || path
  const newTab: EditorTab = {
    path,
    name,
    documentKey: `${targetNodeId.value}:${path}`,
    content: '',
    originalContent: '',
    revision: '',
    isDirty: false,
    fileSize: 0,
    loadState: 'idle',
    saveState: 'idle',
    loadSequence: 0,
    saveSequence: 0,
  }
  tabs.value.push(newTab)
  activePath.value = path

  const reactiveTab = tabs.value.find((tab) => tab.path === path)
  if (reactiveTab) {
    await loadFileContent(reactiveTab)
  }
}

const closeTab = (path: string) => {
  const tab = tabs.value.find((t) => t.path === path)
  if (!tab) return

  if (tab.isDirty) {
    pendingPath.value = path
    confirmationKind.value = 'close'
    confirmDialogVisible.value = true
    return
  }

  performClose(path)
}

const performClose = (path: string) => {
  const index = tabs.value.findIndex((t) => t.path === path)
  if (index === -1) return

  editorRef.value?.disposeDocument(tabs.value[index].documentKey)
  tabs.value.splice(index, 1)

  if (activePath.value === path) {
    if (tabs.value.length > 0) {
      activePath.value = tabs.value[Math.max(0, index - 1)].path
    } else {
      activePath.value = ''
    }
  }
}

const handleConfirm = async () => {
  const path = pendingPath.value
  const kind = confirmationKind.value
  confirmDialogVisible.value = false
  pendingPath.value = ''
  if (!path) return
  if (kind === 'close') {
    performClose(path)
    return
  }
  const tab = tabs.value.find((item) => item.path === path)
  if (tab) await loadFileContent(tab, { discardLocalChanges: true })
}

const handleCancelConfirmation = () => {
  confirmDialogVisible.value = false
  pendingPath.value = ''
}

const handleContentChange = (content: string, documentKey: string) => {
  const tab = tabs.value.find((item) => item.documentKey === documentKey)
  if (!tab) return
  tab.content = content
  tab.isDirty = tab.content !== tab.originalContent
  tab.durability = undefined
  if (tab.saveState === 'failed') {
    tab.saveState = 'idle'
    tab.saveError = undefined
  }
}

const handleSave = async (tab?: EditorTab) => {
  const targetTab = tab || activeTab.value
  if (!targetTab) return
  await saveFileContent(targetTab)
}

const handleSaveByDocumentKey = async (documentKey: string) => {
  const tab = tabs.value.find((item) => item.documentKey === documentKey)
  if (tab) await handleSave(tab)
}

const reloadTab = async (tab?: EditorTab) => {
  const targetTab = tab || activeTab.value
  if (!targetTab || isSaveBusy(targetTab)) return
  if (targetTab.isDirty) {
    pendingPath.value = targetTab.path
    confirmationKind.value = 'reload'
    confirmDialogVisible.value = true
    return
  }
  await loadFileContent(targetTab, { discardLocalChanges: true })
}

const adjustFont = (delta: number) => {
  fontSize.value = Math.min(24, Math.max(10, fontSize.value + delta))
}

const handleTabKeydown = async (event: KeyboardEvent, tab: EditorTab) => {
  const currentIndex = tabs.value.indexOf(tab)
  if (currentIndex < 0) return
  let nextIndex = currentIndex
  if (event.key === 'ArrowLeft')
    nextIndex = (currentIndex - 1 + tabs.value.length) % tabs.value.length
  else if (event.key === 'ArrowRight') nextIndex = (currentIndex + 1) % tabs.value.length
  else if (event.key === 'Home') nextIndex = 0
  else if (event.key === 'End') nextIndex = tabs.value.length - 1
  else return
  event.preventDefault()
  activePath.value = tabs.value[nextIndex].path
  await nextTick()
  document.getElementById(tabDomId(tabs.value[nextIndex]))?.focus()
}

watch(
  () => props.payload?.path,
  (newPath) => {
    if (newPath) {
      void openTab(newPath)
    }
  },
  { immediate: true },
)

watch(
  dirtyTabs,
  (items) => {
    if (!props.windowId) return
    windowStore.updateWindowRuntimeState(props.windowId, {
      dirty: items.length > 0,
      allowsNodeSwitch: false,
      blockLevel: items.length > 0 ? 'dirty' : 'open',
      blockReason:
        items.length > 0
          ? t('app.fileEditor.guardDirty', { count: items.length })
          : t('app.fileEditor.guardOpen'),
    })
  },
  { immediate: true },
)
</script>

<template>
  <div
    class="file-editor-app"
    data-page="file-editor"
    data-seclab-app="file-editor"
    :data-node-id="targetNodeId"
  >
    <!-- Tab 栏 -->
    <div
      v-if="tabs.length > 0"
      class="tabs-header"
      data-ui="editor-tabs"
      data-slot="tabs"
      role="tablist"
      :aria-label="t('app.fileEditor.openFiles')"
    >
      <div
        v-for="tab in tabs"
        :key="tab.path"
        class="editor-tab"
        :class="{ 'is-active': activePath === tab.path }"
      >
        <button
          :id="tabDomId(tab)"
          type="button"
          class="tab-select"
          role="tab"
          :aria-selected="activePath === tab.path"
          :aria-controls="editorPanelId"
          :tabindex="activePath === tab.path ? 0 : -1"
          @click="activePath = tab.path"
          @keydown="handleTabKeydown($event, tab)"
        >
          <span class="tab-status" :class="{ 'is-dirty': tab.isDirty }" aria-hidden="true"></span>
          <span class="tab-name" :title="tab.path">{{ tab.name }}</span>
        </button>
        <button
          type="button"
          class="tab-close"
          :aria-label="t('app.fileEditor.closeTab', { name: tab.name })"
          @click.stop="closeTab(tab.path)"
        >
          <SecLabIcon name="error" :size="14" />
        </button>
      </div>
    </div>

    <!-- 工具栏 -->
    <div class="editor-toolbar" data-ui="editor-toolbar" data-slot="toolbar">
      <div class="toolbar-left" data-slot="path">
        <span v-if="activeTab" class="current-path">{{ activeTab.path }}</span>
      </div>
      <div class="toolbar-right" data-slot="actions">
        <SecLabTag
          v-if="activeTab"
          :type="statusType(activeTab)"
          :title="activeTab.refreshWarning || activeTab.saveError"
        >
          {{ statusLabel(activeTab) }}
        </SecLabTag>
        <SecLabSwitch
          v-model="isWrap"
          :active-text="isWrap ? t('app.fileEditor.wrapOn') : t('app.fileEditor.wrapOff')"
        />
        <SecLabButton size="small" @click="adjustFont(1)">A+</SecLabButton>
        <SecLabButton size="small" @click="adjustFont(-1)">A-</SecLabButton>
        <SecLabButton
          size="small"
          data-ui="editor-reload"
          :disabled="!activeTab || isLoadBusy(activeTab) || isSaveBusy(activeTab)"
          @click="reloadTab()"
        >
          {{ t('app.fileEditor.reload') }}
        </SecLabButton>
        <SecLabButton
          type="primary"
          size="small"
          data-ui="editor-save"
          :disabled="
            !activeTab ||
            !activeTab.isDirty ||
            isLoadBusy(activeTab) ||
            isSaveBusy(activeTab) ||
            activeTab.saveState === 'conflict' ||
            activeTab.capabilities?.canWrite === false
          "
          @click="handleSave()"
        >
          {{ t('app.fileEditor.save') }}
        </SecLabButton>
      </div>
    </div>

    <!-- 编辑器主体 -->
    <div
      :id="editorPanelId"
      class="editor-body"
      data-ui="editor-area"
      data-slot="content"
      role="tabpanel"
      :aria-labelledby="activeTab ? tabDomId(activeTab) : undefined"
    >
      <template v-if="activeTab">
        <div v-if="activeTab.loadState === 'initialLoading'" class="state-overlay">
          {{ t('app.fileEditor.loading') }}
        </div>
        <div v-else-if="activeTab.loadState === 'initialError'" class="state-overlay error">
          {{ activeTab.loadError }}
        </div>
        <MonacoEditor
          v-else
          ref="editorRef"
          :model-value="activeTab.content"
          :document-key="activeTab.documentKey"
          :file-path="activeTab.path"
          :read-only="activeTab.capabilities?.canWrite === false"
          :word-wrap="isWrap"
          :font-size="fontSize"
          :id="editorInputId"
          name="fileContent"
          :aria-label="t('app.fileEditor.editorLabel', { path: activeTab.path })"
          @change="handleContentChange"
          @save="handleSaveByDocumentKey"
        />
      </template>
      <div v-else class="empty-state">
        <SecLabIcon name="file" :size="48" class="empty-icon" />
        <p>{{ t('app.fileEditor.selectFile') }}</p>
      </div>
    </div>

    <!-- 关闭确认对话框 -->
    <SecLabDialog
      :visible="confirmDialogVisible"
      :title="confirmationTitle"
      data-ui="editor-confirmation"
      @close="handleCancelConfirmation"
    >
      <div class="confirmation-message" data-slot="message">{{ confirmationMessage }}</div>
      <div class="confirmation-actions" data-slot="actions">
        <SecLabButton @click="handleCancelConfirmation">{{
          t('confirmation.cancel')
        }}</SecLabButton>
        <SecLabButton
          :type="confirmationKind === 'close' ? 'danger' : 'primary'"
          @click="handleConfirm"
        >
          {{ confirmationLabel }}
        </SecLabButton>
      </div>
    </SecLabDialog>
  </div>
</template>

<style scoped>
.file-editor-app {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  background: var(--sdl-bg-base);
  overflow: hidden;
}

.tabs-header {
  display: flex;
  background: var(--sdl-bg-panel);
  border-bottom: 1px solid var(--sdl-border-subtle);
  overflow-x: auto;
  overflow-y: hidden;
  height: 40px;
  flex-shrink: 0;
}

.tabs-header::-webkit-scrollbar {
  height: 0;
}

.editor-tab {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
  padding: 0 var(--sdl-space-3);
  height: 100%;
  min-width: 120px;
  max-width: 200px;
  background: var(--sdl-bg-muted);
  border-right: 1px solid var(--sdl-border-subtle);
  cursor: pointer;
  user-select: none;
  color: var(--sdl-text-secondary);
  font-size: var(--sdl-font-body-sm);
  transition: all 0.2s ease;
}

.tab-select {
  all: unset;
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
  flex: 1;
  min-width: 0;
  height: 100%;
  cursor: pointer;
}

.tab-select:focus-visible,
.tab-close:focus-visible {
  outline: 2px solid var(--sdl-primary);
  outline-offset: -2px;
}

.editor-tab:hover {
  background: var(--sdl-bg-hover);
  color: var(--sdl-text-primary);
}

.editor-tab.is-active {
  background: var(--sdl-bg-canvas);
  color: var(--sdl-text-primary);
  border-bottom: 2px solid var(--sdl-primary);
}

.tab-status {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: transparent;
  flex-shrink: 0;
}

.tab-status.is-dirty {
  background: var(--sdl-warning);
}

.tab-name {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  flex: 1;
}

.tab-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border-radius: var(--sdl-radius-sm);
  color: var(--sdl-text-muted);
  padding: 0;
  border: 0;
  background: transparent;
  cursor: pointer;
  opacity: 0;
  transition:
    opacity 0.2s,
    background 0.2s;
}

.editor-tab:hover .tab-close,
.editor-tab.is-active .tab-close {
  opacity: 1;
}

.tab-close:hover {
  background: var(--sdl-bg-input);
  color: var(--sdl-text-primary);
}

.empty-tabs-placeholder {
  display: flex;
  align-items: center;
  padding: 0 var(--sdl-space-4);
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-body-sm);
}

.editor-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--sdl-space-2) var(--sdl-space-3);
  background: var(--sdl-bg-panel);
  border-bottom: 1px solid var(--sdl-border-subtle);
  flex-shrink: 0;
}

.toolbar-left {
  display: flex;
  align-items: center;
  overflow: hidden;
}

.current-path {
  font-size: var(--sdl-font-caption);
  color: var(--sdl-text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  text-align: left;
}

.toolbar-right {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
  flex-shrink: 0;
}

.editor-body {
  flex: 1;
  position: relative;
  background: var(--sdl-bg-canvas);
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.state-overlay {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--sdl-bg-canvas);
  color: var(--sdl-text-muted);
  font-size: var(--sdl-font-body);
  z-index: 10;
}

.state-overlay.error {
  color: var(--sdl-danger);
}

.empty-state {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--sdl-space-4);
  color: var(--sdl-text-muted);
}

.empty-icon {
  opacity: 0.5;
}

.confirmation-message {
  padding: var(--sdl-space-5) 0;
}

.confirmation-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--sdl-space-3);
  margin-top: var(--sdl-space-5);
}
</style>
