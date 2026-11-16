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

import { formatUptimePct } from '@/features/performance-metrics/lib/format'
import { toIntlLocale } from '@/i18n/languages'
import { formatNumber, formatTimestampRelative } from '@/lib/format'
import { cn } from '@/lib/utils'

import { serviceStatusCellClass, serviceStatusLabelKey } from '../lib/status'
import type { ServiceStatusCell } from '../types'

export interface ServiceStatusBarProps {
  cells: ServiceStatusCell[]
  /** 无障碍名称，说明这条状态条属于谁。 */
  label: string
  size?: 'sm' | 'md'
}

/**
 * 时间轴状态条：每格代表一个时间片，颜色是那段真实请求的可用率。
 * 没有流量的格子（unknown）不渲染——整行铺满灰块会把真实故障淹掉，
 * 只画有数据的格子后留白本身就说明了「这段没有流量」。
 */
export function ServiceStatusBar(props: ServiceStatusBarProps) {
  const { t, i18n } = useTranslation()
  const locale = toIntlLocale(i18n.resolvedLanguage || i18n.language)
  const height = props.size === 'sm' ? 'h-5' : 'h-9'
  const filledCells = props.cells.filter((cell) => cell.status !== 'unknown')

  return (
    <div
      role='img'
      aria-label={props.label}
      className='flex w-full items-end gap-[2px]'
    >
      {filledCells.map((cell, index) => {
        const detail = `${formatTimestampRelative(cell.start, 'seconds', locale)} · ${formatUptimePct(cell.success_rate)} · ${t('{{count}} requests', { count: formatNumber(cell.requests) })}`
        return (
          <span
            key={cell.start}
            data-testid={`service-status-cell-${index}`}
            title={detail}
            className={cn(
              'min-w-[3px] flex-1 rounded-[2px]',
              height,
              serviceStatusCellClass(cell.status)
            )}
          >
            <span className='sr-only'>{`${t(serviceStatusLabelKey(cell.status))} ${detail}`}</span>
          </span>
        )
      })}
    </div>
  )
}
