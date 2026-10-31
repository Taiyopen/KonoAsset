import { FC } from 'react'
import { Download } from 'lucide-react'
import { DownloadJob } from '@/lib/bindings'
import { Button } from '@/components/ui/button'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Separator } from '@/components/ui/separator'
import { useLocalization } from '@/hooks/use-localization'
import { DownloadJobRow } from '../DownloadJobRow'

type Props = {
  jobs: DownloadJob[]
  activeCount: number
  onCancel: (id: string) => void
  onRetry: (id: string, password: string | null) => void
  onDismiss: (id: string) => void
}

export const DownloadJobsPopover: FC<Props> = ({
  jobs,
  activeCount,
  onCancel,
  onRetry,
  onDismiss,
}) => {
  const { t } = useLocalization()

  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button
          variant="ghost"
          size="icon"
          className="relative text-muted-foreground"
          aria-label={t('download-jobs:title')}
          data-testid="download-jobs-trigger"
        >
          <Download className="size-5" />
          {activeCount > 0 && (
            <span className="absolute right-0 top-0 flex size-4 items-center justify-center rounded-full bg-primary text-[10px] font-bold text-primary-foreground">
              {activeCount}
            </span>
          )}
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-[420px] p-0" align="end">
        <p className="px-3 py-2 text-sm font-bold">
          {t('download-jobs:title')}
        </p>
        <Separator />
        <ScrollArea className="max-h-[400px] [&>[data-slot=scroll-area-viewport]]:max-h-[400px]">
          {jobs.map((job, index) => (
            <div key={job.id}>
              {index > 0 && <Separator />}
              <DownloadJobRow
                job={job}
                onCancel={onCancel}
                onRetry={onRetry}
                onDismiss={onDismiss}
              />
            </div>
          ))}
        </ScrollArea>
      </PopoverContent>
    </Popover>
  )
}
