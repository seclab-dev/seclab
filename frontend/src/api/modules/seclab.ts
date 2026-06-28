import http from '@/api'
import type { SeclabNetworkConfig, SeclabNetworkUpdateResult } from '../interface/seclab'

export const seclabApi = {
  fetchNetwork: () => {
    return http.get<SeclabNetworkConfig>('/seclab/network')
  },
  updateNetwork: (payload: SeclabNetworkConfig) => {
    return http.put<SeclabNetworkUpdateResult>('/seclab/network', payload)
  },
  fetchInterfaces: () => {
    return http.get<{ name: string; ip: string; label: string }[]>('/seclab/network-interfaces')
  },
}
