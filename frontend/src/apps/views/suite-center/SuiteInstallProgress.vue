<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SuiteInstallTaskResponse } from '@/api/interface/suites'
import { SecLabButton, SecLabTag } from '@/components/ui'

const props = defineProps<{
  task: SuiteInstallTaskResponse
  error?: string
}>()

defineEmits<{
  cancel: []
  retry: []
}>()

const { t } = useI18n()
const percent = computed(() => Math.max(0, Math.min(100, props.task.progressPercent)))
const statusType = computed(() => {
  if (props.task.status === 'success') return 'success'
  if (props.task.status === 'failed') return 'danger'
  if (props.task.status === 'canceled') return 'default'
  if (props.task.status === 'canceling') return 'warning'
  return 'primary'
})

function stepLabel() {
  const key = `app.suiteCenter.installProgress.steps.${props.task.currentStep}`
  const label = t(key)
  return label === key ? t('app.suiteCenter.status.unknown') : label
}
</script>

<template>
  <div class="install-task" data-ui="suite-install-task" :data-slot="task.taskId">
    <div class="install-task__header">
      <div>
        <div class="install-task__step">{{ stepLabel() }}</div>
        <div v-if="task.currentImage" class="install-task__image">{{ task.currentImage }}</div>
      </div>
      <SecLabTag :type="statusType" effect="plain">{{ percent }}%</SecLabTag>
    </div>
    <div
      class="install-task__track"
      role="progressbar"
      :aria-label="t('app.suiteCenter.installProgress.ariaLabel')"
      aria-valuemin="0"
      aria-valuemax="100"
      :aria-valuenow="percent"
    >
      <div class="install-task__bar" :style="{ width: `${percent}%` }" />
    </div>
    <div v-if="error || task.error" class="install-task__error" role="alert">
      <span>{{ error || task.error }}</span>
      <SecLabButton v-if="error" size="small" type="secondary" @click="$emit('retry')">
        {{ t('common.retry') }}
      </SecLabButton>
    </div>
    <div v-if="!task.isFinished" class="install-task__actions">
      <SecLabButton
        size="small"
        type="secondary"
        :disabled="task.cancelRequested"
        data-ui="suite-install-cancel"
        @click="$emit('cancel')"
      >
        {{
          task.cancelRequested
            ? t('app.suiteCenter.installProgress.canceling')
            : t('app.suiteCenter.installProgress.cancel')
        }}
      </SecLabButton>
    </div>
  </div>
</template>

<style scoped>
.install-task {
  display: grid;
  gap: var(--sdl-space-3);
  padding: var(--sdl-space-4);
  border: 1px solid var(--sdl-border-subtle);
  border-radius: var(--sdl-radius-md);
  background: var(--sdl-bg-muted);
}

.install-task__header,
.install-task__error,
.install-task__actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sdl-space-3);
}

.install-task__step {
  color: var(--sdl-text-primary);
  font: var(--sdl-font-body);
}

.install-task__image {
  overflow: hidden;
  max-width: 34rem;
  color: var(--sdl-text-muted);
  font: var(--sdl-font-code);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.install-task__track {
  overflow: hidden;
  height: var(--sdl-space-2);
  border-radius: var(--sdl-radius-pill);
  background: var(--sdl-border-default);
}

.install-task__bar {
  height: 100%;
  border-radius: inherit;
  background: var(--sdl-primary);
  transition: width 180ms ease;
}

.install-task__error {
  align-items: flex-start;
  color: var(--sdl-danger);
  font: var(--sdl-font-body-sm);
}

.install-task__actions {
  justify-content: flex-end;
}
</style>
