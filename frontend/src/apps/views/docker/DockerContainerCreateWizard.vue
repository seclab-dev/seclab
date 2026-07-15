<script setup lang="ts">
/**
 * @file DockerContainerCreateWizard.vue
 * @description Docker 容器创建向导，提供镜像选择及容器网络、环境变量、端口、卷挂载等配置，符合 SDL 设计规范。
 */

import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDockerStore } from '@/stores/docker'
import { useNotificationStore } from '@/stores/notification'
import { useContainerCreateForm } from './composables/useContainerCreateForm'
import {
  SecLabButton,
  SecLabSelect,
  SecLabInput,
  SecLabFormItem,
  SecLabCheckbox,
} from '@/components/ui'

const { t } = useI18n()
const store = useDockerStore()
const notificationStore = useNotificationStore()

// ─── 镜像选项 ───
const imageOptions = computed(() => {
  return (store.imagesList || []).map((img) => ({
    value: img.id,
    label:
      img.tags[0] ||
      img.id.replace(/^sha256:/, '').substring(0, 12) ||
      t('app.docker.containers.unknownImage'),
  }))
})

// ─── 重启策略 ───
const restartPolicyOptions = [
  { label: 'no', value: 'no' },
  { label: 'on-failure', value: 'on-failure' },
  { label: 'always', value: 'always' },
  { label: 'unless-stopped', value: 'unless-stopped' },
]

// ─── 网络选项 ───
const networkOptions = computed(() => [
  { label: t('app.docker.containers.default'), value: '' },
  ...(store.networks || []).map((net) => ({
    label: net.name || net.id,
    value: net.name || net.id,
  })),
])

// ─── 容器创建表单 Hook ───
const {
  envRows,
  portRows,
  volumeRows,
  handleSelectedImageIdChange,
  handleContainerNameChange,
  handleContainerCommandChange,
  handleRestartPolicyChange,
  handleNetworkChange,
  handleAutoRemoveChange,
  handleListRowModelUpdate,
  addListRow,
  removeListRow,
  getListActionLabel,
  proceedCreate,
  backToSelect,
  cancelCreate,
  submitCreate,
} = useContainerCreateForm({
  containerForm: computed(() => store.containerForm),
  selectedImageId: computed(() => store.selectedImageId),
  isCreateActive: computed(() => store.isContainerCreateActive),
  t,
  onUpdateContainerForm: (value) => {
    store.containerForm = value
  },
  onUpdateSelectedImageId: (value) => {
    store.selectedImageId = value
  },
  onUpdateStep: (value) => {
    store.containerStep = value
  },
  onCancelCreate: () => {
    store.cancelContainerCreate()
  },
  onSubmitCreate: () => {
    store.submitContainerConfig()
  },
  onCreatePanelOpened: () => {},
  onError: (message) => notificationStore.error(message),
})
</script>

