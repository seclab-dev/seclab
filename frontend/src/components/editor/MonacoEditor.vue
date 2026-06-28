<script setup lang="ts">
/**
 * @file MonacoEditor.vue
 * @description SecLab 平台 Monaco Editor 封装组件。
 * 提供语法高亮、SDL 主题适配、语言自动检测、大文件只读保护等能力。
 * 严格遵循 SDL 设计规范。
 */

import { onMounted, onUnmounted, ref, watch, computed, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { setupMonacoWorkers, monaco } from './monaco-workers'
import {
  SDL_THEME_DARK,
  SDL_THEME_LIGHT,
  createSdlDarkTheme,
  createSdlLightTheme,
} from './sdl-monaco-theme'

setupMonacoWorkers()

/** 文件大小阈值（10MB），超过此值自动进入只读模式 */
const LARGE_FILE_THRESHOLD = 10 * 1024 * 1024

interface Props {
  /** 编辑器内容（v-model） */
  modelValue?: string
  /** 语言标识（如 'json', 'yaml', 'typescript'），不传时自动检测 */
  language?: string
  /** 文件路径，用于自动推断语言 */
  filePath?: string
  /** 是否只读 */
  readOnly?: boolean
  /** 字号 */
  fontSize?: number
  /** 是否自动换行 */
  wordWrap?: boolean
  /** 是否显示 minimap */
  minimap?: boolean
  /** 原始文件字节数，用于大文件只读保护 */
  fileSize?: number
}

const props = withDefaults(defineProps<Props>(), {
  modelValue: '',
  language: '',
  filePath: '',
  readOnly: false,
  fontSize: 13,
  wordWrap: true,
  minimap: true,
  fileSize: 0,
})

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void
  (e: 'save'): void
  (e: 'change', value: string): void
}>()

const { t } = useI18n()
const containerRef = ref<HTMLDivElement | null>(null)
const editorInstance = shallowRef<monaco.editor.IStandaloneCodeEditor | null>(null)
let resizeObserver: ResizeObserver | null = null
let themesRegistered = false

/** 大文件自动只读 */
const isLargeFile = computed(() => props.fileSize > LARGE_FILE_THRESHOLD)
const effectiveReadOnly = computed(() => props.readOnly || isLargeFile.value)

/** 根据文件扩展名推断 Monaco 语言 */
const EXT_LANG_MAP: Record<string, string> = {
  js: 'javascript',
  mjs: 'javascript',
  cjs: 'javascript',
  jsx: 'javascript',
  ts: 'typescript',
  tsx: 'typescript',
  vue: 'html',
  html: 'html',
  htm: 'html',
  css: 'css',
  scss: 'scss',
  less: 'less',
  json: 'json',
  jsonc: 'json',
  yaml: 'yaml',
  yml: 'yaml',
  xml: 'xml',
  svg: 'xml',
  md: 'markdown',
  markdown: 'markdown',
  py: 'python',
  rs: 'rust',
  go: 'go',
  java: 'java',
  c: 'c',
  h: 'c',
  cpp: 'cpp',
  hpp: 'cpp',
  sh: 'shell',
  bash: 'shell',
  zsh: 'shell',
  sql: 'sql',
  dockerfile: 'dockerfile',
  toml: 'ini',
  ini: 'ini',
  conf: 'ini',
  cfg: 'ini',
  env: 'ini',
  lua: 'lua',
  rb: 'ruby',
  php: 'php',
  pl: 'perl',
  r: 'r',
  swift: 'swift',
  kt: 'kotlin',
  kts: 'kotlin',
  graphql: 'graphql',
  gql: 'graphql',
  proto: 'protobuf',
}

/**
 * 从文件路径推断语言
 */
function detectLanguage(filePath: string): string {
  if (!filePath) return 'plaintext'

  const fileName = filePath.split('/').pop() || ''
  const lowerName = fileName.toLowerCase()

  // 特殊文件名匹配
  if (lowerName === 'dockerfile' || lowerName.startsWith('dockerfile.')) return 'dockerfile'
  if (lowerName === 'makefile' || lowerName === 'gnumakefile') return 'makefile'
  if (lowerName === '.gitignore' || lowerName === '.dockerignore') return 'ignore'
  if (lowerName === 'nginx.conf') return 'nginx'

  const ext = fileName.includes('.') ? fileName.split('.').pop()?.toLowerCase() || '' : ''
  return EXT_LANG_MAP[ext] || 'plaintext'
}

const resolvedLanguage = computed(() => {
  if (props.language) return props.language
  return detectLanguage(props.filePath)
})

/** 检测当前是否为浅色主题 */
function isLightTheme(): boolean {
  return document.documentElement.getAttribute('data-theme') === 'light'
}

/** 注册 SDL 主题到 Monaco */
function ensureThemesRegistered() {
  if (themesRegistered) return
  monaco.editor.defineTheme(SDL_THEME_DARK, createSdlDarkTheme())
  monaco.editor.defineTheme(SDL_THEME_LIGHT, createSdlLightTheme())
  themesRegistered = true
}

