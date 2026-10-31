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

// 行の先頭（半角・全角空白の前）のURLから、パスの最後の区切りを取り出す
const lastPathSegment = (lineText: string): string => {
  const url = lineText.trim().split(/\s+/)[0] ?? ''

  let path = url
  try {
    path = new URL(url).pathname
  } catch {
    // URLとして解釈できない場合は文字列全体から取り出す
  }

  return path.split('/').filter(Boolean).pop() ?? ''
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

  return {
    label: `${siteDisplayName(first.site, t)}：${lastPathSegment(first.text)}`,
    count: validLines.length,
  }
}
