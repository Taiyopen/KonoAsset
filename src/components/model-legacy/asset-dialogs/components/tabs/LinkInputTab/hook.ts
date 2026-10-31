import { useContext, useEffect, useState } from 'react'
import { commands, ParsedLine } from '@/lib/bindings'
import { AddAssetDialogContext } from '../../../AddAssetDialog'
import { canProceed } from './logic'

const PARSE_DEBOUNCE_MS = 300

type Props = {
  setTab: (tab: string) => void
}

type ReturnProps = {
  text: string
  setText: (text: string) => void
  parsed: ParsedLine[]
  proceedable: boolean
  backToPreviousTab: () => void
  moveToNextTab: () => void
}

export const useLinkInputTab = ({ setTab }: Props): ReturnProps => {
  const { downloadLinks, setDownloadLinks, setAssetPaths } = useContext(
    AddAssetDialogContext,
  )

  const [text, setText] = useState(downloadLinks ?? '')
  // 解析結果とその元になった入力文字列（入力と食い違う結果を使わないため）
  const [parseResult, setParseResult] = useState<{
    text: string
    lines: ParsedLine[]
  }>({ text: '', lines: [] })

  useEffect(() => {
    if (text.trim() === '') {
      return
    }

    // 入力が変わった場合や、順序が前後して返ってきた古い応答は無視する
    let isStale = false

    const timer = setTimeout(async () => {
      try {
        const lines = await commands.parseDownloadLinks(text)

        if (!isStale) {
          setParseResult({ text, lines })
        }
      } catch (error) {
        console.error('Failed to parse download links:', error)
      }
    }, PARSE_DEBOUNCE_MS)

    return () => {
      isStale = true
      clearTimeout(timer)
    }
  }, [text])

  const parsed =
    text.trim() !== '' && parseResult.text === text ? parseResult.lines : []

  const proceedable = canProceed(parsed)

  const backToPreviousTab = () => {
    setTab('selector')
  }

  const moveToNextTab = () => {
    if (!proceedable) {
      return
    }

    setDownloadLinks(text)
    setAssetPaths([])
    setTab('booth-input')
  }

  return {
    text,
    setText,
    parsed,
    proceedable,
    backToPreviousTab,
    moveToNextTab,
  }
}
