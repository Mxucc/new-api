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
import { useQuery } from '@tanstack/react-query'
import {
  createColumnHelper,
  flexRender,
  getCoreRowModel,
  useReactTable,
  type PaginationState,
} from '@tanstack/react-table'
import { useTranslation } from 'react-i18next'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { getAdminRebateOrderRecords } from '../../api'
import type {
  AdminRebateOrderRecord,
  AdminRebateOrderStatus,
} from '../../types'

type OrderTypeFilter = 'all' | 'topup' | 'subscription'

const columnHelper = createColumnHelper<AdminRebateOrderRecord>()

const STATUS_LABELS: Record<AdminRebateOrderStatus, string> = {
  estimated: '预计返利',
  claimable: '可返利',
  paid: '已返利',
}

const STATUS_COLORS: Record<AdminRebateOrderStatus, string> = {
  estimated: 'bg-amber-500/10 text-amber-700 dark:text-amber-400',
  claimable: 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-400',
  paid: 'bg-slate-500/10 text-slate-700 dark:text-slate-400',
}

const ORDER_TYPE_LABELS: Record<'topup' | 'subscription', string> = {
  topup: '充值',
  subscription: '订阅',
}

function formatAmount(amount: number): string {
  return `¥${(amount / 100).toFixed(2)}`
}

function formatRate(rate?: number | null): string {
  if (rate == null) return '未配置'
  return `${(rate * 100).toFixed(2)}%`
}

function formatDateTime(value: string): string {
  if (!value) return '-'
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return '-'
  return date.toLocaleString()
}

export function RebateOrderRecordsTab() {
  const { t } = useTranslation()
  const [orderType, setOrderType] = useState<OrderTypeFilter>('all')
  const [pagination, setPagination] = useState<PaginationState>({
    pageIndex: 0,
    pageSize: 20,
  })

  const { data, isLoading } = useQuery({
    queryKey: [
      'adminRebateOrderRecords',
      orderType,
      pagination.pageIndex + 1,
      pagination.pageSize,
    ],
    queryFn: async () => {
      const response = await getAdminRebateOrderRecords({
        page: pagination.pageIndex + 1,
        pageSize: pagination.pageSize,
        ...(orderType !== 'all' && { orderType }),
      })
      return response.data
    },
  })

  const columns = useMemo(
    () => [
      columnHelper.accessor('inviterId', {
        header: () => t('Rebate User ID'),
        cell: (info) => <span className='font-mono'>{info.getValue()}</span>,
      }),
      columnHelper.accessor('userGroup', {
        header: () => t('User Group'),
        cell: (info) => <Badge variant='secondary'>{info.getValue()}</Badge>,
      }),
      columnHelper.accessor('orderType', {
        header: () => t('Order Type'),
        cell: (info) => ORDER_TYPE_LABELS[info.getValue()],
      }),
      columnHelper.accessor('orderAmount', {
        header: () => t('Order Amount'),
        cell: (info) => formatAmount(info.getValue()),
      }),
      columnHelper.accessor('rebateAmount', {
        header: () => t('Rebate Amount'),
        cell: (info) =>
          info.row.original.ruleMissing ? (
            <span className='text-muted-foreground'>未配置</span>
          ) : (
            <span className='font-medium'>{formatAmount(info.getValue())}</span>
          ),
      }),
      columnHelper.accessor('rebateRatio', {
        header: () => t('Rebate Rate'),
        cell: (info) => (
          <div className='flex items-center gap-2'>
            <span>{formatRate(info.getValue())}</span>
            {info.row.original.ruleMissing && (
              <Badge variant='outline' className='text-amber-700'>
                未配置规则
              </Badge>
            )}
          </div>
        ),
      }),
      columnHelper.accessor('status', {
        header: () => t('Status'),
        cell: (info) => {
          const status = info.getValue()
          return (
            <Badge variant='outline' className={STATUS_COLORS[status]}>
              {STATUS_LABELS[status]}
            </Badge>
          )
        },
      }),
      columnHelper.accessor('orderTime', {
        header: () => t('Order Time'),
        cell: (info) => (
          <span className='text-muted-foreground'>
            {formatDateTime(info.getValue())}
          </span>
        ),
      }),
      columnHelper.accessor('effectiveAt', {
        header: () => t('Effective At'),
        cell: (info) => (
          <span className='text-muted-foreground'>
            {formatDateTime(info.getValue())}
          </span>
        ),
      }),
    ],
    [t]
  )

  const table = useReactTable({
    data: data?.items ?? [],
    columns,
    pageCount: data ? Math.ceil(data.total / pagination.pageSize) : 0,
    state: { pagination },
    onPaginationChange: setPagination,
    getCoreRowModel: getCoreRowModel(),
    manualPagination: true,
  })

  const handleOrderTypeChange = (value: OrderTypeFilter | null) => {
    if (!value) return
    setOrderType(value)
    setPagination((current) => ({ ...current, pageIndex: 0 }))
  }

  return (
    <Card>
      <CardHeader>
        <div className='flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between'>
          <div>
            <CardTitle>{t('Rebate Records')}</CardTitle>
            <CardDescription>
              {t(
                'Invited customer recharge records are shown without customer identity'
              )}
            </CardDescription>
          </div>
          <Select value={orderType} onValueChange={handleOrderTypeChange}>
            <SelectTrigger className='w-[180px]'>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value='all'>{t('All Order Types')}</SelectItem>
              <SelectItem value='topup'>充值</SelectItem>
              <SelectItem value='subscription'>订阅</SelectItem>
            </SelectContent>
          </Select>
        </div>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <div className='flex items-center justify-center py-8'>
            <div className='text-muted-foreground'>{t('Loading...')}</div>
          </div>
        ) : (
          <>
            <div className='overflow-x-auto rounded-md border'>
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
                      <TableRow
                        key={row.original.orderType + row.original.orderId}
                      >
                        {row.getVisibleCells().map((cell) => (
                          <TableCell key={cell.id}>
                            {flexRender(
                              cell.column.columnDef.cell,
                              cell.getContext()
                            )}
                          </TableCell>
                        ))}
                      </TableRow>
                    ))
                  ) : (
                    <TableRow>
                      <TableCell
                        colSpan={columns.length}
                        className='h-24 text-center'
                      >
                        {t('No rebate records found')}
                      </TableCell>
                    </TableRow>
                  )}
                </TableBody>
              </Table>
            </div>

            {data && data.total > 0 && (
              <div className='flex flex-col gap-3 px-2 py-4 sm:flex-row sm:items-center sm:justify-between'>
                <div className='text-muted-foreground text-sm'>
                  {t('Showing {{from}} to {{to}} of {{total}} records', {
                    from: pagination.pageIndex * pagination.pageSize + 1,
                    to: Math.min(
                      (pagination.pageIndex + 1) * pagination.pageSize,
                      data.total
                    ),
                    total: data.total,
                  })}
                </div>
                <div className='flex items-center gap-2'>
                  <Button
                    variant='outline'
                    size='sm'
                    onClick={() => table.previousPage()}
                    disabled={!table.getCanPreviousPage()}
                  >
                    {t('Previous')}
                  </Button>
                  <div className='min-w-24 text-center text-sm'>
                    {t('Page {{current}} of {{total}}', {
                      current: pagination.pageIndex + 1,
                      total: table.getPageCount(),
                    })}
                  </div>
                  <Button
                    variant='outline'
                    size='sm'
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
  )
}
