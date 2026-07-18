<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { upgradesApi } from '@/api/modules/upgrades'
import type { UpgradePlanDetail } from '@/api/modules/upgrades'
import { useToastStore } from '@/stores/toast'
import http from '@/api'
import { resetAuthState } from '@/router'
import { useWindowManagerStore } from '@/stores/window-manager'
import { SecLabTag } from '@/components/ui'
import { getLoginEntryPath } from '@/utils/login-entry'

const router = useRouter()
const route = useRoute()
const { t } = useI18n()
const toastStore = useToastStore()
const windowStore = useWindowManagerStore()

const upgradePlan = ref<UpgradePlanDetail | null>(null)
const isLoading = ref(true)
const isCancelling = ref(false)
const isRedirecting = ref(false)
const UPGRADE_PLAN_ID_KEY = 'seclab.activeUpgradePlanId'
const UPGRADE_PLAN_DETAIL_KEY = 'seclab.activeUpgradePlanDetail'
let timerId: number | null = null
let missingPlanCount = 0

const routePlanId = computed(() => {
  const value = route.query.planId
  if (typeof value === 'string' && value.trim()) return value.trim()
  return window.sessionStorage.getItem(UPGRADE_PLAN_ID_KEY)
})

const isDisplayablePlan = (plan: UpgradePlanDetail) => {
  return ['pending', 'running', 'succeeded', 'failed', 'canceled'].includes(plan.plan.status)
}

const leaveUpgradePage = () => {
  if (isRedirecting.value) return
  window.sessionStorage.removeItem(UPGRADE_PLAN_ID_KEY)
  window.sessionStorage.removeItem(UPGRADE_PLAN_DETAIL_KEY)
  isRedirecting.value = true
  if (timerId !== null) {
    window.clearInterval(timerId)
    timerId = null
  }
  resetAuthState()
  window.location.replace(getLoginEntryPath())
}

const cachePlan = (plan: UpgradePlanDetail) => {
  window.sessionStorage.setItem(UPGRADE_PLAN_ID_KEY, plan.plan.planId)
  window.sessionStorage.setItem(UPGRADE_PLAN_DETAIL_KEY, JSON.stringify(plan))
}

const hydrateCachedPlan = () => {
  const raw = window.sessionStorage.getItem(UPGRADE_PLAN_DETAIL_KEY)
  if (!raw) return
  try {
    const cached = JSON.parse(raw) as UpgradePlanDetail
    if (isDisplayablePlan(cached)) {
      upgradePlan.value = cached
      isLoading.value = false
    }
  } catch {
    window.sessionStorage.removeItem(UPGRADE_PLAN_DETAIL_KEY)
  }
}

const fetchStatus = async () => {
  try {
    const planId = routePlanId.value
    const response = planId ? await upgradesApi.detail(planId) : await upgradesApi.latestPlan()
    if (response.success && response.data) {
      if (isDisplayablePlan(response.data)) {
        upgradePlan.value = response.data
        cachePlan(response.data)
        missingPlanCount = 0
      } else {
        missingPlanCount += 1
      }
    } else {
      missingPlanCount += 1
    }
    if (!upgradePlan.value && missingPlanCount >= 3) {
      leaveUpgradePage()
    }
  } catch (error) {
    console.error('Failed to fetch latest upgrade plan', error)
    if (!upgradePlan.value) {
      missingPlanCount += 1
      if (missingPlanCount >= 3) {
        leaveUpgradePage()
      }
    }
  } finally {
    isLoading.value = false
  }
}

const cancelUpgrade = async () => {
  const planId = upgradePlan.value?.plan.planId
  if (!planId) return
  isCancelling.value = true
  try {
    const response = await upgradesApi.cancelPlan(planId)
    if (response.success) {
      toastStore.success(t('upgradeProgress.notifications.cancelSuccess'))
      upgradePlan.value = response.data ?? null
    } else {
      toastStore.error(response.message || t('upgradeProgress.notifications.cancelFailed'))
    }
  } catch (error) {
    console.error('Failed to cancel upgrade', error)
    toastStore.error(t('upgradeProgress.notifications.cancelFailed'))
  } finally {
    isCancelling.value = false
  }
}

