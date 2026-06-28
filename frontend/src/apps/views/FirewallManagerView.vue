<script setup lang="ts">
/**
 * @file FirewallManagerView.vue
 * @description SecLab 平台远程主机防火墙状态检测视窗。
 */

import { ref, onMounted, watch, computed } from 'vue'
import { useNodeStore } from '@/stores/node'
import { useWindowManagerStore } from '@/stores/window-manager'
import { systemApi } from '@/api/modules/system'
import type { FirewallStatusInfo } from '@/api/interface/system'
import {
  SecLabCard,
  SecLabTag,
  SecLabAlert,
  SecLabDescriptions,
  SecLabLoading,
  SecLabEmpty,
  SecLabTable,
} from '@/components/ui'
import { useI18n } from 'vue-i18n'

const props = defineProps<{
  isMaximized?: boolean
  windowId?: string
  payload?: Record<string, unknown>
}>()

const { t } = useI18n()
const nodeStore = useNodeStore()
const windowStore = useWindowManagerStore()

const loading = ref(false)
const firewallInfo = ref<FirewallStatusInfo | null>(null)

watch(
  loading,
  (busy) => {
    if (!props.windowId) return
    windowStore.updateWindowRuntimeState(props.windowId, {
      busy,
      allowsNodeSwitch: false,
      blockLevel: busy ? 'busy' : 'open',
      blockReason: busy ? t('app.firewallManager.guardBusy') : t('app.firewallManager.guardOpen'),
    })
  },
  { immediate: true },
)

const systemClient = computed(() => systemApi.forNode(nodeStore.currentNodeId))
const currentNodeName = computed(() => {
  const activeNode = nodeStore.nodes.find((node) => node.id === nodeStore.currentNodeId)
  return activeNode?.name || nodeStore.currentNodeId || '--'
})
const backendColumns = computed(() => [
  { prop: 'backend', label: t('app.firewallManager.backend'), minWidth: 120, slot: 'backend' },
  { prop: 'active', label: t('app.firewallManager.active'), minWidth: 120, slot: 'active' },
  { prop: 'serviceStatus', label: t('app.firewallManager.serviceStatus'), minWidth: 150 },
  { prop: 'version', label: t('app.firewallManager.version'), minWidth: 180, slot: 'version' },
  { prop: 'commandPath', label: t('app.firewallManager.commandPath'), minWidth: 180, slot: 'path' },
])
const ruleColumns = computed(() => [
  { prop: 'backend', label: t('app.firewallManager.backend'), minWidth: 110, slot: 'backend' },
  { prop: 'scope', label: t('app.firewallManager.scope'), minWidth: 130 },
  { prop: 'action', label: t('app.firewallManager.action'), minWidth: 110, slot: 'action' },
  { prop: 'protocol', label: t('app.firewallManager.protocol'), minWidth: 100, slot: 'protocol' },
  { prop: 'port', label: t('app.firewallManager.port'), minWidth: 110, slot: 'port' },
  { prop: 'source', label: t('app.firewallManager.source'), minWidth: 150, slot: 'source' },
  { prop: 'raw', label: t('app.firewallManager.rawRule'), minWidth: 280, slot: 'raw' },
])

/**
 * 触发主机防火墙状态检测
 */
const triggerDetection = async () => {
  loading.value = true
  try {
    const res = await systemClient.value.detectFirewall()
    if (res.data) {
      firewallInfo.value = res.data
    }
  } catch (err) {
    console.error('Failed to detect firewall status:', err)
  } finally {
    loading.value = false
  }
}

// 监听目标计算节点改变，动态自动重测
watch(
  () => nodeStore.currentNodeId,
  () => {
    triggerDetection()
  },
)

onMounted(() => {
  triggerDetection()
})

const firewallInfoItems = computed(() => {
  if (!firewallInfo.value) return []
  return [
    { label: t('app.firewallManager.targetNode'), slot: 'targetNode' },
    { label: t('app.firewallManager.installStatus'), slot: 'installedStatus' },
    { label: t('app.firewallManager.primaryBackend'), slot: 'firewallType' },
    { label: t('app.firewallManager.version'), slot: 'version' },
    { label: t('app.firewallManager.active'), slot: 'activity' },
    { label: t('app.firewallManager.serviceStatus'), slot: 'serviceStatus' },
  ]
})

const backendCount = computed(() => firewallInfo.value?.detectedBackends?.length ?? 0)
const ruleCount = computed(() => firewallInfo.value?.rules?.length ?? 0)

const getBackendTagType = (backend: string) => {
  if (backend === 'ufw') return 'primary'
  if (backend === 'firewalld') return 'warning'
  if (backend === 'nftables') return 'info'
  if (backend === 'iptables') return 'default'
  return 'default'
}

