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
import { api } from '@/lib/api'
import { requireServerSuccess } from '@/lib/server-error-message'

import type {
  ModelIqOverview,
  PelicanConfig,
  PelicanConfigUpdate,
  PelicanHistory,
  PelicanResult,
  PelicanStartAck,
  PelicanTarget,
} from './types'

type Envelope<T> = {
  success: boolean
  message?: string
  data: T
}

export async function getModelIqOverview(): Promise<ModelIqOverview> {
  const res = await api.get<Envelope<ModelIqOverview>>('/api/pelican/overview')
  return requireServerSuccess(res.data).data
}

export async function getPelicanResult(id: number): Promise<PelicanResult> {
  const res = await api.get<Envelope<PelicanResult>>(
    `/api/pelican/results/${id}`
  )
  return requireServerSuccess(res.data).data
}

/** 历史记录按（分组, 模型）查询：两个参数都是必填。 */
export async function getPelicanHistory(
  group: string,
  model: string,
  limit?: number
): Promise<PelicanHistory> {
  const res = await api.get<Envelope<PelicanHistory>>('/api/pelican/history', {
    params: { group, model, limit },
  })
  return requireServerSuccess(res.data).data
}

/**
 * 触发一次探测。后端只做登记就返回，探测在后台跑满三分钟也不需要页面等着，
 * 结果靠轮询 overview 取回。
 */
export async function runPelicanTest(
  target: PelicanTarget
): Promise<PelicanStartAck> {
  const res = await api.post<Envelope<PelicanStartAck>>(
    '/api/pelican/test',
    target
  )
  return requireServerSuccess(res.data).data
}

/** 触发整轮巡视，同样只登记就返回。 */
export async function runAllPelicanTests(): Promise<PelicanStartAck> {
  const res = await api.post<Envelope<PelicanStartAck>>('/api/pelican/test-all')
  return requireServerSuccess(res.data).data
}

export async function getPelicanConfig(): Promise<PelicanConfig> {
  const res = await api.get<Envelope<PelicanConfig>>('/api/pelican/config')
  return requireServerSuccess(res.data).data
}

export async function updatePelicanConfig(payload: {
  targets: PelicanTarget[]
  interval_minutes: number
  prompt: string
  prompt_contract: string
  description: string
}): Promise<PelicanConfigUpdate> {
  const res = await api.put<Envelope<PelicanConfigUpdate>>(
    '/api/pelican/config',
    payload
  )
  return requireServerSuccess(res.data).data
}
