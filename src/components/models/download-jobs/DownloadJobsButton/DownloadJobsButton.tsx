import { FC, useEffect, useRef } from 'react'
import { commands, DownloadJob, Result } from '@/lib/bindings'
import { useAssetFilterStore } from '@/stores/AssetFilterStore'
import { useAssetSummaryViewStore } from '@/stores/AssetSummaryViewStore'
import { useDownloadJobStore } from '@/stores/DownloadJobStore'
import {
  activeCount,
  isCancelRejectedWhileImporting,
} from '@/stores/DownloadJobStore/logic'
import { useLocalization } from '@/hooks/use-localization'
import { useToast } from '@/hooks/use-toast'
import { DownloadJobsPopover } from '../DownloadJobsPopover'

export const DownloadJobsButton: FC = () => {
  const { t } = useLocalization()
  const { toast } = useToast()

  const jobs = useDownloadJobStore((state) => state.jobs)
  const initialize = useDownloadJobStore((state) => state.initialize)
  const removeJob = useDownloadJobStore((state) => state.removeJob)
  const refreshAssetSummaries = useAssetSummaryViewStore(
    (state) => state.refreshAssetSummaries,
  )
  const refreshFilteredIds = useAssetFilterStore(
    (state) => state.refreshFilteredIds,
  )

  const onTransition = (
    job: DownloadJob,
    transition: 'completed' | 'failed',
  ) => {
    if (transition === 'completed') {
      toast({
        title: t('addasset:success-toast'),
        description: job.assetName,
        duration: 3000,
      })
      refreshAssetSummaries()
      refreshFilteredIds()
    } else {
      toast({
        title: t('download-jobs:failed-toast'),
        description: job.assetName,
      })
    }
  }

  // initialize は一度しか購読しないので、最新の t / toast を呼べるよう ref 経由にする
  const onTransitionRef = useRef(onTransition)
  useEffect(() => {
    onTransitionRef.current = onTransition
  })

  useEffect(() => {
    initialize((job, transition) => onTransitionRef.current(job, transition))
  }, [initialize])

  const runCommand = async (
    command: Promise<Result<null, string>>,
    isExpectedError: (error: string) => boolean = () => false,
  ): Promise<Result<null, string>> => {
    const result = await command

    if (result.status === 'error' && !isExpectedError(result.error)) {
      toast({
        title: t('download-jobs:action-failed-toast'),
        description: result.error,
        variant: 'destructive',
      })
    }

    return result
  }

  // 削除はバックエンドがイベントを送らないため、成功時にだけ自分で一覧から外す
  const onDismiss = async (id: string) => {
    const result = await runCommand(commands.dismissDownloadJob(id))

    if (result.status === 'ok') {
      removeJob(id)
    }
  }

  if (jobs.length === 0) {
    return null
  }

  return (
    <DownloadJobsPopover
      jobs={jobs}
      activeCount={activeCount(jobs)}
      onCancel={(id) =>
        runCommand(
          commands.cancelDownloadJob(id),
          isCancelRejectedWhileImporting,
        )
      }
      onRetry={(id, password) =>
        runCommand(commands.retryDownloadJob(id, password))
      }
      onDismiss={onDismiss}
    />
  )
}
