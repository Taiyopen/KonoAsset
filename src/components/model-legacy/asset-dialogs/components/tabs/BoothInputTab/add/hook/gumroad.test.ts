import { afterEach, describe, expect, it, vi } from 'vitest'
import { useBoothInputTabForAddDialog } from '.'
import { act, renderHook } from '@testing-library/react'
import {
  getAndSetAssetInfoFromBoothToForm,
  getAndSetAssetInfoFromGumroadToForm,
} from '../../logic'

vi.mock('../../logic', () => {
  const ok = { status: 'ok', data: { goNext: true, duplicated: false } }
  return {
    getAndSetAssetInfoFromBoothToForm: vi.fn().mockResolvedValue(ok),
    getAndSetAssetInfoFromGumroadToForm: vi.fn().mockResolvedValue(ok),
  }
})

vi.mock('@/hooks/use-toast', () => {
  const toast = vi.fn()
  return { useToast: () => ({ toast }) }
})

vi.mock('@/lib/bindings', () => ({
  commands: {
    getAppState: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        displayStyle: 'GridMedium',
        sort: { sortBy: 'CreatedAt', reversed: true },
      },
    }),
  },
}))

const mockForm = {
  getValues: vi.fn().mockReturnValue(null),
  setValue: vi.fn(),
}

const renderAddHook = () =>
  renderHook(() =>
    useBoothInputTabForAddDialog({
      // @ts-expect-error mockForm is not a valid AssetFormType but satisfies the required fields
      form: mockForm,
      setTab: vi.fn(),
    }),
  )

const typeUrl = (
  result: ReturnType<typeof renderAddHook>['result'],
  url: string,
) => {
  act(() => {
    result.current.onUrlInputChange({
      target: { value: url },
    } as React.ChangeEvent<HTMLInputElement>)
  })
}

describe('BoothInputTab Hook (Gumroad)', () => {
  afterEach(() => {
    vi.clearAllMocks()
  })

  it('fetches from Gumroad for a Gumroad URL', async () => {
    const { result } = renderAddHook()
    const url = 'https://bananabread.gumroad.com/l/ajfrt'

    typeUrl(result, url)
    expect(result.current.canFetch).toBe(true)

    await result.current.getAssetDescriptionFromBooth()

    expect(getAndSetAssetInfoFromGumroadToForm).toHaveBeenCalledWith(
      expect.objectContaining({ gumroadUrl: url }),
    )
    expect(getAndSetAssetInfoFromBoothToForm).not.toHaveBeenCalled()
  })

  it('fetches from BOOTH for a BOOTH URL', async () => {
    const { result } = renderAddHook()

    typeUrl(result, 'https://booth.pm/ja/items/6641548')
    expect(result.current.canFetch).toBe(true)

    await result.current.getAssetDescriptionFromBooth()

    expect(getAndSetAssetInfoFromBoothToForm).toHaveBeenCalledWith(
      expect.objectContaining({ boothItemId: 6641548 }),
    )
    expect(getAndSetAssetInfoFromGumroadToForm).not.toHaveBeenCalled()
  })

  // 新規追加では、前に取得した別プラットフォームのリンクを新しい素材へ持ち越さない
  it('clears a previously fetched BOOTH link before fetching from Gumroad', async () => {
    const { result } = renderAddHook()

    typeUrl(result, 'https://bananabread.gumroad.com/l/ajfrt')
    await result.current.getAssetDescriptionFromBooth()

    expect(mockForm.setValue).toHaveBeenCalledWith('boothItemId', null)
  })

  it('clears a previously fetched Gumroad link before fetching from BOOTH', async () => {
    const { result } = renderAddHook()

    typeUrl(result, 'https://booth.pm/ja/items/6641548')
    await result.current.getAssetDescriptionFromBooth()

    expect(mockForm.setValue).toHaveBeenCalledWith('gumroadUrl', null)
  })

  it('cannot fetch a Gumroad profile URL', async () => {
    const { result } = renderAddHook()

    typeUrl(result, 'https://bananabread.gumroad.com/')
    expect(result.current.canFetch).toBe(false)

    await result.current.getAssetDescriptionFromBooth()

    expect(getAndSetAssetInfoFromGumroadToForm).not.toHaveBeenCalled()
    expect(getAndSetAssetInfoFromBoothToForm).not.toHaveBeenCalled()
  })
})
