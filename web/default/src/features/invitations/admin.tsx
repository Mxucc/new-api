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
import { Settings, ReceiptText, CheckCircle, BarChart3 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { SectionPageLayout } from '@/components/layout'
import { RebateOrderRecordsTab } from './components/admin/rebate-order-records-tab'
import { RebateRulesTab } from './components/admin/rebate-rules-tab'
import { StatisticsTab } from './components/admin/statistics-tab'
import { WithdrawalApprovalsTab } from './components/admin/withdrawal-approvals-tab'

type TabValue = 'rules' | 'records' | 'approvals' | 'statistics'

const DEFAULT_TAB: TabValue = 'rules'

export function InvitationsAdmin() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const search = useSearch({ strict: false }) as { tab?: TabValue }

  // 从 URL query 获取当前 tab
  const currentTab = search.tab || DEFAULT_TAB

  // Tab 切换处理
  const handleTabChange = useCallback(
    (value: string) => {
      navigate({
        to: '/invitations/admin',
        search: { tab: value as TabValue },
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
      } as any)
    },
    [navigate]
  )

  return (
    <SectionPageLayout>
      <SectionPageLayout.Title>
        {t('Rebate Management')}
      </SectionPageLayout.Title>
      <SectionPageLayout.Description>
        {t('Manage rebate rules, withdrawal approvals, and statistics')}
      </SectionPageLayout.Description>
      <SectionPageLayout.Content>
        <div className='mx-auto w-full max-w-7xl'>
          <Tabs value={currentTab} onValueChange={handleTabChange}>
            <TabsList className='mb-6'>
              <TabsTrigger value='rules' className='gap-2'>
                <Settings className='size-4' />
                {t('Rebate Rules')}
              </TabsTrigger>
              <TabsTrigger value='records' className='gap-2'>
                <ReceiptText className='size-4' />
                {t('Rebate Records')}
              </TabsTrigger>
              <TabsTrigger value='approvals' className='gap-2'>
                <CheckCircle className='size-4' />
                {t('Withdrawal Approvals')}
              </TabsTrigger>
              <TabsTrigger value='statistics' className='gap-2'>
                <BarChart3 className='size-4' />
                {t('Statistics')}
              </TabsTrigger>
            </TabsList>

            <TabsContent value='rules'>
              <RebateRulesTab />
            </TabsContent>

            <TabsContent value='records'>
              <RebateOrderRecordsTab />
            </TabsContent>

            <TabsContent value='approvals'>
              <WithdrawalApprovalsTab />
            </TabsContent>

            <TabsContent value='statistics'>
              <StatisticsTab />
            </TabsContent>
          </Tabs>
        </div>
      </SectionPageLayout.Content>
    </SectionPageLayout>
  )
}
