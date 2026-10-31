import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Textarea } from '@/components/ui/textarea'
import { useLocalization } from '@/hooks/use-localization'
import { KeyRound } from 'lucide-react'
import { useLinkInputTab } from './hook'
import { siteDisplayName } from './logic'

type Props = {
  setTab: (tab: string) => void

  tabIndex: number
  totalTabs: number
}

export const LinkInputTab = ({ setTab, tabIndex, totalTabs }: Props) => {
  const { t } = useLocalization()
  const {
    text,
    setText,
    parsed,
    proceedable,
    backToPreviousTab,
    moveToNextTab,
  } = useLinkInputTab({ setTab })

  return (
    <>
      <DialogHeader>
        <DialogTitle>
          ({tabIndex}/{totalTabs}) {t('addasset:link-input')}
        </DialogTitle>
        <DialogDescription>
          {t('addasset:link-input:explanation-text')}
        </DialogDescription>
      </DialogHeader>
      <div className="my-4 space-y-3">
        <Textarea
          rows={6}
          className="font-mono"
          placeholder={t('addasset:link-input:placeholder')}
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
        {parsed.length > 0 && (
          <ul className="max-h-40 space-y-1 overflow-y-auto text-sm">
            {parsed.map((line) => (
              <li
                key={line.lineNumber}
                className="flex flex-row items-center gap-2"
              >
                <span className="w-6 shrink-0 text-right text-muted-foreground">
                  {line.lineNumber}
                </span>
                {line.valid && line.site !== null ? (
                  <>
                    <Badge variant="secondary">
                      {siteDisplayName(line.site, t)}
                    </Badge>
                    {line.kind !== null && (
                      <span className="text-muted-foreground">
                        {line.kind === 'Folder'
                          ? t('addasset:link-input:kind:folder')
                          : t('addasset:link-input:kind:file')}
                      </span>
                    )}
                    {line.hasPassword && (
                      <KeyRound size={14} className="text-muted-foreground" />
                    )}
                  </>
                ) : (
                  <>
                    <span className="truncate font-mono text-destructive">
                      {line.text}
                    </span>
                    <span className="shrink-0 text-destructive">
                      {t('addasset:link-input:invalid')}
                    </span>
                  </>
                )}
              </li>
            ))}
          </ul>
        )}
        <p className="text-sm text-muted-foreground">
          {t('addasset:link-input:supported-sites')}
        </p>
      </div>
      <DialogFooter>
        <Button
          variant="outline"
          className="mr-auto"
          onClick={backToPreviousTab}
        >
          {t('general:button:back')}
        </Button>
        <Button disabled={!proceedable} onClick={moveToNextTab}>
          {t('general:button:next')}
        </Button>
      </DialogFooter>
    </>
  )
}
