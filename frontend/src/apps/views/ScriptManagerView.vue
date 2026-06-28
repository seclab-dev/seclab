<script setup lang="ts">
import { ref, onMounted, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { scriptsApi, type ScriptItem } from '@/api/modules/scripts'
import { useNodeStore } from '@/stores/node'
import { useWindowManagerStore } from '@/stores/window-manager'
import { useConfirmationModalStore } from '@/stores/confirmation-modal'
import { useNotificationStore } from '@/stores/notification'
import SecLabIcon from '@/components/icons/SecLabIcon.vue'
import SecLabSelect from '@/components/ui/SecLabSelect.vue'

const props = defineProps<{
  windowId?: string
}>()

const { t, te } = useI18n()
const nodeStore = useNodeStore()
const windowStore = useWindowManagerStore()
const confirmStore = useConfirmationModalStore()
const notificationStore = useNotificationStore()

const isBuiltIn = (script: ScriptItem | null) => {
  if (!script) return false
  const uuidRegex = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
  return !uuidRegex.test(script.scriptId)
}

const sortScriptList = (list: ScriptItem[]): ScriptItem[] => {
  return [...list].sort((a, b) => {
    const aBuiltIn = isBuiltIn(a)
    const bBuiltIn = isBuiltIn(b)
    if (aBuiltIn && !bBuiltIn) return -1
    if (!aBuiltIn && bBuiltIn) return 1
    return 0
  })
}

const getScriptTitle = (s: ScriptItem) => {
  return te(s.title) ? t(s.title) : s.title
}

const getScriptDesc = (s: ScriptItem) => {
  if (!s.description) return ''
  return te(s.description) ? t(s.description) : s.description
}

const cloneScript = () => {
  if (!selectedScript.value) return
  const original = selectedScript.value
  isEditing.value = true
  isCodeCollapsed.value = false
  selectedScript.value = null
  editForm.value = {
    title: `${getScriptTitle(original)} - Copy`,
    description: getScriptDesc(original),
    content: original.content,
  }
}

// 状态定义
const isLoading = ref(false)
const isCodeCollapsed = ref(true)
const isCopied = ref(false)
const searchQuery = ref('')
const scripts = ref<ScriptItem[]>([])
const selectedScript = ref<ScriptItem | null>(null)

// 节点选择与执行状态
const targetNodeId = ref(nodeStore.currentNodeId || 'local')
const isExecuting = ref(false)
const executeResult = ref<{
  exitCode: number
  stdout: string
  stderr: string
  timedOut: boolean
  startedAt: string
  finishedAt: string
} | null>(null)

// 编辑脚本状态
const isEditing = ref(false)
const editForm = ref({
  title: '',
  description: '',
  content: '',
})

// 控制台输出整合展示
const consoleLogs = ref<string[]>([])

// 获取脚本列表
const loadScripts = async () => {
  isLoading.value = true
  try {
    const res = await scriptsApi.fetchScripts()
    if (res.data) {
      scripts.value = sortScriptList(res.data)
      if (scripts.value.length > 0 && !selectedScript.value) {
        selectScript(scripts.value[0])
      }
    }
  } catch (err) {
    console.error('Failed to load scripts:', err)
  } finally {
    isLoading.value = false
  }
}

// 过滤脚本
const filteredScripts = computed(() => {
  if (!searchQuery.value.trim()) return scripts.value
  const query = searchQuery.value.toLowerCase()
  return scripts.value.filter(
    (s) =>
      s.title.toLowerCase().includes(query) ||
      (s.description && s.description.toLowerCase().includes(query)),
  )
})

const nodeOptions = computed(() => {
  return nodeStore.nodes.map((node) => ({
    value: node.id,
    label: `${node.name} (${node.address}) - ${
      node.status === 'online'
        ? t('app.scriptManager.nodeOnline')
        : t('app.scriptManager.nodeOffline')
    }`,
    disabled: node.status !== 'online',
  }))
})

// 选中脚本
const selectScript = (script: ScriptItem) => {
  selectedScript.value = script
  isEditing.value = false
  isCodeCollapsed.value = true
  executeResult.value = null
  consoleLogs.value = []
  editForm.value = {
    title: script.title,
    description: script.description || '',
    content: script.content,
  }
}

// 切换到创建模式
const enterCreateMode = () => {
  selectedScript.value = null
  isEditing.value = true
  isCodeCollapsed.value = false
  executeResult.value = null
  consoleLogs.value = []
  editForm.value = {
    title: t('app.scriptManager.newScript'),
    description: '',
    content: '#!/bin/bash\n\necho "Hello, SecLab!"\n',
  }
}

// 保存脚本
const saveScript = async () => {
  if (!editForm.value.title.trim())
    return notificationStore.warning(t('app.scriptManager.messages.titleRequired'))
  if (!editForm.value.content.trim())
    return notificationStore.warning(t('app.scriptManager.messages.contentRequired'))

  try {
    if (selectedScript.value) {
      // 更新
      const res = await scriptsApi.updateScript(selectedScript.value.scriptId, {
        title: editForm.value.title,
        description: editForm.value.description,
        content: editForm.value.content,
      })
      if (res.data) {
        // 局部更新
        const index = scripts.value.findIndex((s) => s.scriptId === selectedScript.value?.scriptId)
        if (index !== -1) {
          scripts.value[index] = res.data
        }
        scripts.value = sortScriptList(scripts.value)
        selectedScript.value = res.data
        isEditing.value = false
      }
    } else {
      // 创建
      const res = await scriptsApi.createScript({
        title: editForm.value.title,
        description: editForm.value.description,
        content: editForm.value.content,
      })
      if (res.data) {
        scripts.value.unshift(res.data)
        scripts.value = sortScriptList(scripts.value)
        selectScript(res.data)
      }
    }
  } catch (err) {
    console.error('Failed to save script:', err)
    notificationStore.error(t('app.scriptManager.messages.saveFailed'))
  }
}

// 删除脚本
const deleteSelectedScript = async () => {
  if (!selectedScript.value) return
  const confirmMsg = t('app.scriptManager.delete.confirmMessage', {
    name: getScriptTitle(selectedScript.value),
  })
  const confirmed = await confirmStore.showConfirmation(
    confirmMsg,
    t('app.scriptManager.delete.title'),
    t('app.scriptManager.delete.btn'),
    t('app.scriptManager.cancel'),
  )
  if (!confirmed) return

  try {
    await scriptsApi.deleteScript(selectedScript.value.scriptId)
    const delId = selectedScript.value.scriptId
    scripts.value = scripts.value.filter((s) => s.scriptId !== delId)
    if (scripts.value.length > 0) {
      selectScript(scripts.value[0])
    } else {
      selectedScript.value = null
      isEditing.value = false
    }
  } catch (err) {
    console.error('Failed to delete script:', err)
    notificationStore.error(t('app.scriptManager.messages.deleteFailed'))
  }
}

// 在选择的节点上执行脚本
const runScript = async () => {
  if (!selectedScript.value) return

  const confirmMsg = t('app.scriptManager.execute.confirmMessage', {
    name: getScriptTitle(selectedScript.value),
  })
  const confirmed = await confirmStore.showConfirmation(
    confirmMsg,
    t('app.scriptManager.execute.title'),
    t('app.scriptManager.execute.btn'),
    t('app.scriptManager.cancel'),
  )
  if (!confirmed) return

  const operationId = `script-execution:${targetNodeId.value}:${Date.now()}`
  isExecuting.value = true
  windowStore.registerGlobalOperation({
    operationId,
    nodeId: targetNodeId.value,
    sourceAppId: 'script-manager',
    title: selectedScript.value.title,
    cancellable: false,
    reason: t('app.scriptManager.operationBusy', { title: selectedScript.value.title }),
  })
  consoleLogs.value = [
    t('app.scriptManager.console.initChannel'),
    t('app.scriptManager.console.delivering'),
  ]
  executeResult.value = null

  try {
    const res = await scriptsApi.executeScript(targetNodeId.value, selectedScript.value.content)
    if (res.data) {
      const payload = res.data
      executeResult.value = {
        exitCode: payload.exitCode,
        stdout: payload.stdout,
        stderr: payload.stderr,
        timedOut: payload.timedOut,
        startedAt: new Date(payload.startedAt * 1000).toLocaleString(),
        finishedAt: new Date(payload.finishedAt * 1000).toLocaleString(),
      }

      consoleLogs.value.push(t('app.scriptManager.console.execDone'))

      if (payload.stdout) {
        const lines = payload.stdout.split('\n')
        lines.forEach((l) => consoleLogs.value.push(`[STDOUT] ${l}`))
      }
      if (payload.stderr) {
        const lines = payload.stderr.split('\n')
        lines.forEach((l) => consoleLogs.value.push(`[STDERR] ${l}`))
      }

      if (payload.timedOut) {
        consoleLogs.value.push(t('app.scriptManager.console.timeout'))
      } else {
        consoleLogs.value.push(
          t('app.scriptManager.console.exitCode', { code: String(payload.exitCode) }),
        )
      }
    }
  } catch (err) {
    console.error('Execution failed:', err)
    const errMsg =
      err instanceof Error ? err.message : t('app.scriptManager.messages.networkTimeout')
    consoleLogs.value.push(t('app.scriptManager.console.commError', { error: errMsg }))
  } finally {
    isExecuting.value = false
    windowStore.finishGlobalOperation(operationId)
  }
}

const copyLogs = () => {
  const text = consoleLogs.value.join('\n')
  navigator.clipboard.writeText(text)
  isCopied.value = true
  setTimeout(() => {
    isCopied.value = false
  }, 1500)
}

const clearLogs = () => {
  consoleLogs.value = []
  executeResult.value = null
}

watch(
  isExecuting,
  (executing) => {
    if (!props.windowId) return
    windowStore.updateWindowRuntimeState(props.windowId, {
      busy: executing,
      allowsNodeSwitch: !executing,
      blockLevel: executing ? 'busy' : undefined,
      blockReason: executing ? t('app.scriptManager.runtimeBusy') : undefined,
    })
  },
  { immediate: true },
)

onMounted(() => {
  loadScripts()
})
</script>

<template>
  <div class="script-manager-app">
    <!-- 头部栏 -->
    <div class="header" data-slot="header">
      <div class="header-left">
        <SecLabIcon class="header-icon" name="code" :size="24" />
        <div class="header-info">
          <h1>{{ t('app.scriptManager.title') }}</h1>
          <p>{{ t('app.scriptManager.subtitle') }}</p>
        </div>
      </div>
      <button class="add-btn-gradient" @click="enterCreateMode">
        <SecLabIcon name="plus" :size="16" />
        <span>{{ t('app.scriptManager.newScriptBtn') }}</span>
      </button>
    </div>

    <!-- 主体区域 -->
    <div class="body" data-slot="content">
      <!-- 左侧栏：脚本列表 -->
      <aside class="sidebar-panel">
        <div class="search-box">
          <SecLabIcon class="search-icon" name="search" :size="16" />
          <input
            id="script-search-input"
            v-model="searchQuery"
            type="text"
            name="search"
            :placeholder="t('app.scriptManager.searchPlaceholder')"
            :aria-label="t('app.scriptManager.searchPlaceholder')"
          />
        </div>

        <div v-if="isLoading" class="loading-state">
          <div class="spinner"></div>
          <span>{{ t('app.scriptManager.loading') }}</span>
        </div>

        <ul class="script-list" v-else>
          <li
            v-for="s in filteredScripts"
            :key="s.scriptId"
            :class="['script-item', { active: selectedScript?.scriptId === s.scriptId }]"
            @click="selectScript(s)"
          >
            <div class="item-icon-wrapper">
              <SecLabIcon name="code" :size="16" />
            </div>
            <div class="item-meta">
              <h3>
                <span class="item-title">{{ getScriptTitle(s) }}</span>
                <span :class="['item-badge', isBuiltIn(s) ? 'badge-builtin' : 'badge-custom']">
                  {{
                    isBuiltIn(s)
                      ? t('app.scriptManager.badge.builtin')
                      : t('app.scriptManager.badge.custom')
                  }}
                </span>
              </h3>
            </div>
          </li>
          <li v-if="filteredScripts.length === 0" class="empty-list">
            {{ t('app.scriptManager.emptySearch') }}
          </li>
        </ul>
      </aside>

      <!-- 右侧栏：详情、编辑器与控制台 -->
      <section class="main-panel">
        <!-- 模式一：未选择且非新建 -->
        <div v-if="!selectedScript && !isEditing" class="empty-detail-placeholder">
          <div class="empty-glow-logo">
            <SecLabIcon name="code" :size="48" />
          </div>
          <h2>{{ t('app.scriptManager.emptyStateTitle') }}</h2>
          <p>{{ t('app.scriptManager.emptyStateDesc') }}</p>
        </div>

        <!-- 模式二：展示或编辑详情 -->
        <div v-else class="detail-container">
          <!-- 上方脚本卡片 -->
          <div class="info-card">
            <div class="info-header">
              <div v-if="!isEditing" class="info-text">
                <h2>{{ selectedScript ? getScriptTitle(selectedScript) : '' }}</h2>
                <p class="description">
                  {{
                    (selectedScript ? getScriptDesc(selectedScript) : '') ||
                    t('app.scriptManager.noDescription')
                  }}
                </p>
              </div>
              <div v-else class="info-edit-fields">
                <input
                  v-model="editForm.title"
                  type="text"
                  class="edit-title-input"
                  :placeholder="t('app.scriptManager.editTitlePlaceholder')"
                  :aria-label="t('app.scriptManager.editTitlePlaceholder')"
                />
                <input
                  v-model="editForm.description"
                  type="text"
                  class="edit-desc-input"
                  :placeholder="t('app.scriptManager.editDescPlaceholder')"
                  :aria-label="t('app.scriptManager.editDescPlaceholder')"
                />
              </div>

              <!-- 右侧动作按钮 -->
              <div class="actions-group">
                <button
                  v-if="!isEditing && !isBuiltIn(selectedScript)"
                  class="btn-outline"
                  @click="isEditing = true"
                >
                  <SecLabIcon name="edit" :size="14" />
                  <span>{{ t('app.scriptManager.editScript') }}</span>
                </button>
                <button
                  v-if="!isEditing && isBuiltIn(selectedScript)"
                  class="btn-outline"
                  @click="cloneScript"
                >
                  <SecLabIcon name="copy" :size="14" />
                  <span>{{ t('app.scriptManager.cloneScript') }}</span>
                </button>
                <button v-if="isEditing" class="btn-success" @click="saveScript">
                  <SecLabIcon name="save" :size="14" />
                  <span>{{ t('app.scriptManager.saveChanges') }}</span>
                </button>
                <button
                  v-if="isEditing && selectedScript"
                  class="btn-outline"
                  @click="isEditing = false"
                >
                  <span>{{ t('app.scriptManager.cancel') }}</span>
                </button>
                <button
                  v-if="selectedScript && !isBuiltIn(selectedScript)"
                  class="btn-danger"
                  @click="deleteSelectedScript"
                >
                  <SecLabIcon name="trash" :size="14" />
                  <span>{{ t('app.scriptManager.deleteBtn') }}</span>
                </button>
              </div>
            </div>

            <!-- 代码编辑区 -->
            <div
              class="code-editor-panel"
              :class="{ 'is-collapsed': isCodeCollapsed && !isEditing }"
            >
              <div class="editor-header">
                <div class="dot-group">
                  <span class="dot red"></span>
                  <span class="dot yellow"></span>
                  <span class="dot green"></span>
                </div>
                <span class="editor-filename">shell-script.sh</span>
                <button
                  v-if="!isEditing"
                  class="collapse-toggle-btn"
                  @click="isCodeCollapsed = !isCodeCollapsed"
                >
                  {{
                    isCodeCollapsed
                      ? t('app.scriptManager.expandCode')
                      : t('app.scriptManager.collapseCode')
                  }}
                </button>
              </div>
              <textarea
                v-if="isEditing"
                v-model="editForm.content"
                class="code-textarea"
                spellcheck="false"
                aria-label="Script content"
              ></textarea>
              <pre v-else class="code-preview"><code>{{ selectedScript?.content }}</code></pre>

              <!-- 折叠渐变遮罩 -->
              <div
                v-if="isCodeCollapsed && !isEditing"
                class="collapsed-overlay"
                @click="isCodeCollapsed = false"
              >
                <span>{{ t('app.scriptManager.expandCode') }}</span>
              </div>
            </div>
          </div>

          <!-- 下方执行控制台 -->
          <div class="execution-card" v-if="selectedScript">
            <div class="exec-header">
              <div class="selector-wrapper">
                <span id="target-node-select-label" class="selector-label">{{
                  t('app.scriptManager.targetNode')
                }}</span>
                <SecLabSelect
                  id="target-node-select"
                  aria-labelledby="target-node-select-label"
                  v-model="targetNodeId"
                  :options="nodeOptions"
                  :disabled="isExecuting"
                  class="node-select"
                />
              </div>

              <div class="exec-buttons">
                <button class="btn-run-glowing" :disabled="isExecuting" @click="runScript">
                  <div v-if="isExecuting" class="mini-spinner"></div>
                  <SecLabIcon v-else name="play" :size="14" />
                  <span>{{
                    isExecuting ? t('app.scriptManager.running') : t('app.scriptManager.runBtn')
                  }}</span>
                </button>
              </div>
            </div>

            <!-- 终端日志输出 -->
            <div class="terminal-log-wrapper">
              <div class="terminal-header">
                <div class="terminal-title">
                  <span class="pulse-dot" :class="{ active: isExecuting }"></span>
                  <span>{{ t('app.scriptManager.terminalTitle') }}</span>
                </div>
                <div class="terminal-tools">
                  <button class="tool-btn" @click="copyLogs" :disabled="consoleLogs.length === 0">
                    {{ isCopied ? t('app.scriptManager.copied') : t('app.scriptManager.copyLogs') }}
                  </button>
                  <button class="tool-btn" @click="clearLogs" :disabled="consoleLogs.length === 0">
                    {{ t('app.scriptManager.clearConsole') }}
                  </button>
                </div>
              </div>

              <div class="terminal-body" data-native-context-menu>
                <div v-if="consoleLogs.length === 0" class="terminal-empty">
                  {{ t('app.scriptManager.terminalReady') }}
                </div>
                <div
                  v-for="(log, idx) in consoleLogs"
                  :key="idx"
                  :class="[
                    'log-line',
                    {
                      'system-log': log.startsWith('[系统') || log.startsWith('[System]'),
                      'stdout-log': log.startsWith('[STDOUT]'),
                      'stderr-log': log.startsWith('[STDERR]'),
                      'error-log':
                        log.startsWith('[错误') ||
                        log.startsWith('[警告') ||
                        log.startsWith('[System]') ||
                        log.startsWith('[Warning]') ||
                        log.startsWith('[Error]'),
                    },
                  ]"
                >
                  {{ log }}
                </div>
              </div>
            </div>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
