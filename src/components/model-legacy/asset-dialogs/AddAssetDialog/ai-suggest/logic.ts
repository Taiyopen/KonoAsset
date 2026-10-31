import { AiSuggestErrorKind, AiSuggestion, AssetType } from '@/lib/bindings'
import { AssetFormType } from '@/lib/form'

/** AIが直近で書き込んだ値。null はそのフィールドにAIの値を書き込んでいないことを示す */
export type AiFilledValues = {
  category: string | null
  supportedAvatars: string[] | null
  tags: string[] | null
}

export const EMPTY_AI_FILLED: AiFilledValues = {
  category: null,
  supportedAvatars: null,
  tags: null,
}

/** 既存の候補に無い、AIが新規に提案した値 */
export type AiNewValues = {
  category: string | null
  supportedAvatars: string[]
  tags: string[]
}

export type AiSuggestViewState = {
  loading: boolean
  newValues: AiNewValues
}

export const EMPTY_AI_NEW_VALUES: AiNewValues = {
  category: null,
  supportedAvatars: [],
  tags: [],
}

export const shouldRunAiSuggest = (p: {
  enabled: boolean
  apiKey: string
  boothItemId: number | null
}): boolean => p.enabled && p.apiKey.trim().length > 0 && p.boothItemId !== null

export const requestKey = (boothItemId: number, assetType: AssetType): string =>
  `${boothItemId}:${assetType}`

export const isResponseForCurrentForm = (
  key: string,
  form: AssetFormType,
): boolean => {
  const boothItemId = form.getValues('boothItemId')

  if (boothItemId === null) {
    return false
  }

  return key === requestKey(boothItemId, form.getValues('assetType'))
}

const arraysEqual = (a: string[], b: string[]): boolean =>
  a.length === b.length && a.every((v, i) => v === b[i])

export const applyAiSuggestion = (
  form: AssetFormType,
  s: AiSuggestion,
  previous: AiFilledValues,
): AiFilledValues => {
  const result: AiFilledValues = { ...previous }

  const currentCategory = form.getValues('category')
  if (currentCategory === '' || currentCategory === previous.category) {
    const value = s.category?.value ?? ''
    form.setValue('category', value)
    result.category = value
  }

  const currentAvatars = form.getValues('supportedAvatars')
  if (
    currentAvatars.length === 0 ||
    (previous.supportedAvatars !== null &&
      arraysEqual(currentAvatars, previous.supportedAvatars))
  ) {
    const value = s.supportedAvatars.map((v) => v.value)
    form.setValue('supportedAvatars', value)
    result.supportedAvatars = value
  }

  const currentTags = form.getValues('tags')
  if (
    currentTags.length === 0 ||
    (previous.tags !== null && arraysEqual(currentTags, previous.tags))
  ) {
    const value = s.tags.map((v) => v.value)
    form.setValue('tags', value)
    result.tags = value
  }

  return result
}

/** 提案が無い状態。AIが書き込んだ未変更の値を取り除くために使う */
export const EMPTY_AI_SUGGESTION: AiSuggestion = {
  category: null,
  supportedAvatars: [],
  tags: [],
}

export const collectNewValues = (s: AiSuggestion): AiNewValues => ({
  category: s.category?.isNew ? s.category.value : null,
  supportedAvatars: s.supportedAvatars
    .filter((v) => v.isNew)
    .map((v) => v.value),
  tags: s.tags.filter((v) => v.isNew).map((v) => v.value),
})

export const errorMessageKey = (e: AiSuggestErrorKind): string | null => {
  switch (e) {
    case 'NotConfigured':
      return null
    case 'InvalidApiKey':
      return 'addasset:ai-suggest:error:invalid-api-key'
    case 'InsufficientCredit':
      return 'addasset:ai-suggest:error:insufficient-credit'
    case 'RateLimited':
      return 'addasset:ai-suggest:error:rate-limited'
    case 'Overloaded':
      return 'addasset:ai-suggest:error:overloaded'
    case 'Timeout':
    case 'Network':
    case 'Refused':
    case 'Truncated':
    case 'InvalidResponse':
      return 'addasset:ai-suggest:error:failed'
    default:
      // バックエンドが未知の文字列を返した場合でも、必ず表示可能なメッセージにする
      return 'addasset:ai-suggest:error:failed'
  }
}
