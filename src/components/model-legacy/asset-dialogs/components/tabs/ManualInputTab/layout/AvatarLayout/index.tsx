import { AiNewValues } from '@/components/model-legacy/asset-dialogs/AddAssetDialog/ai-suggest/logic'
import { Label } from '@/components/ui/label'
import MultipleSelector, { Option } from '@/components/ui/multi-select'
import { commands } from '@/lib/bindings'
import { AssetFormType } from '@/lib/form'
import { useLocalization } from '@/hooks/use-localization'

import { useEffect, useState } from 'react'

type Props = {
  form: AssetFormType
  aiNewValues?: AiNewValues
}

export const AvatarLayout = ({ form, aiNewValues }: Props) => {
  const { t } = useLocalization()
  const [tagCandidates, setTagCandidates] = useState<Option[]>([])

  const fetchTagCandidates = async () => {
    const result = await commands.getAllAssetTags(null)

    if (result.status === 'error') {
      console.error(result.error)
      return
    }

    setTagCandidates(
      result.data.map((entry) => ({
        label: entry.value,
        value: entry.value,
        priority: entry.priority,
      })),
    )
  }

  useEffect(() => {
    fetchTagCandidates()
  }, [])

  if (!form) {
    return <div>Loading...</div>
  }

  return (
    <div className="w-full flex flex-row space-x-2 mb-4">
      <div className="w-full space-y-2">
        <Label> {t('general:tag')} </Label>
        <MultipleSelector
          options={tagCandidates}
          placeholder={t('addasset:tag:placeholder')}
          className="max-w-[600px]"
          badgeClassName="max-w-[540px]"
          hidePlaceholderWhenSelected
          creatable
          emptyIndicator={
            <p className="text-center text-lg text-muted-foreground">
              {t('addasset:empty-indicator')}
            </p>
          }
          highlightedValues={aiNewValues?.tags}
          highlightLabel={t('addasset:ai-suggest:new')}
          value={form.watch('tags')}
          onChange={(value) => form.setValue('tags', value)}
        />
        <p className="text-muted-foreground text-sm">
          {t('addasset:tag:explanation-text')}
        </p>
      </div>
    </div>
  )
}
