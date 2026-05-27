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
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useForm } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { z } from 'zod'
import { useMutation } from '@tanstack/react-query'
import { Search } from 'lucide-react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
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
import type { RebateRecord } from '../../types'
import { getUserRebateDetails } from '../../api'

const formSchema = z.object({
  userId: z
    .string()
    .min(1, 'User ID is required')
    .refine((val) => !isNaN(Number(val)) && Number(val) > 0, {
      message: 'User ID must be a positive number',
    }),
})

type FormData = z.infer<typeof formSchema>

export function UserRebateDetails() {
  const { t } = useTranslation()
  const [records, setRecords] = useState<RebateRecord[]>([])

  const {
    register,
    handleSubmit,
    formState: { errors },
  } = useForm<FormData>({
    resolver: zodResolver(formSchema),
    defaultValues: { userId: '' },
  })

  // 查询用户返利详情
  const queryMutation = useMutation({
    mutationFn: (userId: number) => getUserRebateDetails(userId),
    onSuccess: (response) => {
      setRecords(response.data?.items ?? [])
    },
    onError: () => {
      setRecords([])
    },
  })

  const onSubmit = (data: FormData) => {
    const userId = Number(data.userId)
    queryMutation.mutate(userId)
  }

  const formatAmount = (amount: number) => {
    return `¥${(amount / 100).toFixed(2)}`
  }

  const formatDate = (dateString: string) => {
    return new Date(dateString).toLocaleString()
  }

  const getStatusBadge = (status: string) => {
    const variants: Record<string, 'default' | 'secondary' | 'destructive' | 'outline'> = {
      pending: 'default',
      requested: 'secondary',
      approved: 'outline',
      completed: 'outline',
    }
    return (
      <Badge variant={variants[status] || 'default'}>
        {t(status.charAt(0).toUpperCase() + status.slice(1))}
      </Badge>
    )
  }

  const formatOrderType = (type: string) => {
    const types: Record<string, string> = {
      topup: t('Top-up'),
      subscription: t('Subscription'),
      other: t('Other'),
    }
    return types[type] || type
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>{t('User Rebate Details')}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        <form onSubmit={handleSubmit(onSubmit)} className="flex gap-2">
          <div className="flex-1 space-y-2">
            <Label htmlFor="userId" className="sr-only">
              {t('User ID')}
            </Label>
            <Input
              id="userId"
              type="text"
              placeholder={t('Enter user ID')}
              {...register('userId')}
            />
            {errors.userId && (
              <p className="text-sm text-destructive">{errors.userId.message}</p>
            )}
          </div>
          <Button type="submit" disabled={queryMutation.isPending}>
            <Search className="mr-2 size-4" />
            {queryMutation.isPending ? t('Querying...') : t('Query')}
          </Button>
        </form>

        {queryMutation.isSuccess && (
          <div className="overflow-x-auto">
            {records.length === 0 ? (
              <div className="flex items-center justify-center py-8">
                <div className="text-muted-foreground">
                  {t('No rebate records found for this user')}
                </div>
              </div>
            ) : (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>{t('Order Type')}</TableHead>
                    <TableHead>{t('Order Amount')}</TableHead>
                    <TableHead>{t('Rebate Amount')}</TableHead>
                    <TableHead>{t('Rebate Rate')}</TableHead>
                    <TableHead>{t('Status')}</TableHead>
                    <TableHead>{t('Created At')}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {records.map((record) => (
                    <TableRow key={record.id}>
                      <TableCell>{formatOrderType(record.orderType)}</TableCell>
                      <TableCell>{formatAmount(record.orderAmount)}</TableCell>
                      <TableCell className="font-medium">
                        {formatAmount(record.rebateAmount)}
                      </TableCell>
                      <TableCell>
                        {(record.rebateRatio * 100).toFixed(2)}%
                      </TableCell>
                      <TableCell>{getStatusBadge(record.status)}</TableCell>
                      <TableCell className="text-muted-foreground">
                        {formatDate(record.createdAt)}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </div>
        )}

        {queryMutation.isError && (
          <div className="rounded-md bg-destructive/10 p-4 text-sm text-destructive">
            {t('Failed to query user rebate details. Please check the user ID and try again.')}
          </div>
        )}
      </CardContent>
    </Card>
  )
}
