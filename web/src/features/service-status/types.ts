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

/** 服务状态只有两档：可用与不可用（区间内没有流量按可用处理）。 */
export type ServiceStatus = 'available' | 'unavailable'

export type ServiceStatusCell = {
  start: number
  requests: number
  success_rate: number
  status: ServiceStatus
}

export type ServiceStatusModel = {
  /** 同一个模型在不同分组可能走不同渠道，状态按（分组, 模型）分开统计。 */
  group: string
  model_name: string
  requests: number
  success_rate: number
  avg_latency_ms: number
  status: ServiceStatus
  cells: ServiceStatusCell[]
}

export type ServiceStatusView = {
  generated_at: number
  window_hours: number
  window_start: number
  window_end: number
  cell_seconds: number
  data_through: number
  requests: number
  success_rate: number
  status: ServiceStatus
  cells: ServiceStatusCell[]
  models: ServiceStatusModel[]
}