/* 全局变量与奢华玻璃配色 */
.script-manager-app {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--sdl-bg-canvas);
  color: var(--sdl-text-secondary);
  font-family: var(--sdl-font-family);
  overflow: hidden;
}

/* 头部栏 */
.header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 18px 24px;
  background: var(--sdl-bg-panel);
  border-bottom: 1px solid var(--sdl-border-subtle);
  backdrop-filter: blur(10px);
}

.header-left {
  display: flex;
  align-items: center;
  gap: 14px;
}

.header-icon {
  color: var(--sdl-primary);
  filter: drop-shadow(0 0 8px var(--sdl-primary-active));
}

.header-info h1 {
  font-size: 18px;
  font-weight: 600;
  color: var(--sdl-text-primary);
  margin: 0;
  letter-spacing: 0.5px;
}

.header-info p {
  font-size: 12px;
  color: var(--sdl-text-muted);
  margin: 4px 0 0 0;
}

.add-btn-gradient {
  display: flex;
  align-items: center;
  gap: 8px;
  background: linear-gradient(135deg, var(--sdl-primary) 0%, var(--sdl-primary-hover) 100%);
  border: none;
  border-radius: 6px;
  padding: 8px 16px;
  color: var(--sdl-text-inverse);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  box-shadow: 0 4px 12px rgba(31, 111, 235, 0.25);
  transition: all 0.3s ease;
}

