<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { SecLabButton, SecLabDialog } from '@/components/ui'
import { validateSuitePackage } from './useSuiteCenter'

const props = defineProps<{
  visible: boolean
  busy: boolean
}>()

const emit = defineEmits<{
  close: []
  submit: [file: File]
}>()

const { t } = useI18n()
const fileInput = ref<HTMLInputElement | null>(null)
const selectedFile = ref<File | null>(null)
const validationError = ref('')

function reset() {
  selectedFile.value = null
  validationError.value = ''
  if (fileInput.value) fileInput.value.value = ''
}

function handleClose() {
  if (props.busy) return
  emit('close')
}

function handleFile(event: Event) {
  selectedFile.value = (event.target as HTMLInputElement).files?.[0] ?? null
  validationError.value = ''
  if (!selectedFile.value) return
  const validation = validateSuitePackage(selectedFile.value)
  if (!validation.valid) {
    validationError.value = t(`app.suiteCenter.importValidation.${validation.reason}`)
  }
}

function submit() {
  if (selectedFile.value && !validationError.value) emit('submit', selectedFile.value)
}

watch(
  () => props.visible,
  (visible) => {
    if (!visible) reset()
  },
)
</script>

<template>
  <SecLabDialog
    :visible="visible"
    :title="t('app.suiteCenter.importTitle')"
    width="32rem"
    :close-on-click-overlay="!busy"
    @close="handleClose"
  >
    <div class="import-dialog" data-ui="suite-import-dialog">
      <label class="import-dialog__picker" for="suite-package-file">
        <span class="import-dialog__label">{{ t('app.suiteCenter.selectPackage') }}</span>
        <span class="import-dialog__hint">{{ t('app.suiteCenter.importFormatHint') }}</span>
        <input
          id="suite-package-file"
          ref="fileInput"
          type="file"
          accept=".slsp"
          :disabled="busy"
          @change="handleFile"
        />
      </label>
      <div v-if="selectedFile" class="import-dialog__file" data-slot="selected-package">
        {{ selectedFile.name }}
      </div>
      <div v-if="validationError" class="import-dialog__error" role="alert">
        {{ validationError }}
      </div>
    </div>
    <template #footer>
      <SecLabButton type="secondary" :disabled="busy" @click="handleClose">
        {{ t('common.cancel') }}
      </SecLabButton>
      <SecLabButton
        type="primary"
        :disabled="!selectedFile || !!validationError"
        :loading="busy"
        data-ui="suite-confirm-import"
        @click="submit"
      >
        {{ t('app.suiteCenter.confirmImport') }}
      </SecLabButton>
    </template>
  </SecLabDialog>
</template>

<style scoped>
.import-dialog {
  display: grid;
  gap: var(--sdl-space-3);
}

.import-dialog__picker {
  display: grid;
  gap: var(--sdl-space-2);
  padding: var(--sdl-space-5);
  border: 1px dashed var(--sdl-border-strong);
  border-radius: var(--sdl-radius-md);
  background: var(--sdl-bg-muted);
  cursor: pointer;
}

.import-dialog__label {
  color: var(--sdl-text-primary);
  font: var(--sdl-font-subtitle);
}

.import-dialog__hint,
.import-dialog__file,
.import-dialog__error {
  color: var(--sdl-text-muted);
  font: var(--sdl-font-body-sm);
}

.import-dialog__file {
  overflow-wrap: anywhere;
  color: var(--sdl-text-secondary);
}

.import-dialog__error {
  color: var(--sdl-danger);
}
</style>
