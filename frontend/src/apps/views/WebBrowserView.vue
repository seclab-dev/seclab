<script setup lang="ts">
/**
 * @file WebBrowserView.vue
 * @description SecLab 平台自研通用沙箱网络浏览器，支持接收 HTML 代码载入、地址输入导航与一键物理刷新。
 */

import { ref, watch, onMounted, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'
import { SecLabInput, SecLabButton } from '@/components/ui'
import { useThemeStore } from '@/stores/theme'

const themeStore = useThemeStore()

const props = defineProps<{
  isMaximized?: boolean
  payload?: Record<string, unknown>
}>()

const { t } = useI18n()

/** 地址栏当前输入的 URL */
const inputUrl = ref('simulation://rule-preview')
/** iframe 实际指向的目标 URL */
const iframeUrl = ref('')
/** iframe 实际需要通过 srcdoc 静态高保真渲染的 HTML 字符串 */
const iframeSrcdoc = ref('')

/** 默认的网页加载提示页模板 */
const buildDefaultWelcomeHtml = () => {
  const isDark = themeStore.currentTheme !== 'light'
  const bgColor = isDark ? '#0b1020' : '#f4f7fb'
  const textColor = isDark ? '#94a3b8' : '#4b5d7f'
  const h1Color = isDark ? '#00c8ff' : '#1d63ed'
  const pColor = isDark ? '#6f8198' : '#8a9bb1'

  return `<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
  body { background-color: ${bgColor}; color: ${textColor}; font-family: sans-serif; display: flex; flex-direction: column; align-items: center; justify-content: center; height: 90vh; margin: 0; }
  h1 { color: ${h1Color}; font-size: 2.2em; margin-bottom: 8px; }
  p { font-size: 14px; max-width: 440px; text-align: center; line-height: 1.6; color: ${pColor}; }
</style>
</head>
<body>
  <h1>SecLab Explorer</h1>
  <p>${t('app.webBrowser.welcome')}</p>
</body>
</html>`
}

/**
 * 根据传入的 Payload 动态解析是渲染 HTML 字符串，还是渲染网页 URL
 */
const loadFromPayload = () => {
  if (props.payload?.html) {
    let rawHtml = props.payload.html as string

    // 强力注入 SecLab 窄版暗黑滚动条样式与边缘自适应撑满参数
    const isDark = themeStore.currentTheme !== 'light'
    const scrollTrackBg = isDark ? '#0b1020' : '#f4f7fb'
    const scrollThumbBg = isDark ? 'rgba(148, 163, 184, 0.32)' : 'rgba(108, 122, 149, 0.3)'
    const scrollThumbHoverBg = isDark ? 'rgba(148, 163, 184, 0.45)' : 'rgba(108, 122, 149, 0.45)'

    const injectStyle = `<style>
  html, body {
    margin: 0 !important;
    padding: 0 !important;
    width: 100% !important;
    height: 100% !important;
  }
  /* SecLab 平台自适应主题滚动条 */
  ::-webkit-scrollbar { width: 8px; height: 8px; }
  ::-webkit-scrollbar-track { background: ${scrollTrackBg}; }
  ::-webkit-scrollbar-thumb { background: ${scrollThumbBg}; border-radius: 4px; border: 2px solid ${scrollTrackBg}; }
  ::-webkit-scrollbar-thumb:hover { background: ${scrollThumbHoverBg}; }
</style>`

    if (rawHtml.includes('</head>')) {
      rawHtml = rawHtml.replace('</head>', `${injectStyle}</head>`)
    } else {
      rawHtml = `${injectStyle}${rawHtml}`
    }

    iframeSrcdoc.value = rawHtml
    iframeUrl.value = ''
    inputUrl.value = 'simulation://rules/mock-preview.html'
  } else if (props.payload?.url) {
    const urlStr = props.payload.url as string
    iframeUrl.value = urlStr
    iframeSrcdoc.value = ''
    inputUrl.value = urlStr
  } else {
    iframeSrcdoc.value = buildDefaultWelcomeHtml()
    iframeUrl.value = ''
    inputUrl.value = 'about:blank'
  }
}

/**
 * 触发地址栏导航。支持对虚拟 simulation 协议的识别以及非 HTTP 域名的补全。
 */
const handleNavigate = () => {
  let target = inputUrl.value.trim()
  if (!target) return

  if (target === 'simulation://rules/mock-preview.html' || target.startsWith('simulation://')) {
    loadFromPayload()
    return
  }

  if (target === 'about:blank') {
    iframeSrcdoc.value = buildDefaultWelcomeHtml()
    iframeUrl.value = ''
    return
  }

  // 智能域名补全
  if (!/^https?:\/\//i.test(target)) {
    target = 'http://' + target
  }
  inputUrl.value = target

  // 判断是否是相同 URL 重新加载，强制重置 iframe 解决多次点击 Go 或重复跳转无效的问题
  const isSameUrl = iframeUrl.value === target && !iframeSrcdoc.value

  iframeSrcdoc.value = ''
  if (isSameUrl) {
    iframeUrl.value = ''
    nextTick(() => {
      iframeUrl.value = target
    })
  } else {
    iframeUrl.value = target
  }
}

/**
 * 物理刷新动作，实现 iframe 重载
 */
const handleRefresh = () => {
  if (iframeSrcdoc.value) {
    const temp = iframeSrcdoc.value
    iframeSrcdoc.value = ''
    setTimeout(() => {
      iframeSrcdoc.value = temp
    }, 50)
  } else {
    const temp = iframeUrl.value
    iframeUrl.value = ''
    setTimeout(() => {
      iframeUrl.value = temp
    }, 50)
  }
}

watch(
  () => props.payload,
  () => {
    loadFromPayload()
  },
  { deep: true },
)

watch(
  () => themeStore.currentTheme,
  () => {
    loadFromPayload()
  },
)

onMounted(() => {
  loadFromPayload()
})
</script>

<template>
  <div class="browser-app flex-column" data-page="web-browser">
    <!-- 顶部地址控制栏 -->
    <div class="browser-navbar" data-slot="header">
      <div class="nav-controls flex-layout gap-2">
        <button class="nav-btn" disabled :title="t('app.webBrowser.back')">&larr;</button>
        <button class="nav-btn" disabled :title="t('app.webBrowser.forward')">&rarr;</button>
        <button
          class="nav-btn refresh-btn"
          @click="handleRefresh"
          :title="t('app.webBrowser.refresh')"
        >
          &#8635;
        </button>
      </div>

      <div class="address-input-wrapper flex-1">
        <SecLabInput
          v-model="inputUrl"
          :placeholder="t('app.webBrowser.placeholder')"
          class="address-input"
          @keyup.enter="handleNavigate"
        />
      </div>

      <div class="actions">
        <SecLabButton type="primary" size="small" class="go-btn" @click="handleNavigate">
          Go
        </SecLabButton>
      </div>
    </div>

    <!-- 浏览器核心视口呈现区 -->
    <div class="browser-viewport flex-1" data-slot="content">
      <iframe
        v-if="iframeSrcdoc"
        :srcdoc="iframeSrcdoc"
        class="browser-iframe"
        data-native-context-menu
      ></iframe>
      <iframe
        v-else-if="iframeUrl"
        :key="iframeUrl"
        :src="iframeUrl"
        class="browser-iframe"
        data-native-context-menu
      ></iframe>
    </div>
  </div>
</template>

<style scoped>
.browser-app {
  height: 100%;
  background-color: var(--sdl-bg-panel);
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.browser-navbar {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-2);
  padding: 4px var(--sdl-space-3);
  background-color: var(--sdl-bg-muted);
  border-bottom: 1px solid var(--sdl-border-subtle);
  flex-shrink: 0;
}

.nav-controls {
  display: flex;
}

.nav-btn {
  background: transparent;
  border: none;
  color: var(--sdl-text-muted);
  font-size: 16px;
  cursor: pointer;
  width: 28px;
  height: 28px;
  border-radius: var(--sdl-radius-sm);
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.15s ease;
}

.nav-btn:not(:disabled):hover {
  color: var(--sdl-text-primary);
  background-color: var(--sdl-bg-hover);
}

.nav-btn:disabled {
  opacity: 0.35;
  cursor: not-allowed;
}

.refresh-btn {
  font-size: 18px;
  font-weight: bold;
}

.address-input-wrapper {
  flex: 1;
  min-width: 0;
  max-width: none !important;
}

.address-input {
  width: 100% !important;
  max-width: none !important;
}

.address-input-wrapper :deep(.sl-input-wrapper) {
  width: 100% !important;
  max-width: none !important;
  height: 28px !important;
}

.address-input-wrapper :deep(.sl-input-inner-wrapper) {
  width: 100% !important;
  max-width: none !important;
  height: 100% !important;
  border-radius: var(--sdl-radius-md);
}

.address-input-wrapper :deep(.sl-input) {
  width: 100% !important;
  max-width: none !important;
  font-size: 12px;
  height: 100% !important;
  padding: 0 var(--sdl-space-2) !important;
}

.go-btn {
  height: 28px;
  padding: 0 var(--sdl-space-3);
  font-size: 12px;
  font-weight: bold;
  border-radius: var(--sdl-radius-md);
  min-width: 48px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}

.browser-viewport {
  background-color: #ffffff;
  position: relative;
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.browser-iframe {
  width: 100%;
  flex: 1;
  border: none;
  background-color: #ffffff;
}
</style>
