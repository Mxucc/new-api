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
import { History, Play, Sparkles } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import {
  StaticDataTable,
  staticDataTableClassNames as tableStyles,
} from '@/components/data-table'
import { GroupBadge } from '@/components/group-badge'
import { StatusBadge } from '@/components/status-badge'
import { Button } from '@/components/ui/button'
import { formatLatency } from '@/features/performance-metrics/lib/format'
import { toIntlLocale } from '@/i18n/languages'
import { formatTimestampRelative } from '@/lib/format'

import { pelicanTargetValue } from '../lib/iq'
import type { ModelIqStatus } from '../types'
import { ModelIqScore } from './model-iq-score'

export interface ModelIqTableProps {
  models: ModelIqStatus[]
  isAdmin: boolean
  /** 正在跑的组合（`分组/模型`），用来禁用重复触发。 */
  busyTarget: string | null
  /** 整轮巡视进行中时，逐行触发也要一起禁用。 */
  sweepRunning: boolean
  onPreview: (status: ModelIqStatus) => void
  onHistory: (status: ModelIqStatus) => void
  onRun: (status: ModelIqStatus) => void
}

function ModelIqActions(props: {
  status: ModelIqStatus
  isAdmin: boolean
  busyTarget: string | null
  sweepRunning: boolean
  onPreview: (status: ModelIqStatus) => void
  onHistory: (status: ModelIqStatus) => void
  onRun: (status: ModelIqStatus) => void
}) {
  const { t } = useTranslation()
  const key = pelicanTargetValue({
    group: props.status.group,
    model: props.status.model_name,
  })
  const running = props.busyTarget === key
  return (
    <div className='flex items-center justify-end gap-1.5'>
      <Button
        variant='ghost'
        size='sm'
        onClick={() => props.onHistory(props.status)}
      >
        <History data-icon='inline-start' />
        {t('History')}
      </Button>
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
          disabled={props.busyTarget !== null || props.sweepRunning}
          onClick={() => props.onRun(props.status)}
        >
          <Play data-icon='inline-start' />
          {running ? t('Testing…') : t('Run test')}
        </Button>
      ) : null}
    </div>
  )
}

/** 站点每个（分组, 模型）组合的状态与智商。桌面用表格，窄屏用卡片。 */
export function ModelIqTable(props: ModelIqTableProps) {
  const { t, i18n } = useTranslation()
  const locale = toIntlLocale(i18n.resolvedLanguage || i18n.language)

  return (
    <>
      <StaticDataTable
        className='hidden md:block'
        tableClassName='min-w-[880px] text-sm'
        headerRowClassName={tableStyles.compactHeaderRow}
        data={props.models}
        getRowKey={(status) =>
          pelicanTargetValue({
            group: status.group,
            model: status.model_name,
          })
        }
        columns={[
          {
            id: 'group',
            header: t('Group'),
            className: tableStyles.compactHeaderCell,
            cellClassName: tableStyles.compactCell,
            cell: (status) => (
              <GroupBadge group={status.group} copyable={false} />
            ),
          },
          {
            id: 'model',
            header: t('Model'),
            className: tableStyles.compactHeaderCell,
            cellClassName: tableStyles.compactCell,
            cell: (status) => (
              <div className='flex min-w-0 items-center gap-2'>
                <span className='truncate font-medium'>
                  {status.model_name}
                </span>
                {status.available ? null : (
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
            cell: (status) => (
              <div className='flex justify-end'>
                <ModelIqScore status={status} />
              </div>
            ),
          },
          {
            id: 'latency',
            header: t('Latency'),
            className: tableStyles.compactHeaderCellRight,
            cellClassName: tableStyles.compactMutedNumericCell,
            cell: (status) => formatLatency(status.latency_ms),
          },
          {
            id: 'tested_at',
            header: t('Tested at'),
            className: tableStyles.compactHeaderCellRight,
            cellClassName: tableStyles.compactMutedNumericCell,
            cell: (status) =>
              status.tested_at
                ? formatTimestampRelative(status.tested_at, 'seconds', locale)
                : '—',
          },
          {
            id: 'actions',
            header: '',
            className: tableStyles.actionHeaderCell,
            cellClassName: tableStyles.compactCell,
            cell: (status) => (
              <ModelIqActions
                status={status}
                isAdmin={props.isAdmin}
                busyTarget={props.busyTarget}
                sweepRunning={props.sweepRunning}
                onPreview={props.onPreview}
                onHistory={props.onHistory}
                onRun={props.onRun}
              />
            ),
          },
        ]}
      />

      <ul className='space-y-2 md:hidden' aria-label={t('Model IQ')}>
        {props.models.map((status) => (
          <li
            key={pelicanTargetValue({
              group: status.group,
              model: status.model_name,
            })}
            className='bg-card rounded-lg border p-3'
            data-testid={`model-iq-card-${status.group}-${status.model_name}`}
          >
            <div className='flex items-start justify-between gap-2'>
              <div className='min-w-0'>
                <div className='flex min-w-0 items-center gap-2'>
                  <GroupBadge group={status.group} copyable={false} />
                  <span className='truncate text-sm font-medium'>
                    {status.model_name}
                  </span>
                </div>
                <p className='text-muted-foreground mt-0.5 text-xs'>
                  {formatLatency(status.latency_ms)} ·{' '}
                  {status.tested_at
                    ? formatTimestampRelative(
                        status.tested_at,
                        'seconds',
                        locale
                      )
                    : t('Not tested')}
                </p>
              </div>
              <ModelIqScore status={status} />
            </div>
            {status.available ? null : (
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
                status={status}
                isAdmin={props.isAdmin}
                busyTarget={props.busyTarget}
                sweepRunning={props.sweepRunning}
                onPreview={props.onPreview}
                onHistory={props.onHistory}
                onRun={props.onRun}
              />
            </div>
          </li>
        ))}
      </ul>
    </>
  )
}
