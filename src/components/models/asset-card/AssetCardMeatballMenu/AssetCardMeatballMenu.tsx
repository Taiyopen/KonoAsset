import { FC } from 'react'
import { useAssetCardMeatballMenu } from './hook'
import { InternalAssetCardMeatballMenu } from './internal'

type Props = {
  id: string
  className?: string
  boothItemID?: number
  gumroadUrl?: string
  openEditAssetDialog: () => void
}

export const AssetCardMeatballMenu: FC<Props> = ({
  id,
  className,
  boothItemID,
  gumroadUrl,
  openEditAssetDialog,
}) => {
  const {
    boothUrl,
    executeAssetDeletion,
    openDataManagementDialog,
    useTrashBin,
  } = useAssetCardMeatballMenu({
    id,
    boothItemID,
  })

  return (
    <InternalAssetCardMeatballMenu
      className={className}
      boothUrl={boothUrl ?? undefined}
      gumroadUrl={gumroadUrl}
      openEditAssetDialog={openEditAssetDialog}
      openDataManagementDialog={openDataManagementDialog}
      executeAssetDeletion={executeAssetDeletion}
      useTrashBin={useTrashBin}
    />
  )
}
