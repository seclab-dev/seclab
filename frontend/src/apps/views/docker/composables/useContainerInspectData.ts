import { dockerApi } from '@/api/modules/docker'
import type * as dockerType from '@/api/interface/docker'
import { ref, watch, type Ref } from 'vue'
import { useI18n } from 'vue-i18n'

type UseContainerInspectDataOptions = {
  selectedContainerId: Ref<string | null>
  onError: (message: string) => void
}

export const useContainerInspectData = ({
  selectedContainerId,
  onError,
}: UseContainerInspectDataOptions) => {
  const { t } = useI18n()
  const inspectDetail = ref<dockerType.ContainerInspect | null>(null)
  const isInspectLoading = ref(false)

  let requestVersion = 0

  const reset = () => {
    requestVersion += 1
    inspectDetail.value = null
    isInspectLoading.value = false
  }

  const load = async (id: string) => {
    const currentRequest = ++requestVersion
    isInspectLoading.value = true

    try {
      const res = await dockerApi.inspectContainer(id)

      if (currentRequest !== requestVersion) return
      if (selectedContainerId.value !== id) return

      if (res.success && res.data) {
        inspectDetail.value = res.data
        return
      }

      inspectDetail.value = null
      onError(
        t('app.docker.messages.inspectContainerFailed', {
          message: res.message || t('common.unknownError'),
        }),
      )
    } finally {
      if (currentRequest === requestVersion) {
        isInspectLoading.value = false
      }
    }
  }

  watch(
    selectedContainerId,
    (id) => {
      if (!id) {
        reset()
        return
      }
      inspectDetail.value = null
      void load(id)
    },
    { immediate: true },
  )

  return {
    inspectDetail,
    isInspectLoading,
    reloadInspect: async () => {
      const id = selectedContainerId.value
      if (!id) return
      await load(id)
    },
    reset,
  }
}
