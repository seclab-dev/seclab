export type {
  PlatformLog,
  PlatformLogList,
  LogModule,
  PlatformLogQuery,
  LogStatus,
} from '@/api/generated'

export const LOG_MODULE = {
  System: 'System',
  Auth: 'Auth',
  Docker: 'Docker',
} as const
