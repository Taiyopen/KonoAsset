import {
  convertToBoothURL,
  extractBoothItemId,
  isGumroadURL,
} from '@/lib/utils'
import { useState, ChangeEvent } from 'react'
import { AssetFormType } from '@/lib/form'
import { useToast } from '@/hooks/use-toast'
import {
  getAndSetAssetInfoFromBoothToForm,
  getAndSetAssetInfoFromGumroadToForm,
} from '../../logic'
import { useLocalization } from '@/hooks/use-localization'

type Props = {
  form: AssetFormType
  goToNextTab: () => void
  setImageUrls: (imageUrls: string[]) => void
}

type ReturnProps = {
  getAssetDescriptionFromBooth: () => Promise<void>
  onUrlInputChange: (e: ChangeEvent<HTMLInputElement>) => void
  fetching: boolean
  boothUrlInput: string
  canFetch: boolean
}

export const useBoothInputTabForEditDialog = ({
  form,
  goToNextTab,
  setImageUrls,
}: Props): ReturnProps => {
  const formBoothItemId = form.getValues('boothItemId')
  const formBoothUrl =
    formBoothItemId !== null
      ? convertToBoothURL(formBoothItemId)
      : (form.getValues('gumroadUrl') ?? '')

  const [boothUrlInput, setBoothUrlInput] = useState(formBoothUrl)
  const [boothItemId, setBoothItemId] = useState(formBoothItemId)
  const [fetching, setFetching] = useState(false)

  const { t } = useLocalization()
  const { toast } = useToast()

  const canFetch = boothItemId !== null || isGumroadURL(boothUrlInput)

  const getAssetDescriptionFromBooth = async () => {
    if (fetching || !canFetch) {
      return
    }

    try {
      setFetching(true)

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
        goToNextTab()
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

  return {
    getAssetDescriptionFromBooth,
    onUrlInputChange,
    fetching,
    boothUrlInput,
    canFetch,
  }
}
