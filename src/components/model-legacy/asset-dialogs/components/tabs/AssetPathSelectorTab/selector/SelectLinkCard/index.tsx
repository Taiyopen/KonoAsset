import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter } from '@/components/ui/card'
import { ChevronRight, Link } from 'lucide-react'
import { useLocalization } from '@/hooks/use-localization'

type Props = {
  onClick: () => void
}

export const SelectLinkCard = ({ onClick }: Props) => {
  const { t } = useLocalization()
  return (
    <Card>
      <CardContent>
        <div className="m-8 mb-2">
          <Link
            size={80}
            className="block mx-auto text-muted-foreground dark:text-foreground"
          />
        </div>
      </CardContent>
      <CardFooter className="px-4">
        <Button className="mx-auto" onClick={onClick}>
          <ChevronRight size={24} />
          {t('addasset:select-link')}
        </Button>
      </CardFooter>
    </Card>
  )
}
