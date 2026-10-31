import { PreferenceContext } from '@/components/context/PreferenceContext'
import { useLocalization } from '@/hooks/use-localization'
import { useToast } from '@/hooks/use-toast'
import { commands } from '@/lib/bindings'
import { AssetFormType } from '@/lib/form'
import { useContext, useEffect, useRef, useState } from 'react'
import {
  AiFilledValues,
  AiNewValues,
  AiSuggestViewState,
  applyAiSuggestion,
  collectNewValues,
  EMPTY_AI_FILLED,
  EMPTY_AI_NEW_VALUES,
  errorMessageKey,
  isResponseForCurrentForm,
  requestKey,
  shouldRunAiSuggest,
} from './logic'

type Props = {
  form: AssetFormType
  tab: string
  dialogOpen: boolean
}

export const useAiSuggest = ({
  form,
  tab,
  dialogOpen,
}: Props): AiSuggestViewState => {
  const [loading, setLoading] = useState(false)
  const [newValues, setNewValues] = useState<AiNewValues>(EMPTY_AI_NEW_VALUES)

  // 直近でリクエストした boothItemId:assetType（StrictMode での二重実行と再実行の抑止）
  const lastKeyRef = useRef<string | null>(null)
  // 古いレスポンスを破棄するためのリクエスト世代
  const generationRef = useRef(0)
  const filledRef = useRef<AiFilledValues>(EMPTY_AI_FILLED)

  const { preference } = useContext(PreferenceContext)
  const { toast } = useToast()
  const { t } = useLocalization()

  const enabled = preference.aiSuggestEnabled
  const apiKey = preference.aiSuggestApiKey

  useEffect(() => {
    if (!dialogOpen) {
      return
    }

    lastKeyRef.current = null
    generationRef.current += 1
    filledRef.current = EMPTY_AI_FILLED
    setLoading(false)
    setNewValues(EMPTY_AI_NEW_VALUES)
  }, [dialogOpen])

  useEffect(() => {
    if (!dialogOpen || tab !== 'manual-input') {
      return
    }

    const boothItemId = form.getValues('boothItemId')

    if (!shouldRunAiSuggest({ enabled, apiKey, boothItemId })) {
      return
    }

    if (boothItemId === null) {
      return
    }

    const assetType = form.getValues('assetType')
    const key = requestKey(boothItemId, assetType)

    if (lastKeyRef.current === key) {
      return
    }

    lastKeyRef.current = key
    const generation = ++generationRef.current

    setLoading(true)

    const run = async () => {
      try {
        const result = await commands.suggestAssetFieldsWithAi(
          boothItemId,
          assetType,
        )

        if (generationRef.current !== generation) {
          return
        }

        setLoading(false)

        if (!isResponseForCurrentForm(key, form)) {
          return
        }

        if (result.status === 'error') {
          const messageKey = errorMessageKey(result.error)

          if (messageKey !== null) {
            toast({ title: t(messageKey), duration: 5000 })
          }

          return
        }

        filledRef.current = applyAiSuggestion(
          form,
          result.data,
          filledRef.current,
        )
        setNewValues(collectNewValues(result.data))
      } catch (error) {
        console.error('Failed to suggest asset fields with AI:', error)

        if (generationRef.current === generation) {
          setLoading(false)
        }
      }
    }

    run()
  }, [dialogOpen, tab, form, enabled, apiKey, toast, t])

  return { loading, newValues }
}
