/**
 * @file useFileOperations.ts
 * @description 封装固定节点上的同步文件操作、持久任务与分块传输。
 */
import { computed, type Ref } from 'vue'
import { fsApi } from '@/api/modules/fs'
import type { FileOperation, FileOperationItemRequest, FileOperationTask } from '@/api/interface/fs'
import { useToastStore } from '@/stores/toast'
import { useI18n } from 'vue-i18n'

const UPLOAD_CHUNK_BYTES = 8 * 1024 * 1024
const TASK_POLL_INTERVAL_MS = 500
const TERMINAL_TASK_STATUSES = new Set(['succeeded', 'failed', 'cancelled'])
const TERMINAL_TRANSFER_STATUSES = new Set(['completed', 'failed', 'cancelled', 'expired'])

const newIdempotencyKey = () =>
  globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`

const wait = (delayMs: number) => new Promise((resolve) => window.setTimeout(resolve, delayMs))

export function useFileOperations(nodeId: Readonly<Ref<string>>) {
  const toastStore = useToastStore()
  const { t } = useI18n()
  const fsClient = computed(() => fsApi.forNode(nodeId.value))

  const createFile = async (path: string, content = '') => {
    const res = await fsClient.value.createFile({ path, content })
    if (!res.success) toastStore.error(res.message || t('app.fileManager.writeFailed'))
    return res.success
  }

  const mkdir = async (path: string, recursive = false) => {
    const res = await fsClient.value.mkdir({ path, recursive })
    if (!res.success) toastStore.error(res.message || t('app.fileManager.mkdirFailed'))
    return res.success
  }

  /** 串行等待同一任务，确保轮询不会重叠。 */
  const pollTask = async (task: FileOperationTask) => {
    let current = task
    while (!TERMINAL_TASK_STATUSES.has(current.status)) {
      await wait(TASK_POLL_INTERVAL_MS)
      const res = await fsClient.value.taskDetail(current.taskId)
      if (!res.success || !res.data) {
        toastStore.error(res.message || t('app.fileManager.fetchError'))
        return false
      }
      current = res.data
    }
    if (current.status !== 'succeeded') {
      toastStore.error(current.errorSummary || t('app.fileManager.fetchError'))
      return false
    }
    return true
  }

  const runPathTask = async (
    operation: FileOperation,
    items: FileOperationItemRequest[],
    recursive = false,
    targetDirectory?: string,
  ) => {
    const res = await fsClient.value.createTask({
      operation,
      items,
      targetDirectory,
      recursive,
      overwrite: false,
      idempotencyKey: newIdempotencyKey(),
    })
    if (!res.success || !res.data) {
      toastStore.error(res.message || t('app.fileManager.fetchError'))
      return false
    }
    return pollTask(res.data)
  }

  const removePath = (path: string, recursive = false, expectedRevision?: string) =>
    runPathTask('remove', [{ path, expectedRevision }], recursive)

  const renamePath = (from: string, to: string, expectedRevision?: string) =>
    runPathTask('move', [{ path: from, targetPath: to, expectedRevision }], true)

  const copyPath = (from: string, to: string, expectedRevision?: string) =>
    runPathTask('copy', [{ path: from, targetPath: to, expectedRevision }], true)

  /** 串行跟踪单个传输，避免同一传输产生重叠轮询。 */
  const pollTransfer = async (transferId: string) => {
    while (true) {
      await wait(TASK_POLL_INTERVAL_MS)
      const res = await fsClient.value.transferDetail(transferId)
      if (!res.success || !res.data) {
        toastStore.error(res.message || t('app.fileManager.fetchError'))
        return false
      }
      if (!TERMINAL_TRANSFER_STATUSES.has(res.data.status)) continue
      if (res.data.status !== 'completed') {
        toastStore.error(res.data.errorSummary || t('app.fileManager.downloadFailed'))
        return false
      }
      return true
    }
  }

  const downloadFile = async (path: string, fileName?: string, expectedRevision?: string) => {
    const client = fsClient.value
    const res = await client.createTransfer({
      direction: 'download',
      path,
      expectedRevision,
      overwrite: false,
    })
    if (!res.success || !res.data) {
      toastStore.error(res.message || t('app.fileManager.downloadFailed'))
      return false
    }
    const link = document.createElement('a')
    link.href = client.downloadUrl(res.data.transferId)
    link.download = fileName || path.split('/').pop() || 'download'
    document.body.appendChild(link)
    link.click()
    link.remove()
    return pollTransfer(res.data.transferId)
  }

  const uploadFile = async (targetPath: string, file: File) => {
    const client = fsClient.value
    const active = await client.activeTransfers()
    if (!active.success) {
      toastStore.error(active.message || t('app.fileManager.uploadFailed'))
      return false
    }
    const resumable = active.data?.find(
      (transfer) =>
        transfer.direction === 'upload' &&
        transfer.path === targetPath &&
        transfer.sizeBytes === file.size &&
        !TERMINAL_TRANSFER_STATUSES.has(transfer.status),
    )
    const created = resumable
      ? { success: true, data: resumable, message: '' }
      : await client.createTransfer({
          direction: 'upload',
          path: targetPath,
          sizeBytes: file.size,
          overwrite: false,
        })
    if (!created.success || !created.data) {
      toastStore.error(created.message || t('app.fileManager.uploadFailed'))
      return false
    }

    const transferId = created.data.transferId
    try {
      for (
        let start = created.data.transferredBytes;
        start < file.size;
        start += UPLOAD_CHUNK_BYTES
      ) {
        const endExclusive = Math.min(start + UPLOAD_CHUNK_BYTES, file.size)
        const chunk = await file.slice(start, endExclusive).arrayBuffer()
        const uploaded = await client.uploadChunk(
          transferId,
          chunk,
          start,
          endExclusive - 1,
          file.size,
        )
        if (!uploaded.success)
          throw new Error(uploaded.message || t('app.fileManager.uploadFailed'))
      }
      const completed = await client.completeTransfer(transferId)
      if (!completed.success)
        throw new Error(completed.message || t('app.fileManager.uploadFailed'))
      return true
    } catch (error) {
      await client.cancelTransfer(transferId)
      toastStore.error(error instanceof Error ? error.message : t('app.fileManager.uploadFailed'))
      return false
    }
  }

  /** 页面恢复后继续等待当前节点的活动文件任务。 */
  const resumeActiveTasks = async () => {
    const res = await fsClient.value.activeTasks()
    if (!res.success || !res.data?.length) return false
    await Promise.all(res.data.map(pollTask))
    return true
  }

  /** 页面恢复后继续跟踪已开始的下载；上传在用户重新选择同一文件后续传。 */
  const resumeActiveTransfers = async () => {
    const res = await fsClient.value.activeTransfers()
    const downloads = res.data?.filter((transfer) => transfer.direction === 'download') || []
    if (!res.success || downloads.length === 0) return false
    await Promise.all(downloads.map((transfer) => pollTransfer(transfer.transferId)))
    return true
  }

  return {
    createFile,
    mkdir,
    removePath,
    renamePath,
    copyPath,
    runPathTask,
    downloadFile,
    uploadFile,
    resumeActiveTasks,
    resumeActiveTransfers,
  }
}