const getActionTagType = (action: string) => {
  const normalized = action.toLowerCase()
  if (normalized.includes('accept') || normalized.includes('allow') || normalized === 'service') {
    return 'success'
  }
  if (normalized.includes('drop') || normalized.includes('deny') || normalized.includes('reject')) {
    return 'danger'
  }
  return 'default'
}
</script>

<template>
  <div class="firewall-app flex-column" data-page="firewall-manager">
    <!-- 主体内容呈现区 -->
    <div class="content flex-1 overflow-auto">
      <div class="content-container">
        <SecLabLoading :loading="loading" :text="t('app.firewallManager.loading')">
          <div v-if="firewallInfo" class="firewall-dashboard">
            <div class="summary-grid mb-4" data-ui="firewall-summary-grid">
              <div class="summary-tile">
                <span class="summary-label">{{ t('app.firewallManager.detectedBackends') }}</span>
                <strong>{{ backendCount }}</strong>
              </div>
              <div class="summary-tile">
                <span class="summary-label">{{ t('app.firewallManager.ruleCount') }}</span>
                <strong>{{ ruleCount }}</strong>
              </div>
              <div class="summary-tile">
                <span class="summary-label">{{ t('app.firewallManager.platform') }}</span>
                <strong>{{ firewallInfo.platform?.toUpperCase() || '--' }}</strong>
              </div>
            </div>

            <!-- 安全体检摘要 -->
            <SecLabCard class="mb-4">
              <template #title>
                <div class="card-title flex-layout gap-2">
                  <span>{{ t('app.firewallManager.hostSummary') }}</span>
                </div>
              </template>

              <SecLabDescriptions
                :items="firewallInfoItems"
                :data="firewallInfo"
                :column="2"
                border
              >
                <template #targetNode>
                  <span class="code-text">{{ currentNodeName }}</span>
                </template>
                <template #installedStatus>
                  <span v-if="firewallInfo.installed" class="font-bold text-success">
                    {{ t('app.firewallManager.detected') }}
                  </span>
                  <span v-else class="font-bold text-muted">
                    {{ t('app.firewallManager.unknownProtection') }}
                  </span>
                </template>
                <template #firewallType>
                  <SecLabTag
                    v-if="firewallInfo.primaryBackend"
                    :type="getBackendTagType(firewallInfo.primaryBackend)"
                    >{{ firewallInfo.primaryBackend.toUpperCase() }}</SecLabTag
                  >
                  <span v-else class="text-muted">--</span>
                </template>
                <template #version>
                  <span v-if="firewallInfo.version" class="code-text">{{
                    firewallInfo.version
                  }}</span>
                  <span v-else class="text-muted">--</span>
                </template>
                <template #activity>
                  <SecLabTag v-if="firewallInfo.active" type="success">{{
                    t('app.firewallManager.activeStatus')
                  }}</SecLabTag>
                  <SecLabTag v-else-if="firewallInfo.installed" type="danger" size="small">{{
                    t('app.firewallManager.inactiveStatus')
                  }}</SecLabTag>
                  <span v-else class="text-muted">--</span>
                </template>
                <template #serviceStatus>
                  <span class="code-text">{{ firewallInfo.serviceStatus }}</span>
                </template>
              </SecLabDescriptions>
            </SecLabCard>

            <SecLabCard class="mb-4">
              <template #title>
                <span>{{ t('app.firewallManager.backendInventory') }}</span>
              </template>
              <SecLabTable
                :data="firewallInfo.detectedBackends || []"
                :columns="backendColumns"
                border
              >
                <template #backend="{ row }">
                  <SecLabTag :type="getBackendTagType(row.backend)">{{
                    row.backend.toUpperCase()
                  }}</SecLabTag>
                </template>
                <template #active="{ row }">
                  <SecLabTag :type="row.active ? 'success' : 'warning'">
                    {{
                      row.active
                        ? t('app.firewallManager.activeStatus')
                        : t('app.firewallManager.inactiveStatus')
                    }}
                  </SecLabTag>
                </template>
                <template #version="{ row }">
                  <span class="code-text">{{ row.version || '--' }}</span>
                </template>
                <template #path="{ row }">
                  <span class="code-text">{{ row.commandPath || '--' }}</span>
                </template>
                <template #empty>
                  <SecLabEmpty :description="t('app.firewallManager.noBackend')" />
                </template>
              </SecLabTable>
            </SecLabCard>

            <SecLabCard class="mb-4">
              <template #title>
                <span>{{ t('app.firewallManager.ruleSummary') }}</span>
              </template>
              <SecLabTable :data="firewallInfo.rules || []" :columns="ruleColumns" border>
                <template #backend="{ row }">
                  <SecLabTag :type="getBackendTagType(row.backend)">{{
                    row.backend.toUpperCase()
                  }}</SecLabTag>
                </template>
                <template #action="{ row }">
                  <SecLabTag :type="getActionTagType(row.action)">{{ row.action }}</SecLabTag>
                </template>
                <template #protocol="{ row }">
                  <span class="code-text">{{ row.protocol || '--' }}</span>
                </template>
                <template #port="{ row }">
                  <span class="code-text">{{ row.port || '--' }}</span>
                </template>
                <template #source="{ row }">
                  <span class="code-text">{{ row.source || '--' }}</span>
                </template>
                <template #raw="{ row }">
                  <span class="raw-rule">{{ row.raw }}</span>
                </template>
                <template #empty>
                  <SecLabEmpty :description="t('app.firewallManager.noRules')" />
                </template>
              </SecLabTable>
            </SecLabCard>

            <SecLabAlert
              v-if="firewallInfo.warnings?.length"
              type="warning"
              :title="t('app.firewallManager.warningTitle')"
              class="mb-4"
            >
              <ul class="warning-list">
                <li v-for="warning in firewallInfo.warnings" :key="warning">
                  {{ warning }}
                </li>
              </ul>
            </SecLabAlert>

            <!-- 边界防御诊断报告 -->
            <SecLabCard v-if="firewallInfo.installed">
              <template #title>
                <span>{{ t('app.firewallManager.diagnosisTitle') }}</span>
              </template>
              <div class="diagnosis-content">
                <div v-if="firewallInfo.active" class="diagnosis-status healthy">
                  <div class="status-indicator healthy"></div>
                  <div class="status-details">
                    <h4>{{ t('app.firewallManager.healthyTitle') }}</h4>
                    <p>
                      {{ t('app.firewallManager.healthyDesc') }}
                    </p>
                  </div>
                </div>
                <div v-else class="diagnosis-status warned">
                  <div class="status-indicator warned"></div>
                  <div class="status-details">
                    <h4>{{ t('app.firewallManager.inactiveTitle') }}</h4>
                    <p>
                      {{ t('app.firewallManager.inactiveDesc') }}
                    </p>
                  </div>
                </div>
              </div>
            </SecLabCard>
          </div>

          <div v-else class="empty-status">
            <SecLabEmpty :description="t('app.firewallManager.empty')" />
          </div>
        </SecLabLoading>
      </div>
    </div>
  </div>