/** 初始化编辑器实例 */
function createEditor() {
  if (!containerRef.value) return

  ensureThemesRegistered()

  const currentTheme = isLightTheme() ? SDL_THEME_LIGHT : SDL_THEME_DARK

  const editor = monaco.editor.create(containerRef.value, {
    value: props.modelValue,
    language: resolvedLanguage.value,
    theme: currentTheme,
    readOnly: effectiveReadOnly.value,
    fontSize: props.fontSize,
    fontFamily: 'var(--sdl-font-mono)',
    lineHeight: 1.6,
    wordWrap: props.wordWrap ? 'on' : 'off',
    minimap: { enabled: props.minimap },
    automaticLayout: false,
    scrollBeyondLastLine: false,
    renderLineHighlight: 'line',
    cursorBlinking: 'smooth',
    cursorSmoothCaretAnimation: 'on',
    smoothScrolling: true,
    bracketPairColorization: { enabled: true },
    guides: { bracketPairs: true, indentation: true },
    padding: { top: 8, bottom: 8 },
    scrollbar: {
      verticalScrollbarSize: 8,
      horizontalScrollbarSize: 8,
      verticalSliderSize: 8,
      horizontalSliderSize: 8,
    },
    contextmenu: true,
    fixedOverflowWidgets: true,
  })

  // 注册 Ctrl+S 保存快捷键
  editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
    emit('save')
  })

  // 内容变更回调
  editor.onDidChangeModelContent(() => {
    const value = editor.getValue()
    emit('update:modelValue', value)
    emit('change', value)
  })

  editorInstance.value = editor

  // 监听容器尺寸变化以触发 layout
  if ('ResizeObserver' in window) {
    resizeObserver = new ResizeObserver(() => {
      editor.layout()
    })
    resizeObserver.observe(containerRef.value)
  }
}

// --- 响应式 prop 同步 ---

/** 外部 modelValue 变更 → 同步到编辑器（避免自身触发的循环） */
watch(
  () => props.modelValue,
  (newVal) => {
    const editor = editorInstance.value
    if (!editor) return
    if (editor.getValue() !== newVal) {
      editor.setValue(newVal)
    }
  },
)

/** 语言变更 → 更新 model language */
watch(resolvedLanguage, (lang) => {
  const editor = editorInstance.value
  if (!editor) return
  const model = editor.getModel()
  if (model) {
    monaco.editor.setModelLanguage(model, lang)
  }
})

/** readOnly 变更 */
watch(effectiveReadOnly, (val) => {
  editorInstance.value?.updateOptions({ readOnly: val })
})

/** fontSize 变更 */
watch(
  () => props.fontSize,
  (val) => {
    editorInstance.value?.updateOptions({ fontSize: val })
  },
)

/** wordWrap 变更 */
watch(
  () => props.wordWrap,
  (val) => {
    editorInstance.value?.updateOptions({ wordWrap: val ? 'on' : 'off' })
  },
)

/** minimap 变更 */
watch(
  () => props.minimap,
  (val) => {
    editorInstance.value?.updateOptions({ minimap: { enabled: val } })
  },
)

/** 监听 data-theme 切换以动态更新主题 */
let themeObserver: MutationObserver | null = null

function watchThemeSwitch() {
  if (typeof MutationObserver === 'undefined') return

  themeObserver = new MutationObserver(() => {
    // 主题变更时重新注册主题色并应用
    themesRegistered = false
    ensureThemesRegistered()
    const theme = isLightTheme() ? SDL_THEME_LIGHT : SDL_THEME_DARK
    monaco.editor.setTheme(theme)
  })

  themeObserver.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['data-theme'],
  })
}

onMounted(() => {
  createEditor()
  watchThemeSwitch()
})

onUnmounted(() => {
  themeObserver?.disconnect()
  themeObserver = null
  resizeObserver?.disconnect()
  resizeObserver = null
  editorInstance.value?.dispose()
  editorInstance.value = null
})

/**
 * 暴露编辑器实例供父组件命令式操作
 */
defineExpose({
  /** 获取原始 Monaco 编辑器实例 */
  getEditor: () => editorInstance.value,
  /** 手动触发 layout */
  layout: () => editorInstance.value?.layout(),
  /** 聚焦编辑器 */
  focus: () => editorInstance.value?.focus(),
})
</script>

<template>
  <div class="monaco-editor-wrapper" data-ui="monaco-editor" data-native-context-menu>
    <div v-if="isLargeFile" class="large-file-hint" data-ui="large-file-warning">
      <span class="hint-icon">⚠</span>
      <span>{{ t('app.fileEditor.largeFileReadonly') }}</span>
    </div>
    <div ref="containerRef" class="monaco-container" />
  </div>
</template>

<style scoped>
.monaco-editor-wrapper {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  min-height: 0;
  position: relative;
}

.monaco-container {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.large-file-hint {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
  padding: var(--sdl-space-1) var(--sdl-space-3);
  background: var(--sdl-warning-soft);
  color: var(--sdl-warning);
  font-size: var(--sdl-font-caption);
  border-bottom: 1px solid var(--sdl-border-subtle);
  flex-shrink: 0;
}

.hint-icon {
  font-size: 14px;
}
</style>
