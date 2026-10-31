import { setupAndRender } from '@/test/init'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, screen } from '@testing-library/react'
import { AiSuggestSettings } from '.'

vi.mock('@/hooks/use-localization', () => {
  return {
    useLocalization: () => ({
      t: (key: string) => key,
    }),
  }
})

const placeholderKey = 'preference:settings:ai-suggest:api-key:placeholder'

describe('AiSuggestSettings', () => {
  afterEach(() => {
    cleanup()
  })

  it('passes the trimmed key to setApiKey on blur', async () => {
    const setApiKey = vi.fn()
    const { user } = setupAndRender(
      <AiSuggestSettings
        enabled={true}
        apiKey=""
        setEnabled={() => {}}
        setApiKey={setApiKey}
      />,
    )
    const input = screen.getByPlaceholderText(placeholderKey)

    await user.type(input, '  sk-ant-abc  ')
    expect(setApiKey).not.toHaveBeenCalled()

    await user.tab()

    expect(setApiKey).toHaveBeenCalledOnce()
    expect(setApiKey).toHaveBeenCalledWith('sk-ant-abc')
  })

  it('does not call setApiKey when the value is unchanged', async () => {
    const setApiKey = vi.fn()
    const { user } = setupAndRender(
      <AiSuggestSettings
        enabled={true}
        apiKey="sk-ant-abc"
        setEnabled={() => {}}
        setApiKey={setApiKey}
      />,
    )
    const input = screen.getByPlaceholderText(placeholderKey)

    await user.click(input)
    await user.tab()

    expect(setApiKey).not.toHaveBeenCalled()
  })

  it('toggles key visibility with the eye button', async () => {
    const { user } = setupAndRender(
      <AiSuggestSettings
        enabled={true}
        apiKey="sk-ant-abc"
        setEnabled={() => {}}
        setApiKey={() => {}}
      />,
    )
    const input = screen.getByPlaceholderText(placeholderKey)
    expect(input.getAttribute('type')).toBe('password')

    await user.click(screen.getByRole('button'))

    expect(input.getAttribute('type')).toBe('text')
  })
})