const handleFinishAction = async () => {
  if (timerId !== null) {
    window.clearInterval(timerId)
    timerId = null
  }
  const status = upgradePlan.value?.plan.status
  if (status === 'succeeded') {
    try {
      await http.post('/auth/logout')
    } catch (error) {
      console.error('Logout request failed', error)
    }

    window.sessionStorage.removeItem(UPGRADE_PLAN_ID_KEY)
    window.sessionStorage.removeItem(UPGRADE_PLAN_DETAIL_KEY)
    resetAuthState()

    const windowsToClose = [...windowStore.openWindows]
    windowsToClose.forEach((w) => windowStore.closeWindow(w.id))

    window.location.replace(getLoginEntryPath())
  } else {
    window.sessionStorage.removeItem(UPGRADE_PLAN_ID_KEY)
    window.sessionStorage.removeItem(UPGRADE_PLAN_DETAIL_KEY)
    router.push('/')
  }
}

const getFinishActionLabel = () => {
  const status = upgradePlan.value?.plan.status
  if (status === 'succeeded') {
    return t('upgradeProgress.labels.reLogin')
  }
  return t('upgradeProgress.labels.returnToSystem')
}

// 升级完成状态计算
const isFinished = computed(() => {
  const status = upgradePlan.value?.plan.status
  return status === 'succeeded' || status === 'failed' || status === 'canceled'
})

const getTargetStatus = (target: { status: string; subStatus?: string | null }) => {
  if (target.status === 'running' && target.subStatus) {
    return target.subStatus
  }
  return target.status
}

const parseDownloadingPercent = (status: string) => {
  if (!status.startsWith('downloading:')) return null
  const rawPercent = status.split(':')[1]?.replace('%', '')
  const percent = Number(rawPercent)
  if (!Number.isFinite(percent)) return null
  return Math.max(0, Math.min(100, percent))
}

const getTargetProgress = (target: { status: string; subStatus?: string | null }) => {
  const status = getTargetStatus(target)
  const downloadingPercent = parseDownloadingPercent(status)
  if (downloadingPercent !== null) {
    return Math.round(10 + downloadingPercent * 0.4)
  }

  switch (status) {
    case 'pending':
    case 'deferred':
      return 0
    case 'running':
      return 10
    case 'downloading':
      return 20
    case 'downloaded':
      return 50
    case 'verifying':
      return 60
    case 'staging':
      return 70
    case 'prepared':
      return 80
    case 'applying':
      return 85
    case 'restarting':
    case 'restart_scheduled':
      return 90
    case 'succeeded':
    case 'failed':
    case 'canceled':
      return 100
    default:
      return target.status === 'running' ? 10 : 0
  }
}

// 升级进度百分比计算：按每个 target 的阶段进度求平均。
const progressPercent = computed(() => {
  if (!upgradePlan.value || upgradePlan.value.targets.length === 0) return 0
  const total = upgradePlan.value.targets.reduce(
    (sum, target) => sum + getTargetProgress(target),
    0,
  )
  return Math.round(total / upgradePlan.value.targets.length)
})

// 升级对象排序：如果有主控（controller），其卡片始终置顶于第一位呈现
const sortedTargets = computed(() => {
  if (!upgradePlan.value || !upgradePlan.value.targets) return []
  return [...upgradePlan.value.targets].sort((a, b) => {
    if (a.targetType === 'controller' && b.targetType !== 'controller') return -1
    if (a.targetType !== 'controller' && b.targetType === 'controller') return 1
    return 0
  })
})

const pageTitle = computed(() => {
  switch (upgradePlan.value?.plan.status) {
    case 'pending':
      return t('upgradeProgress.title.preparing')
    case 'running':
      return t('upgradeProgress.title.running')
    case 'succeeded':
      return t('upgradeProgress.title.succeeded')
    case 'failed':
      return t('upgradeProgress.title.failed')
    case 'canceled':
      return t('upgradeProgress.title.canceled')
    default:
      return isLoading.value
        ? t('upgradeProgress.title.fetching')
        : t('upgradeProgress.title.default')
  }
})

