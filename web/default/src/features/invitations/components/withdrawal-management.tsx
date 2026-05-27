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
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useForm } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { z } from 'zod'
import { toast } from 'sonner'
import { format } from 'date-fns'
import {
  createColumnHelper,
  flexRender,
  getCoreRowModel,
  useReactTable,
  type PaginationState,
} from '@tanstack/react-table'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Badge } from '@/components/ui/badge'
import {
  getAvailableRebates,
  requestWithdrawal,
  getMyRequests,
} from '../api'
import type { WithdrawalRequest, WithdrawalStatus } from '../types'

const columnHelper = createColumnHelper<WithdrawalRequest>()

// 提现表单验证 schema
const withdrawalSchema = z.object({
  amount: z.number().min(1, 'Amount must be greater than 0'),
})

type WithdrawalFormValues = z.infer<typeof withdrawalSchema>

// 状态徽章颜色映射
const STATUS_COLORS: Record<WithdrawalStatus, string> = {
  pending: 'bg-yellow-500/10 text-yellow-700 dark:text-yellow-400',
  approved: 'bg-green-500/10 text-green-700 dark:text-green-400',
  rejected: 'bg-red-500/10 text-red-700 dark:text-red-400',
  completed: 'bg-gray-500/10 text-gray-700 dark:text-gray-400',
}

