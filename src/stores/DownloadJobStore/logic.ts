import { DownloadErrorKind, DownloadJob } from '@/lib/bindings'

export const upsertJob = (
  jobs: DownloadJob[],
  job: DownloadJob,
): DownloadJob[] => {
  const index = jobs.findIndex((j) => j.id === job.id)

  if (index === -1) {
    return [job, ...jobs]
  }

  return jobs.map((j, i) => (i === index ? job : j))
}

export const detectTransitions = (
  prev: DownloadJob | undefined,
  next: DownloadJob,
): 'completed' | 'failed' | null => {
  if (prev?.status === next.status) {
    return null
  }

  switch (next.status) {
    case 'Completed':
      return 'completed'
    case 'Failed':
      return 'failed'
    default:
      return null
  }
}

export const progressPercent = (job: DownloadJob): number | null => {
  if (job.totalBytes === null || job.totalBytes === 0) {
    return null
  }

  const percent = (job.downloadedBytes / job.totalBytes) * 100

  return Math.min(100, Math.max(0, percent))
}

export const activeCount = (jobs: DownloadJob[]): number =>
  jobs.filter(
    (job) =>
      job.status === 'Queued' ||
      job.status === 'Downloading' ||
      job.status === 'Importing',
  ).length

const ERROR_KIND_LOCALE_KEYS: Record<DownloadErrorKind, string> = {
  PasswordRequired: 'download-jobs:error:password-required',
  WrongPassword: 'download-jobs:error:wrong-password',
  NotFound: 'download-jobs:error:not-found',
  LoginRequired: 'download-jobs:error:login-required',
  RateLimited: 'download-jobs:error:rate-limited',
  NotAFile: 'download-jobs:error:not-a-file',
  ImportFailed: 'download-jobs:error:import-failed',
  Other: 'download-jobs:error:other',
}

export const errorKindLocaleKey = (kind: DownloadErrorKind | null): string =>
  ERROR_KIND_LOCALE_KEYS[kind ?? 'Other']

export const removeJob = (jobs: DownloadJob[], id: string): DownloadJob[] =>
  jobs.filter((job) => job.id !== id)

// manager.rs の cancel がインポート中のジョブを断る時のメッセージ。行は自動で更新されるので通知しない
export const isCancelRejectedWhileImporting = (error: string): boolean =>
  error.endsWith('while importing')
