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
import type { StatusVariant } from '@/components/status-badge'

import type { ServiceStatus } from '../types'

/** 可选的统计区间；后端只接受这三个值。 */
export const SERVICE_STATUS_WINDOWS = [
  { hours: 24, labelKey: 'Last 24 hours' },
  { hours: 72, labelKey: 'Last 3 days' },
  { hours: 168, labelKey: 'Last 7 days' },
] as const

const STATUS_LABEL_KEYS: Record<ServiceStatus, string> = {
  operational: 'Operational',
  degraded: 'Degraded',
  partial: 'Partial outage',
  unknown: 'No traffic',
}

const STATUS_VARIANTS: Record<ServiceStatus, StatusVariant> = {
  operational: 'success',
  degraded: 'warning',
  partial: 'danger',
  unknown: 'neutral',
}

const STATUS_HEADLINE_KEYS: Record<ServiceStatus, string> = {
  operational: 'Requests are being served normally.',
  degraded: 'Some requests failed in this window.',
  partial: 'A significant share of requests failed in this window.',
  unknown: 'No traffic in this window yet.',
}

const STATUS_CELL_CLASSES: Record<ServiceStatus, string> = {
  operational: 'bg-success',
  degraded: 'bg-warning',
  partial: 'bg-destructive',
  // 没有流量的格子留灰色，不能画成绿色假装正常。
  unknown: 'bg-muted-foreground/20',
}

export function serviceStatusLabelKey(status: ServiceStatus): string {
  return STATUS_LABEL_KEYS[status] ?? STATUS_LABEL_KEYS.unknown
}

export function serviceStatusVariant(status: ServiceStatus): StatusVariant {
  return STATUS_VARIANTS[status] ?? 'neutral'
}

export function serviceStatusHeadlineKey(status: ServiceStatus): string {
  return STATUS_HEADLINE_KEYS[status] ?? STATUS_HEADLINE_KEYS.unknown
}

export function serviceStatusCellClass(status: ServiceStatus): string {
  return STATUS_CELL_CLASSES[status] ?? STATUS_CELL_CLASSES.unknown
}

/** 一格代表多长时间：半小时以内讲分钟，否则讲小时。 */
export function serviceStatusIntervalKey(cellSeconds: number): {
  key: string
  count: number
} {
  if (cellSeconds < 3600) {
    return {
      key: '{{count}} min per cell',
      count: Math.round(cellSeconds / 60),
    }
  }
  const hours = cellSeconds / 3600
  return {
    key: '{{count}} h per cell',
    count: Number.isInteger(hours) ? hours : Number(hours.toFixed(1)),
  }
}
