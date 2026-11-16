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

/** 时间条上一小格：只代表它自己那 30 分钟，没采到样时 verdict 是 pending。 */
export type ModelIqStripCell = {
  start: number
  verdict: ModelIqVerdict
  score: number
  tested_at: number
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
  /** 这次结果来自哪条渠道：上游报 502 时据此定位。 */
  channel_id: number
  dimensions: string[]
  last_error?: string
  /** 该组合当前有一次探测在跑：格子显示加载态，页面保持轮询。 */
  testing: boolean
  /** 最近 24 小时切成的小格，一条结果也会铺满整条。 */
  strip: ModelIqStripCell[]
}

export type ModelIqOverview = {
  models: ModelIqStatus[]
  total: number
  latest_tested_at: number
  /** 管理员配置的题面（含交付约定）。 */
  prompt: string
  /** 管理员配置的页面说明；为空时前端用翻译好的默认说明。 */
  description: string
  /** 整轮巡视是否在跑。 */
  sweeping: boolean
  /** 巡视开始时间，0 表示没在跑。 */
  sweeping_since: number
}

export type PelicanResult = {
  id: number
  group: string
  model_name: string
  status: 'success' | 'failed'
  verdict: ModelIqVerdict
  score: number
  latency_ms: number
  channel_id: number
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
  channel_id: number
  dimensions: string[]
  error: string
  created_at: number
}

export type PelicanHistory = {
  group: string
  model_name: string
  history: PelicanHistoryPoint[]
}

/**
 * 后台巡视设置：探测白名单（每个分组一个模型）、间隔、题面与页面说明，
 * 以及探测用量的记账管理员。探测由系统自己跑，所有用户看同一份结果。
 */
export type PelicanConfig = {
  targets: PelicanTarget[]
  interval_minutes: number
  /** 用量记在这个管理员名下。 */
  owner_user_id: number
  owner_username: string
  /** 管理员填的题面与说明；空字符串表示用内置默认。 */
  prompt: string
  prompt_contract: string
  description: string
  /** 内置默认值，设置页用作输入框的占位提示。 */
  default_prompt: string
  default_contract: string
  default_description: string
  available_targets: PelicanTarget[]
  missing_targets: PelicanTarget[]
}

/** 保存巡视设置后的回执：记账管理员由服务端按会话填写。 */
export type PelicanConfigUpdate = {
  targets: PelicanTarget[]
  interval_minutes: number
  owner_user_id: number
  owner_username: string
  prompt: string
  prompt_contract: string
  description: string
}

/**
 * 异步触发的回执：探测在后台跑，页面只知道自己有没有真的启动一次。
 * `running` 为真表示已有一次同类探测在跑，这次没有重复触发。
 */
export type PelicanStartAck = {
  started: boolean
  running?: boolean
  /** 整轮巡视本轮要测的组合数，单组合触发时不返回。 */
  targets?: number
}