<template>
  <div class="docker-card docker-card-create" data-ui="docker-container-create-wizard">
    <div class="container-create-panel">
      <div class="create-panel-header">
        <h3>
          {{
            store.containerStep === 'selectImage'
              ? t('app.docker.containers.selectImage')
              : t('app.docker.containers.configure')
          }}
        </h3>
      </div>
      <div class="create-panel-body sl-form">
        <!-- 步骤 1：选择镜像 -->
        <div v-if="store.containerStep === 'selectImage'" class="create-panel-step">
          <SecLabFormItem :label="t('app.docker.containers.selectImage')">
            <SecLabSelect
              :model-value="store.selectedImageId ?? ''"
              :placeholder="t('app.docker.containers.selectImagePrompt')"
              :options="imageOptions"
              @update:model-value="handleSelectedImageIdChange"
              data-ui="create-select-image"
            />
          </SecLabFormItem>
        </div>

        <!-- 步骤 2：配置容器参数 -->
        <div v-else class="container-config">
          <div class="selected-image" v-if="store.selectedImageLabel">
            <strong>{{ t('app.docker.containers.image') }}:</strong>
            {{ store.selectedImageLabel }}
          </div>

          <div class="sl-form-grid">
            <SecLabFormItem :label="t('app.docker.containers.containerName')">
              <SecLabInput
                :model-value="store.containerForm?.name || ''"
                :placeholder="t('app.docker.containers.containerNamePlaceholder')"
                @update:model-value="handleContainerNameChange"
                data-ui="create-container-name"
              />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.docker.containers.startCommand')">
              <SecLabInput
                :model-value="store.containerForm?.command || ''"
                :placeholder="t('app.docker.containers.startCommandPlaceholder')"
                @update:model-value="handleContainerCommandChange"
                data-ui="create-start-command"
              />
            </SecLabFormItem>
          </div>

          <!-- 环境变量 -->
          <SecLabFormItem :label="t('app.docker.containers.envVars')">
            <div class="list-inputs">
              <div v-for="(row, index) in envRows" :key="`env-${index}`" class="list-row">
                <SecLabInput
                  :model-value="row.key"
                  class="input-key"
                  :placeholder="t('app.docker.containers.varName')"
                  @update:model-value="(val) => handleListRowModelUpdate('env', index, 'key', val)"
                />
                <SecLabInput
                  :model-value="row.value"
                  class="input-value"
                  :placeholder="t('app.docker.containers.varValue')"
                  @update:model-value="
                    (val) => handleListRowModelUpdate('env', index, 'value', val)
                  "
                />
                <SecLabButton @click="removeListRow('env', index)" size="small">
                  {{ getListActionLabel(envRows) }}
                </SecLabButton>
              </div>
              <div class="list-add">
                <SecLabButton size="small" @click="addListRow('env')">
                  + {{ t('app.docker.containers.add') }}
                </SecLabButton>
              </div>
            </div>
          </SecLabFormItem>

          <!-- 端口映射 -->
          <SecLabFormItem :label="t('app.docker.containers.portMappings')">
            <div class="list-inputs">
              <div v-for="(row, index) in portRows" :key="`ports-${index}`" class="list-row">
                <SecLabInput
                  :model-value="row.key"
                  class="input-key"
                  :placeholder="t('app.docker.containers.hostPort')"
                  @update:model-value="
                    (val) => handleListRowModelUpdate('ports', index, 'key', val)
                  "
                />
                <SecLabInput
                  :model-value="row.value"
                  class="input-value"
                  :placeholder="t('app.docker.containers.containerPort')"
                  @update:model-value="
                    (val) => handleListRowModelUpdate('ports', index, 'value', val)
                  "
                />
                <SecLabButton @click="removeListRow('ports', index)" size="small">
                  {{ getListActionLabel(portRows) }}
                </SecLabButton>
              </div>
              <div class="list-add">
                <SecLabButton size="small" @click="addListRow('ports')">
                  + {{ t('app.docker.containers.add') }}
                </SecLabButton>
              </div>
            </div>
          </SecLabFormItem>

          <!-- 卷挂载 -->
          <SecLabFormItem :label="t('app.docker.containers.volumeMounts')">
            <div class="list-inputs">
              <div v-for="(row, index) in volumeRows" :key="`volumes-${index}`" class="list-row">
                <SecLabInput
                  :model-value="row.key"
                  class="input-key"
                  :placeholder="t('app.docker.containers.hostPath')"
                  @update:model-value="
                    (val) => handleListRowModelUpdate('volumes', index, 'key', val)
                  "
                />
                <SecLabInput
                  :model-value="row.value"
                  class="input-value"
                  :placeholder="t('app.docker.containers.containerPath')"
                  @update:model-value="
                    (val) => handleListRowModelUpdate('volumes', index, 'value', val)
                  "
                />
                <SecLabButton @click="removeListRow('volumes', index)" size="small">
                  {{ getListActionLabel(volumeRows) }}
                </SecLabButton>
              </div>
              <div class="list-add">
                <SecLabButton size="small" @click="addListRow('volumes')">
                  + {{ t('app.docker.containers.add') }}
                </SecLabButton>
              </div>
            </div>
          </SecLabFormItem>

          <!-- 重启策略 & 网络 -->
          <div class="form-row sl-form-grid">
            <SecLabFormItem :label="t('app.docker.containers.restartPolicy')">
              <SecLabSelect
                :model-value="store.containerForm?.restartPolicy || 'no'"
                :options="restartPolicyOptions"
                @update:model-value="handleRestartPolicyChange"
                data-ui="create-restart-policy"
              />
            </SecLabFormItem>
            <SecLabFormItem :label="t('app.docker.containers.network')">
              <SecLabSelect
                :model-value="store.containerForm?.network || ''"
                :options="networkOptions"
                @update:model-value="handleNetworkChange"
                data-ui="create-network"
              />
            </SecLabFormItem>
          </div>

          <!-- 自动删除 -->
          <SecLabFormItem>
            <SecLabCheckbox
              :model-value="store.containerForm?.autoRemove || false"
              @update:model-value="handleAutoRemoveChange"
              data-ui="create-auto-remove"
            >
              {{ t('app.docker.containers.autoRemove') }}
            </SecLabCheckbox>
          </SecLabFormItem>
        </div>
      </div>
      <div class="create-panel-footer">
        <SecLabButton @click="cancelCreate" data-ui="create-cancel-btn">
          {{ t('app.docker.containers.cancel') }}
        </SecLabButton>
        <SecLabButton
          v-if="store.containerStep === 'config'"
          @click="backToSelect"
          data-ui="create-back-btn"
        >
          {{ t('app.docker.containers.backToSelect') }}
        </SecLabButton>
        <SecLabButton
          v-if="store.containerStep === 'selectImage'"
          type="primary"
          @click="proceedCreate"
          data-ui="create-next-btn"
        >
          {{ t('app.docker.containers.nextStep') }}
        </SecLabButton>
        <SecLabButton v-else type="primary" @click="submitCreate" data-ui="create-submit-btn">
          {{ t('app.docker.containers.createContainer') }}
        </SecLabButton>
      </div>
    </div>
  </div>
