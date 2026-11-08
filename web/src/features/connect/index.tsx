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
import { useQuery } from '@tanstack/react-query'
import { useNavigate, useSearch } from '@tanstack/react-router'
import { LayoutGrid, ShieldCheck } from 'lucide-react'
import { useCallback, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { ErrorState } from '@/components/error-state'
import { SectionPageLayout } from '@/components/layout'
import { LoadingState } from '@/components/loading-state'
import { Badge } from '@/components/ui/badge'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'

import { CONNECT_ME_QUERY_KEY, getConnectMe } from './api'
import { ConnectAppsTab } from './components/connect-apps-tab'
import { ConnectAuthorizationsTab } from './components/connect-authorizations-tab'
import { ConnectUnavailableState } from './components/connect-unavailable-state'
import {
  getConnectErrorMessage,
  getConnectErrorStatus,
  isConnectServiceUnavailable,
} from './lib/error'
import { DEFAULT_CONNECT_TAB, isConnectTab } from './types'

export function Connect() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const search = useSearch({ strict: false }) as { tab?: unknown }

  const meQuery = useQuery({
    queryKey: CONNECT_ME_QUERY_KEY,
    queryFn: getConnectMe,
    // A deployment without the Connect service must not be probed repeatedly.
    retry: false,
    staleTime: 5 * 60_000,
    meta: { errorToast: false },
  })

  const activeTab = isConnectTab(search.tab) ? search.tab : DEFAULT_CONNECT_TAB

  const handleTabChange = useCallback(
    (value: string) => {
      if (!isConnectTab(value) || value === activeTab) return
      // The Connect route lives in the route file, so only its search is
      // updated and the current path is kept.
      void navigate({
        to: '.',
        search: (prev: Record<string, unknown>) => ({ ...prev, tab: value }),
        replace: true,
      } as never)
    },
    [activeTab, navigate]
  )

  const me = meQuery.data
  let body: ReactNode
  if (meQuery.isPending) {
    body = <LoadingState />
  } else if (meQuery.isError) {
    body = isConnectServiceUnavailable(meQuery.error) ? (
      <ConnectUnavailableState
        signedOut={getConnectErrorStatus(meQuery.error) === 401}
        onRetry={() => {
          void meQuery.refetch()
        }}
      />
    ) : (
      <ErrorState
        description={getConnectErrorMessage(
          meQuery.error,
          t('Failed to reach the Connect service')
        )}
        onRetry={() => {
          void meQuery.refetch()
        }}
      />
    )
  } else {
    body = (
      <Tabs value={activeTab} onValueChange={handleTabChange}>
        <TabsList className='mb-4'>
          <TabsTrigger value='apps' className='gap-2'>
            <LayoutGrid className='size-4' aria-hidden='true' />
            {t('My applications')}
          </TabsTrigger>
          <TabsTrigger value='authorizations' className='gap-2'>
            <ShieldCheck className='size-4' aria-hidden='true' />
            {t('My authorizations')}
          </TabsTrigger>
        </TabsList>

        <TabsContent value='apps'>
          <ConnectAppsTab showOwner={me?.is_admin === true} />
        </TabsContent>

        <TabsContent value='authorizations'>
          <ConnectAuthorizationsTab />
        </TabsContent>
      </Tabs>
    )
  }

  return (
    <SectionPageLayout>
      <SectionPageLayout.Title>{t('Connect')}</SectionPageLayout.Title>
      <SectionPageLayout.Description>
        {t('Manage OAuth2 applications and the access you have granted.')}
      </SectionPageLayout.Description>
      {me != null && (
        <SectionPageLayout.Actions>
          <span className='text-muted-foreground text-sm'>{me.username}</span>
          {me.is_admin && (
            <Badge variant='secondary'>{t('Administrator')}</Badge>
          )}
        </SectionPageLayout.Actions>
      )}
      <SectionPageLayout.Content>
        <div className='mx-auto w-full max-w-5xl'>{body}</div>
      </SectionPageLayout.Content>
    </SectionPageLayout>
  )
}
