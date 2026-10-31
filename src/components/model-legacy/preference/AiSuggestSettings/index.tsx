import { Label } from '@/components/ui/label'

import { FC, useEffect, useState } from 'react'
import { Eye, EyeOff } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { useLocalization } from '@/hooks/use-localization'

type Props = {
  enabled: boolean
  apiKey: string
  setEnabled: (enabled: boolean) => void
  setApiKey: (apiKey: string) => void
}

export const AiSuggestSettings: FC<Props> = ({
  enabled,
  apiKey,
  setEnabled,
  setApiKey,
}) => {
  const { t } = useLocalization()
  const [localApiKey, setLocalApiKey] = useState(apiKey)
  const [visible, setVisible] = useState(false)

  useEffect(() => {
    setLocalApiKey(apiKey)
  }, [apiKey])

  const commitApiKey = () => {
    const trimmed = localApiKey.trim()
    setLocalApiKey(trimmed)
    if (trimmed !== apiKey) {
      setApiKey(trimmed)
    }
  }

  return (
    <div className="space-y-4 w-full">
      <div className="flex flex-row items-center w-full gap-8">
        <div className="space-y-2 shrink min-w-0 overflow-hidden">
          <Label className="text-lg">
            {t('preference:settings:ai-suggest')}
          </Label>
          <p className="text-muted-foreground text-sm wrap-break-word">
            {t('preference:settings:ai-suggest:explanation-text')}
          </p>
        </div>
        <Switch
          className="ml-auto shrink-0"
          checked={enabled}
          onCheckedChange={setEnabled}
        />
      </div>
      <div className="space-y-2 max-w-md">
        <Label>{t('preference:settings:ai-suggest:api-key')}</Label>
        <div className="flex flex-row items-center gap-2">
          <Input
            type={visible ? 'text' : 'password'}
            value={localApiKey}
            placeholder={t(
              'preference:settings:ai-suggest:api-key:placeholder',
            )}
            autoComplete="off"
            spellCheck={false}
            onChange={(e) => setLocalApiKey(e.target.value)}
            onBlur={commitApiKey}
          />
          <Button
            type="button"
            variant="ghost"
            size="icon"
            aria-label={t('preference:settings:ai-suggest:api-key')}
            onClick={() => setVisible((v) => !v)}
          >
            {visible ? <EyeOff /> : <Eye />}
          </Button>
        </div>
      </div>
    </div>
  )
}
