import { beforeEach, describe, expect, it, vi } from 'vitest'
import { getAndSetAssetInfoFromGumroadToForm } from '.'
import { commands, GumroadAssetInfo } from '@/lib/bindings'
import { AssetFormType } from '@/lib/form'

const canonicalUrl = 'https://bananabread.gumroad.com/l/ajfrt'

const mockGumroadAssetInfo: GumroadAssetInfo = {
  url: canonicalUrl,
  name: 'N',
  creator: 'C',
  imageUrls: ['https://public-files.gumroad.com/a'],
  publishedAt: 1687351822000,
}

vi.mock('@/lib/bindings', () => ({
  commands: {
    getAssetInfoFromGumroad: vi.fn(),
    getAssetDisplaysByGumroadUrl: vi.fn(),
    resolvePximgFilename: vi.fn(),
  },
}))

const createForm = (initial: Record<string, unknown>) => {
  const values: Record<string, unknown> = { ...initial }
  return {
    values,
    form: {
      setValue: (key: string, value: unknown) => {
        values[key] = value
      },
      getValues: (key: string) => values[key],
    } as unknown as AssetFormType,
  }
}

describe('getAndSetAssetInfoFromGumroadToForm', () => {
  beforeEach(() => {
    vi.mocked(commands.getAssetInfoFromGumroad).mockResolvedValue({
      status: 'ok',
      data: mockGumroadAssetInfo,
    })
    vi.mocked(commands.getAssetDisplaysByGumroadUrl).mockResolvedValue({
      status: 'ok',
      data: [],
    })
    vi.mocked(commands.resolvePximgFilename).mockResolvedValue({
      status: 'ok',
      data: 'imageFilename',
    })
  })

  it('fills form and stores canonical url', async () => {
    const { form, values } = createForm({})
    const setImageUrls = vi.fn()

    const result = await getAndSetAssetInfoFromGumroadToForm({
      gumroadUrl: 'https://gumroad.com/l/ajfrt?x=1',
      form,
      setImageUrls,
    })

    expect(values.name).toBe('N')
    expect(values.creator).toBe('C')
    expect(values.publishedAt).toBe(1687351822000)
    expect(values.gumroadUrl).toBe(canonicalUrl)
    expect(values.imageFilename).toBe('imageFilename')
    expect(setImageUrls).toHaveBeenCalledWith([
      'https://public-files.gumroad.com/a',
    ])
    expect(commands.getAssetInfoFromGumroad).toHaveBeenCalledWith(
      'https://gumroad.com/l/ajfrt?x=1',
    )
    expect(commands.resolvePximgFilename).toHaveBeenCalledWith(
      'https://public-files.gumroad.com/a',
    )
    expect(commands.getAssetDisplaysByGumroadUrl).toHaveBeenCalledWith(
      canonicalUrl,
    )
    expect(result).toEqual({
      status: 'ok',
      data: { goNext: true, duplicated: false },
    })
  })

  it('keeps boothItemId and assetType', async () => {
    const { form, values } = createForm({
      boothItemId: 6641548,
      assetType: 'WorldObject',
    })

    await getAndSetAssetInfoFromGumroadToForm({
      gumroadUrl: canonicalUrl,
      form,
      setImageUrls: vi.fn(),
    })

    expect(values.boothItemId).toBe(6641548)
    expect(values.assetType).toBe('WorldObject')
  })

  it('returns duplicated', async () => {
    vi.mocked(commands.getAssetDisplaysByGumroadUrl).mockResolvedValue({
      status: 'ok',
      // @ts-expect-error only the item identity matters for this test
      data: [{ id: 'existing' }],
    })
    const { form } = createForm({})

    const result = await getAndSetAssetInfoFromGumroadToForm({
      gumroadUrl: canonicalUrl,
      form,
      setImageUrls: vi.fn(),
    })

    expect(result).toEqual({
      status: 'ok',
      data: {
        goNext: false,
        duplicated: true,
        duplicatedItems: [{ id: 'existing' }],
      },
    })
  })

  it('returns error', async () => {
    vi.mocked(commands.getAssetInfoFromGumroad).mockResolvedValue({
      status: 'error',
      error: 'not found',
    })
    const { form, values } = createForm({ name: 'before' })

    const result = await getAndSetAssetInfoFromGumroadToForm({
      gumroadUrl: canonicalUrl,
      form,
      setImageUrls: vi.fn(),
    })

    expect(result).toEqual({ status: 'error', error: 'not found' })
    expect(values.name).toBe('before')
  })
})
