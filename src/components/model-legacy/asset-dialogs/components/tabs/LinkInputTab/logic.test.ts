import { describe, expect, it } from 'vitest'
import { ParsedLine } from '@/lib/bindings'
import { canProceed, representativeLinkLabel, siteDisplayName } from './logic'

const line = (p: Partial<ParsedLine>): ParsedLine => ({
  lineNumber: 1,
  text: '',
  site: null,
  kind: null,
  hasPassword: false,
  valid: false,
  ...p,
})

describe('canProceed', () => {
  it('requires at least one line and all valid', () => {
    expect(canProceed([])).toBe(false)
    expect(
      canProceed([
        line({ valid: true, site: 'Workupload' }),
        line({ valid: false }),
      ]),
    ).toBe(false)
    expect(canProceed([line({ valid: true, site: 'Workupload' })])).toBe(true)
  })
})

describe('siteDisplayName', () => {
  it('returns display names for each site', () => {
    const t = (k: string) => k
    expect(siteDisplayName('Workupload', t)).toBe('workupload')
    expect(siteDisplayName('Pixeldrain', t)).toBe('pixeldrain')
    expect(siteDisplayName('GoogleDrive', t)).toBe('Google Drive')
    expect(siteDisplayName('OneDrive', t)).toBe('OneDrive')
    expect(siteDisplayName('Direct', t)).toBe('addasset:link-input:site:direct')
  })
})

describe('representativeLinkLabel', () => {
  it('uses first valid line', () => {
    expect(
      representativeLinkLabel(
        [
          line({
            valid: true,
            site: 'Workupload',
            text: 'https://workupload.com/archive/q2hv3vcBcy 12345',
          }),
          line({
            lineNumber: 2,
            valid: true,
            site: 'Pixeldrain',
            text: 'https://pixeldrain.com/u/pHP3XpbB',
          }),
        ],
        (k) => k,
      ),
    ).toEqual({ label: 'workupload：q2hv3vcBcy', count: 2 })
  })

  it('skips invalid lines and ignores query strings and trailing slashes', () => {
    expect(
      representativeLinkLabel(
        [
          line({ text: 'garbage' }),
          line({
            lineNumber: 2,
            valid: true,
            site: 'Direct',
            text: 'https://example.com/files/a.zip?dl=1',
          }),
        ],
        (k) => k,
      ),
    ).toEqual({ label: 'addasset:link-input:site:direct：a.zip', count: 1 })
  })

  it('handles fullwidth space separated passwords', () => {
    expect(
      representativeLinkLabel(
        [
          line({
            valid: true,
            site: 'Pixeldrain',
            text: 'https://pixeldrain.com/u/abc/　pass',
          }),
        ],
        (k) => k,
      ).label,
    ).toBe('pixeldrain：abc')
  })

  const labelOf = (site: ParsedLine['site'], text: string) =>
    representativeLinkLabel([line({ valid: true, site, text })], (k) => k).label

  it('uses the Google Drive id, not view or edit', () => {
    expect(
      labelOf('GoogleDrive', 'https://drive.google.com/file/d/1XyZ/view'),
    ).toBe('Google Drive：1XyZ')
    expect(
      labelOf('GoogleDrive', 'https://docs.google.com/file/d/1XyZ/edit?usp=x'),
    ).toBe('Google Drive：1XyZ')
    expect(
      labelOf('GoogleDrive', 'https://drive.google.com/drive/u/0/folders/1F'),
    ).toBe('Google Drive：1F')
    expect(
      labelOf('GoogleDrive', 'https://drive.google.com/open?id=1Q&usp=x'),
    ).toBe('Google Drive：1Q')
  })

  it('shows only the site name when the url has no path', () => {
    expect(labelOf('OneDrive', 'https://onedrive.live.com/?id=abc')).toBe(
      'OneDrive',
    )
  })

  it('decodes direct link names and keeps malformed escapes', () => {
    expect(
      labelOf('Direct', 'https://example.com/%E7%B4%A0%E6%9D%90.zip'),
    ).toBe('addasset:link-input:site:direct：素材.zip')
    expect(labelOf('Direct', 'https://example.com/100%.zip')).toBe(
      'addasset:link-input:site:direct：100%.zip',
    )
  })

  it('returns empty label when no valid lines', () => {
    expect(representativeLinkLabel([line({})], (k) => k)).toEqual({
      label: '',
      count: 0,
    })
  })
})
