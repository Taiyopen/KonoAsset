import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook, waitFor } from '@testing-library/react'
import { createElement, ReactNode } from 'react'
import { useForm } from 'react-hook-form'
import { PreferenceContext } from '@/components/context/PreferenceContext'
import { AiSuggestion } from '@/lib/bindings'
import { AssetFormFields, AssetFormType } from '@/lib/form'
import { getDefaultPreferences } from '@/lib/utils'
import { useAiSuggest } from './hook'

const mocks = vi.hoisted(() => ({
  suggest: vi.fn(),
  toast: vi.fn(),
  t: (id: string) => id,
}))

vi.mock('@/lib/bindings', () => ({
  commands: { suggestAssetFieldsWithAi: mocks.suggest },
}))

vi.mock('@/hooks/use-toast', () => ({
  useToast: () => ({ toast: mocks.toast }),
}))

vi.mock('@/hooks/use-localization', () => ({
  useLocalization: () => ({ t: mocks.t }),
}))

const suggestion = (
  category: string,
  tags: string[] = [],
  isNew = false,
): AiSuggestion => ({
  category: { value: category, isNew },
  supportedAvatars: [],
  tags: tags.map((value) => ({ value, isNew: false })),
})

const ok = (data: AiSuggestion) => ({ status: 'ok' as const, data })

const preferenceValue = {
  preference: {
    ...getDefaultPreferences(),
    aiSuggestEnabled: true,
    aiSuggestApiKey: 'sk',
  },
  setPreference: async () => {},
}

const wrapper = ({ children }: { children: ReactNode }) =>
  createElement(
    PreferenceContext.Provider,
    { value: preferenceValue },
    children,
  )

const setup = (initialTab = 'booth-input') =>
  renderHook(
    ({ tab, dialogOpen }: { tab: string; dialogOpen: boolean }) => {
      const form = useForm<AssetFormFields>({
        defaultValues: {
          assetType: 'Avatar',
          name: '',
          creator: '',
          imageFilename: null,
          boothItemId: 1,
          tags: [],
          memo: null,
          dependencies: [],
          category: '',
          supportedAvatars: [],
          publishedAt: null,
        },
      }) as AssetFormType
      const ai = useAiSuggest({ form, tab, dialogOpen })
      return { form, ...ai }
    },
    { wrapper, initialProps: { tab: initialTab, dialogOpen: true } },
  )

