export interface FsEntry {
  name: string
  path: string
  entryType: string
  size: number
  modified?: number
  created?: number
}

export interface ReadResponse {
  path: string
  content: string
  isText: boolean
}

export interface UploadResult {
  saved: string[]
}

export interface HomeResponse {
  path: string
}
