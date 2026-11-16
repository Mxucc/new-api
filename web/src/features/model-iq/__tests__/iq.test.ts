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
  buildIqGrid,
  iqCellClass,
  iqDimensionLabelKey,
  iqHasScore,
  iqNeedsPolling,
  iqVerdictLabelKey,
  iqVerdictVariant,
  pelicanTargetLabel,
  summarizeModelIq,
} from '../lib/iq'
import type { ModelIqStatus, ModelIqVerdict } from '../types'

function status(overrides: Partial<ModelIqStatus> = {}): ModelIqStatus {
  return {
    group: 'default',
    model_name: 'gpt-4.1',
    available: true,
    verdict: 'healthy',
    score: 100,
    latency_ms: 1200,
    tested_at: 1700000000,
    result_id: 1,
    dimensions: ['drawing'],
    testing: false,
    ...overrides,
  }
}

describe('model IQ verdict mapping', () => {
  it('labels every verdict and falls back to the untested label', () => {
    expect(iqVerdictLabelKey('healthy')).toBe('Healthy')
    expect(iqVerdictLabelKey('degraded')).toBe('Degraded')
    expect(iqVerdictLabelKey('poor')).toBe('Poor')
    expect(iqVerdictLabelKey('failed')).toBe('Failed')
    expect(iqVerdictLabelKey('pending')).toBe('Not tested')
  })

  it('uses a danger badge for poor and failed runs only', () => {
    expect(iqVerdictVariant('healthy')).toBe('success')
    expect(iqVerdictVariant('degraded')).toBe('warning')
    expect(iqVerdictVariant('poor')).toBe('danger')
    expect(iqVerdictVariant('failed')).toBe('danger')
    expect(iqVerdictVariant('pending')).toBe('neutral')
  })

  it('treats a failed transport run as an outcome without an IQ number', () => {
    const scored: ModelIqVerdict[] = ['healthy', 'degraded', 'poor']
    for (const verdict of scored) {
      expect(iqHasScore(verdict)).toBe(true)
    }
    expect(iqHasScore('failed')).toBe(false)
    expect(iqHasScore('pending')).toBe(false)
  })

  it('maps every scored dimension to a label key', () => {
    expect(iqDimensionLabelKey('drawing')).toBe('Drawing')
    expect(iqDimensionLabelKey('delivery_partial')).toBe('Delivery')
    expect(iqDimensionLabelKey('unknown-dimension')).toBeNull()
  })
})

describe('pelican target label', () => {
  it('keeps a pair apart from the same model in another group', () => {
    expect(pelicanTargetLabel({ group: 'vip', model: 'gpt-4.1' })).toBe(
      'vip · gpt-4.1'
    )
    expect(pelicanTargetLabel({ group: 'default', model: 'gpt-4.1' })).not.toBe(
      pelicanTargetLabel({ group: 'vip', model: 'gpt-4.1' })
    )
  })
})

describe('summarizeModelIq', () => {
  it('counts tested, healthy and delisted combinations separately', () => {
    const summary = summarizeModelIq([
      status(),
      status({ model_name: 'claude', verdict: 'degraded', score: 65 }),
      status({ model_name: 'never-probed', verdict: 'pending', score: 0 }),
      status({ model_name: 'gone', available: false, verdict: 'failed' }),
    ])
    expect(summary).toEqual({
      tested: 3,
      healthy: 1,
      untested: 1,
      unavailable: 1,
    })
  })

  it('reports zeroes for an empty model list', () => {
    expect(summarizeModelIq([])).toEqual({
      tested: 0,
      healthy: 0,
      untested: 0,
      unavailable: 0,
    })
  })

describe('buildIqGrid', () => {
  it('lays out groups as rows and probed models as columns', () => {
    const grid = buildIqGrid([
      status({ model_name: 'gpt-4.1' }),
      status({ model_name: 'claude', verdict: 'degraded', score: 65 }),
      status({ group: 'vip', model_name: 'claude', verdict: 'poor' }),
    ])

    expect(grid.columns).toEqual(['claude', 'gpt-4.1'])
    expect(grid.rows.map((row) => row.group)).toEqual(['default', 'vip'])
    expect(grid.rows[0].cells.map((cell) => cell?.model_name)).toEqual([
      'claude',
      'gpt-4.1',
    ])
    // vip 分组没有 gpt-4.1 的结果：这一列留空，列才对得上。
    expect(
      grid.rows[1].cells.map((cell) => cell?.model_name ?? null)
    ).toEqual(['claude', null])
    expect(grid.total).toBe(3)
  })

  it('skips a combination that was never probed', () => {
    const grid = buildIqGrid([
      status(),
      status({ model_name: 'never-probed', verdict: 'pending', score: 0 }),
    ])
    expect(grid.columns).toEqual(['gpt-4.1'])
    expect(grid.total).toBe(1)
  })

  it('keeps an unprobed combination that is being tested right now', () => {
    const grid = buildIqGrid([
      status({
        model_name: 'never-probed',
        verdict: 'pending',
        score: 0,
        testing: true,
      }),
    ])
    expect(grid.columns).toEqual(['never-probed'])
    expect(grid.total).toBe(1)
  })
})

describe('iqCellClass', () => {
  it('maps every verdict onto a semantic colour token', () => {
    expect(iqCellClass('healthy')).toContain('success')
    expect(iqCellClass('degraded')).toContain('warning')
    expect(iqCellClass('poor')).toContain('destructive')
    expect(iqCellClass('failed')).toContain('destructive')
    expect(iqCellClass('pending')).toContain('muted')
  })
})

describe('iqNeedsPolling', () => {
  it('polls while a sweep or a single probe is running', () => {
    expect(iqNeedsPolling({ sweeping: true, models: [status()] })).toBe(true)
    expect(
      iqNeedsPolling({ sweeping: false, models: [status({ testing: true })] })
    ).toBe(true)
  })

  it('stops polling once everything is idle or nothing was loaded', () => {
    expect(iqNeedsPolling({ sweeping: false, models: [status()] })).toBe(false)
    expect(iqNeedsPolling(undefined)).toBe(false)
  })
})
})
