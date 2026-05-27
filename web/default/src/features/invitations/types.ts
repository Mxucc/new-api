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

// 邀请统计
export interface InvitationStats {
  invitationCode: string
  invitedCount: number
  totalRebate: number
  completedRebate: number
  pendingWithdrawal: number
}

// 返利记录状态
export type RebateStatus = 'pending' | 'requested' | 'approved' | 'completed'

// 订单类型
export type OrderType = 'topup' | 'subscription' | 'other'

// 管理员返利记录展示状态
export type AdminRebateOrderStatus = 'estimated' | 'claimable' | 'paid'

// 返利记录
export interface RebateRecord {
  id: number
  inviterId: number
  inviteeId: number
  orderType: OrderType
  orderAmount: number
  rebateAmount: number
  rebateRatio: number
  status: RebateStatus
  createdAt: string
  updatedAt: string
}

// 提现申请状态
export type WithdrawalStatus = 'pending' | 'approved' | 'rejected' | 'completed'

// 提现申请
export interface WithdrawalRequest {
  id: number
  userId: number
  amount: number
  status: WithdrawalStatus
  rebateRecordIds: number[]
  createdAt: string
  updatedAt: string
  approvedAt?: string
  completedAt?: string
  rejectedAt?: string
  rejectReason?: string
}

// 提现申请详情（包含返利记录）
export interface WithdrawalRequestDetail extends WithdrawalRequest {
  rebateRecords: RebateRecord[]
}

// API 响应格式
export interface ApiResponse<T = unknown> {
  success: boolean
  message?: string
  data?: T
}

// 分页参数
export interface PaginationParams {
  page?: number
  pageSize?: number
}

// 分页响应
export interface PaginatedResponse<T> {
  items: T[]
  total: number
  page: number
  pageSize: number
}

// 提现申请表单数据
export interface WithdrawalFormData {
  amount: number
  rebateRecordIds: number[]
}

// ============================================================================
// 管理员类型定义
// ============================================================================

// 返利规则
export interface RebateRule {
  id: number
  user_group: string
  rule_type: 'subscription' | 'topup'
  rebate_rate: string
  created_at: number
  updated_at: number
}

// 用户组
export interface UserGroup {
  name: string
  user_count: number
}

// 系统配置
export interface SystemConfig {
  min_withdrawal_amount: number
  withdrawal_frequency_days: number
}

// 管理员提现申请（不显示具体用户信息）
export interface WithdrawalRequestAdmin extends WithdrawalRequestDetail {
  inviter_id: number
}

// 返利统计
export interface RebateStats {
  total_rebate: number
  completed_rebate: number
  pending_rebate: number
  requested_rebate: number
  approved_rebate: number
  total_invitations: number
}

// 管理员返利订单记录
export interface AdminRebateOrderRecord {
  orderType: 'topup' | 'subscription'
  orderId: number
  inviterId: number
  userGroup: string
  orderAmount: number
  rebateAmount: number
  rebateRatio?: number | null
  status: AdminRebateOrderStatus
  ruleMissing: boolean
  localRebateRecordId?: number | null
  localRebateStatus?: string | null
  orderTime: string
  effectiveAt: string
}

// 返利规则表单数据
export interface RebateRuleFormData {
  user_group: string
  rule_type: 'subscription' | 'topup'
  rebate_rate: string
}

// 提现审批操作数据
export interface WithdrawalApprovalData {
  note?: string
}

export interface WithdrawalRejectionData {
  reason: string
  note?: string
}
