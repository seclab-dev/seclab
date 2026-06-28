/**
 * @file useFileEditor.ts
 * @description 封装文件编辑器加载/保存逻辑及状态管理。
 */
import { computed } from 'vue'
import { fsApi } from '@/api/modules/fs'
import { useNotificationStore } from '@/stores/notification'
import { useNodeStore } from '@/stores/node'
import { useI18n } from 'vue-i18n'

export interface EditorTab {
  path: string
  name: string
  content: string
  originalContent: string
  isDirty: boolean
  fileSize: number
  isLoaded: boolean
  isLoading: boolean
  error?: string
}

export function useFileEditor() {
  const nodeStore = useNodeStore()
  const notificationStore = useNotificationStore()
  const { t } = useI18n()
  const fsClient = computed(() => fsApi.forNode(nodeStore.currentNodeId))

  const loadFileContent = async (tab: EditorTab) => {
    tab.isLoading = true
    tab.error = ''
    try {
      const res = await fsClient.value.readFile(tab.path)
      if (!res.success || !res.data) {
        tab.error = res.message || t('app.fileEditor.loadError')
      } else if (!res.data.isText) {
        tab.error = t('app.fileEditor.previewUnsupported')
      } else {
        tab.content = res.data.content
        tab.originalContent = res.data.content
        tab.isLoaded = true
        // Try to estimate file size if not provided by ls
        if (!tab.fileSize) {
          tab.fileSize = new Blob([res.data.content]).size
        }
      }
    } catch (e: unknown) {
      tab.error = (e instanceof Error ? e.message : String(e)) || t('app.fileEditor.loadError')
    } finally {
      tab.isLoading = false
    }
  }

  const saveFileContent = async (tab: EditorTab) => {
    if (!tab.isDirty || tab.isLoading) return false

    try {
      const res = await fsClient.value.writeFile({
        path: tab.path,
        content: tab.content,
        overwrite: true,
      })
      if (!res.success) {
        notificationStore.error(
          t('app.fileEditor.saveFailed', { message: res.message || t('app.common.unknownError') }),
        )
        return false
      }
      tab.originalContent = tab.content
      tab.isDirty = false
      notificationStore.success(t('app.fileEditor.saveSuccess'))
      return true
    } catch (e: unknown) {
      notificationStore.error(
        t('app.fileEditor.saveFailed', {
          message: (e instanceof Error ? e.message : String(e)) || t('app.common.unknownError'),
        }),
      )
      return false
    }
  }

  return {
    loadFileContent,
    saveFileContent,
  }
}
