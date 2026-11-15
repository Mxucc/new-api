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
import { Play, Sparkles } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import {
  StaticDataTable,
  staticDataTableClassNames as tableStyles,
} from '@/components/data-table'
import { StatusBadge } from '@/components/status-badge'
import { Button } from '@/components/ui/button'
import { formatLatency } from '@/features/performance-metrics/lib/format'
import { toIntlLocale } from '@/i18n/languages'
import { formatTimestampRelative } from '@/lib/format'

import type { ModelIqStatus } from '../types'
import { ModelIqScore } from './model-iq-score'

export interface ModelIqTableProps {
  models: ModelIqStatus[]
  isAdmin: boolean
  /** 正在跑的模型名，用来禁用重复触发。 */
  busyModel: string | null
  onPreview: (status: ModelIqStatus) => void
  onRun: (modelName: string) => void
}

function ModelIqActions(props: {
  status: ModelIqStatus
  isAdmin: boolean
  busyModel: string | null
  onPreview: (status: ModelIqStatus) => void
  onRun: (modelName: string) => void
}) {
  const { t } = useTranslation()
  const running = props.busyModel === props.status.model_name
  return (
    <div className='flex items-center justify-end gap-1.5'>
      <Button
        variant='ghost'
        size='sm'
        disabled={props.status.result_id <= 0}
        onClick={() => props.onPreview(props.status)}
      >
        <Sparkles data-icon='inline-start' />
        {t('Preview')}
      </Button>
      {props.isAdmin ? (
        <Button
          variant='outline'
          size='sm'
          disabled={props.busyModel !== null}
          onClick={() => props.onRun(props.status.model_name)}
        >
          <Play data-icon='inline-start' />
          {running ? t('Testing…') : t('Run test')}
        </Button>
      ) : null}
    </div>
  )
}

/** 站点模型状态与各自的智商。桌面用表格，窄屏用卡片，信息一致。 */
export function ModelIqTable(props: ModelIqTableProps) {
  const { t, i18n } = useTranslation()
  const locale = toIntlLocale(i18n.resolvedLanguage || i18n.language)

  return (
    <>
      <StaticDataTable
        className='hidden md:block'
        tableClassName='min-w-[760px] text-sm'
        headerRowClassName={tableStyles.compactHeaderRow}
        data={props.models}
        getRowKey={(model) => model.model_name}
        columns={[
          {
            id: 'model',
            header: t('Model'),
            className: tableStyles.compactHeaderCell,
            cellClassName: tableStyles.compactCell,
            cell: (model) => (
              <div className='flex min-w-0 items-center gap-2'>
                <span className='truncate font-medium'>{model.model_name}</span>
                {model.available ? null : (
                  <StatusBadge
                    label={t('Unavailable')}
                    variant='neutral'
                    copyable={false}
                  />
                )}
              </div>
            ),
          },
          {
            id: 'iq',
            header: t('IQ score'),
            className: tableStyles.compactHeaderCellRight,
            cellClassName: tableStyles.compactCell,
            cell: (model) => (
              <div className='flex justify-end'>
                <ModelIqScore status={model} />
              </div>
            ),
          },
          {
            id: 'latency',
            header: t('Latency'),
            className: tableStyles.compactHeaderCellRight,
            cellClassName: tableStyles.compactMutedNumericCell,
            cell: (model) => formatLatency(model.latency_ms),
          },
          {
            id: 'tested_at',
            header: t('Tested at'),
            className: tableStyles.compactHeaderCellRight,
            cellClassName: tableStyles.compactMutedNumericCell,
            cell: (model) =>
              model.tested_at
                ? formatTimestampRelative(model.tested_at, 'seconds', locale)
                : '—',
          },
          {
            id: 'actions',
            header: '',
            className: tableStyles.actionHeaderCell,
            cellClassName: tableStyles.compactCell,
            cell: (model) => (
              <ModelIqActions
                status={model}
                isAdmin={props.isAdmin}
                busyModel={props.busyModel}
                onPreview={props.onPreview}
                onRun={props.onRun}
              />
            ),
          },
        ]}
      />

      <ul className='space-y-2 md:hidden' aria-label={t('Model IQ')}>
        {props.models.map((model) => (
          <li
            key={model.model_name}
            className='bg-card rounded-lg border p-3'
            data-testid={`model-iq-card-${model.model_name}`}
          >
            <div className='flex items-start justify-between gap-2'>
              <div className='min-w-0'>
                <p className='truncate text-sm font-medium'>
                  {model.model_name}
                </p>
                <p className='text-muted-foreground mt-0.5 text-xs'>
                  {formatLatency(model.latency_ms)} ·{' '}
                  {model.tested_at
                    ? formatTimestampRelative(
                        model.tested_at,
                        'seconds',
                        locale
                      )
                    : t('Not tested')}
                </p>
              </div>
              <ModelIqScore status={model} />
            </div>
            {model.available ? null : (
              <div className='mt-2'>
                <StatusBadge
                  label={t('Unavailable')}
                  variant='neutral'
                  copyable={false}
                />
              </div>
            )}
            <div className='mt-2 flex justify-end'>
              <ModelIqActions
                status={model}
                isAdmin={props.isAdmin}
                busyModel={props.busyModel}
                onPreview={props.onPreview}
                onRun={props.onRun}
              />
            </div>
          </li>
        ))}
      </ul>
    </>
  )
}