</template>

<style scoped>
.firewall-app {
  height: 100%;
  background-color: var(--sdl-bg-canvas);
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.content {
  padding: var(--sdl-space-4);
  box-sizing: border-box;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
  scrollbar-gutter: stable;
}

.content-container {
  max-width: 1180px;
  margin: 0 auto;
}

.alert-desc {
  margin: 0;
  line-height: 1.6;
}

.code-text {
  font-family: monospace;
  background-color: var(--sdl-bg-muted);
  padding: 2px 6px;
  border-radius: var(--sdl-radius-sm);
  color: var(--sdl-text-primary);
  font-size: 12px;
}

.badge-text {
  font-family: monospace;
  font-weight: bold;
  color: var(--sdl-primary);
}

.summary-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: var(--sdl-space-4);
}

.summary-tile {
  background-color: var(--sdl-bg-card);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-md);
  padding: var(--sdl-space-4);
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-2);
  min-width: 0;
}

.summary-tile strong {
  font-size: 24px;
  line-height: 1.1;
  color: var(--sdl-text-primary);
}

.summary-label {
  color: var(--sdl-text-muted);
  font-size: 12px;
}

.raw-rule {
  font-family: monospace;
  color: var(--sdl-text-secondary);
  white-space: normal;
  word-break: break-all;
}

.warning-list {
  margin: 0;
  padding-left: var(--sdl-space-4);
  color: var(--sdl-text-secondary);
  line-height: 1.6;
}

.diagnosis-content {
  padding: var(--sdl-space-2);
}

.diagnosis-status {
  display: flex;
  gap: var(--sdl-space-4);
  align-items: flex-start;
}

.status-indicator {
  width: 12px;
  height: 12px;
  border-radius: var(--sdl-radius-pill);
  margin-top: 6px;
  flex-shrink: 0;
}

.status-indicator.healthy {
  background-color: var(--sdl-success);
  box-shadow: 0 0 0 6px var(--sdl-success-soft);
}

.status-indicator.warned {
  background-color: var(--sdl-warning);
  box-shadow: 0 0 0 6px var(--sdl-warning-soft);
}

.status-details h4 {
  margin: 0 0 var(--sdl-space-2);
  font-size: 16px;
  font-weight: 600;
}

.status-details p {
  margin: 0;
  color: var(--sdl-text-secondary);
  line-height: 1.6;
  font-size: 13px;
}

.diagnosis-status.healthy .status-details h4 {
  color: var(--sdl-success);
}

.diagnosis-status.warned .status-details h4 {
  color: var(--sdl-warning);
}

.mb-4 {
  margin-bottom: var(--sdl-space-4);
}

@media (max-width: 760px) {
  .summary-grid {
    grid-template-columns: 1fr;
  }
}
</style>
