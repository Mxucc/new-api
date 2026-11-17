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

/**
 * 格子底色：沿用状态语义色，但用半透明填充而不是实心块，
 * 好让格子里的分数保持正文对比度（实心 success 上放数字读不清）。
 */
const VERDICT_CELL_CLASSES: Record<ModelIqVerdict, string> = {
  healthy: 'border-success/30 bg-success/15 text-success',
  degraded: 'border-warning/30 bg-warning/15 text-warning',
  poor: 'border-destructive/30 bg-destructive/15 text-destructive',
  failed: 'border-destructive/25 bg-destructive/10 text-destructive',
  pending: 'border-border bg-muted text-muted-foreground',
}

export function iqCellClass(verdict: ModelIqVerdict): string {
  return VERDICT_CELL_CLASSES[verdict] ?? VERDICT_CELL_CLASSES.pending
}

// 智商格子也只有两档：可用（healthy）与不可用（其余判定）。failed 是传输失败，
// 会被当成「这次没采到样」，不会进时间条。
const STRIP_CELL_CLASSES: Record<ModelIqVerdict, string> = {
  healthy: 'bg-success',
  degraded: 'bg-destructive',
  poor: 'bg-destructive',
  failed: 'bg-destructive',
  // 一次都没测过的格子留灰。
  pending: 'bg-muted',
}

/** 时间条上一小格的颜色：可用绿、不可用红，没测过是灰的。 */
export function iqStripCellClass(verdict: ModelIqVerdict): string {
  return STRIP_CELL_CLASSES[verdict] ?? STRIP_CELL_CLASSES.pending
}

/** 时间条上一小格的文案：同样只有可用与不可用两档。 */
export function iqStripVerdictLabelKey(verdict: ModelIqVerdict): string {
  if (verdict === 'pending') {
    return 'Not tested'
  }
  return verdict === 'healthy' ? 'Available' : 'Unavailable'
}

/** 一行 = 一个探测项。每个分组只配一个模型，所以不需要模型列，模型名写在格子里。 */
export type IqRow = {
  group: string
  status: ModelIqStatus
}

export function buildIqRows(models: ModelIqStatus[]): IqRow[] {
  return [...models]
    .sort(
      (a, b) =>
        a.group.localeCompare(b.group) ||
        a.model_name.localeCompare(b.model_name)
    )
    .map((status) => ({ group: status.group, status }))
}

/**
 * 可用率：真正采到样的格子里「可用」的占比（0–1）。
 * 沿用上一次成功结果的格子不参与统计——上游 502 只是没采到样，不算降智。
 * 一次采样都没有时返回 null，界面显示「—」。
 */
export function iqAvailabilityRate(
  cells: ModelIqStatus['strip']
): number | null {
  let sampled = 0
  let available = 0
  for (const cell of cells) {
    if (!cell.sampled) {
      continue
    }
    sampled += 1
    if (cell.verdict === 'healthy') {
      available += 1
    }
  }
  return sampled === 0 ? null : available / sampled
}

/** 有探测在跑才继续轮询：整轮巡视或者任一格子还在测。 */
export function iqNeedsPolling(
  overview:
    | {
        sweeping: boolean
        models: ModelIqStatus[]
      }
    | undefined
): boolean {
  if (!overview) {
    return false
  }
  return overview.sweeping || overview.models.some((model) => model.testing)
}
