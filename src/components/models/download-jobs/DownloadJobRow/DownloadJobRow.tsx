import { FC, useState } from 'react'
import { DownloadJob, DownloadJobStatus } from '@/lib/bindings'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Progress } from '@/components/ui/progress'
import { useLocalization } from '@/hooks/use-localization'
import { cn } from '@/lib/utils'
import { siteDisplayName } from '@/components/model-legacy/asset-dialogs/components/tabs/LinkInputTab/logic'
import {
  errorKindLocaleKey,
  progressPercent,
} from '@/stores/DownloadJobStore/logic'

type Props = {
  job: DownloadJob
  onCancel: (id: string) => void
  onRetry: (id: string, password: string | null) => void
  onDismiss: (id: string) => void
}

const STATUS_LOCALE_KEYS: Record<DownloadJobStatus, string> = {
  Queued: 'download-jobs:status:queued',
  Downloading: 'download-jobs:status:downloading',
  Importing: 'download-jobs:status:importing',
  Completed: 'download-jobs:status:completed',
  Failed: 'download-jobs:status:failed',
  Cancelled: 'download-jobs:status:cancelled',
}

export const DownloadJobRow: FC<Props> = ({
  job,
  onCancel,
  onRetry,
  onDismiss,
}) => {
  const { t } = useLocalization()
  const [password, setPassword] = useState('')

  const percent = progressPercent(job)
  const needsPassword =
    job.errorKind === 'PasswordRequired' || job.errorKind === 'WrongPassword'
  const showProgress =
    job.status === 'Downloading' || job.status === 'Importing'
  const isIndeterminate = showProgress && percent === null
  const canRetry = !needsPassword || password !== ''
  const retry = () => {
    if (canRetry) {
      onRetry(job.id, needsPassword ? password : null)
    }
  }

  const fileProgress =
    job.currentFile === null
      ? null
      : job.fileCount === null
        ? `${job.currentFile} (${job.fileIndex})`
        : `${job.currentFile} (${job.fileIndex}/${job.fileCount})`

  return (
    <div
      className="flex flex-col gap-2 p-3"
      data-testid="download-job-row"
      data-status={job.status}
    >
      <div className="flex flex-row items-center gap-2">
        <div className="min-w-0 grow">
          <p className="truncate text-sm font-medium">{job.assetName}</p>
          <p className="truncate text-xs text-muted-foreground">
            {siteDisplayName(job.site, t)}
          </p>
        </div>
        <span
          className={cn(
            'shrink-0 text-xs',
            job.status === 'Failed'
              ? 'text-destructive'
              : 'text-muted-foreground',
          )}
        >
          {t(STATUS_LOCALE_KEYS[job.status])}
        </span>
      </div>

      {showProgress && (
        <Progress
          className={cn(isIndeterminate && 'animate-pulse')}
          value={isIndeterminate ? 100 : percent}
        />
      )}

      {showProgress && fileProgress !== null && (
        <p className="truncate text-xs text-muted-foreground">{fileProgress}</p>
      )}

      {job.status === 'Failed' && (
        <div className="flex flex-col gap-0.5">
          <p className="text-sm text-destructive">
            {t(errorKindLocaleKey(job.errorKind))}
          </p>
          {job.error !== null && (
            <p className="break-all text-xs text-muted-foreground">
              {job.error}
            </p>
          )}
        </div>
      )}

      <div className="flex flex-row items-center justify-end gap-2 empty:hidden">
        {(job.status === 'Queued' || job.status === 'Downloading') && (
          <Button variant="outline" size="sm" onClick={() => onCancel(job.id)}>
            {t('download-jobs:button:cancel')}
          </Button>
        )}

        {job.status === 'Failed' && (
          <>
            {needsPassword && (
              <Input
                className="h-8 min-w-0 grow"
                value={password}
                placeholder={t('download-jobs:password-placeholder')}
                onChange={(e) => setPassword(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    retry()
                  }
                }}
              />
            )}
            <Button
              variant="outline"
              size="sm"
              disabled={!canRetry}
              onClick={retry}
            >
              {t('download-jobs:button:retry')}
            </Button>
          </>
        )}

        {(job.status === 'Failed' ||
          job.status === 'Completed' ||
          job.status === 'Cancelled') && (
          <Button variant="ghost" size="sm" onClick={() => onDismiss(job.id)}>
            {t('download-jobs:button:dismiss')}
          </Button>
        )}
      </div>
    </div>
  )
}