</template>

<style scoped>
.docker-card {
  display: flex;
  flex-direction: column;
  min-height: 0;
  margin-top: var(--sdl-space-3);
  background-color: var(--sdl-bg-panel);
  padding: var(--sdl-space-5);
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-lg);
  box-shadow: var(--sdl-shadow-panel);
  margin-bottom: var(--sdl-space-5);
  flex-grow: 1;
  overflow: hidden;
}

.docker-card-create {
  flex-grow: 0;
  min-height: auto;
}

.container-create-panel {
  border: 1px solid var(--sdl-border-default);
  border-radius: var(--sdl-radius-lg);
  padding: var(--sdl-space-5);
  background: var(--sdl-bg-card);
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}

.create-panel-header h3 {
  margin: 0 0 var(--sdl-space-3) 0;
  color: var(--sdl-primary);
  font-size: var(--sdl-font-title);
}

.create-panel-body {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
  flex: 1;
  min-height: 0;
  overflow: auto;
}

.container-config {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-3);
}

.selected-image {
  padding: var(--sdl-space-3);
  background: var(--sdl-bg-muted);
  border: 1px solid var(--sdl-border-brand);
  border-radius: var(--sdl-radius-md);
  font-size: var(--sdl-font-body-sm);
  color: var(--sdl-text-primary);
}

.selected-image strong {
  color: var(--sdl-primary);
  margin-right: var(--sdl-space-1);
}

.form-row {
  display: flex;
  gap: var(--sdl-space-4);
  flex-wrap: wrap;
}

.list-inputs {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-2);
}

.list-row {
  display: flex;
  gap: var(--sdl-space-2);
  align-items: center;
}

.input-key {
  min-width: 140px;
  flex: 1;
}

.input-value {
  min-width: 180px;
  flex: 2;
}

.list-add {
  margin-top: var(--sdl-space-1);
}

.create-panel-footer {
  display: flex;
  gap: var(--sdl-space-3);
  margin-top: var(--sdl-space-4);
  flex-wrap: wrap;
  flex-shrink: 0;
  justify-content: flex-end;
}
</style>
