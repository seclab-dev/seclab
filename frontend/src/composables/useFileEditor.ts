/**
 * @file useFileEditor.ts
 * @description 封装固定节点上的文件编辑器加载、乐观并发保存和最新请求状态。
 */
import { computed, type Ref } from 'vue'
import { fsApi } from '@/api/modules/fs'
import { useNotificationStore } from '@/stores/notification'
import { useI18n } from 'vue-i18n'

export interface EditorTab {
  path: string
  name: string
  content: string
  originalContent: string
  revision: string
  isDirty: boolean
  fileSize: number
  isLoaded: boolean
  isLoading: boolean
  isSaving: boolean
  requestSequence: number
  error?: string
}

export function useFileEditor(nodeId: Readonly<Ref<string>>) {
  const notificationStore = useNotificationStore()
  const { t } = useI18n()
  const fsClient = computed(() => fsApi.forNode(nodeId.value))

  const loadFileContent = async (tab: EditorTab) => {
    const requestSequence = ++tab.requestSequence
    const requestNodeId = nodeId.value
    tab.isLoading = true
    tab.error = ''
    try {
      const res = await fsApi.forNode(requestNodeId).readFile(tab.path)
      if (requestSequence !== tab.requestSequence || requestNodeId !== nodeId.value) return
      if (!res.success || !res.data) {
        tab.error = res.message || t('app.fileEditor.loadError')
      } else {
        tab.content = res.data.content
        tab.originalContent = res.data.content
        tab.revision = res.data.revision
        tab.fileSize = res.data.sizeBytes
        tab.isLoaded = true
        tab.isDirty = false
      }
    } catch (error) {
      if (requestSequence !== tab.requestSequence || requestNodeId !== nodeId.value) return
      tab.error = error instanceof Error ? error.message : t('app.fileEditor.loadError')
    } finally {
      if (requestSequence === tab.requestSequence && requestNodeId === nodeId.value) {
        tab.isLoading = false
      }
    }
  }

  const saveFileContent = async (tab: EditorTab) => {
    if (!tab.isDirty || tab.isLoading || tab.isSaving) return false

    const contentToSave = tab.content
    tab.isSaving = true
    try {
      const res = await fsClient.value.writeFile({
        path: tab.path,
        content: contentToSave,
        expectedRevision: tab.revision,
      })
      if (!res.success || !res.data) {
        notificationStore.error(
          t('app.fileEditor.saveFailed', { message: res.message || t('app.common.unknownError') }),
        )
        return false
      }
      tab.originalContent = contentToSave
      tab.fileSize = res.data.sizeBytes
      tab.revision = res.data.revision
      tab.isDirty = tab.content !== contentToSave
      notificationStore.success(t('app.fileEditor.saveSuccess'))
      return true
    } catch (error) {
      notificationStore.error(
        t('app.fileEditor.saveFailed', {
          message: error instanceof Error ? error.message : t('app.common.unknownError'),
        }),
      )
      return false
    } finally {
      tab.isSaving = false
    }
  }

  return {
    loadFileContent,
    saveFileContent,
  }
}