const planStatusLabel = computed(() => {
  switch (upgradePlan.value?.plan.status) {
    case 'pending':
      return t('upgradeProgress.status.preparing')
    case 'running':
      return t('upgradeProgress.status.executing')
    case 'succeeded':
      return t('upgradeProgress.status.finished')
    case 'failed':
      return t('upgradeProgress.status.failed')
    case 'canceled':
      return t('upgradeProgress.status.canceled')
    default:
      return t('upgradeProgress.status.unknown')
  }
})

const getStatusType = (status: string) => {
  switch (status) {
    case 'succeeded':
      return 'success'
    case 'failed':
      return 'danger'
    case 'running':
    case 'downloading':
    case 'verifying':
    case 'staging':
    case 'applying':
    case 'restarting':
      return 'info'
    case 'deferred':
      return 'warning'
    default:
      return 'default'
  }
}

const getStatusLabel = (status: string) => {
  if (status.startsWith('downloading:')) {
    const percent = status.split(':')[1] || '0%'
    return t('upgradeProgress.status.downloadingPercent', { percent })
  }
  switch (status) {
    case 'pending':
      return t('upgradeProgress.status.pending')
    case 'running':
      return t('upgradeProgress.status.running')
    case 'downloading':
      return t('upgradeProgress.status.downloading')
    case 'downloaded':
      return t('upgradeProgress.status.downloaded')
    case 'verifying':
      return t('upgradeProgress.status.verifying')
    case 'staging':
      return t('upgradeProgress.status.staging')
    case 'applying':
      return t('upgradeProgress.status.applying')
    case 'restarting':
    case 'restart_scheduled':
      return t('upgradeProgress.status.restarting')
    case 'succeeded':
      return t('upgradeProgress.status.succeeded')
    case 'failed':
      return t('upgradeProgress.status.failed')
    case 'deferred':
      return t('upgradeProgress.status.deferred')
    case 'canceled':
      return t('upgradeProgress.status.canceled')
    default:
      return status
  }
}

const shortNodeId = (nodeId?: string | null) => {
  if (!nodeId) return ''
  return nodeId.length > 12 ? `${nodeId.slice(0, 8)}...${nodeId.slice(-4)}` : nodeId
}

const getTargetName = (target: { targetType: string; nodeId?: string | null }) => {
  if (target.targetType === 'controller') return t('upgradeProgress.targetType.controllerNode')
  if (!target.nodeId) return t('upgradeProgress.targetType.unknownNode')
  return upgradePlan.value?.nodeNames?.[target.nodeId] || shortNodeId(target.nodeId)
}

const getEventTargetName = (targetId?: string) => {
  if (!targetId) return ''
  const target = upgradePlan.value?.targets.find((t) => t.targetId === targetId)
  if (!target) return ''
  return getTargetName(target)
}

onMounted(() => {
  hydrateCachedPlan()
  void fetchStatus()
  timerId = window.setInterval(() => {
    void fetchStatus()
  }, 3000)
})

onUnmounted(() => {
  if (timerId !== null) {
    window.clearInterval(timerId)
  }
})
</script>

