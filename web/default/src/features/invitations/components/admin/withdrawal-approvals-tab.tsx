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
import { useQuery } from '@tanstack/react-query'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Label } from '@/components/ui/label'
import type { WithdrawalStatus } from '../../types'
import { getWithdrawalRequests } from '../../api'
import { WithdrawalRequestsTable } from './withdrawal-requests-table'

export function WithdrawalApprovalsTab() {
  const { t } = useTranslation()
  const [statusFilter, setStatusFilter] = useState<WithdrawalStatus | 'all'>('all')

  // 获取提现申请列表
  const { data: requestsData, isLoading } = useQuery({
    queryKey: ['adminWithdrawalRequests', statusFilter],
    queryFn: async () => {
      const params = statusFilter === 'all' ? {} : { status: statusFilter }
      const response = await getWithdrawalRequests(params)
      return response.data
    },
  })

  return (
    <Card>
      <CardHeader>
        <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
          <CardTitle>{t('Withdrawal Approvals')}</CardTitle>
          <div className="flex items-center gap-2">
            <Label htmlFor="status-filter" className="whitespace-nowrap">
              {t('Status')}:
            </Label>
            <Select
              value={statusFilter}
              onValueChange={(value) => setStatusFilter(value as WithdrawalStatus | 'all')}
            >
              <SelectTrigger id="status-filter" className="w-[180px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t('All')}</SelectItem>
                <SelectItem value="pending">{t('Pending')}</SelectItem>
                <SelectItem value="approved">{t('Approved')}</SelectItem>
                <SelectItem value="rejected">{t('Rejected')}</SelectItem>
                <SelectItem value="completed">{t('Completed')}</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>
      </CardHeader>
      <CardContent>
        <WithdrawalRequestsTable
          requests={requestsData?.items ?? []}
          loading={isLoading}
        />
      </CardContent>
    </Card>
  )
}
