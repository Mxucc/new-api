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
import { useTranslation } from 'react-i18next'

import {
  StaticDataTable,
  staticDataTableClassNames as tableStyles,
} from '@/components/data-table'
import { StatusBadge } from '@/components/status-badge'
import {
  formatLatency,
  formatUptimePct,
} from '@/features/performance-metrics/lib/format'
import { formatNumber } from '@/lib/format'
import { cn } from '@/lib/utils'

import { serviceStatusLabelKey, serviceStatusVariant } from '../lib/status'
import type { ServiceStatusModel } from '../types'
import { ServiceStatusBar } from './service-status-bar'

export interface ServiceStatusModelsProps {
  models: ServiceStatusModel[]
}

function ModelStatusBadge(props: { model: ServiceStatusModel }) {
  const { t } = useTranslation()
  return (
    <StatusBadge
      label={t(serviceStatusLabelKey(props.model.status))}
      variant={serviceStatusVariant(props.model.status)}
      copyable={false}
    />
  )
}

/** 各模型的状态条与关键指标：可用率、请求数、平均延迟。 */
export function ServiceStatusModels(props: ServiceStatusModelsProps) {
  const { t } = useTranslation()

  return (
    <>
      <StaticDataTable
        className='hidden md:block'
        tableClassName='min-w-[820px] text-sm'
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
              <span className='block max-w-[220px] truncate font-medium'>
                {model.model_name}
              </span>
            ),
          },
          {
            id: 'status',
            header: t('Status'),
            className: tableStyles.compactHeaderCell,
            cellClassName: tableStyles.compactCell,
            cell: (model) => <ModelStatusBadge model={model} />,
          },
          {
            id: 'availability',
            header: t('Availability'),
            className: tableStyles.compactHeaderCellRight,
            cellClassName: tableStyles.compactMutedNumericCell,
            cell: (model) => formatUptimePct(model.success_rate),
          },
          {
            id: 'requests',
            header: t('Requests'),
            className: tableStyles.compactHeaderCellRight,
            cellClassName: tableStyles.compactMutedNumericCell,
            cell: (model) => formatNumber(model.requests),
          },
          {
            id: 'latency',
            header: t('Average latency'),
            className: tableStyles.compactHeaderCellRight,
            cellClassName: tableStyles.compactMutedNumericCell,
            cell: (model) => formatLatency(model.avg_latency_ms),
          },
          {
            id: 'timeline',
            header: t('Availability over time'),
            className: cn(tableStyles.compactHeaderCell, 'w-[220px]'),
            cellClassName: tableStyles.compactCell,
            cell: (model) => (
              <ServiceStatusBar
                cells={model.cells}
                size='sm'
                label={`${model.model_name} ${t('Availability over time')}`}
              />
            ),
          },
        ]}
      />

      <ul className='space-y-2 md:hidden'>
        {props.models.map((model) => (
          <li
            key={model.model_name}
            className='bg-card rounded-lg border p-3'
            data-testid={`service-status-model-${model.model_name}`}
          >
            <div className='flex items-start justify-between gap-2'>
              <span className='min-w-0 truncate text-sm font-medium'>
                {model.model_name}
              </span>
              <ModelStatusBadge model={model} />
            </div>
            <div className='text-muted-foreground mt-1 flex flex-wrap gap-x-3 gap-y-0.5 text-xs'>
              <span>
                {t('Availability')} {formatUptimePct(model.success_rate)}
              </span>
              <span>
                {t('{{count}} requests', {
                  count: formatNumber(model.requests),
                })}
              </span>
              <span>{formatLatency(model.avg_latency_ms)}</span>
            </div>
            <div className='mt-2'>
              <ServiceStatusBar
                cells={model.cells}
                size='sm'
                label={`${model.model_name} ${t('Availability over time')}`}
              />
            </div>
          </li>
        ))}
      </ul>
    </>
  )
}