.add-btn-gradient:hover {
  transform: translateY(-1px);
  box-shadow: 0 6px 16px rgba(31, 111, 235, 0.4);
}

/* 主体 */
.body {
  display: flex;
  flex: 1;
  overflow: hidden;
}

/* 左侧栏 */
.sidebar-panel {
  width: 280px;
  border-right: 1px solid var(--sdl-border-subtle);
  background: var(--sdl-bg-canvas);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.search-box {
  display: flex;
  align-items: center;
  padding: 12px;
  margin: 16px;
  background: var(--sdl-bg-input);
  border: 1px solid var(--sdl-border-default);
  border-radius: 6px;
  gap: 10px;
}

.search-icon {
  color: var(--sdl-text-muted);
}

.search-box input {
  background: transparent;
  border: none;
  color: var(--sdl-text-primary);
  font-size: 13px;
  width: 100%;
  outline: none;
}

.loading-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  padding: 40px 0;
  color: #8b949e;
}

.spinner {
  width: 24px;
  height: 24px;
  border: 2px solid rgba(56, 139, 253, 0.2);
  border-top-color: #58a6ff;
  border-radius: 50%;
  animation: spin 1s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.script-list {
  flex: 1;
  overflow-y: auto;
  list-style: none;
  padding: 0 12px 20px 12px;
  margin: 0;
}

.script-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  margin-bottom: 8px;
  border-radius: 8px;
  cursor: pointer;
  background: var(--sdl-bg-panel);
  border: 1px solid var(--sdl-border-subtle);
  transition: all 0.2s ease;
}

