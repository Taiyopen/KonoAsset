import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import { ParsedLine } from '@/lib/bindings'
import { useLinkInputTab } from './hook'

const parseDownloadLinks = vi.hoisted(() => vi.fn())

vi.mock('@/lib/bindings', () => ({
  commands: { parseDownloadLinks },
}))

vi.mock('../../../AddAssetDialog', async () => {
  const { createContext } = await import('react')

  return {
    AddAssetDialogContext: createContext({
      setAssetPaths: () => {},
      setDownloadLinks: () => {},
    }),
  }
})

const validLine = (text: string): ParsedLine => ({
  lineNumber: 1,
  text,
  site: 'Pixeldrain',
  kind: 'File',
  hasPassword: false,
  valid: true,
})

describe('useLinkInputTab', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    parseDownloadLinks.mockReset()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('debounces parsing by 300ms and enables proceeding', async () => {
    parseDownloadLinks.mockImplementation(async (text: string) => [
      validLine(text),
    ])
    const { result } = renderHook(() => useLinkInputTab({ setTab: vi.fn() }))

    act(() => result.current.setText('a'))
    act(() => result.current.setText('ab'))
    await act(async () => {
      await vi.advanceTimersByTimeAsync(299)
    })
    expect(parseDownloadLinks).not.toHaveBeenCalled()
    expect(result.current.proceedable).toBe(false)

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1)
    })
    expect(parseDownloadLinks).toHaveBeenCalledTimes(1)
    expect(parseDownloadLinks).toHaveBeenCalledWith('ab')
    expect(result.current.proceedable).toBe(true)
  })

  it('ignores a response whose input is no longer current', async () => {
    let resolveFirst: (lines: ParsedLine[]) => void = () => {}
    parseDownloadLinks
      .mockImplementationOnce(
        () =>
          new Promise<ParsedLine[]>((resolve) => {
            resolveFirst = resolve
          }),
      )
      .mockImplementationOnce(async (text: string) => [validLine(text)])
    const { result } = renderHook(() => useLinkInputTab({ setTab: vi.fn() }))

    act(() => result.current.setText('first'))
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300)
    })

    act(() => result.current.setText('second'))
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300)
    })
    expect(result.current.parsed[0].text).toBe('second')

    await act(async () => {
      resolveFirst([validLine('first')])
    })
    expect(result.current.parsed[0].text).toBe('second')
  })
})
