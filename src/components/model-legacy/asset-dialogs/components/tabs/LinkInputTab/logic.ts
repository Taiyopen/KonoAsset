import { ParsedLine, SiteKind } from '@/lib/bindings'

export const canProceed = (parsed: ParsedLine[]): boolean =>
  parsed.length > 0 && parsed.every((line) => line.valid)

export const siteDisplayName = (
  site: SiteKind,
  t: (key: string) => string,
): string => {
  switch (site) {
    case 'Workupload':
      return 'workupload'
    case 'Pixeldrain':
      return 'pixeldrain'
    case 'GoogleDrive':
      return 'Google Drive'
    case 'OneDrive':
      return 'OneDrive'
    case 'Direct':
      return t('addasset:link-input:site:direct')
  }
}

const decode = (segment: string): string => {
  try {
    return decodeURIComponent(segment)
  } catch {
    return segment
  }
}

// 行の先頭（半角・全角空白の前）のURLから表示用の名前を取り出す。
// Google Drive は view / edit ではなくファイル・フォルダの ID、それ以外はパスの最後の区切り
const linkName = (site: SiteKind, lineText: string): string => {
  const text = lineText.trim().split(/\s+/)[0] ?? ''

  let url: URL | null = null
  try {
    url = new URL(text)
  } catch {
    // URLとして解釈できない場合は文字列全体から取り出す
  }

  const segments = (url?.pathname ?? text).split('/').filter(Boolean)

  if (site === 'GoogleDrive') {
    const index = segments.findIndex((s) => s === 'd' || s === 'folders')
    const id = index === -1 ? undefined : segments[index + 1]
    return id ?? url?.searchParams.get('id') ?? ''
  }

  return decode(segments.pop() ?? '')
}

export const representativeLinkLabel = (
  parsed: ParsedLine[],
  t: (key: string) => string,
): { label: string; count: number } => {
  const validLines = parsed.filter((line) => line.valid)
  const first = validLines[0]

  if (first === undefined || first.site === null) {
    return { label: '', count: validLines.length }
  }

  const site = siteDisplayName(first.site, t)
  const name = linkName(first.site, first.text)

  return {
    label: name === '' ? site : `${site}：${name}`,
    count: validLines.length,
  }
}
