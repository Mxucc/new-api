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
  available: 'Available',
  unavailable: 'Unavailable',
}

const STATUS_VARIANTS: Record<ServiceStatus, StatusVariant> = {
  available: 'success',
  unavailable: 'danger',
}

const STATUS_HEADLINE_KEYS: Record<ServiceStatus, string> = {
  available: 'Requests are being served normally.',
  unavailable: 'Some requests failed in this window.',
}

const STATUS_CELL_CLASSES: Record<ServiceStatus, string> = {
  available: 'bg-success',
  unavailable: 'bg-destructive',
}

export function serviceStatusLabelKey(status: ServiceStatus): string {
  return STATUS_LABEL_KEYS[status] ?? STATUS_LABEL_KEYS.available
}

export function serviceStatusVariant(status: ServiceStatus): StatusVariant {
  return STATUS_VARIANTS[status] ?? 'success'
}

export function serviceStatusHeadlineKey(status: ServiceStatus): string {
  return STATUS_HEADLINE_KEYS[status] ?? STATUS_HEADLINE_KEYS.available
}

export function serviceStatusCellClass(status: ServiceStatus): string {
  return STATUS_CELL_CLASSES[status] ?? STATUS_CELL_CLASSES.available
}

/** 状态行的标题：状态按（分组, 模型）统计，标题要同时写清两者。 */
export function serviceStatusTargetLabel(
  group: string,
  modelName: string
): string {
  return `${group} · ${modelName}`
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
