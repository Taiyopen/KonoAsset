import { describe, expect, it, vi } from 'vitest'
import { buildDownloadPreAsset, sendDownloadImportRequest } from '.'
import { AssetDescription, PreWorldObject } from '@/lib/bindings'

const requestDownloadImport = vi.hoisted(() =>
  vi.fn().mockResolvedValue({ status: 'ok', data: 'job-1' }),
)

vi.mock('@/lib/bindings', () => ({
  commands: { requestDownloadImport },
}))

const description: AssetDescription = {
  name: 'name',
  creator: 'creator',
  imageFilename: null,
  tags: [],
  memo: null,
  dependencies: [],
  boothItemId: null,
  createdAt: 1,
  publishedAt: null,
}

describe('buildDownloadPreAsset', () => {
  it('wraps by asset type', () => {
    const preAsset: PreWorldObject = { description, category: 'Furniture' }
    expect(buildDownloadPreAsset('WorldObject', preAsset)).toEqual({
      status: 'ok',
      data: { WorldObject: { description, category: 'Furniture' } },
    })
    expect(buildDownloadPreAsset('Avatar', { description })).toEqual({
      status: 'ok',
      data: { Avatar: { description } },
    })
  })

  it('returns an error for an unsupported asset type', () => {
    // @ts-expect-error testing runtime guard
    const r = buildDownloadPreAsset('Unknown', { description })
    expect(r.status).toBe('error')
  })
})

describe('sendDownloadImportRequest', () => {
  it('sends links text with the wrapped pre asset', async () => {
    const result = await sendDownloadImportRequest(
      'OtherAsset',
      'https://example.com/a.zip',
      { description, category: 'x' },
    )
    expect(result).toEqual({ status: 'ok', data: 'job-1' })
    expect(requestDownloadImport).toHaveBeenCalledWith({
      linksText: 'https://example.com/a.zip',
      preAsset: { OtherAsset: { description, category: 'x' } },
    })
  })
})