.script-item:hover {
  background: var(--sdl-bg-hover);
  border-color: var(--sdl-border-default);
}

.script-item.active {
  background: var(--sdl-bg-active);
  border-color: var(--sdl-border-brand);
  box-shadow: inset 0 0 12px rgba(0, 200, 255, 0.05);
}

.item-icon-wrapper {
  background: var(--sdl-bg-muted);
  border-radius: 6px;
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--sdl-primary);
  border: 1px solid var(--sdl-border-default);
  flex-shrink: 0;
}

.script-item.active .item-icon-wrapper {
  background: var(--sdl-primary);
  border-color: var(--sdl-primary-active);
  color: var(--sdl-text-inverse);
}

.item-meta {
  flex: 1;
  min-width: 0;
}

.item-meta h3 {
  font-size: 12px;
  font-weight: 600;
  color: var(--sdl-text-primary);
  margin: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.item-title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}

.item-badge {
  font-size: 10px;
  font-weight: 500;
  padding: 1px 5px;
  border-radius: 4px;
  line-height: 1.2;
  white-space: nowrap;
}

.badge-builtin {
  background: rgba(0, 168, 204, 0.1);
  color: #00a8cc;
  border: 1px solid rgba(0, 168, 204, 0.2);
}

.badge-custom {
  background: rgba(120, 120, 120, 0.08);
  color: var(--sdl-text-muted);
  border: 1px solid var(--sdl-border-default);
}

