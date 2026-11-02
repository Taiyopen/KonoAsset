import { AssetSummary, commands, Result } from '@/lib/bindings'
import { AssetFormType } from '@/lib/form'

type Props = {
  boothItemId: number
  form: AssetFormType
  setImageUrls: (imageUrls: string[]) => void
}

type GumroadProps = {
  gumroadUrl: string
  form: AssetFormType
  setImageUrls: (imageUrls: string[]) => void
}

type ReturnProps =
  | {
      goNext: true
      duplicated: false
    }
  | {
      goNext: false
      duplicated: true
      duplicatedItems: AssetSummary[]
    }

export const getAndSetAssetInfoFromBoothToForm = async ({
  boothItemId,
  form,
  setImageUrls,
}: Props): Promise<Result<ReturnProps, string>> => {
  const result = await commands.getAssetInfoFromBooth(boothItemId)

  if (result.status === 'error') {
    return result
  }

  const data = result.data

  form.setValue('name', data.name)
  form.setValue('creator', data.creator)
  form.setValue('publishedAt', data.publishedAt)
  form.setValue('boothItemId', boothItemId)
  form.setValue('assetType', data.estimatedAssetType ?? 'Avatar')

  await setImages(form, setImageUrls, data.imageUrls)

  return toDuplicationResult(
    await commands.getAssetDisplaysByBoothId(boothItemId),
  )
}

// Gumroad にはカテゴリがないため、assetType と boothItemId は変更しない
export const getAndSetAssetInfoFromGumroadToForm = async ({
  gumroadUrl,
  form,
  setImageUrls,
}: GumroadProps): Promise<Result<ReturnProps, string>> => {
  const result = await commands.getAssetInfoFromGumroad(gumroadUrl)

  if (result.status === 'error') {
    return result
  }

  const data = result.data

  form.setValue('name', data.name)
  form.setValue('creator', data.creator)
  form.setValue('publishedAt', data.publishedAt)
  form.setValue('gumroadUrl', data.url)

  await setImages(form, setImageUrls, data.imageUrls)

  // 入力された URL ではなく正式な URL で重複を確認する
  return toDuplicationResult(
    await commands.getAssetDisplaysByGumroadUrl(data.url),
  )
}

const setImages = async (
  form: AssetFormType,
  setImageUrls: (imageUrls: string[]) => void,
  imageUrls: string[],
) => {
  setImageUrls(imageUrls)
  if (imageUrls.length > 0) {
    const imageResolveResult = await commands.resolvePximgFilename(
      imageUrls[0],
    )

    if (imageResolveResult.status === 'ok') {
      form.setValue('imageFilename', imageResolveResult.data)
    }
  }
}

const toDuplicationResult = (
  duplicationCheckResult: Result<AssetSummary[], string>,
): Result<ReturnProps, string> => {
  if (duplicationCheckResult.status === 'error') {
    return {
      status: 'error',
      error: duplicationCheckResult.error,
    }
  }

  if (duplicationCheckResult.data.length > 0) {
    return {
      status: 'ok',
      data: {
        goNext: false,
        duplicated: true,
        duplicatedItems: duplicationCheckResult.data,
      },
    }
  }

  return {
    status: 'ok',
    data: {
      goNext: true,
      duplicated: false,
    },
  }
}
