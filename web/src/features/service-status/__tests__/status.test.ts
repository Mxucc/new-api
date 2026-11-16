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
import { describe, expect, it } from 'vitest'

import {
  SERVICE_STATUS_WINDOWS,
  serviceStatusCellClass,
  serviceStatusHeadlineKey,
  serviceStatusIntervalKey,
  serviceStatusLabelKey,
  serviceStatusVariant,
} from '../lib/status'
import type { ServiceStatus } from '../types'

describe('service status presentation', () => {
  it('labels and colors every status band', () => {
    const bands: ServiceStatus[] = [
      'operational',
      'degraded',
      'partial',
      'unknown',
    ]
    expect(bands.map(serviceStatusLabelKey)).toEqual([
      'Operational',
      'Degraded',
      'Partial outage',
      'No traffic',
    ])
    expect(bands.map(serviceStatusVariant)).toEqual([
      'success',
      'warning',
      'danger',
      'neutral',
    ])
  })

  it('treats a slot without traffic as available', () => {
    expect(serviceStatusCellClass('operational')).toBe('bg-success')
    expect(serviceStatusCellClass('degraded')).toBe('bg-warning')
    expect(serviceStatusCellClass('partial')).toBe('bg-destructive')
    // 没有流量的格子按可用处理，状态条保持连续。
    expect(serviceStatusCellClass('unknown')).toBe('bg-success')
  })

  it('describes a band with a sentence a user can act on', () => {
    expect(serviceStatusHeadlineKey('operational')).toBe(
      'Requests are being served normally.'
    )
    expect(serviceStatusHeadlineKey('degraded')).toBe(
      'Some requests failed in this window.'
    )
    expect(serviceStatusHeadlineKey('partial')).toBe(
      'A significant share of requests failed in this window.'
    )
    expect(serviceStatusHeadlineKey('unknown')).toBe(
      'No traffic in this window yet.'
    )
  })

  it('falls back to the unknown band for an unrecognized status', () => {
    const surprise = 'maintenance' as ServiceStatus
    expect(serviceStatusLabelKey(surprise)).toBe('No traffic')
    expect(serviceStatusVariant(surprise)).toBe('neutral')
  })

  it('describes the cell width in minutes or hours', () => {
    expect(serviceStatusIntervalKey(1800)).toEqual({
      key: '{{count}} min per cell',
      count: 30,
    })
    expect(serviceStatusIntervalKey(5400)).toEqual({
      key: '{{count}} h per cell',
      count: 1.5,
    })
    expect(serviceStatusIntervalKey(12600)).toEqual({
      key: '{{count}} h per cell',
      count: 3.5,
    })
  })

  it('offers the three windows the API accepts', () => {
    expect(SERVICE_STATUS_WINDOWS.map((window) => window.hours)).toEqual([
      24, 72, 168,
    ])
  })
})