.empty-list {
  text-align: center;
  padding: 40px 0;
  font-size: 12px;
  color: var(--sdl-text-muted);
}

/* 右侧面板 */
.main-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: var(--sdl-bg-canvas);
}

.empty-detail-placeholder {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--sdl-text-muted);
  gap: 16px;
}

.empty-glow-logo {
  width: 90px;
  height: 90px;
  background: radial-gradient(circle, var(--sdl-border-brand) 0%, transparent 70%);
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--sdl-primary);
  filter: drop-shadow(0 0 16px var(--sdl-border-brand));
}

.empty-detail-placeholder h2 {
  font-size: 16px;
  color: var(--sdl-text-primary);
  font-weight: 500;
  margin: 0;
}

.empty-detail-placeholder p {
  font-size: 12px;
  margin: 0;
}

.detail-container {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: 20px;
  gap: 20px;
  overflow-y: auto;
}

/* 上方信息卡片 */
.info-card {
  background: var(--sdl-bg-panel);
  border: 1px solid var(--sdl-border-strong);
  border-radius: 8px;
  padding: 20px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.info-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: 20px;
}

.info-text h2 {
  font-size: 16px;
  font-weight: 600;
  color: var(--sdl-text-primary);
  margin: 0;
}

.info-text .description {
  font-size: 12px;
  color: var(--sdl-text-muted);
  margin: 6px 0 0 0;
  line-height: 1.5;
}

