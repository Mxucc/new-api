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
import { useNavigate, useSearch } from '@tanstack/react-router'
import { useCallback, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { ErrorState } from '@/components/error-state'
import { SectionPageLayout } from '@/components/layout'
import { LoadingState } from '@/components/loading-state'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'

import { AccessListsPanel } from './components/access-lists-panel'
import { AccessNotice } from './components/access-notice'
import { ConfigPanel } from './components/config-panel'
import { CooldownsPanel } from './components/cooldowns-panel'
import { LogsPanel } from './components/logs-panel'
import { OverviewPanel } from './components/overview-panel'
import { AI_BRIDGE_TABS, resolveAiBridgeTab } from './constants'
import {
  useAiBridgeControl,
  useAiBridgeIdentity,
  useAiBridgeSnapshot,
} from './hooks/use-ai-bridge-console'
import { resolveAiBridgeAccessState } from './lib/errors'

export function AiBridgeConsole() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const search = useSearch({ strict: false }) as { tab?: string }
  const currentTab = resolveAiBridgeTab(search.tab)
  const identity = useAiBridgeIdentity()
  const snapshotQuery = useAiBridgeSnapshot(identity.isSuccess)
  const controlQuery = useAiBridgeControl(identity.isSuccess)
  const csrfToken = identity.data?.csrf_token ?? ''

  const handleTabChange = useCallback(
    (value: string) => {
      // The route file owns the route id; only the `tab` search value changes here.
      void navigate({
        search: (previous: Record<string, unknown>) => ({
          ...previous,
          tab: value,
        }),
        replace: true,
      } as never)
    },
    [navigate]
  )

  let body: ReactNode
  if (identity.isPending) {
    body = <LoadingState />
  } else if (identity.isError) {
    body = <AccessNotice state={resolveAiBridgeAccessState(identity.error)} />
  } else if (snapshotQuery.isError) {
    body = (
      <ErrorState
        onRetry={() => {
          void snapshotQuery.refetch()
          void controlQuery.refetch()
        }}
      />
    )
  } else if (!snapshotQuery.data) {
    body = <LoadingState />
  } else {
    const snapshot = snapshotQuery.data
    let panel: ReactNode
    if (currentTab === 'cooldowns') {
      panel = <CooldownsPanel snapshot={snapshot} csrfToken={csrfToken} />
    } else if (currentTab === 'access') {
      panel = <AccessListsPanel snapshot={snapshot} csrfToken={csrfToken} />
    } else if (currentTab === 'config') {
      panel = controlQuery.data ? (
        <ConfigPanel control={controlQuery.data} csrfToken={csrfToken} />
      ) : (
        <LoadingState />
      )
    } else if (currentTab === 'logs') {
      panel = <LogsPanel snapshot={snapshot} />
    } else {
      panel = <OverviewPanel snapshot={snapshot} control={controlQuery.data} />
    }

    body = (
      <Tabs value={currentTab} onValueChange={handleTabChange}>
        <TabsList className='mb-6 h-auto flex-wrap justify-start'>
          {AI_BRIDGE_TABS.map((tab) => {
            const Icon = tab.icon
            return (
              <TabsTrigger key={tab.value} value={tab.value} className='gap-2'>
                <Icon aria-hidden='true' className='size-4' />
                {t(tab.label)}
              </TabsTrigger>
            )
          })}
        </TabsList>
        <TabsContent value={currentTab}>{panel}</TabsContent>
      </Tabs>
    )
  }

  return (
    <SectionPageLayout>
      <SectionPageLayout.Title>
        {t('AI Bridge Console')}
      </SectionPageLayout.Title>
      <SectionPageLayout.Description>
        {t('Monitor and configure the AI Bridge gateway service.')}
      </SectionPageLayout.Description>
      <SectionPageLayout.Content>
        <div className='mx-auto w-full max-w-7xl'>{body}</div>
      </SectionPageLayout.Content>
    </SectionPageLayout>
  )
}