<template>
  <div class="upgrade-container" data-page="upgrade-progress">
    <div class="upgrade-card">
      <!-- 头部：标题与主状态 -->
      <header class="upgrade-header">
        <div class="brand">
          <span
            class="pulse-dot"
            :class="{ 'pulse-active': upgradePlan?.plan.status === 'running' }"
          ></span>
          <h1>{{ pageTitle }}</h1>
        </div>
        <div v-if="upgradePlan" class="plan-info-header">
          <span class="target-ver-badge"
            >{{ t('upgradeProgress.labels.targetVersion') }}:
            {{ upgradePlan.plan.targetVersion }}</span
          >
          <span :class="['status-tag', getStatusType(upgradePlan.plan.status)]">
            {{ planStatusLabel }}
          </span>
        </div>
      </header>

      <!-- 未加载完成骨架屏 -->
      <div v-if="isLoading && !upgradePlan" class="loading-state">
        <div class="spinner"></div>
        <p>{{ t('upgradeProgress.title.fetching') }}</p>
      </div>

      <!-- 升级信息主体 -->
      <div v-else-if="upgradePlan" class="upgrade-body">
        <!-- 进度条 -->
        <section class="progress-section">
          <div class="progress-meta">
            <span>
              {{ t('upgradeProgress.labels.totalProgress') }}
              ({{ t('upgradeProgress.labels.targetCount', { count: upgradePlan.targets.length }) }})
            </span>
            <span class="percent-text">{{ progressPercent }}%</span>
          </div>
          <div class="progress-bar-wrapper">
            <div class="progress-bar" :style="{ width: `${progressPercent}%` }"></div>
          </div>
        </section>

        <!-- 节点列表 -->
        <section class="targets-section">
          <h3>{{ t('upgradeProgress.labels.targetDetail') }}</h3>
          <div class="targets-grid">
            <div
              v-for="target in sortedTargets"
              :key="target.targetId"
              class="target-item-card"
              data-ui="upgrade-target-card"
            >
              <div class="target-meta">
                <span class="target-type-badge" :class="target.targetType">
                  {{
                    target.targetType === 'controller'
                      ? t('upgradeProgress.targetType.controller')
                      : t('upgradeProgress.targetType.agent')
                  }}
                </span>
                <span class="target-name">
                  {{ getTargetName(target) }}
                </span>
              </div>
              <div v-if="target.nodeId" class="target-node-id code-text">
                {{ shortNodeId(target.nodeId) }}
              </div>
              <div class="target-details">
                <div class="detail-row">
                  <span class="label">{{ t('upgradeProgress.labels.currentVersion') }}:</span>
                  <span class="val code-text">{{
                    target.status === 'succeeded'
                      ? target.targetVersion
                      : target.currentVersion || t('upgradeProgress.status.unknown')
                  }}</span>
                </div>
                <div class="detail-row">
                  <span class="label">{{ t('upgradeProgress.labels.targetVersion') }}:</span>
                  <span class="val code-text">{{ target.targetVersion }}</span>
                </div>
                <div class="detail-row status-row">
                  <span class="label">{{ t('upgradeProgress.labels.currentStatus') }}:</span>
                  <span :class="['status-badge-text', getStatusType(getTargetStatus(target))]">
                    {{ getStatusLabel(getTargetStatus(target)) }}
                  </span>
                </div>
              </div>
              <!-- 错误详情回显 -->
              <div v-if="target.errorDetail" class="error-box">
                {{ target.errorDetail }}
              </div>
            </div>
          </div>
        </section>

        <!-- 升级日志 -->
        <section class="events-section">
          <h3>{{ t('upgradeProgress.labels.activityLog') }}</h3>
          <div class="events-timeline">
            <div v-for="event in upgradePlan.events" :key="event.eventId" class="timeline-item">
              <span class="event-time code-text">
                {{ new Date(event.createdAt).toLocaleTimeString() }}
              </span>
              <span class="event-marker"></span>
              <div
                class="event-desc"
                style="display: flex; align-items: center; gap: var(--sdl-space-2); flex-wrap: wrap"
              >
                <SecLabTag v-if="getEventTargetName(event.targetId)" type="info" size="small">
                  {{ getEventTargetName(event.targetId) }}
                </SecLabTag>
                <span class="event-msg">{{ event.message }}</span>
                <span class="event-type-label code-text">[{{ event.eventType }}]</span>
              </div>
            </div>
            <div v-if="upgradePlan.events.length === 0" class="no-events">
              {{ t('upgradeProgress.labels.noLogs') }}
            </div>
          </div>
        </section>

        <!-- 底部控制面板 -->
        <footer class="upgrade-actions">
          <button
            v-if="upgradePlan.plan.status === 'running'"
            class="btn btn-danger"
            :disabled="isCancelling"
            @click="cancelUpgrade"
          >
            {{
              isCancelling
                ? t('upgradeProgress.labels.cancelling')
                : t('upgradeProgress.labels.cancelUpgrade')
            }}
          </button>
          <button v-if="isFinished" class="btn btn-primary" @click="handleFinishAction">
            {{ getFinishActionLabel() }}
          </button>
        </footer>
      </div>

      <!-- 无有效计划时离开升级专用页面 -->
      <div v-else class="loading-state">
        <div class="spinner"></div>
        <p>
          {{
            isRedirecting
              ? t('upgradeProgress.labels.leavingPage')
              : t('upgradeProgress.labels.confirmingStatus')
          }}
        </p>
      </div>
    </div>
  </div>
</template>

