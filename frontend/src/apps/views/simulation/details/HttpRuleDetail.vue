<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { SecLabButton, SecLabTable, SecLabTag } from '@/components/ui'

const props = defineProps<{
  config: Record<string, unknown> | null
}>()

const emit = defineEmits<{
  launchBrowser: []
}>()

const { t } = useI18n()

const exploitPaths = computed(
  () => (props.config?.exploit_paths as Array<Record<string, unknown>>) || [],
)
const html = computed(
  () => (props.config?.html as string | undefined) || t('app.simulation.rules.defaultNginxHtml'),
)
</script>

<template>
  <div class="section-card">
    <h4>{{ t('app.simulation.rules.exploitPaths.interceptRoutes') }}</h4>
    <div class="table-scroll-container">
      <SecLabTable
        :data="exploitPaths"
        :columns="[
          { label: t('common.index'), width: 70, align: 'center', slot: 'index' },
          { prop: 'path', label: t('app.simulation.rules.exploitPaths.path'), minWidth: 180 },
          {
            prop: 'trigger_method',
            label: t('app.simulation.rules.exploitPaths.method'),
            width: 100,
            align: 'center',
            slot: 'method',
          },
          {
            prop: 'response_status',
            label: t('app.simulation.rules.exploitPaths.status'),
            width: 100,
            align: 'center',
          },
        ]"
        border
      >
        <template #index="{ index }">
          {{ index + 1 }}
        </template>
        <template #method="{ row }">
          <SecLabTag type="warning" size="small">{{ row.trigger_method || 'ANY' }}</SecLabTag>
        </template>
        <template #empty>
          <div class="empty-placeholder">
            {{ t('app.simulation.rules.exploitPaths.noRoutes') }}
          </div>
        </template>
      </SecLabTable>
    </div>
  </div>

  <div class="section-card">
    <div class="section-header-row flex-layout flex-between flex-align-center">
      <h4>{{ t('app.simulation.rules.mockResponseHtml') }}</h4>
      <SecLabButton
        type="primary"
        size="small"
        class="browser-launch-btn"
        :title="t('app.simulation.rules.preview.launchBrowser')"
        @click="emit('launchBrowser')"
      >
        {{ t('app.simulation.rules.preview.launchBrowser') }}
      </SecLabButton>
    </div>

    <div class="code-preview-container">
      <pre class="code-preview"><code>{{ html }}</code></pre>
    </div>
  </div>
</template>

<style scoped>
.flex-layout {
  display: flex;
}

.flex-between {
  justify-content: space-between;
}

.flex-align-center {
  align-items: center;
}

.section-card h4 {
  margin: 0 0 12px;
  font-size: 14px;
  color: var(--sdl-primary);
  font-weight: 600;
}

.table-scroll-container {
  max-height: 220px;
  overflow-y: auto;
  border-radius: var(--sdl-radius-md);
  border: 1px solid var(--sdl-border-subtle);
}

.empty-placeholder {
  text-align: center;
  padding: 32px 16px;
  color: var(--sdl-text-subtle);
  font-size: 13px;
}

.section-header-row {
  margin-bottom: 14px;
}

.section-header-row h4 {
  margin: 0 !important;
}

.browser-launch-btn {
  height: 24px;
  line-height: 1;
  padding: 0 var(--sdl-space-2);
  font-size: 11px;
}

.code-preview-container {
  max-height: 180px;
  overflow: auto;
  border-radius: var(--sdl-radius-md);
  border: 1px solid var(--sdl-border-subtle);
  background-color: var(--sdl-bg-input);
}

.code-preview {
  margin: 0;
  padding: var(--sdl-space-3);
  font-family: var(--sdl-font-mono);
  font-size: var(--sdl-font-body-sm);
  color: var(--sdl-text-secondary);
  white-space: pre-wrap;
  word-break: break-all;
}
</style>
