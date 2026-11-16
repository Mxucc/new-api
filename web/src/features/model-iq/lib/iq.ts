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

import type { ModelIqStatus, ModelIqVerdict, PelicanTarget } from '../types'

/** i18n keys, so a verdict is rendered through `t()` everywhere. */
const VERDICT_LABEL_KEYS: Record<ModelIqVerdict, string> = {
  healthy: 'Healthy',
  degraded: 'Degraded',
  poor: 'Poor',
  failed: 'Failed',
  pending: 'Not tested',
}

const VERDICT_VARIANTS: Record<ModelIqVerdict, StatusVariant> = {
  healthy: 'success',
  degraded: 'warning',
  poor: 'danger',
  failed: 'danger',
  pending: 'neutral',
}

const DIMENSION_LABEL_KEYS: Record<string, string> = {
  drawing: 'Drawing',
  subject: 'Subject',
  vehicle: 'Vehicle',
  animation: 'Animation',
  delivery: 'Delivery',
  delivery_partial: 'Delivery',
}

export function iqVerdictLabelKey(verdict: ModelIqVerdict): string {
  return VERDICT_LABEL_KEYS[verdict] ?? VERDICT_LABEL_KEYS.pending
}

export function iqVerdictVariant(verdict: ModelIqVerdict): StatusVariant {
  return VERDICT_VARIANTS[verdict] ?? 'neutral'
}

export function iqDimensionLabelKey(dimension: string): string | null {
  return DIMENSION_LABEL_KEYS[dimension] ?? null
}

/**
 * （分组, 模型）组合在页面上的稳定标识：既做 React key，也做巡视设置里多选的取值。
 * 后端按同样的两个字段校验，所以这里必须能还原回组合。
 */
export function pelicanTargetValue(target: PelicanTarget): string {
  return `${target.group}/${target.model}`
}

export function pelicanTargetFromValue(value: string): PelicanTarget {
  const separator = value.indexOf('/')
  if (separator < 0) {
    return { group: '', model: value }
  }
  return {
    group: value.slice(0, separator),
    model: value.slice(separator + 1),
  }
}

/** 展示用的组合名：`分组 · 模型`，避免同一模型的不同分组看起来一样。 */
export function pelicanTargetLabel(target: PelicanTarget): string {
  return `${target.group} · ${target.model}`
}

/** Only a scored run has an IQ number; a model that was never probed does not. */
export function iqHasScore(verdict: ModelIqVerdict): boolean {
  return verdict === 'healthy' || verdict === 'degraded' || verdict === 'poor'
}

export function summarizeModelIq(models: ModelIqStatus[]): {
  tested: number
  healthy: number
  untested: number
  unavailable: number
} {
  let tested = 0
  let healthy = 0
  let untested = 0
  let unavailable = 0
  for (const model of models) {
    if (model.verdict === 'pending') {
      untested += 1
    } else {
      tested += 1
    }
    if (model.verdict === 'healthy') {
      healthy += 1
    }
    if (!model.available) {
      unavailable += 1
    }
  }
  return { tested, healthy, untested, unavailable }
}