.info-edit-fields {
  display: flex;
  flex-direction: column;
  gap: 8px;
  flex: 1;
}

.edit-title-input {
  background: var(--sdl-bg-input);
  border: 1px solid var(--sdl-border-default);
  border-radius: 6px;
  padding: 8px 12px;
  font-size: 14px;
  font-weight: 600;
  color: var(--sdl-text-primary);
  outline: none;
}

.edit-title-input:focus {
  border-color: var(--sdl-primary);
}

.edit-desc-input {
  background: var(--sdl-bg-input);
  border: 1px solid var(--sdl-border-default);
  border-radius: 6px;
  padding: 6px 12px;
  font-size: 12px;
  color: var(--sdl-text-secondary);
  outline: none;
}

.edit-desc-input:focus {
  border-color: var(--sdl-primary);
}

.actions-group {
  display: flex;
  align-items: center;
  gap: 8px;
}

.btn-outline {
  display: flex;
  align-items: center;
  gap: 6px;
  background: rgba(33, 38, 45, 0.8);
  border: 1px solid rgba(240, 246, 250, 0.15);
  border-radius: 6px;
  padding: 6px 14px;
  color: #c9d1d9;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s ease;
}

.btn-outline:hover {
  background: rgba(48, 54, 65, 0.8);
  border-color: rgba(240, 246, 250, 0.3);
}

