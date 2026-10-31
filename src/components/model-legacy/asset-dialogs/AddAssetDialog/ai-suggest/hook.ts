import { PreferenceContext } from '@/components/context/PreferenceContext'
import { useLocalization } from '@/hooks/use-localization'
import { useToast } from '@/hooks/use-toast'
import { commands } from '@/lib/bindings'
import { AssetFormType } from '@/lib/form'
import { useCallback, useContext, useEffect, useRef, useState } from 'react'
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

type ReturnProps = {
  viewState: AiSuggestViewState
  /** フォームを再設定するときに呼び、AI提案の状態と実行中のリクエストを破棄する */
  reset: () => void
}

export const useAiSuggest = ({ form, tab, dialogOpen }: Props): ReturnProps => {
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

  const enabled = preference.aiSuggestEnabled ?? false
  const apiKey = preference.aiSuggestApiKey ?? ''

  const reset = useCallback(() => {
    lastKeyRef.current = null
    generationRef.current += 1
    filledRef.current = EMPTY_AI_FILLED
    setLoading(false)
    setNewValues(EMPTY_AI_NEW_VALUES)
  }, [])

  // ダイアログの開閉時は状態を破棄する（閉じた後に届くレスポンスやトーストも破棄される）
  useEffect(() => {
    reset()
  }, [dialogOpen, reset])

  useEffect(() => {
    if (!dialogOpen || tab !== 'manual-input') {
      return
    }

    const boothItemId = form.getValues('boothItemId')

    if (!shouldRunAiSuggest({ enabled, apiKey, boothItemId })) {
      return
    }

    // shouldRunAiSuggest で保証済みだが、型を絞り込むために必要
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

    const releaseKey = () => {
      if (lastKeyRef.current === key) {
        lastKeyRef.current = null
      }
    }

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
          // 別の項目・種類に切り替わっているので破棄し、戻ってきたときに再実行できるようにする
          releaseKey()
          return
        }

        if (result.status === 'error') {
          releaseKey()

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
          releaseKey()
          toast({
            title: t('addasset:ai-suggest:error:failed'),
            duration: 5000,
          })
        }
      }
    }

    run()
  }, [dialogOpen, tab, form, enabled, apiKey, toast, t])

  return { viewState: { loading, newValues }, reset }
}
