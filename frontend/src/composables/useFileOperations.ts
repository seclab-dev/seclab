/**
 * @file useFileOperations.ts
 * @description 封装对文件系统的基础操作 (增、删、改、上传、下载、复制)。
 */
import { computed } from 'vue'
import { fsApi } from '@/api/modules/fs'
import { useNotificationStore } from '@/stores/notification'
import { useNodeStore } from '@/stores/node'
import { useI18n } from 'vue-i18n'

export function useFileOperations() {
  const nodeStore = useNodeStore()
  const notificationStore = useNotificationStore()
  const { t } = useI18n()
  const fsClient = computed(() => fsApi.forNode(nodeStore.currentNodeId))

  const writeFile = async (
    path: string,
    content: string,
    createIfMissing = true,
    overwrite = true,
  ) => {
    const res = await fsClient.value.writeFile({ path, content, createIfMissing, overwrite })
    if (!res.success) {
      notificationStore.error(res.message || t('app.fileManager.writeFailed'))
    }
    return res.success
  }

  const mkdir = async (path: string, recursive = false) => {
    const res = await fsClient.value.mkdir({ path, recursive })
    if (!res.success) {
      notificationStore.error(res.message || t('app.fileManager.mkdirFailed'))
    }
    return res.success
  }

  const removePath = async (path: string, recursive = false) => {
    const res = await fsClient.value.removePath({ path, recursive })
    if (!res.success) {
      notificationStore.error(res.message || t('app.fileManager.removeFailed'))
    }
    return res.success
  }

  const renamePath = async (from: string, to: string, overwrite = false) => {
    const res = await fsClient.value.renamePath({ from, to, overwrite })
    if (!res.success) {
      notificationStore.error(res.message || t('app.fileManager.renameFailed'))
    }
    return res.success
  }

  const copyPath = async (from: string, to: string, overwrite = false) => {
    const res = await fsClient.value.copyPath({ from, to, overwrite })
    if (!res.success) {
      notificationStore.error(res.message || t('app.fileManager.copyFailed'))
    }
    return res.success
  }

  const downloadFile = async (path: string, fileName?: string) => {
    try {
      const res = await fsClient.value.downloadFile(path)
      if (!res.success || !res.data) {
        notificationStore.error(res.message || t('app.fileManager.downloadFailed'))
        return false
      }
      const url = window.URL.createObjectURL(new Blob([res.data]))
      const link = document.createElement('a')
      link.href = url
      link.download = fileName || path.split('/').pop() || 'download'
      document.body.appendChild(link)
      link.click()
      document.body.removeChild(link)
      window.URL.revokeObjectURL(url)
      return true
    } catch (e: unknown) {
      notificationStore.error(
        (e instanceof Error ? e.message : String(e)) || t('app.fileManager.downloadFailed'),
      )
      return false
    }
  }

  const uploadFile = async (targetPath: string, file: File, overwrite = false) => {
    const formData = new FormData()
    formData.append('file', file)
    const res = await fsClient.value.uploadFile(targetPath, formData, overwrite)
    if (!res.success) {
      notificationStore.error(res.message || t('app.fileManager.uploadFailed'))
    }
    return res.success
  }

  return {
    writeFile,
    mkdir,
    removePath,
    renamePath,
    copyPath,
    downloadFile,
    uploadFile,
  }
}