.btn-success {
  display: flex;
  align-items: center;
  gap: 6px;
  background: #238636;
  border: none;
  border-radius: 6px;
  padding: 6px 14px;
  color: #ffffff;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: background-color 0.2s ease;
}

.btn-success:hover {
  background-color: #2ea043;
}

.btn-danger {
  display: flex;
  align-items: center;
  gap: 6px;
  background: transparent;
  border: 1px solid rgba(248, 81, 73, 0.4);
  border-radius: 6px;
  padding: 6px 14px;
  color: #f85149;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s ease;
}

.btn-danger:hover {
  background: rgba(248, 81, 73, 0.15);
  border-color: #f85149;
}

/* 编辑器框 */
.code-editor-panel {
  background: var(--sdl-bg-base);
  border: 1px solid var(--sdl-border-strong);
  border-radius: 6px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  position: relative;
  transition: all 0.3s ease;
}

.code-editor-panel.is-collapsed {
  max-height: 120px;
}

.code-editor-panel.is-collapsed .code-preview {
  max-height: 80px;
  min-height: 80px;
  overflow: hidden;
  padding-bottom: 30px;
}

.collapse-toggle-btn {
  background: transparent;
  border: 1px solid var(--sdl-border-default);
  border-radius: 4px;
  padding: 2px 8px;
  color: var(--sdl-text-secondary);
  font-size: 11px;
  cursor: pointer;
  transition: all 0.2s ease;
}

.collapse-toggle-btn:hover {
  border-color: var(--sdl-primary);
  color: var(--sdl-primary);
}

.collapsed-overlay {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  height: 50px;
  background: linear-gradient(to bottom, transparent, var(--sdl-bg-base));
  display: flex;
  align-items: flex-end;
  justify-content: center;
  padding-bottom: 8px;
  cursor: pointer;
  color: var(--sdl-primary);
  font-size: 12px;
  font-weight: 500;
  transition: all 0.2s ease;
}

.collapsed-overlay:hover {
  color: var(--sdl-primary-hover);
  background: linear-gradient(to bottom, transparent, rgba(31, 111, 235, 0.15));
}

.editor-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  background: var(--sdl-bg-muted);
  padding: 8px 16px;
  border-bottom: 1px solid var(--sdl-border-subtle);
}

.dot-group {
  display: flex;
  gap: 6px;
}

.dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  display: inline-block;
}

.dot.red {
  background-color: #ff5f56;
}
.dot.yellow {
  background-color: #ffbd2e;
}
.dot.green {
  background-color: #27c93f;
}

.editor-filename {
  font-family: var(--sdl-font-mono);
  font-size: 11px;
  color: var(--sdl-text-muted);
}

.code-textarea {
  background: transparent;
  border: none;
  resize: vertical;
  min-height: 200px;
  padding: 16px;
  color: var(--sdl-text-primary);
  font-family: var(--sdl-font-mono);
  font-size: 13px;
  line-height: 1.5;
  outline: none;
  width: 100%;
  box-sizing: border-box;
}

