/*
Copyright (C) 2023-2026 QuantumNous

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU Affero General Public License as
published by the Free Software Foundation, either version 3 of the
License, or (at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
GNU Affero General Public License for more details.

You should have received a copy of the GNU Affero General Public License
along with this program. If not, see <https://www.gnu.org/licenses/>.

For commercial licensing, please contact support@quantumnous.com
*/
import { useQuery } from '@tanstack/react-query'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { Dialog } from '@/components/dialog'
import { LoadingState } from '@/components/loading-state'

import { getPelicanResult } from '../api'
import { iqDimensionLabelKey } from '../lib/iq'

export interface PelicanPreviewDialogProps {
  open: boolean
  modelName: string
  resultId: number
  onOpenChange: (open: boolean) => void
}

/**
 * 预览模型交付的动画。
 * 源码来自上游模型，因此放在 sandbox（带 allow-scripts、不带 allow-same-origin）里渲染，
 * 脚本能跑起来看动画，但拿不到本页的 cookie、token 或 DOM。
 */
export function PelicanPreviewDialog(props: PelicanPreviewDialogProps) {
  const { t } = useTranslation()
  const query = useQuery({
    queryKey: ['model-iq', 'result', props.resultId],
    queryFn: () => getPelicanResult(props.resultId),
    enabled: props.open && props.resultId > 0,
  })

  // 加载、有画面与没画面三种状态按顺序判定，避免嵌套三元。
  let body: ReactNode
  if (query.isLoading) {
    body = <LoadingState />
  } else if (query.data?.output) {
    body = (
      <div className='space-y-3'>
        <iframe
          title={`${props.modelName} ${t('Animation preview')}`}
          srcDoc={query.data.output}
          sandbox='allow-scripts'
          referrerPolicy='no-referrer'
          className='aspect-[4/3] w-full rounded-lg border bg-white'
          data-testid='pelican-preview-frame'
        />
        <div className='text-muted-foreground flex flex-wrap gap-2 text-xs'>
          {query.data.dimensions.map((dimension) => {
            const labelKey = iqDimensionLabelKey(dimension)
            return (
              <span
                key={dimension}
                className='bg-muted rounded-full px-2 py-0.5'
              >
                {labelKey ? t(labelKey) : dimension}
              </span>
            )
          })}
        </div>
      </div>
    )
  } else {
    body = (
      <p className='text-muted-foreground text-sm'>
        {query.data?.error ||
          t('The model did not return a renderable answer.')}
      </p>
    )
  }

  return (
    <Dialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      title={t('Pelican IQ test')}
      description={props.modelName}
      contentClassName='sm:max-w-3xl'
    >
      {body}
      <p className='text-muted-foreground text-xs'>
        {t(
          'The animation renders in a sandboxed frame, so scripts in the model output cannot reach this page.'
        )}
      </p>
    </Dialog>
  )
}
