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

export type ModelIqStatus = {
  model_name: string
  /** False once every channel serving the model is gone. */
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

export type PelicanRunSummary = {
  tested: number
  failed: number
}