<style scoped>
.upgrade-container {
  height: 100vh;
  min-height: 0;
  display: flex;
  justify-content: center;
  align-items: center;
  background: radial-gradient(circle at center, #0f172a 0%, #020617 100%);
  color: #e2e8f0;
  font-family:
    'Outfit',
    -apple-system,
    BlinkMacSystemFont,
    'Segoe UI',
    Roboto,
    sans-serif;
  padding: 24px;
}

.upgrade-card {
  width: 100%;
  max-width: 860px;
  max-height: calc(100vh - 48px);
  min-height: 0;
  background: rgba(15, 23, 42, 0.6);
  border: 1px solid rgba(148, 163, 184, 0.1);
  box-shadow: 0 20px 40px rgba(0, 0, 0, 0.4);
  backdrop-filter: blur(16px);
  border-radius: 16px;
  padding: 32px;
  display: flex;
  flex-direction: column;
  gap: 24px;
}

.upgrade-header {
  flex-shrink: 0;
  display: flex;
  justify-content: space-between;
  align-items: center;
  border-bottom: 1px solid rgba(148, 163, 184, 0.1);
  padding-bottom: 20px;
}

.upgrade-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 20px;
}

.brand {
  display: flex;
  align-items: center;
  gap: 12px;
}

.brand h1 {
  font-size: 24px;
  font-weight: 600;
  letter-spacing: 0;
  background: linear-gradient(135deg, #38bdf8 0%, #3b82f6 100%);
  -webkit-background-clip: text;
  background-clip: text;
  -webkit-text-fill-color: transparent;
  margin: 0;
}

.pulse-dot {
  width: 10px;
  height: 10px;
  background-color: #ef4444;
  border-radius: 50%;
  position: relative;
}

.pulse-active {
  background-color: #10b981;
}

.pulse-active::after {
  content: '';
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  border-radius: 50%;
  border: 2px solid #10b981;
  animation: pulse-ring 1.8s infinite ease-out;
}

@keyframes pulse-ring {
  0% {
    transform: scale(0.8);
    opacity: 1;
  }
  100% {
    transform: scale(2.4);
    opacity: 0;
  }
}

.plan-info-header {
  display: flex;
  align-items: center;
  gap: 12px;
}

.target-ver-badge {
  background: rgba(56, 189, 248, 0.1);
  color: #38bdf8;
  font-size: 13px;
  padding: 4px 10px;
  border-radius: 6px;
  border: 1px solid rgba(56, 189, 248, 0.2);
  font-family: monospace;
}

.status-tag {
  font-size: 11px;
  font-weight: 700;
  padding: 4px 10px;
  border-radius: 6px;
  letter-spacing: 0.5px;
}

.status-tag.success {
  background: rgba(16, 185, 129, 0.15);
  color: #10b981;
  border: 1px solid rgba(16, 185, 129, 0.3);
}

.status-tag.danger {
  background: rgba(239, 68, 68, 0.15);
  color: #ef4444;
  border: 1px solid rgba(239, 68, 68, 0.3);
}

.status-tag.info {
  background: rgba(59, 130, 246, 0.15);
  color: #3b82f6;
  border: 1px solid rgba(59, 130, 246, 0.3);
}

.status-tag.warning {
  background: rgba(245, 158, 11, 0.15);
  color: #f59e0b;
  border: 1px solid rgba(245, 158, 11, 0.3);
}

.status-tag.default {
  background: rgba(148, 163, 184, 0.15);
  color: #94a3b8;
  border: 1px solid rgba(148, 163, 184, 0.3);
}

.loading-state,
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 60px 0;
  gap: 16px;
  color: #94a3b8;
}

