<script setup lang="ts">
/** Docker Compose 项目详情抽屉，只展示服务事实并提供项目级操作。 */
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDockerStore } from '@/stores/docker'
import type * as dockerType from '@/api/interface/docker'
import SecLabTable, { type SecLabTableColumn } from '@/components/ui/SecLabTable.vue'
import {
  SecLabAlert,
  SecLabButton,
  SecLabDescriptions,
  SecLabDialog,
  SecLabDrawer,
  SecLabEmpty,
  SecLabFormItem,
  SecLabInput,
  SecLabLoading,
  SecLabTag,
} from '@/components/ui'

const props = defineProps<{ projectName: string }>()
const emit = defineEmits<{
  (event: 'close'): void
  (event: 'configure', name: string): void
  (event: 'redeploy', name: string): void
  (event: 'showTasks'): void
}>()
const visible = defineModel<boolean>({ required: true })
const { t } = useI18n()
const store = useDockerStore()
const scaleVisible = ref(false)
const scaleService = ref('')
const scaleReplicas = ref(1)
const scaleError = ref('')

const detail = computed(() => store.projectDetail)
const summary = computed(() => detail.value?.summary)
const descriptions = computed(() => {
  if (!summary.value) return []
  return [
    {
      label: t('app.docker.projects.columns.name'),
      value: summary.value.name,
    },
    {
      label: t('app.docker.projects.columns.management'),
      value: t(`app.docker.projects.management.${summary.value.management.kind}`),
    },
    {
      label: t('app.docker.projects.columns.status'),
      value: t(`app.docker.projects.status.${summary.value.runtimeState}`),
    },
    { label: t('app.docker.projects.columns.services'), value: String(summary.value.serviceCount) },
    {
      label: t('app.docker.projects.columns.runningTotal'),
      value: `${summary.value.containerStates.running}/${summary.value.containerStates.total}`,
    },
    {
      label: t('app.docker.projects.configuration.state'),
      value: t(`app.docker.projects.configuration.states.${summary.value.configurationState}`),
    },
  ]
})

const serviceColumns = computed<SecLabTableColumn[]>(() => [
  { label: t('app.docker.projects.detail.serviceName'), minWidth: 150, prop: 'name' },
  ...(detail.value?.services.some((service) => service.image)
    ? [{ label: t('app.docker.projects.detail.image'), minWidth: 180, slot: 'image' }]
    : []),
  { label: t('app.docker.projects.detail.status'), width: 120, slot: 'status' },
  { label: t('app.docker.projects.columns.runningTotal'), width: 120, slot: 'runningTotal' },
  ...(summary.value?.capabilities.canScale
    ? [{ label: t('app.docker.projects.columns.actions'), width: 100, slot: 'actions' }]
    : []),
])
const containerColumns = computed<SecLabTableColumn[]>(() => {
  const containers = detail.value?.services.flatMap((service) => service.containers) || []
  return [
    { label: t('app.docker.projects.detail.containerName'), minWidth: 150, prop: 'name' },
    { label: t('app.docker.projects.detail.status'), width: 110, prop: 'state' },
    ...(containers.some((container) => container.ipAddresses.length)
      ? [{ label: t('app.docker.projects.detail.ip'), minWidth: 150, slot: 'addresses' }]
      : []),
    ...(containers.some((container) => container.publishedPorts.length)
      ? [{ label: t('app.docker.projects.detail.ports'), minWidth: 180, slot: 'ports' }]
      : []),
  ]
})

const statusType = (state: dockerType.DockerProjectRuntimeState) => {
  if (state === 'running') return 'success'
  if (state === 'partial') return 'warning'
  if (state === 'stopped') return 'info'
  return 'default'
}

const openScale = (service: dockerType.DockerProjectServiceSummary) => {
  scaleService.value = service.name
  scaleReplicas.value = service.containerStates.total
  scaleError.value = ''
  scaleVisible.value = true
}

const submitScale = async () => {
  if (!Number.isInteger(scaleReplicas.value) || scaleReplicas.value < 0) {
    scaleError.value = t('app.docker.projects.detail.invalidReplicas')
    return
  }
  if (
    await store.scaleComposeProjectService(
      props.projectName,
      scaleService.value,
      scaleReplicas.value,
    )
  ) {
    scaleVisible.value = false
    emit('showTasks')
  }
}
</script>

