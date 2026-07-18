<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SuiteCatalogItem, SuiteInstanceSummary } from '@/api/interface/suites'
import { SecLabButton, SecLabCheckbox, SecLabDialog } from '@/components/ui'

const props = defineProps<{
  deleteTarget: SuiteCatalogItem | null
  uninstallSuite: SuiteCatalogItem | null
  uninstallInstance?: SuiteInstanceSummary
  busy: boolean
}>()

defineEmits<{
  closeDelete: []
  confirmDelete: []
  closeUninstall: []
  confirmUninstall: [removeData: boolean]
}>()

const { t } = useI18n()
const removeData = ref(false)

watch(
  () => props.uninstallSuite,
  () => {
    removeData.value = false
  },
)
</script>

<template>
  <SecLabDialog
    :visible="!!deleteTarget"
    :title="t('app.suiteCenter.deleteTitle')"
    width="30rem"
    :close-on-click-overlay="!busy"
    @close="$emit('closeDelete')"
  >
    <div class="danger-dialog" data-ui="suite-delete-confirmation">
      {{ t('app.suiteCenter.deleteConfirm', { name: deleteTarget?.name }) }}
    </div>
    <template #footer>
      <SecLabButton type="secondary" :disabled="busy" @click="$emit('closeDelete')">
        {{ t('common.cancel') }}
      </SecLabButton>
      <SecLabButton type="danger" :loading="busy" @click="$emit('confirmDelete')">
        {{ t('common.delete') }}
      </SecLabButton>
    </template>
  </SecLabDialog>

  <SecLabDialog
    :visible="!!uninstallSuite && !!uninstallInstance"
    :title="t('app.suiteCenter.uninstallTitle')"
    width="32rem"
    :close-on-click-overlay="!busy"
    @close="$emit('closeUninstall')"
  >
    <div class="danger-dialog" data-ui="suite-uninstall-confirmation">
      <p>{{ t('app.suiteCenter.uninstallConfirm', { name: uninstallSuite?.name }) }}</p>
      <SecLabCheckbox v-model="removeData" :disabled="busy">
        {{ t('app.suiteCenter.removeData') }}
      </SecLabCheckbox>
      <div v-if="removeData" class="danger-dialog__warning" role="alert">
        <strong>{{ t('app.suiteCenter.removeDataSecondWarningTitle') }}</strong>
        <span>{{ t('app.suiteCenter.removeDataWarning') }}</span>
      </div>
    </div>
    <template #footer>
      <SecLabButton type="secondary" :disabled="busy" @click="$emit('closeUninstall')">
        {{ t('common.cancel') }}
      </SecLabButton>
      <SecLabButton
        type="danger"
        :loading="busy"
        data-ui="suite-confirm-uninstall"
        @click="$emit('confirmUninstall', removeData)"
      >
        {{
          removeData
            ? t('app.suiteCenter.uninstallAndRemoveData')
            : t('app.suiteCenter.actions.uninstall')
        }}
      </SecLabButton>
    </template>
  </SecLabDialog>
</template>

<style scoped>
.danger-dialog {
  display: grid;
  gap: var(--sdl-space-4);
  color: var(--sdl-text-secondary);
  font: var(--sdl-font-body);
}

.danger-dialog p {
  margin: 0;
}

.danger-dialog__warning {
  display: grid;
  gap: var(--sdl-space-2);
  padding: var(--sdl-space-3);
  border: 1px solid var(--sdl-danger);
  border-radius: var(--sdl-radius-md);
  color: var(--sdl-danger);
  background: var(--sdl-danger-soft);
}
</style>