describe('useAiSuggest', () => {
  beforeEach(() => {
    mocks.suggest.mockReset()
    mocks.toast.mockReset()
  })

  it('calls the command once on entering manual-input and fills empty fields', async () => {
    mocks.suggest.mockResolvedValue(ok(suggestion('A-cat', ['A'], true)))
    const { result, rerender } = setup()

    expect(mocks.suggest).not.toHaveBeenCalled()

    rerender({ tab: 'manual-input', dialogOpen: true })

    await waitFor(() =>
      expect(result.current.form.getValues('category')).toBe('A-cat'),
    )
    expect(mocks.suggest).toHaveBeenCalledTimes(1)
    expect(mocks.suggest).toHaveBeenCalledWith(1, 'Avatar')
    expect(result.current.form.getValues('tags')).toEqual(['A'])
    expect(result.current.viewState.loading).toBe(false)
    expect(result.current.viewState.newValues.category).toBe('A-cat')
  })

  it('does not call again for the same tab, item and type', async () => {
    mocks.suggest.mockResolvedValue(ok(suggestion('A-cat')))
    const { result, rerender } = setup('manual-input')

    await waitFor(() =>
      expect(result.current.form.getValues('category')).toBe('A-cat'),
    )

    rerender({ tab: 'manual-input', dialogOpen: true })
    rerender({ tab: 'booth-input', dialogOpen: true })
    rerender({ tab: 'manual-input', dialogOpen: true })

    expect(mocks.suggest).toHaveBeenCalledTimes(1)
  })

  it('re-requests after reset and keeps a user value equal to the old AI value', async () => {
    mocks.suggest.mockResolvedValue(ok(suggestion('A-cat')))
    const { result, rerender } = setup('manual-input')

    await waitFor(() =>
      expect(result.current.form.getValues('category')).toBe('A-cat'),
    )

    // Re-seed the form with the same boothItemId; the user types the old AI value
    rerender({ tab: 'booth-input', dialogOpen: true })
    act(() => {
      result.current.reset()
      result.current.form.setValue('category', 'A-cat')
    })

    mocks.suggest.mockResolvedValue(ok(suggestion('B-cat')))
    rerender({ tab: 'manual-input', dialogOpen: true })

    await waitFor(() => expect(mocks.suggest).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(result.current.viewState.loading).toBe(false))
    expect(result.current.form.getValues('category')).toBe('A-cat')
  })

  it('discards a response that resolves after the asset type changed', async () => {
    let resolve: (v: ReturnType<typeof ok>) => void = () => {}
    mocks.suggest.mockReturnValue(
      new Promise((r) => {
        resolve = r
      }),
    )
    const { result } = setup('manual-input')

    await waitFor(() => expect(result.current.viewState.loading).toBe(true))

    act(() => {
      result.current.form.setValue('assetType', 'WorldObject')
    })
    await act(async () => {
      resolve(ok(suggestion('A-cat')))
    })

    expect(result.current.form.getValues('category')).toBe('')
    expect(result.current.viewState.loading).toBe(false)
    expect(mocks.toast).not.toHaveBeenCalled()
  })

  it('toasts on error and retries when re-entering the tab', async () => {
    mocks.suggest.mockResolvedValue({ status: 'error', error: 'RateLimited' })
    const { result, rerender } = setup('manual-input')

    await waitFor(() => expect(mocks.toast).toHaveBeenCalledTimes(1))
    expect(mocks.toast).toHaveBeenCalledWith({
      title: 'addasset:ai-suggest:error:rate-limited',
      duration: 5000,
    })
    expect(result.current.viewState.loading).toBe(false)

    mocks.suggest.mockResolvedValue(ok(suggestion('A-cat')))
    rerender({ tab: 'booth-input', dialogOpen: true })
    rerender({ tab: 'manual-input', dialogOpen: true })

    await waitFor(() =>
      expect(result.current.form.getValues('category')).toBe('A-cat'),
    )
    expect(mocks.suggest).toHaveBeenCalledTimes(2)
  })

  it('toasts a generic failure when the command throws', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    mocks.suggest.mockRejectedValue(new Error('boom'))
    const { result } = setup('manual-input')

    await waitFor(() => expect(mocks.toast).toHaveBeenCalledTimes(1))
    expect(mocks.toast).toHaveBeenCalledWith({
      title: 'addasset:ai-suggest:error:failed',
      duration: 5000,
    })
    expect(result.current.viewState.loading).toBe(false)
  })

  it('drops a response that arrives after the dialog closed', async () => {
    let resolve: (v: ReturnType<typeof ok>) => void = () => {}
    mocks.suggest.mockReturnValue(
      new Promise((r) => {
        resolve = r
      }),
    )
    const { result, rerender } = setup('manual-input')

    await waitFor(() => expect(result.current.viewState.loading).toBe(true))

    rerender({ tab: 'manual-input', dialogOpen: false })
    await act(async () => {
      resolve(ok(suggestion('A-cat')))
    })

    expect(result.current.form.getValues('category')).toBe('')
    expect(mocks.toast).not.toHaveBeenCalled()
  })

  it('does not touch the form when the response arrives off the review tab, and applies it from cache on re-entry', async () => {
    let resolve: (v: ReturnType<typeof ok>) => void = () => {}
    mocks.suggest.mockReturnValue(
      new Promise((r) => {
        resolve = r
      }),
    )
    const { result, rerender } = setup('manual-input')

    await waitFor(() => expect(result.current.viewState.loading).toBe(true))

    rerender({ tab: 'additional-input', dialogOpen: true })
    await act(async () => {
      resolve(ok(suggestion('A-cat', ['A'], true)))
    })

    expect(result.current.form.getValues('category')).toBe('')
    expect(result.current.form.getValues('tags')).toEqual([])
    expect(result.current.viewState.newValues.category).toBeNull()
    expect(result.current.viewState.loading).toBe(false)

    rerender({ tab: 'manual-input', dialogOpen: true })

    await waitFor(() =>
      expect(result.current.form.getValues('category')).toBe('A-cat'),
    )
    expect(result.current.form.getValues('tags')).toEqual(['A'])
    expect(result.current.viewState.newValues.category).toBe('A-cat')
    expect(mocks.suggest).toHaveBeenCalledTimes(1)
  })

  it('calls the command only twice when switching A -> B -> A', async () => {
    mocks.suggest.mockImplementation(async (_id: number, type: string) =>
      ok(suggestion(type === 'Avatar' ? 'A-cat' : 'B-cat')),
    )
    const { result, rerender } = setup('manual-input')

    await waitFor(() =>
      expect(result.current.form.getValues('category')).toBe('A-cat'),
    )

    act(() => {
      result.current.form.setValue('assetType', 'WorldObject')
    })
    rerender({ tab: 'booth-input', dialogOpen: true })
    rerender({ tab: 'manual-input', dialogOpen: true })

    await waitFor(() =>
      expect(result.current.form.getValues('category')).toBe('B-cat'),
    )
    expect(mocks.suggest).toHaveBeenCalledTimes(2)

    act(() => {
      result.current.form.setValue('assetType', 'Avatar')
    })
    rerender({ tab: 'booth-input', dialogOpen: true })
    rerender({ tab: 'manual-input', dialogOpen: true })

    await waitFor(() =>
      expect(result.current.form.getValues('category')).toBe('A-cat'),
    )
    expect(mocks.suggest).toHaveBeenCalledTimes(2)
  })

  it('does not fire a second request for a key that is still in flight', async () => {
    mocks.suggest.mockReturnValue(new Promise(() => {}))
    const { result, rerender } = setup('manual-input')

    await waitFor(() => expect(result.current.viewState.loading).toBe(true))

    rerender({ tab: 'booth-input', dialogOpen: true })
    rerender({ tab: 'manual-input', dialogOpen: true })

    expect(mocks.suggest).toHaveBeenCalledTimes(1)
  })

  it('does not call the command when the dialog is closed and reopened on manual-input', async () => {
    mocks.suggest.mockResolvedValue(ok(suggestion('A-cat')))
    const { result, rerender } = setup('manual-input')

    await waitFor(() =>
      expect(result.current.form.getValues('category')).toBe('A-cat'),
    )
    expect(mocks.suggest).toHaveBeenCalledTimes(1)

    rerender({ tab: 'manual-input', dialogOpen: false })
    rerender({ tab: 'manual-input', dialogOpen: true })

    expect(mocks.suggest).toHaveBeenCalledTimes(1)
  })

  it('clears the new-value badges as soon as a request starts for a different item', async () => {
    mocks.suggest.mockResolvedValueOnce(ok(suggestion('A-cat', ['A'], true)))
    const { result, rerender } = setup('manual-input')

    await waitFor(() =>
      expect(result.current.viewState.newValues.category).toBe('A-cat'),
    )

    mocks.suggest.mockReturnValue(new Promise(() => {}))
    act(() => {
      result.current.form.setValue('boothItemId', 2)
    })
    rerender({ tab: 'booth-input', dialogOpen: true })
    rerender({ tab: 'manual-input', dialogOpen: true })

    await waitFor(() => expect(mocks.suggest).toHaveBeenCalledTimes(2))
    expect(result.current.viewState.newValues.category).toBeNull()
  })

  it('removes unmodified AI values from the previous item when the next item fails', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    mocks.suggest.mockResolvedValueOnce(ok(suggestion('A-cat', ['A'], true)))
    const { result, rerender } = setup('manual-input')

    await waitFor(() =>
      expect(result.current.form.getValues('category')).toBe('A-cat'),
    )

    mocks.suggest.mockResolvedValue({ status: 'error', error: 'Timeout' })
    act(() => {
      result.current.form.setValue('boothItemId', 2)
    })
    rerender({ tab: 'booth-input', dialogOpen: true })
    rerender({ tab: 'manual-input', dialogOpen: true })

    await waitFor(() => expect(mocks.toast).toHaveBeenCalledTimes(1))
    expect(result.current.form.getValues('category')).toBe('')
    expect(result.current.form.getValues('tags')).toEqual([])
    expect(result.current.viewState.newValues.category).toBeNull()
    expect(result.current.viewState.newValues.tags).toEqual([])
  })

  it('keeps user-modified values when the next item fails', async () => {
    mocks.suggest.mockResolvedValueOnce(ok(suggestion('A-cat', ['A'])))
    const { result, rerender } = setup('manual-input')

    await waitFor(() =>
      expect(result.current.form.getValues('category')).toBe('A-cat'),
    )

    act(() => {
      result.current.form.setValue('category', 'mine')
      result.current.form.setValue('boothItemId', 2)
    })
    mocks.suggest.mockResolvedValue({ status: 'error', error: 'Timeout' })
    rerender({ tab: 'booth-input', dialogOpen: true })
    rerender({ tab: 'manual-input', dialogOpen: true })

    await waitFor(() => expect(mocks.toast).toHaveBeenCalledTimes(1))
    expect(result.current.form.getValues('category')).toBe('mine')
    expect(result.current.form.getValues('tags')).toEqual([])
  })
})
