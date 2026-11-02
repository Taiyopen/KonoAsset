import {
  convertToBoothURL,
  extractBoothItemId,
  isGumroadURL,
} from '@/lib/utils'
import { useState, useContext, useEffect, ChangeEvent } from 'react'
import { AddAssetDialogContext } from '../../../../../AddAssetDialog'
import { sep } from '@tauri-apps/api/path'
import { AssetFormType } from '@/lib/form'
import { useToast } from '@/hooks/use-toast'
import {
  getAndSetAssetInfoFromBoothToForm,
  getAndSetAssetInfoFromGumroadToForm,
} from '../../logic'
import { useLocalization } from '@/hooks/use-localization'
import { commands, ParsedLine } from '@/lib/bindings'
import { representativeLinkLabel } from '../../../LinkInputTab/logic'

type Props = {
  form: AssetFormType
  setTab: (tab: string) => void
  setImageUrls: (imageUrls: string[]) => void
}

type ReturnProps = {
  representativeImportFilename: string
  importFileCount: number
  getAssetDescriptionFromBooth: () => Promise<void>
  onUrlInputChange: (e: ChangeEvent<HTMLInputElement>) => void
  fetching: boolean
  boothUrlInput: string
  canFetch: boolean
  moveToNextTab: () => void
  backToPreviousTab: () => void
}

export const useBoothInputTabForAddDialog = ({
  form,
  setTab,
  setImageUrls,
}: Props): ReturnProps => {
  const formBoothItemId = form.getValues('boothItemId')
  const formBoothUrl =
    formBoothItemId !== null
      ? convertToBoothURL(formBoothItemId)
      : (form.getValues('gumroadUrl') ?? '')

  const [boothItemId, setBoothItemId] = useState(formBoothItemId)
  const [boothUrlInput, setBoothUrlInput] = useState(formBoothUrl)
  const [fetching, setFetching] = useState(false)

  const { t } = useLocalization()
  const { toast } = useToast()

  const { assetPaths, downloadLinks, setDuplicateWarningItems } = useContext(
    AddAssetDialogContext,
  )

  const [parsedLinks, setParsedLinks] = useState<ParsedLine[]>([])

  useEffect(() => {
    if (downloadLinks === undefined) {
      return
    }

    let isCancelled = false

    commands
      .parseDownloadLinks(downloadLinks)
      .then((lines) => {
        if (!isCancelled) {
          setParsedLinks(lines)
        }
      })
      .catch((error) => {
        console.error('Failed to parse download links:', error)
      })

    return () => {
      isCancelled = true
    }
  }, [downloadLinks])

  const backToPreviousTab = () => {
    setTab(downloadLinks !== undefined ? 'link-input' : 'selector')
  }

  const moveToNextTab = () => {
    setTab('asset-type-selector')
  }

  const moveToDuplicationWarning = () => {
    setTab('duplicate-warning')
  }

  const canFetch = boothItemId !== null || isGumroadURL(boothUrlInput)

  const getAssetDescriptionFromBooth = async () => {
    if (fetching || !canFetch) {
      return
    }

    try {
      setFetching(true)

      // 前に取得した別プラットフォームのリンクを、別の商品の素材へ持ち越さない
      // （両方のリンクを持たせたい場合は編集画面で追加する）
      form.setValue(boothItemId !== null ? 'gumroadUrl' : 'boothItemId', null)

      const result =
        boothItemId !== null
          ? await getAndSetAssetInfoFromBoothToForm({
              boothItemId: boothItemId,
              form: form,
              setImageUrls,
            })
          : await getAndSetAssetInfoFromGumroadToForm({
              gumroadUrl: boothUrlInput,
              form: form,
              setImageUrls,
            })

      if (result.status === 'ok') {
        const data = result.data

        if (data.goNext) {
          moveToNextTab()
        } else if (data.duplicated) {
          setDuplicateWarningItems(data.duplicatedItems)
          moveToDuplicationWarning()
        }
      } else {
        toast({
          title: t('addasset:booth-input:failed-to-get-info'),
          description: result.error,
        })
      }
    } finally {
      setFetching(false)
    }
  }

  const onUrlInputChange = (e: ChangeEvent<HTMLInputElement>) => {
    const url = e.target.value
    setBoothUrlInput(url)

    const extractIdResult = extractBoothItemId(url)

    if (extractIdResult.status === 'ok') {
      setBoothItemId(extractIdResult.data)
    } else {
      setBoothItemId(null)
    }
  }

  const linkLabel =
    downloadLinks !== undefined
      ? representativeLinkLabel(parsedLinks, t)
      : undefined

  const representativeImportFilename =
    linkLabel !== undefined
      ? linkLabel.label
      : assetPaths !== undefined && assetPaths.length > 0
        ? (assetPaths[0].split(sep()).pop() as string)
        : t('addasset:booth-input:no-file-selected')

  const importFileCount =
    linkLabel !== undefined
      ? linkLabel.count
      : assetPaths !== undefined
        ? assetPaths.length
        : 0

  return {
    representativeImportFilename,
    importFileCount,
    getAssetDescriptionFromBooth,
    onUrlInputChange,
    fetching,
    boothUrlInput,
    canFetch,
    moveToNextTab,
    backToPreviousTab,
  }
}