export function WithdrawalManagement() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [pagination, setPagination] = useState<PaginationState>({
    pageIndex: 0,
    pageSize: 10,
  })

  // 获取可提现金额
  const { data: availableData } = useQuery({
    queryKey: ['availableRebates'],
    queryFn: async () => {
      const response = await getAvailableRebates()
      return response.data
    },
  })

  // 获取提现申请列表
  const { data: requestsData, isLoading } = useQuery({
    queryKey: ['withdrawalRequests', pagination.pageIndex + 1, pagination.pageSize],
    queryFn: async () => {
      const response = await getMyRequests({
        page: pagination.pageIndex + 1,
        pageSize: pagination.pageSize,
      })
      return response.data
    },
  })

  // 提现申请表单
  const {
    register,
    handleSubmit,
    formState: { errors },
    reset,
  } = useForm<WithdrawalFormValues>({
    resolver: zodResolver(withdrawalSchema),
    defaultValues: {
      amount: 0,
    },
  })

  // 提现申请 mutation
  const withdrawalMutation = useMutation({
    mutationFn: async (values: WithdrawalFormValues) => {
      if (!availableData?.recordIds || availableData.recordIds.length === 0) {
        throw new Error('No available rebates to withdraw')
      }
      return requestWithdrawal({
        amount: Math.round(values.amount * 100), // 转换为分
        rebateRecordIds: availableData.recordIds,
      })
    },
    onSuccess: () => {
      toast.success(t('Withdrawal request submitted successfully'))
      reset()
      queryClient.invalidateQueries({ queryKey: ['availableRebates'] })
      queryClient.invalidateQueries({ queryKey: ['withdrawalRequests'] })
    },
    onError: (error: Error) => {
      toast.error(error.message || t('Failed to submit withdrawal request'))
    },
  })

  // 提交提现申请
  const onSubmit = (values: WithdrawalFormValues) => {
    withdrawalMutation.mutate(values)
  }

  // 定义表格列
  const columns = useMemo(
    () => [
      columnHelper.accessor('amount', {
        header: () => t('Amount'),
        cell: (info) => `$${(info.getValue() / 100).toFixed(2)}`,
      }),
      columnHelper.accessor('status', {
        header: () => t('Status'),
        cell: (info) => {
          const status = info.getValue()
          const statusLabels: Record<WithdrawalStatus, string> = {
            pending: t('Pending'),
            approved: t('Approved'),
            rejected: t('Rejected'),
            completed: t('Completed'),
          }
          return (
            <Badge variant="outline" className={STATUS_COLORS[status]}>
              {statusLabels[status]}
            </Badge>
          )
        },
      }),
      columnHelper.accessor('createdAt', {
        header: () => t('Created At'),
        cell: (info) => format(new Date(info.getValue()), 'yyyy-MM-dd HH:mm'),
      }),
      columnHelper.accessor('approvedAt', {
        header: () => t('Approved At'),
        cell: (info) => {
          const value = info.getValue()
          return value ? format(new Date(value), 'yyyy-MM-dd HH:mm') : '-'
        },
      }),
      columnHelper.accessor('completedAt', {
        header: () => t('Completed At'),
        cell: (info) => {
          const value = info.getValue()
          return value ? format(new Date(value), 'yyyy-MM-dd HH:mm') : '-'
        },
      }),
      columnHelper.accessor('rejectReason', {
        header: () => t('Reject Reason'),
        cell: (info) => info.getValue() || '-',
      }),
    ],
    [t]
  )

  // 创建表格实例
  const table = useReactTable({
    data: requestsData?.items ?? [],
    columns,
    pageCount: requestsData ? Math.ceil(requestsData.total / pagination.pageSize) : 0,
    state: {
      pagination,
    },
    onPaginationChange: setPagination,
    getCoreRowModel: getCoreRowModel(),
    manualPagination: true,
  })

  const availableAmount = availableData?.amount ?? 0

  return (
    <div className="space-y-6">
      {/* 提现申请表单 */}
      <Card>
        <CardHeader>
          <CardTitle>{t('Request Withdrawal')}</CardTitle>
          <CardDescription>
            {t('Submit a withdrawal request for your completed rebates')}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
            <div className="space-y-2">
              <Label>{t('Available Amount')}</Label>
              <div className="text-2xl font-bold">
                ${(availableAmount / 100).toFixed(2)}
              </div>
            </div>

            <div className="space-y-2">
              <Label htmlFor="amount">{t('Withdrawal Amount')}</Label>
              <Input
                id="amount"
                type="number"
                step="0.01"
                placeholder="0.00"
                {...register('amount', { valueAsNumber: true })}
              />
              {errors.amount && (
                <p className="text-sm text-red-500">{errors.amount.message}</p>
              )}
            </div>

            <Button
              type="submit"
              disabled={
                withdrawalMutation.isPending ||
                availableAmount === 0 ||
                !availableData?.recordIds ||
                availableData.recordIds.length === 0
              }
            >
              {withdrawalMutation.isPending ? t('Submitting...') : t('Submit Request')}
            </Button>
          </form>
        </CardContent>
      </Card>

      {/* 提现申请列表 */}
      <Card>
        <CardHeader>
          <CardTitle>{t('Withdrawal Requests')}</CardTitle>
          <CardDescription>{t('View your withdrawal request history')}</CardDescription>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <div className="flex items-center justify-center py-8">
              <div className="text-muted-foreground">{t('Loading...')}</div>
            </div>
          ) : (
            <>
              <div className="rounded-md border">
                <Table>
                  <TableHeader>
                    {table.getHeaderGroups().map((headerGroup) => (
                      <TableRow key={headerGroup.id}>
                        {headerGroup.headers.map((header) => (
                          <TableHead key={header.id}>
                            {header.isPlaceholder
                              ? null
                              : flexRender(
                                  header.column.columnDef.header,
                                  header.getContext()
                                )}
                          </TableHead>
                        ))}
                      </TableRow>
                    ))}
                  </TableHeader>
                  <TableBody>
                    {table.getRowModel().rows.length > 0 ? (
                      table.getRowModel().rows.map((row) => (
                        <TableRow key={row.id}>
                          {row.getVisibleCells().map((cell) => (
                            <TableCell key={cell.id}>
                              {flexRender(cell.column.columnDef.cell, cell.getContext())}
                            </TableCell>
                          ))}
                        </TableRow>
                      ))
                    ) : (
                      <TableRow>
                        <TableCell colSpan={columns.length} className="h-24 text-center">
                          {t('No requests found')}
                        </TableCell>
                      </TableRow>
                    )}
                  </TableBody>
                </Table>
              </div>

              {/* 分页控制 */}
              {requestsData && requestsData.total > 0 && (
                <div className="flex items-center justify-between px-2 py-4">
                  <div className="text-sm text-muted-foreground">
                    {t('Showing {{from}} to {{to}} of {{total}} records', {
                      from: pagination.pageIndex * pagination.pageSize + 1,
                      to: Math.min(
                        (pagination.pageIndex + 1) * pagination.pageSize,
                        requestsData.total
                      ),
                      total: requestsData.total,
                    })}
                  </div>
                  <div className="flex items-center gap-2">
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => table.previousPage()}
                      disabled={!table.getCanPreviousPage()}
                    >
                      {t('Previous')}
                    </Button>
                    <div className="text-sm">
                      {t('Page {{current}} of {{total}}', {
                        current: pagination.pageIndex + 1,
                        total: table.getPageCount(),
                      })}
                    </div>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => table.nextPage()}
                      disabled={!table.getCanNextPage()}
                    >
                      {t('Next')}
                    </Button>
                  </div>
                </div>
              )}
            </>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