.spinner {
  width: 40px;
  height: 40px;
  border: 3px solid rgba(56, 189, 248, 0.15);
  border-top-color: #38bdf8;
  border-radius: 50%;
  animation: spin 1s infinite linear;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.progress-section {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.progress-meta {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 14px;
  color: #94a3b8;
}

.percent-text {
  font-size: 20px;
  font-weight: 700;
  color: #38bdf8;
}

.progress-bar-wrapper {
  height: 10px;
  background: rgba(15, 23, 42, 0.8);
  border-radius: 9999px;
  overflow: hidden;
  border: 1px solid rgba(255, 255, 255, 0.05);
}

.progress-bar {
  height: 100%;
  background: linear-gradient(90deg, #3b82f6 0%, #38bdf8 100%);
  border-radius: 9999px;
  transition: width 0.4s cubic-bezier(0.4, 0, 0.2, 1);
}

.targets-section,
.events-section {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.targets-section {
  flex: 1 1 260px;
  min-height: 180px;
}

.events-section {
  flex-shrink: 0;
}

.targets-section h3,
.events-section h3 {
  font-size: 16px;
  font-weight: 600;
  color: #f1f5f9;
  margin: 0;
}

.targets-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(250px, 1fr));
  gap: 16px;
  min-height: 0;
  overflow-y: auto;
  padding-right: 4px;
  scrollbar-gutter: stable;
}

.target-item-card {
  background: rgba(30, 41, 59, 0.35);
  border: 1px solid rgba(148, 163, 184, 0.08);
  border-radius: 10px;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.target-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.05);
  padding-bottom: 8px;
}

.target-type-badge {
  font-size: 11px;
  font-weight: 600;
  padding: 2px 6px;
  border-radius: 4px;
}

.target-type-badge.controller {
  background: rgba(139, 92, 246, 0.15);
  color: #a78bfa;
}

.target-type-badge.agent {
  background: rgba(59, 130, 246, 0.15);
  color: #60a5fa;
}

.target-name {
  font-size: 13px;
  font-weight: 600;
  color: #cbd5e1;
}

.target-node-id {
  font-size: 11px;
  color: #64748b;
}

.target-details {
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 12px;
}

.detail-row {
  display: flex;
  justify-content: space-between;
}

.detail-row .label {
  color: #64748b;
}

.detail-row .val {
  color: #cbd5e1;
}

.status-badge-text {
  font-weight: 600;
}

.status-badge-text.success {
  color: #10b981;
}

.status-badge-text.danger {
  color: #ef4444;
}

.status-badge-text.info {
  color: #38bdf8;
}

.status-badge-text.warning {
  color: #f59e0b;
}

.status-badge-text.default {
  color: #94a3b8;
}

.error-box {
  background: rgba(239, 68, 68, 0.1);
  border: 1px solid rgba(239, 68, 68, 0.2);
  border-radius: 6px;
  padding: 8px;
  font-size: 11px;
  color: #fca5a5;
  word-break: break-all;
}

.events-timeline {
  max-height: 180px;
  overflow-y: auto;
  background: rgba(15, 23, 42, 0.4);
  border: 1px solid rgba(148, 163, 184, 0.08);
  border-radius: 10px;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.timeline-item {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  font-size: 13px;
}

.event-time {
  color: #64748b;
  min-width: 70px;
  font-size: 12px;
}

.event-marker {
  width: 6px;
  height: 6px;
  background-color: #3b82f6;
  border-radius: 50%;
  margin-top: 6px;
  flex-shrink: 0;
}

.event-desc {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}

.event-msg {
  color: #cbd5e1;
}

.event-type-label {
  color: #475569;
  font-size: 11px;
}

.no-events {
  text-align: center;
  padding: 20px 0;
  color: #64748b;
  font-size: 13px;
}

.upgrade-actions {
  flex-shrink: 0;
  display: flex;
  justify-content: flex-end;
  border-top: 1px solid rgba(148, 163, 184, 0.1);
  padding-top: 20px;
}

.btn {
  font-family: inherit;
  font-size: 14px;
  font-weight: 600;
  padding: 10px 22px;
  border-radius: 8px;
  border: none;
  cursor: pointer;
  transition: all 0.2s;
}

.btn-primary {
  background: #3b82f6;
  color: #ffffff;
}

.btn-primary:hover {
  background: #2563eb;
  box-shadow: 0 0 16px rgba(59, 130, 246, 0.4);
}

.btn-danger {
  background: rgba(239, 68, 68, 0.15);
  color: #f87171;
  border: 1px solid rgba(239, 68, 68, 0.3);
}

.btn-danger:hover {
  background: #ef4444;
  color: #ffffff;
  box-shadow: 0 0 16px rgba(239, 68, 68, 0.4);
}

.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
  box-shadow: none !important;
}

.code-text {
  font-family: monospace;
}
</style>