<template>
  <SecLabDrawer
    v-model="visible"
    :title="t('app.docker.projects.detail.title', { name: projectName })"
    width="860px"
    data-ui="project-detail-drawer"
    @close="emit('close')"
  >
    <div class="project-detail" data-slot="detail">
      <SecLabAlert
        v-if="summary?.management.readOnly"
        type="info"
        :title="t('app.docker.projects.detail.readOnlyTitle')"
        :description="t(`app.docker.projects.detail.readOnly.${summary.management.manageVia}`)"
        show-icon
      />
      <SecLabAlert
        v-if="store.projectDetailError"
        type="error"
        :title="t('app.docker.projects.detail.loadFailed')"
        :description="store.projectDetailError"
        show-icon
      />
      <SecLabDescriptions v-if="summary" :items="descriptions" :column="2" border />
      <div
        v-if="summary && !summary.management.readOnly"
        class="detail-actions"
        data-ui="project-actions"
      >
        <SecLabButton
          type="secondary"
          :disabled="!summary.capabilities.canEditConfiguration"
          @click="emit('configure', projectName)"
        >
          {{ t('app.docker.projects.actions.editCompose') }}
        </SecLabButton>
        <SecLabButton
          type="warning"
          :disabled="!summary.capabilities.canRedeploy"
          @click="emit('redeploy', projectName)"
        >
          {{ t('app.docker.projects.actions.redeploy') }}
        </SecLabButton>
      </div>

      <div v-if="detail?.services.length" class="service-block" data-slot="services">
        <div class="block-title">{{ t('app.docker.projects.detail.services') }}</div>
        <SecLabTable :data="detail.services" :columns="serviceColumns" border>
          <template #image="{ row }: { row: dockerType.DockerProjectServiceSummary }">
            <span v-if="row.image" class="mono-text">{{ row.image }}</span>
          </template>
          <template #status="{ row }: { row: dockerType.DockerProjectServiceSummary }">
            <SecLabTag :type="statusType(row.runtimeState)">{{
              t(`app.docker.projects.status.${row.runtimeState}`)
            }}</SecLabTag>
          </template>
          <template #runningTotal="{ row }: { row: dockerType.DockerProjectServiceSummary }">
            {{ row.containerStates.running }}/{{ row.containerStates.total }}
          </template>
          <template #actions="{ row }: { row: dockerType.DockerProjectServiceSummary }">
            <SecLabButton
              v-if="summary?.capabilities.canScale"
              size="small"
              type="secondary"
              @click="openScale(row)"
            >
              {{ t('app.docker.projects.detail.scale') }}
            </SecLabButton>
          </template>
        </SecLabTable>

        <div
          v-for="service in detail.services.filter((item) => item.containers.length)"
          :key="service.name"
          class="container-block"
          data-slot="service-containers"
        >
          <div class="block-title">{{ service.name }}</div>
          <SecLabTable :data="service.containers" :columns="containerColumns" border>
            <template #addresses="{ row }: { row: dockerType.DockerProjectContainerSummary }">
              <span v-if="row.ipAddresses.length" class="mono-text">{{
                row.ipAddresses.join(', ')
              }}</span>
            </template>
            <template #ports="{ row }: { row: dockerType.DockerProjectContainerSummary }">
              <span v-if="row.publishedPorts.length" class="mono-text">{{
                row.publishedPorts.join(', ')
              }}</span>
            </template>
          </SecLabTable>
        </div>
      </div>
      <SecLabEmpty
        v-else-if="detail && !store.projectDetailLoading"
        :description="t('app.docker.projects.detail.emptyServices')"
      />
      <SecLabLoading :loading="store.projectDetailLoading && !detail" cover />
    </div>

    <SecLabDialog
      :visible="scaleVisible"
      :title="t('app.docker.projects.detail.scaleTitle', { service: scaleService })"
      width="460px"
      data-ui="project-scale-dialog"
      @close="scaleVisible = false"
    >
      <div data-slot="body">
        <SecLabAlert v-if="scaleError" type="error" :title="scaleError" show-icon />
        <SecLabFormItem
          :label="t('app.docker.projects.detail.replicas')"
          for="docker-project-service-replicas"
          required
        >
          <SecLabInput
            id="docker-project-service-replicas"
            v-model.number="scaleReplicas"
            name="serviceReplicas"
            type="number"
            min="0"
          />
        </SecLabFormItem>
      </div>
      <template #footer>
        <SecLabButton @click="scaleVisible = false">{{ t('common.cancel') }}</SecLabButton>
        <SecLabButton type="primary" :loading="store.projectMutationLoading" @click="submitScale">{{
          t('common.confirm')
        }}</SecLabButton>
      </template>
    </SecLabDialog>
  </SecLabDrawer>
</template>

<style scoped>
.project-detail {
  position: relative;
  min-height: 100%;
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-4);
}
.detail-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--sdl-space-2);
}
.service-block,
.container-block {
  display: flex;
  flex-direction: column;
  gap: var(--sdl-space-2);
}
.container-block {
  margin-top: var(--sdl-space-3);
}
.block-title {
  color: var(--sdl-text-secondary);
  font-size: var(--sdl-font-size-sm);
  font-weight: var(--sdl-font-weight-semibold);
}
.mono-text {
  font-family: var(--sdl-font-mono);
  font-size: var(--sdl-font-size-xs);
  overflow-wrap: anywhere;
}
</style>
