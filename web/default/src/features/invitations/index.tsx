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
import { useCallback } from 'react'
import { useNavigate, useSearch } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { useQuery } from '@tanstack/react-query'
import { Gift, History, Wallet } from 'lucide-react'
import { SectionPageLayout } from '@/components/layout'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { InvitationCodeCard } from './components/invitation-code-card'
import { RebateRecordsTable } from './components/rebate-records-table'
import { RebateTrendChart } from './components/rebate-trend-chart'
import { WithdrawalManagement } from './components/withdrawal-management'
import { getMyCode, getRebateRecords } from './api'

type TabValue = 'invite' | 'records' | 'withdrawal'

const DEFAULT_TAB: TabValue = 'invite'

export function Invitations() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const search = useSearch({ from: '/_authenticated/invitations' })

  // 从 URL query 获取当前 tab，默认为 'invite'
  const currentTab = (search.tab as TabValue) || DEFAULT_TAB

  // 获取邀请码和统计信息
  const { data: statsData, isLoading: statsLoading } = useQuery({
    queryKey: ['invitationStats'],
    queryFn: async () => {
      const response = await getMyCode()
      return response.data
    },
  })

  // 获取返利记录（用于图表）
  const { data: recordsData } = useQuery({
    queryKey: ['rebateRecordsForChart'],
    queryFn: async () => {
      const response = await getRebateRecords({ pageSize: 1000 })
      return response.data
    },
  })

  // Tab 切换处理
  const handleTabChange = useCallback(
    (value: string) => {
      navigate({
        to: '/invitations',
        search: { tab: value as TabValue },
      })
    },
    [navigate]
  )

  return (
    <SectionPageLayout>
      <SectionPageLayout.Title>{t('Invitation Rebate')}</SectionPageLayout.Title>
      <SectionPageLayout.Description>
        {t('Invite friends to earn rebates')}
      </SectionPageLayout.Description>
      <SectionPageLayout.Content>
        <div className="mx-auto w-full max-w-7xl">
          <Tabs value={currentTab} onValueChange={handleTabChange}>
            <TabsList className="mb-6">
              <TabsTrigger value="invite" className="gap-2">
                <Gift className="size-4" />
                {t('My Invitation')}
              </TabsTrigger>
              <TabsTrigger value="records" className="gap-2">
                <History className="size-4" />
                {t('Rebate Records')}
              </TabsTrigger>
              <TabsTrigger value="withdrawal" className="gap-2">
                <Wallet className="size-4" />
                {t('Withdrawal Management')}
              </TabsTrigger>
            </TabsList>

            <TabsContent value="invite">
              <InvitationCodeCard stats={statsData ?? null} loading={statsLoading} />
            </TabsContent>

            <TabsContent value="records">
              <div className="space-y-6">
                <RebateTrendChart records={recordsData?.items ?? []} days={30} />
                <RebateRecordsTable />
              </div>
            </TabsContent>

            <TabsContent value="withdrawal">
              <WithdrawalManagement />
            </TabsContent>
          </Tabs>
        </div>
      </SectionPageLayout.Content>
    </SectionPageLayout>
  )
}