.code-preview {
  margin: 0;
  padding: 16px;
  min-height: 200px;
  max-height: 400px;
  overflow: auto;
  color: var(--sdl-success);
  font-family: var(--sdl-font-mono);
  font-size: 13px;
  line-height: 1.5;
  white-space: pre-wrap;
  text-align: left;
}

/* 下方执行面板 */
.execution-card {
  background: var(--sdl-bg-panel);
  border: 1px solid var(--sdl-border-strong);
  border-radius: 8px;
  padding: 20px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.exec-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: 16px;
}

.selector-wrapper {
  display: flex;
  align-items: center;
  gap: 10px;
}

.selector-label {
  font-size: 12px;
  font-weight: 500;
  color: var(--sdl-text-muted);
  white-space: nowrap;
}

.node-select {
  min-width: 280px;
}

.btn-run-glowing {
  display: flex;
  align-items: center;
  gap: 8px;
  background: linear-gradient(135deg, var(--sdl-success) 0%, var(--sdl-primary) 100%);
  border: none;
  border-radius: 6px;
  padding: 8px 18px;
  color: #ffffff;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  box-shadow: 0 4px 12px rgba(31, 111, 235, 0.25);
  transition: all 0.3s ease;
}

.btn-run-glowing:hover:not(:disabled) {
  transform: translateY(-1px);
  box-shadow: 0 6px 16px rgba(31, 111, 235, 0.4);
}

.btn-run-glowing:disabled {
  background: var(--sdl-border-strong);
  box-shadow: none;
  cursor: not-allowed;
  color: var(--sdl-text-muted);
}

.mini-spinner {
  width: 14px;
  height: 14px;
  border: 2px solid rgba(255, 255, 255, 0.2);
  border-top-color: #ffffff;
  border-radius: 50%;
  animation: spin 1s linear infinite;
}

/* 控制台本体 */
.terminal-log-wrapper {
  background: var(--sdl-bg-base);
  border: 1px solid var(--sdl-border-strong);
  border-radius: 6px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.terminal-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  background: var(--sdl-bg-muted);
  padding: 10px 16px;
  border-bottom: 1px solid var(--sdl-border-subtle);
}

.terminal-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.5px;
  color: #8b949e;
}

.pulse-dot {
  width: 8px;
  height: 8px;
  background-color: #8b949e;
  border-radius: 50%;
}

.pulse-dot.active {
  background-color: #2ea043;
  animation: pulse 1.2s infinite ease-in-out;
}

@keyframes pulse {
  0% {
    transform: scale(0.9);
    opacity: 0.6;
  }
  50% {
    transform: scale(1.2);
    opacity: 1;
  }
  100% {
    transform: scale(0.9);
    opacity: 0.6;
  }
}

.terminal-tools {
  display: flex;
  gap: 8px;
}

.tool-btn {
  background: rgba(33, 38, 45, 0.7);
  border: 1px solid rgba(240, 246, 250, 0.1);
  border-radius: 4px;
  padding: 4px 10px;
  font-size: 11px;
  color: #c9d1d9;
  cursor: pointer;
  transition: all 0.2s ease;
}

.tool-btn:hover:not(:disabled) {
  background: rgba(48, 54, 65, 0.9);
  border-color: rgba(240, 246, 250, 0.2);
}

.tool-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.terminal-body {
  height: 260px;
  padding: 16px;
  overflow-y: auto;
  font-family: var(--sdl-font-mono);
  font-size: 12px;
  line-height: 1.6;
  background: var(--sdl-bg-canvas);
  display: flex;
  flex-direction: column;
  gap: 4px;
  text-align: left;
}

.terminal-empty {
  color: var(--sdl-text-muted);
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
}

.log-line {
  white-space: pre-wrap;
  word-break: break-all;
}

.system-log {
  color: var(--sdl-primary);
}

.stdout-log {
  color: var(--sdl-text-primary);
}

.stderr-log {
  color: var(--sdl-danger);
}

.error-log {
  color: var(--sdl-danger);
  font-weight: bold;
}
</style>
