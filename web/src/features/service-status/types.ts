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

/** 状态档位由真实请求的可用率推出，unknown 表示区间内没有流量。 */
export type ServiceStatus = 'operational' | 'degraded' | 'partial' | 'unknown'

export type ServiceStatusCell = {
  start: number
  requests: number
  success_rate: number
  status: ServiceStatus
}

export type ServiceStatusModel = {
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
