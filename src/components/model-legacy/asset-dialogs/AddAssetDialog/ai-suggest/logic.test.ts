import { beforeEach, describe, expect, it } from 'vitest'
import { renderHook } from '@testing-library/react'
import { useForm } from 'react-hook-form'
import { AiSuggestion, AiSuggestErrorKind } from '@/lib/bindings'
import { AssetFormFields, AssetFormType } from '@/lib/form'
import {
  applyAiSuggestion,
  collectNewValues,
  EMPTY_AI_FILLED,
  errorMessageKey,
  isResponseForCurrentForm,
  requestKey,
  shouldRunAiSuggest,
} from './logic'

const sv = (value: string, isNew = false) => ({ value, isNew })

const suggestion = (s: {
  category: string | null
  supportedAvatars: string[]
  tags: string[]
}): AiSuggestion => ({
  category: s.category === null ? null : sv(s.category),
  supportedAvatars: s.supportedAvatars.map((v) => sv(v)),
  tags: s.tags.map((v) => sv(v)),
})

describe('ai-suggest logic', () => {
  let form: AssetFormType

  beforeEach(() => {
    const { result } = renderHook(() =>
      useForm<AssetFormFields>({
        defaultValues: {
          assetType: 'Avatar',
          name: '',
          creator: '',
          imageFilename: null,
          boothItemId: null,
          tags: [],
          memo: null,
          dependencies: [],
          category: '',
          supportedAvatars: [],
          publishedAt: null,
        },
      }),
    )
    form = result.current as AssetFormType
  })

  it('fills only empty fields', () => {
    form.setValue('tags', ['手動'])

    const filled = applyAiSuggestion(
      form,
      suggestion({
        category: '衣装',
        supportedAvatars: ['マヌカ'],
        tags: ['VRChat'],
      }),
      EMPTY_AI_FILLED,
    )

    expect(form.getValues('category')).toBe('衣装')
    expect(form.getValues('supportedAvatars')).toEqual(['マヌカ'])
    expect(form.getValues('tags')).toEqual(['手動'])
    expect(filled.category).toBe('衣装')
    expect(filled.supportedAvatars).toEqual(['マヌカ'])
    expect(filled.tags).toBeNull()
  })

  it('replaces AI-filled untouched values on re-run but keeps user edits', () => {
    const first = applyAiSuggestion(
      form,
      suggestion({
        category: '衣装',
        supportedAvatars: ['マヌカ'],
        tags: ['A'],
      }),
      EMPTY_AI_FILLED,
    )
    form.setValue('tags', ['A', '手動'])

    const second = applyAiSuggestion(
      form,
      suggestion({ category: 'ワールド', supportedAvatars: [], tags: ['B'] }),
      first,
    )

    expect(form.getValues('category')).toBe('ワールド')
    expect(form.getValues('supportedAvatars')).toEqual([])
    expect(form.getValues('tags')).toEqual(['A', '手動'])
    expect(second.category).toBe('ワールド')
    expect(second.supportedAvatars).toEqual([])
    expect(second.tags).toEqual(['A'])
  })

  it('writes an empty suggestion to clear a previous AI value', () => {
    const first = applyAiSuggestion(
      form,
      suggestion({ category: '衣装', supportedAvatars: [], tags: [] }),
      EMPTY_AI_FILLED,
    )

    applyAiSuggestion(
      form,
      suggestion({ category: null, supportedAvatars: [], tags: [] }),
      first,
    )

    expect(form.getValues('category')).toBe('')
  })

  it('ignores responses for another item or type', () => {
    form.setValue('boothItemId', 1)
    form.setValue('assetType', 'WorldObject')

    expect(
      isResponseForCurrentForm(requestKey(1, 'AvatarWearable'), form),
    ).toBe(false)
    expect(isResponseForCurrentForm(requestKey(2, 'WorldObject'), form)).toBe(
      false,
    )
    expect(isResponseForCurrentForm(requestKey(1, 'WorldObject'), form)).toBe(
      true,
    )
  })

  it('runs only when enabled with a non-blank key and a BOOTH item', () => {
    expect(
      shouldRunAiSuggest({ enabled: true, apiKey: 'sk', boothItemId: 1 }),
    ).toBe(true)
    expect(
      shouldRunAiSuggest({ enabled: true, apiKey: '  \n', boothItemId: 1 }),
    ).toBe(false)
    expect(
      shouldRunAiSuggest({ enabled: true, apiKey: '', boothItemId: 1 }),
    ).toBe(false)
    expect(
      shouldRunAiSuggest({ enabled: false, apiKey: 'sk', boothItemId: 1 }),
    ).toBe(false)
    expect(
      shouldRunAiSuggest({ enabled: true, apiKey: 'sk', boothItemId: null }),
    ).toBe(false)
  })

  it('collects only new values', () => {
    const newValues = collectNewValues({
      category: sv('新カテゴリ', true),
      supportedAvatars: [sv('既存'), sv('新アバター', true)],
      tags: [sv('既存タグ'), sv('新タグ', true)],
    })

    expect(newValues).toEqual({
      category: '新カテゴリ',
      supportedAvatars: ['新アバター'],
      tags: ['新タグ'],
    })

    expect(
      collectNewValues({
        category: sv('既存'),
        supportedAvatars: [],
        tags: [],
      }),
    ).toEqual({ category: null, supportedAvatars: [], tags: [] })
  })

  it('maps error kinds to messages', () => {
    expect(errorMessageKey('NotConfigured')).toBeNull()
    expect(errorMessageKey('InvalidApiKey')).toBe(
      'addasset:ai-suggest:error:invalid-api-key',
    )
    expect(errorMessageKey('InsufficientCredit')).toBe(
      'addasset:ai-suggest:error:insufficient-credit',
    )
    expect(errorMessageKey('RateLimited')).toBe(
      'addasset:ai-suggest:error:rate-limited',
    )
    expect(errorMessageKey('Overloaded')).toBe(
      'addasset:ai-suggest:error:overloaded',
    )
    const failed: AiSuggestErrorKind[] = [
      'Timeout',
      'Network',
      'Refused',
      'Truncated',
      'InvalidResponse',
    ]
    for (const k of failed)
      expect(errorMessageKey(k)).toBe('addasset:ai-suggest:error:failed')
  })

  it('falls back to the generic failure message for unknown error strings', () => {
    expect(errorMessageKey('SomethingNew' as AiSuggestErrorKind)).toBe(
      'addasset:ai-suggest:error:failed',
    )
  })
})
