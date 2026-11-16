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

/** Verdict bands returned by the pelican probe. */
export type ModelIqVerdict =
  | 'healthy'
  | 'degraded'
  | 'poor'
  | 'failed'
  | 'pending'

/** The scored dimensions the probe looks for in the delivered drawing. */
export type ModelIqDimension =
  | 'drawing'
  | 'subject'
  | 'vehicle'
  | 'animation'
  | 'delivery'
  | 'delivery_partial'

/** 一次探测的目标：同一个模型在不同分组走不同渠道，分数要分开看。 */
export type PelicanTarget = {
  group: string
  model: string
}

export type ModelIqStatus = {
  /** 该分数来自哪个分组。 */
  group: string
  model_name: string
  /** False once every channel serving this group/model is gone. */
  available: boolean
  verdict: ModelIqVerdict
  score: number
  latency_ms: number
  tested_at: number
  result_id: number
  dimensions: string[]
  last_error?: string
}

export type ModelIqOverview = {
  models: ModelIqStatus[]
  total: number
  latest_tested_at: number
  prompt: string
}

export type PelicanResult = {
  id: number
  group: string
  model_name: string
  status: 'success' | 'failed'
  verdict: ModelIqVerdict
  score: number
  latency_ms: number
  dimensions: string[]
  output: string
  error: string
  created_at: number
}

/** 历史记录里的一个点：只有元数据，不含动画源码。 */
export type PelicanHistoryPoint = {
  id: number
  group: string
  status: 'success' | 'failed'
  verdict: ModelIqVerdict
  score: number
  latency_ms: number
  dimensions: string[]
  error: string
  created_at: number
}

export type PelicanHistory = {
  group: string
  model_name: string
  history: PelicanHistoryPoint[]
}

/** 后台巡视设置：要测的组合、间隔，以及当前可测与已失效的组合。 */
export type PelicanConfig = {
  targets: PelicanTarget[]
  interval_minutes: number
  available_targets: PelicanTarget[]
  missing_targets: PelicanTarget[]
}

export type PelicanRunSummary = {
  tested: number
  failed: number
}
