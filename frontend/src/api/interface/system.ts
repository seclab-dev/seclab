export interface SystemAboutInfo {
  hostname: string
  osName: string
  osVersion: string
  kernelVersion: string
  architecture: string
  cpuModel: string
  cpuVendor: string
  cpuPhysicalCores: number
  cpuLogicalCores: number
  cpuFrequencyMhz: number
  cpuCacheL3: string
  memoryTotalBytes: number
  memoryAvailableBytes: number
  swapTotalBytes: number
  swapUsedBytes: number
  uptimeSeconds: number
  bootTimeEpoch: number
  timezone: string
  locale: string
  virtualization: string
  primaryInterface: string
  macAddress: string
  ipv4Addresses: string[]
  ipv6Addresses: string[]
  defaultGateway: string
  dnsServers: string[]
  diskTotalBytes: number
}

export interface DiskInfo {
  name: string
  sizeBytes: number
  partitionCount: number
  diskType: string
  model: string
  serial: string
  partitionTable: string
  isSystemDisk: boolean
  readOnly: boolean
  partitions: DiskPartitionInfo[]
}

export interface DiskPartitionInfo {
  name: string
  sizeBytes: number
  usedBytes: number | null
  availableBytes: number | null
  usagePercent: number | null
  mountpoint: string
  filesystem: string
}

export interface FirewallStatusInfo {
  installed: boolean
  firewallType: string | null
  version: string | null
  active: boolean
  serviceStatus: string
  platform: string
  primaryBackend: string | null
  detectedBackends: FirewallBackendStatus[]
  rules: FirewallRuleSummary[]
  warnings: string[]
}

export interface FirewallBackendStatus {
  backend: string
  installed: boolean
  active: boolean
  version: string | null
  serviceStatus: string
  commandPath: string | null
}

export interface FirewallRuleSummary {
  backend: string
  scope: string
  action: string
  protocol: string | null
  port: string | null
  source: string | null
  raw: string
}
