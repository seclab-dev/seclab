<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { SecLabTable, SecLabTag } from '@/components/ui'

const props = defineProps<{
  config: Record<string, unknown> | null
}>()

const { t } = useI18n()

const redisKeyRows = computed(() =>
  Object.entries((props.config?.keys as Record<string, unknown> | undefined) || {}).map(
    ([key, value]) => ({ key, value }),
  ),
)

const redisCommandRows = computed(
  () => (props.config?.command_responses as Array<Record<string, unknown>> | undefined) || [],
)
</script>

<template>
  <div class="section-card">
    <h4>{{ t('app.simulation.rules.detailSections.redisKeys') }}</h4>
    <div class="table-scroll-container">
      <SecLabTable
        :data="redisKeyRows"
        :columns="[
          { label: t('common.index'), width: 70, align: 'center', slot: 'index' },
          { prop: 'key', label: 'Key', minWidth: 180 },
          { prop: 'value', label: 'Value', minWidth: 260 },
        ]"
        border
      >
        <template #index="{ index }">
          {{ index + 1 }}
        </template>
        <template #empty>
          <div class="empty-placeholder">{{ t('common.none') }}</div>
        </template>
      </SecLabTable>
    </div>
  </div>

  <div class="section-card">
    <h4>{{ t('app.simulation.rules.detailSections.redisCommands') }}</h4>
    <div class="table-scroll-container">
      <SecLabTable
        :data="redisCommandRows"
        :columns="[
          { label: t('common.index'), width: 70, align: 'center', slot: 'index' },
          { prop: 'command', label: 'Command', width: 140 },
          { prop: 'event_type', label: 'Event', width: 150, slot: 'event' },
          { prop: 'response', label: 'Response', minWidth: 260 },
        ]"
        border
      >
        <template #index="{ index }">
          {{ index + 1 }}
        </template>
        <template #event="{ row }">
          <SecLabTag :type="row.event_type === 'exploit_attempt' ? 'danger' : 'info'" size="small">
            {{ row.event_type || 'redis_command' }}
          </SecLabTag>
        </template>
        <template #empty>
          <div class="empty-placeholder">{{ t('common.none') }}</div>
        </template>
      </SecLabTable>
    </div>
  </div>
</template>

<style scoped>
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
</style>
