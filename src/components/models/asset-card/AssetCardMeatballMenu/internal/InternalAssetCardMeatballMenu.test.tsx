import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, screen } from '@testing-library/react'
import { InternalAssetCardMeatballMenu } from '.'
import { setupAndRender } from '@/test/init'

vi.mock('@/hooks/use-localization', () => {
  return {
    useLocalization: () => ({
      t: (key: string) => key,
    }),
  }
})

const openMenu = async (gumroadUrl?: string) => {
  const { user } = setupAndRender(
    <InternalAssetCardMeatballMenu
      gumroadUrl={gumroadUrl}
      openEditAssetDialog={vi.fn()}
      openDataManagementDialog={vi.fn()}
      executeAssetDeletion={vi.fn()}
      useTrashBin={true}
    />,
  )

  await user.click(screen.getByRole('button'))
}

describe('InternalAssetCardMeatballMenu', () => {
  afterEach(() => {
    cleanup()
  })

  it('shows a link to Gumroad when gumroadUrl is set', async () => {
    await openMenu('https://bananabread.gumroad.com/l/ajfrt')

    const link = screen
      .getByText('assetcard:more-button:open-gumroad')
      .closest('a')

    expect(link?.getAttribute('href')).toBe(
      'https://bananabread.gumroad.com/l/ajfrt',
    )
    expect(link?.getAttribute('target')).toBe('_blank')
  })

  it('hides the Gumroad link when gumroadUrl is not set', async () => {
    await openMenu()

    expect(screen.getByText('assetcard:more-button:open-booth')).toBeTruthy()
    expect(screen.queryByText('assetcard:more-button:open-gumroad')).toBeNull()
  })
})
