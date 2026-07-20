<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { SecLabButton, SecLabDialog } from '@/components/ui'
import SecLabIcon from '@/components/icons/SecLabIcon.vue'
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

function openFilePicker() {
  if (!props.busy) fileInput.value?.click()
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
      <div class="import-dialog__picker" data-slot="package-picker">
        <div class="import-dialog__info">
          <div class="import-dialog__filename" data-slot="selected-package">
            {{ selectedFile?.name || t('app.suiteCenter.noPackageSelected') }}
          </div>
          <div class="import-dialog__hint">{{ t('app.suiteCenter.importFormatHint') }}</div>
        </div>
        <input
          id="suite-package-file"
          ref="fileInput"
          class="import-dialog__native-input"
          name="suitePackageFile"
          type="file"
          accept=".slsp"
          :disabled="busy"
          :aria-label="t('app.suiteCenter.selectPackage')"
          @change="handleFile"
        />
        <SecLabButton
          class="import-dialog__select-button"
          :disabled="busy"
          data-ui="suite-file-select"
          @click="openFilePicker"
        >
          <SecLabIcon name="package" :size="14" />
          {{ t('app.suiteCenter.selectPackage') }}
        </SecLabButton>
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
        {{ t('common.import') }}
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
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: center;
  gap: var(--sdl-space-3);
  min-width: 0;
  padding: var(--sdl-space-3);
  border: 1px solid var(--sdl-border-subtle);
  border-radius: var(--sdl-radius-md);
  background: var(--sdl-bg-muted);
}

.import-dialog__info {
  min-width: 0;
}

.import-dialog__filename {
  overflow: hidden;
  color: var(--sdl-text-primary);
  font: var(--sdl-font-body);
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.import-dialog__select-button :deep(.sl-button-content) {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: var(--sdl-space-2);
  line-height: 1;
}

.import-dialog__native-input {
  display: none;
}

.import-dialog__hint,
.import-dialog__error {
  color: var(--sdl-text-muted);
  font: var(--sdl-font-body-sm);
}

.import-dialog__hint {
  margin-top: var(--sdl-space-1);
}

.import-dialog__error {
  color: var(--sdl-danger);
}

@media (max-width: 30rem) {
  .import-dialog__picker {
    grid-template-columns: minmax(0, 1fr);
  }

  .import-dialog__select-button {
    width: 100%;
  }
}
</style>
