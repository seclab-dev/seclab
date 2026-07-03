<script setup lang="ts">
/**
 * @file DockerImageRegistry.vue
 * @description Docker 镜像仓库页面，负责从远端仓库拉取镜像到当前节点。
 */

import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { dockerApi } from '@/api/modules/docker'
import { useDockerStore } from '@/stores/docker'
import { useNodeStore } from '@/stores/node'
import { useNotificationStore } from '@/stores/notification'
import { SecLabAlert, SecLabButton, SecLabInput } from '@/components/ui'

const { t } = useI18n()
const dockerStore = useDockerStore()
const nodeStore = useNodeStore()
const notificationStore = useNotificationStore()

const imageName = ref('')
const isPulling = ref(false)

const currentNodeName = computed(() => {
  const node = nodeStore.nodes.find((item) => item.id === nodeStore.currentNodeId)
  return node?.name || nodeStore.currentNodeId || 'local'
})

const canPull = computed(() => imageName.value.trim().length > 0 && !isPulling.value)

const pullImage = async () => {
  const normalizedImageName = imageName.value.trim()
  if (!normalizedImageName) return

  isPulling.value = true
  try {
    const response = await dockerApi.forNode(nodeStore.currentNodeId).pullImage(normalizedImageName)
    if (response.success) {
      notificationStore.success(t('app.docker.images.registry.success'))
      imageName.value = ''
      await dockerStore.fetchImagesList()
    } else {
      notificationStore.error(
        t('app.docker.images.registry.failed', {
          error: response.message || t('common.unknownError'),
        }),
      )
    }
  } catch (error) {
    console.error('Failed to pull image:', error)
    const errMsg = error instanceof Error ? error.message : String(error)
    notificationStore.error(t('app.docker.images.registry.failed', { error: errMsg }))
  } finally {
    isPulling.value = false
  }
}
</script>

<template>
  <div class="registry-page" data-ui="docker-image-registry">
    <SecLabAlert
      type="info"
      show-icon
      :description="t('app.docker.images.registry.targetHint', { node: currentNodeName })"
      data-ui="registry-target-hint"
    />

    <div class="registry-form" data-ui="registry-pull-form">
      <SecLabInput
        v-model="imageName"
        :placeholder="t('app.docker.images.registry.placeholder')"
        :disabled="isPulling"
        data-ui="registry-image-input"
        @keyup.enter="pullImage"
      />
      <SecLabButton
        type="primary"
        :disabled="!canPull"
        class="registry-pull-button"
        data-ui="registry-pull-button"
        @click="pullImage"
      >
        <span v-if="isPulling" class="spinner">⏳</span>
        <span>{{
          isPulling ? t('app.docker.images.registry.pulling') : t('app.docker.images.registry.pull')
        }}</span>
      </SecLabButton>
    </div>
  </div>
</template>

<style scoped>
.registry-page {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-4);
  height: 100%;
  padding: var(--sdl-space-1);
}

.registry-form {
  display: flex;
  align-items: center;
  gap: var(--sdl-space-3);
  max-width: 640px;
}

.registry-form :deep(.sdl-input-wrapper) {
  flex: 1;
}

.registry-pull-button {
  min-width: 96px;
  height: 38px;
  flex-shrink: 0;
}

.spinner {
  display: inline-block;
  animation: spin 1s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
