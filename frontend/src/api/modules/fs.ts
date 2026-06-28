import http from '@/api'
import type { FsEntry, HomeResponse, ReadResponse, UploadResult } from '../interface/fs'

const buildFsPath = (path: string, nodeId?: string) => {
  if (!nodeId || nodeId === 'local') {
    return path
  }
  return `/node/${encodeURIComponent(nodeId)}${path}`
}

const createScopedFsApi = (nodeId?: string) => ({
  listEntries: (path: string, recursive = false, showHidden = false) => {
    return http.get<FsEntry[]>(buildFsPath('/agent/fs/ls', nodeId), { path, recursive, showHidden })
  },
  home: () => {
    return http.get<HomeResponse>(buildFsPath('/agent/fs/home', nodeId))
  },
  readFile: (path: string) => {
    return http.get<ReadResponse>(buildFsPath('/agent/fs/read', nodeId), { path })
  },
  writeFile: (payload: {
    path: string
    content: string
    createIfMissing?: boolean
    overwrite?: boolean
  }) => {
    return http.post<null>(buildFsPath('/agent/fs/write', nodeId), payload)
  },
  mkdir: (payload: { path: string; recursive?: boolean }) => {
    return http.post<null>(buildFsPath('/agent/fs/mkdir', nodeId), payload)
  },
  removePath: (payload: { path: string; recursive?: boolean }) => {
    return http.post<null>(buildFsPath('/agent/fs/remove', nodeId), payload)
  },
  renamePath: (payload: { from: string; to: string; overwrite?: boolean }) => {
    return http.post<null>(buildFsPath('/agent/fs/rename', nodeId), payload)
  },
  copyPath: (payload: { from: string; to: string; overwrite?: boolean }) => {
    return http.post<null>(buildFsPath('/agent/fs/copy', nodeId), payload)
  },
  uploadFile: (targetPath: string, data: FormData, overwrite = false) => {
    return http.upload<UploadResult>(
      `${buildFsPath('/agent/fs/upload', nodeId)}?path=${encodeURIComponent(targetPath)}`,
      data,
      {
        params: { overwrite },
        headers: { 'Content-Type': 'multipart/form-data' },
      },
    )
  },
  downloadFile: (path: string) => {
    return http.get<Blob>(
      buildFsPath('/agent/fs/download', nodeId),
      { path },
      { responseType: 'blob' },
    )
  },
})

export const fsApi = Object.assign(createScopedFsApi(), {
  forNode: createScopedFsApi,
})
