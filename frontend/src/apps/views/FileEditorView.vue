<script setup lang="ts">
/**
 * @file FileEditorView.vue
 * @description 单窗口多标签文件编辑器。集成 Monaco Editor，支持多文件同时编辑、快捷键保存、脏标记提示等。
 */
import { computed, ref, watch } from 'vue'
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

const confirmDialogVisible = ref(false)
const tabToClose = ref<string>('')

const activeTab = computed(() => tabs.value.find((t) => t.path === activePath.value))
const dirtyTabs = computed(() => tabs.value.filter((tab) => tab.isDirty))

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
    content: '',
    originalContent: '',
    revision: '',
    isDirty: false,
    fileSize: 0,
    isLoaded: false,
    isLoading: true,
    isSaving: false,
    requestSequence: 0,
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
    tabToClose.value = path
    confirmDialogVisible.value = true
    return
  }

  performClose(path)
}

const performClose = (path: string) => {
  const index = tabs.value.findIndex((t) => t.path === path)
  if (index === -1) return

  tabs.value.splice(index, 1)

  if (activePath.value === path) {
    if (tabs.value.length > 0) {
      activePath.value = tabs.value[Math.max(0, index - 1)].path
    } else {
      activePath.value = ''
    }
  }
}

const handleConfirmClose = () => {
  if (tabToClose.value) {
    performClose(tabToClose.value)
  }
  confirmDialogVisible.value = false
  tabToClose.value = ''
}

const handleCancelClose = () => {
  confirmDialogVisible.value = false
  tabToClose.value = ''
}

const handleContentChange = (tab: EditorTab) => {
  tab.isDirty = tab.content !== tab.originalContent
}

const handleSave = async (tab?: EditorTab) => {
  const targetTab = tab || activeTab.value
  if (!targetTab) return
  await saveFileContent(targetTab)
}

const reloadTab = async (tab?: EditorTab) => {
  const targetTab = tab || activeTab.value
  if (!targetTab) return
  await loadFileContent(targetTab)
}

const adjustFont = (delta: number) => {
  fontSize.value = Math.min(24, Math.max(10, fontSize.value + delta))
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
    <div class="tabs-header" v-if="tabs.length > 0" data-ui="editor-tabs">
      <div
        v-for="tab in tabs"
        :key="tab.path"
        class="editor-tab"
        :class="{ 'is-active': activePath === tab.path }"
        @click="activePath = tab.path"
      >
        <span class="tab-status" :class="{ 'is-dirty': tab.isDirty }"></span>
        <span class="tab-name" :title="tab.path">{{ tab.name }}</span>
        <span class="tab-close" @click.stop="closeTab(tab.path)">
          <SecLabIcon name="error" :size="14" />
        </span>
      </div>
    </div>

    <!-- 工具栏 -->
    <div class="editor-toolbar" data-ui="editor-toolbar">
      <div class="toolbar-left">
        <span v-if="activeTab" class="current-path">{{ activeTab.path }}</span>
      </div>
      <div class="toolbar-right">
        <SecLabTag v-if="activeTab" :type="activeTab.isDirty ? 'warning' : 'success'">
          {{ activeTab.isDirty ? t('app.fileEditor.unsaved') : t('app.fileEditor.saved') }}
        </SecLabTag>
        <SecLabSwitch
          v-model="isWrap"
          :active-text="isWrap ? t('app.fileEditor.wrapOn') : t('app.fileEditor.wrapOff')"
        />
        <SecLabButton size="small" @click="adjustFont(1)">A+</SecLabButton>
        <SecLabButton size="small" @click="adjustFont(-1)">A-</SecLabButton>
        <SecLabButton
          size="small"
          :disabled="!activeTab || activeTab.isLoading"
          @click="() => reloadTab()"
        >
          {{ t('app.fileEditor.reload') }}
        </SecLabButton>
        <SecLabButton
          type="primary"
          size="small"
          :disabled="!activeTab || !activeTab.isDirty || activeTab.isLoading || activeTab.isSaving"
          @click="() => handleSave()"
        >
          {{ t('app.fileEditor.save') }}
        </SecLabButton>
      </div>
    </div>

    <!-- 编辑器主体 -->
    <div class="editor-body" data-ui="editor-area">
      <template v-if="activeTab">
        <div v-if="activeTab.isLoading" class="state-overlay">
          {{ t('app.fileEditor.loading') }}
        </div>
        <div v-else-if="activeTab.error" class="state-overlay error">
          {{ activeTab.error }}
        </div>
        <div v-else-if="!activeTab.isLoaded" class="state-overlay">
          {{ t('app.fileEditor.previewUnsupported') }}
        </div>
        <MonacoEditor
          v-else
          v-model="activeTab.content"
          :file-path="activeTab.path"
          :file-size="activeTab.fileSize"
          :word-wrap="isWrap"
          :font-size="fontSize"
          @change="handleContentChange(activeTab)"
          @save="handleSave(activeTab)"
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
      :title="t('app.fileEditor.closeUnsavedTitle')"
      @close="handleCancelClose"
    >
      <div style="padding: 20px 0">{{ t('app.fileEditor.closeUnsavedMessage') }}</div>
      <div style="display: flex; justify-content: flex-end; gap: 12px; margin-top: 20px">
        <SecLabButton @click="handleCancelClose">{{ t('confirmation.cancel') }}</SecLabButton>
        <SecLabButton type="danger" @click="handleConfirmClose">
          {{ t('app.fileEditor.closeDirectly') }}
        </SecLabButton>
      </div>
    </SecLabDialog>
  </div>
</template>

<style scoped>
.file-editor-app {
  height: 100%;
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
  direction: rtl;
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
</style>
