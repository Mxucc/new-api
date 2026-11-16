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

export type IqGridCell = {
  status: ModelIqStatus
  /** 这一格自己没有结果，用同列最近的一格补上；记下来源好让界面淡显。 */
  borrowedFrom?: string
}

export type IqGridRow = {
  group: string
  /** 这一行自己配置的目标，用于「立即测试」；格子里显示的可能是借来的数据。 */
  own: ModelIqStatus | null
  /** 与 columns 一一对应：该分组既没有数据、也借不到数据时留空。 */
  cells: (IqGridCell | null)[]
}

export type IqGrid = {
  /** 列 = 分组配置的模型，按名称排序，使各行对齐。 */
  columns: string[]
  rows: IqGridRow[]
  /** 真正画出来的组合数。 */
  total: number
}

/**
 * 行 = 分组，列 = 分组配置的模型。空格与「还没测过」的格子用同一列里最近的一格
 * 真实结果补上（淡显并标出来源）：只测过一次时整片空白看着像页面坏了。
 */
export function buildIqGrid(models: ModelIqStatus[]): IqGrid {
  const columns = [...new Set(models.map((status) => status.model_name))].sort(
    (a, b) => a.localeCompare(b)
  )
  const byGroup = new Map<string, Map<string, ModelIqStatus>>()
  for (const status of models) {
    const byModel = byGroup.get(status.group) ?? new Map()
    byModel.set(status.model_name, status)
    byGroup.set(status.group, byModel)
  }
  const rows: IqGridRow[] = [...byGroup.keys()]
    .sort((a, b) => a.localeCompare(b))
    .map((group) => {
      const byModel = byGroup.get(group)
      const own =
        columns
          .map((name) => byModel?.get(name))
          .find((status): status is ModelIqStatus => status !== undefined) ??
        null
      return {
        group,
        own,
        cells: columns.map((name) => {
          const status = byModel?.get(name)
          return status ? { status } : null
        }),
      }
    })
  columns.forEach((_, column) => {
    // 补格子的来源只取真正的结果：借来的格子不能再被借一次，否则来源会串。
    const known = rows
      .map((row, index) => ({
        index,
        cell: row.cells[column],
        group: row.group,
      }))
      .filter(
        (entry): entry is { index: number; cell: IqGridCell; group: string } =>
          entry.cell !== null && entry.cell.status.verdict !== 'pending'
      )
    if (known.length === 0) {
      return
    }
    rows.forEach((row, index) => {
      const current = row.cells[column]
      // 自己的结果优先，正在测的那一格也不覆盖。
      if (
        current &&
        (current.status.verdict !== 'pending' || current.status.testing)
      ) {
        return
      }
      const nearest = known.reduce((best, entry) =>
        Math.abs(entry.index - index) < Math.abs(best.index - index)
          ? entry
          : best
      )
      row.cells[column] = {
        status: nearest.cell.status,
        borrowedFrom: nearest.group,
      }
    })
  })
  return { columns, rows, total: models.length }
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
