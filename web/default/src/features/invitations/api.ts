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
import type {
  ApiResponse,
  InvitationStats,
  RebateRecord,
  WithdrawalRequest,
  WithdrawalFormData,
  PaginationParams,
  PaginatedResponse,
  RebateStatus,
} from './types'

const BASE_PATH = '/invitations/api/invitations'

// ============================================================================
// 用户 API
// ============================================================================

/**
 * 获取我的邀请码和统计信息
 */
export async function getMyCode(): Promise<ApiResponse<InvitationStats>> {
  const res = await api.get(`${BASE_PATH}/my-code`)
  return res.data
}

/**
 * 获取返利记录列表
 */
export async function getRebateRecords(
  params?: PaginationParams & { status?: RebateStatus }
): Promise<ApiResponse<PaginatedResponse<RebateRecord>>> {
  const res = await api.get(`${BASE_PATH}/rebate-records`, { params })
  return res.data
}

/**
 * 获取可提现金额（已完成但未申请提现的返利总额）
 */
export async function getAvailableRebates(): Promise<
  ApiResponse<{ amount: number; recordIds: number[] }>
> {
  const res = await api.get(`${BASE_PATH}/available-rebates`)
  return res.data
}

/**
 * 申请提现
 */
export async function requestWithdrawal(
  data: WithdrawalFormData
): Promise<ApiResponse<WithdrawalRequest>> {
  const res = await api.post(`${BASE_PATH}/request-withdrawal`, data)
  return res.data
}

/**
 * 获取我的提现申请列表
 */
export async function getMyRequests(
  params?: PaginationParams
): Promise<ApiResponse<PaginatedResponse<WithdrawalRequest>>> {
  const res = await api.get(`${BASE_PATH}/my-requests`, { params })
  return res.data
}

// ============================================================================
// 管理员 API
// ============================================================================

const ADMIN_BASE_PATH = '/invitations/api/admin'

/**
 * 获取返利规则列表
 */
export async function getRebateRules(): Promise<
  ApiResponse<import('./types').RebateRule[]>
> {
  const res = await api.get(`${ADMIN_BASE_PATH}/rebate-rules`)
  return res.data
}

/**
 * 创建返利规则
 */
export async function createRebateRule(
  data: import('./types').RebateRuleFormData
): Promise<ApiResponse<import('./types').RebateRule>> {
  const res = await api.post(`${ADMIN_BASE_PATH}/rebate-rules`, data)
  return res.data
}

/**
 * 更新返利规则
 */
export async function updateRebateRule(
  id: number,
  data: import('./types').RebateRuleFormData
): Promise<ApiResponse<import('./types').RebateRule>> {
  const res = await api.put(`${ADMIN_BASE_PATH}/rebate-rules/${id}`, data)
  return res.data
}

/**
 * 删除返利规则
 */
export async function deleteRebateRule(id: number): Promise<ApiResponse<void>> {
  const res = await api.delete(`${ADMIN_BASE_PATH}/rebate-rules/${id}`)
  return res.data
}

/**
 * 获取用户组列表
 */
export async function getUserGroups(): Promise<
  ApiResponse<import('./types').UserGroup[]>
> {
  const res = await api.get(`${ADMIN_BASE_PATH}/user-groups`)
  return res.data
}

/**
 * 获取系统配置
 */
export async function getSystemConfig(): Promise<
  ApiResponse<import('./types').SystemConfig>
> {
  const res = await api.get(`${ADMIN_BASE_PATH}/system-config`)
  return res.data
}

/**
 * 更新系统配置
 */
export async function updateSystemConfig(
  data: import('./types').SystemConfig
): Promise<ApiResponse<import('./types').SystemConfig>> {
  const res = await api.put(`${ADMIN_BASE_PATH}/system-config`, data)
  return res.data
}

/**
 * 获取提现申请列表（管理员）
 */
export async function getWithdrawalRequests(
  params?: PaginationParams & { status?: import('./types').WithdrawalStatus }
): Promise<
  ApiResponse<PaginatedResponse<import('./types').WithdrawalRequestAdmin>>
> {
  const res = await api.get(`${ADMIN_BASE_PATH}/withdrawal-requests`, { params })
  return res.data
}

/**
 * 通过提现申请
 */
export async function approveWithdrawal(
  id: number,
  data?: import('./types').WithdrawalApprovalData
): Promise<ApiResponse<import('./types').WithdrawalRequestAdmin>> {
  const res = await api.post(
    `${ADMIN_BASE_PATH}/withdrawal-requests/${id}/approve`,
    data
  )
  return res.data
}

/**
 * 拒绝提现申请
 */
export async function rejectWithdrawal(
  id: number,
  data: import('./types').WithdrawalRejectionData
): Promise<ApiResponse<import('./types').WithdrawalRequestAdmin>> {
  const res = await api.post(
    `${ADMIN_BASE_PATH}/withdrawal-requests/${id}/reject`,
    data
  )
  return res.data
}

/**
 * 标记提现完成
 */
export async function completeWithdrawal(
  id: number
): Promise<ApiResponse<import('./types').WithdrawalRequestAdmin>> {
  const res = await api.post(
    `${ADMIN_BASE_PATH}/withdrawal-requests/${id}/complete`
  )
  return res.data
}

/**
 * 获取返利统计
 */
export async function getRebateStats(): Promise<
  ApiResponse<import('./types').RebateStats>
> {
  const res = await api.get(`${ADMIN_BASE_PATH}/rebate-stats`)
  return res.data
}

/**
 * 获取用户返利详情
 */
export async function getUserRebateDetails(
  userId: number
): Promise<ApiResponse<PaginatedResponse<RebateRecord>>> {
  const res = await api.get(`${ADMIN_BASE_PATH}/users/${userId}/rebate-details`)
  return res.data
}
