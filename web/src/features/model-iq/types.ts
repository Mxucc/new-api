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

/**
 * new-api 的一个分组到 magpie 一个路由组的映射。两边之间还隔着 sub2api，
 * provider 名对不上，所以只映射到组：动哪个成员由 magpie-ops 现场判定。
 */
export type PelicanMagpieGroup = {
  group: string
  /** magpie 的分组 id，例如 never-stuck。 */
  magpie_group: string
}

/** magpie-ops 动作记录里我们关心的一小部分：动了谁、做了什么、当时智商。 */
export type PelicanMagpieActionResult = {
  ref: string
  kind: string
  /** 旧记录没有这个字段，所以可缺省；0 分是合法值。 */
  score?: number
}

/** magpie-ops 分组里的一个成员，设置页只用来展示分组里有什么。 */
export type PelicanMagpieMember = {
  ref: string
  provider?: string
  providerName?: string
  verdict?: string
  score?: number
  disabled?: boolean
  /** 本工具关的（会自己恢复）还是管理员手工关的。 */
  disabledByUs?: boolean
  lastProbedAt?: string
  /** magpie-ops 里「不列入自动化」：不排序、不处置。 */
  excluded?: boolean
  /** 排除时是否仍然测智（只测不处置）。 */
  probe?: boolean
}

/** magpie-ops GET /api/groups 里的一个分组：映射下拉的选项来自它。 */
export type PelicanMagpieGroupOption = {
  id: string
  name: string
  routing?: string
  /** magpie-ops 自己的巡检开关：两边都跑就是重复探测。 */
  policyEnabled?: boolean
  intervalMinutes?: number
  members: PelicanMagpieMember[]
  /** magpie-ops 真正会测的成员：一个渠道只测一个模型，比 members 少。 */
  probeRefs?: string[]
}
/** 时间条上一小格：一格对应一次巡视间隔。没采到样时判定沿用上一次成功的结果。 */
export type ModelIqStripCell = {
  start: number
  verdict: ModelIqVerdict
  score: number
  tested_at: number
  /** 这一格真的跑过一次并拿到了输出；false 表示判定沿用上一次成功的结果。 */
  sampled: boolean
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
  /** 这条结果的来源：空表示本机测的，`magpie-ops` 表示直接取了它的结果（没有重复花钱）。 */
  source?: string
  /** 它真正被测出来的时刻（复用 magpie-ops 时比 tested_at 早）。 */
  observed_at?: number
  /** 这个分组映射到的 magpie 分组；没配联动时为空。 */
  magpie_group?: string
  /** 上一次请 magpie 调整这个分组的时间（Unix 秒）。 */
  magpie_adjusted_at?: number
  /** 上一次调整的动作明细：动了谁、做了什么、当时智商。 */
  magpie_actions?: PelicanMagpieActionResult[]
  /** 没有结构化动作时的状态文本：试运行 / 无需调整 / 读不到动作记录。 */
  magpie_result?: string
  /** 上一次调整失败的原因：magpie 停机或 token 不对时显示它。 */
  magpie_error?: string
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
  /** 空表示本机测的；`magpie-ops` 表示这条结论直接取了 magpie-ops 的结果。 */
  source?: string
  /** 复用 magpie-ops 时它真正被测出来的时刻。 */
  observed_at?: number
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
  /** magpie 联动的地址；空表示不联动。 */
  magpie_url: string
  /** 服务端只告诉我们「有没有保存 token」，绝不回显明文。 */
  magpie_token_set: boolean
  /** 开了自动调整的分组及其 magpie 分组。 */
  magpie_groups: PelicanMagpieGroup[]
  /** 同一分组两次调整之间的最小间隔（分钟）。 */
  magpie_cooldown_minutes: number
  /** 只记录本来会调谁，不真的改 magpie。 */
  magpie_dry_run: boolean
  /**
   * 复用 magpie-ops 结果的窗口（分钟）：联动开着时它刚测过的分组不再自己测。
   * null 表示跟随默认（联动开着就复用 60 分钟），0 表示关掉复用。
   */
  magpie_reuse_minutes?: number | null
  /** magpie-ops 那边的分组清单，拿不到时是空数组。 */
  /** 绑定了 magpie 分组：模型以 magpie-ops 为准，设置页不再选模型。 */
  magpie_read_only?: boolean
  /** 每个映射分组最近一次拿到的「24 小时内最佳结果」。 */
  magpie_best?: Record<string, PelicanMagpieBest>
  /** 取最佳结果的时间，取不到时是错误信息。 */
  magpie_best_at?: string
  magpie_groups_available: PelicanMagpieGroupOption[]
  /** 列不出分组时的原因：magpie 停机或 token 不对。 */
  magpie_groups_error: string
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
  magpie_url: string
  /** 留空表示沿用已保存的 token；要清除必须显式带 magpie_token_clear。 */
  magpie_token: string
  magpie_token_clear: boolean
  magpie_groups: PelicanMagpieGroup[]
  magpie_cooldown_minutes: number
  magpie_dry_run: boolean
  /** 复用窗口；null 表示跟随默认，0 表示关掉。 */
  magpie_reuse_minutes?: number | null
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

/** magpie-ops 给的一个分组的「最佳结果」：24 小时内最高分（降智也算）。 */
export type PelicanMagpieBest = {
  group: string
  hours: number
  considered: number
  best?: {
    ref: string
    model?: string
    score: number
    verdict?: string
    at?: string
    rate?: number
  }
}
