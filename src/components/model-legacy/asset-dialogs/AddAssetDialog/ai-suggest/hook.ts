import { PreferenceContext } from '@/components/context/PreferenceContext'
import { useLocalization } from '@/hooks/use-localization'
import { useToast } from '@/hooks/use-toast'
import { AiSuggestion, commands } from '@/lib/bindings'
import { AssetFormType } from '@/lib/form'
import { useCallback, useContext, useEffect, useRef, useState } from 'react'
import {
  AiFilledValues,
  AiNewValues,
  AiSuggestViewState,
  applyAiSuggestion,
  collectNewValues,
  EMPTY_AI_FILLED,
  EMPTY_AI_SUGGESTION,
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

  // reset() のたびに増え、古いリクエストのレスポンスを丸ごと破棄するための世代
  const epochRef = useRef(0)
  // 実行中の boothItemId:assetType（StrictMode での二重実行と重複リクエストの抑止）
  const inFlightRef = useRef<Set<string>>(new Set())
  // 成功した boothItemId:assetType ごとの結果。同じ組み合わせでは再度APIを呼ばない
  const cacheRef = useRef<Map<string, AiSuggestion>>(new Map())
  // いまフォームに反映済みの boothItemId:assetType
  const appliedKeyRef = useRef<string | null>(null)
  const filledRef = useRef<AiFilledValues>(EMPTY_AI_FILLED)
  // filledRef の値が由来する boothItemId
  const filledItemIdRef = useRef<number | null>(null)
  // 最新のタブ（レスポンス到着時に確認画面にいるかを判定する）
  const tabRef = useRef(tab)
  const prevDialogOpenRef = useRef(dialogOpen)

  const { preference } = useContext(PreferenceContext)
  const { toast } = useToast()
  const { t } = useLocalization()

  const enabled = preference.aiSuggestEnabled ?? false
  const apiKey = preference.aiSuggestApiKey ?? ''

  useEffect(() => {
    tabRef.current = tab
  }, [tab])

  const reset = useCallback(() => {
    epochRef.current += 1
    inFlightRef.current.clear()
    cacheRef.current.clear()
    appliedKeyRef.current = null
    filledRef.current = EMPTY_AI_FILLED
    filledItemIdRef.current = null
    setLoading(false)
    setNewValues(EMPTY_AI_NEW_VALUES)
  }, [])

  // ダイアログの開閉時は状態を破棄する（閉じた後に届くレスポンスやトーストも破棄される）
  useEffect(() => {
    reset()
  }, [dialogOpen, reset])

  useEffect(() => {
    // 開いた直後のコミットでは、フォームのクリア前の値でリクエストしないようにスキップする
    const justOpened = dialogOpen && !prevDialogOpenRef.current
    prevDialogOpenRef.current = dialogOpen

    if (!dialogOpen || tab !== 'manual-input' || justOpened) {
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

    if (appliedKeyRef.current === key) {
      return
    }

    const applySuggestion = (data: AiSuggestion) => {
      filledRef.current = applyAiSuggestion(form, data, filledRef.current)
      filledItemIdRef.current = boothItemId
      appliedKeyRef.current = key
      setNewValues(collectNewValues(data))
    }

    const cached = cacheRef.current.get(key)

    if (cached !== undefined) {
      applySuggestion(cached)
      return
    }

    if (inFlightRef.current.has(key)) {
      return
    }

    // 別の項目のAI提案が残っている間は、新規バッジを出し続けない
    if (
      filledItemIdRef.current !== null &&
      filledItemIdRef.current !== boothItemId
    ) {
      setNewValues(EMPTY_AI_NEW_VALUES)
    }

    // 別の項目で失敗したときに、前の項目のAI入力値（未変更のもの）を取り除く
    const discardPreviousItemValues = () => {
      if (
        filledItemIdRef.current === null ||
        filledItemIdRef.current === boothItemId
      ) {
        return
      }

      applyAiSuggestion(form, EMPTY_AI_SUGGESTION, filledRef.current)
      filledRef.current = EMPTY_AI_FILLED
      filledItemIdRef.current = null
      appliedKeyRef.current = null
      setNewValues(EMPTY_AI_NEW_VALUES)
    }

    const epoch = epochRef.current

    inFlightRef.current.add(key)
    setLoading(true)

    const settle = () => {
      inFlightRef.current.delete(key)
      setLoading(inFlightRef.current.size > 0)
    }

    const run = async () => {
      try {
        const result = await commands.suggestAssetFieldsWithAi(
          boothItemId,
          assetType,
        )

        if (epochRef.current !== epoch) {
          return
        }

        settle()

        const isCurrent = isResponseForCurrentForm(key, form)

        if (result.status === 'error') {
          if (!isCurrent) {
            return
          }

          discardPreviousItemValues()

          const messageKey = errorMessageKey(result.error)

          if (messageKey !== null) {
            toast({ title: t(messageKey), duration: 5000 })
          }

          return
        }

        // 成功した結果は、別の項目・種類や別のタブにいても保持して再利用する
        cacheRef.current.set(key, result.data)

        if (!isCurrent || tabRef.current !== 'manual-input') {
          return
        }

        applySuggestion(result.data)
      } catch (error) {
        console.error('Failed to suggest asset fields with AI:', error)

        if (epochRef.current === epoch) {
          settle()

          if (isResponseForCurrentForm(key, form)) {
            discardPreviousItemValues()
          }

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
