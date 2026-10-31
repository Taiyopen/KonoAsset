import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import MultipleSelector from './multi-select'

vi.mock('@/hooks/use-localization', () => {
  return {
    useLocalization: () => ({
      t: (key: string) => key,
    }),
  }
})

describe('MultipleSelector', () => {
  afterEach(() => {
    cleanup()
  })

  it('marks highlighted badges with the label', () => {
    render(
      <MultipleSelector
        value={['A', 'B']}
        highlightedValues={['B']}
        highlightLabel="新增"
      />,
    )

    expect(
      screen
        .getByText('B')
        .closest('[data-badge]')
        ?.hasAttribute('data-highlighted'),
    ).toBe(true)
    expect(
      screen
        .getByText('A')
        .closest('[data-badge]')
        ?.hasAttribute('data-highlighted'),
    ).toBe(false)
    expect(screen.getAllByText('新增')).toHaveLength(1)
  })

  it('does not highlight anything when no highlightedValues are given', () => {
    render(<MultipleSelector value={['A']} highlightLabel="新增" />)

    expect(
      screen
        .getByText('A')
        .closest('[data-badge]')
        ?.hasAttribute('data-highlighted'),
    ).toBe(false)
    expect(screen.queryByText('新增')).toBeNull()
  })
})
