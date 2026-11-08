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
import { ShieldAlert, Timer } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { ConfirmDialog } from '@/components/confirm-dialog'
import { StaticDataTable } from '@/components/data-table'
import { StatusBadge } from '@/components/status-badge'
import { TruncatedText } from '@/components/truncated-text'
import { Button } from '@/components/ui/button'
import { TitledCard } from '@/components/ui/titled-card'

import {
  AI_BRIDGE_COOLDOWN_KIND_BY_SCOPE,
  AI_BRIDGE_COOLDOWN_REASON_LABEL_KEYS,
  AI_BRIDGE_COOLDOWN_SCOPE_LABEL_KEYS,
} from '../constants'
import { useClearAiBridgeCooldown } from '../hooks/use-ai-bridge-console'
import { formatDuration } from '../lib/format'
import type {
  AiBridgeAuthCooldown,
  AiBridgeCooldownKind,
  AiBridgeRetryCooldown,
  AiBridgeSnapshot,
} from '../types'
import { TruncatedNotice } from './truncated-notice'

interface PendingCooldown {
  kind: AiBridgeCooldownKind
  id: string
}

interface CooldownsPanelProps {
  snapshot: AiBridgeSnapshot
  csrfToken: string
}

export function CooldownsPanel(props: CooldownsPanelProps) {
  const { t } = useTranslation()
  const clearCooldown = useClearAiBridgeCooldown()
  const [pending, setPending] = useState<PendingCooldown | null>(null)
  const authCooldowns = props.snapshot.auth_cooldowns
  const retryCooldowns = props.snapshot.retry_cooldowns

  const handleConfirm = () => {
    if (!pending) return
    clearCooldown.mutate(
      { csrfToken: props.csrfToken, kind: pending.kind, id: pending.id },
      { onSettled: () => setPending(null) }
    )
  }

  return (
    <div className='space-y-6'>
      <TitledCard
        title={t('Authentication cooldowns')}
        description={t(
          'Upstream authentication failures temporarily block an API Key or client IP.'
        )}
        icon={<ShieldAlert />}
        iconTone='chart-3'
      >
        <div className='space-y-3'>
          {authCooldowns.truncated && (
            <TruncatedNotice
              shown={authCooldowns.cooldowns.length}
              total={authCooldowns.total}
            />
          )}
          <StaticDataTable
            columns={[
              {
                id: 'scope',
                header: t('Scope'),
                cell: (row: AiBridgeAuthCooldown) =>
                  t(
                    AI_BRIDGE_COOLDOWN_SCOPE_LABEL_KEYS[row.scope] ?? row.scope
                  ),
              },
              {
                id: 'identifier',
                header: t('Identifier'),
                cell: (row: AiBridgeAuthCooldown) => (
                  <TruncatedText text={row.identifier} />
                ),
              },
              {
                id: 'remaining',
                header: t('Remaining'),
                className: 'whitespace-nowrap',
                cell: (row: AiBridgeAuthCooldown) => (
                  <span className='tabular-nums'>
                    {formatDuration(row.remaining_secs)}
                  </span>
                ),
              },
              {
                id: 'status',
                header: t('Status'),
                cell: (row: AiBridgeAuthCooldown) => (
                  <StatusBadge
                    label={String(row.status)}
                    variant='warning'
                    copyable={false}
                  />
                ),
              },
              {
                id: 'reason',
                header: t('Reason'),
                cell: (row: AiBridgeAuthCooldown) =>
                  t(
                    AI_BRIDGE_COOLDOWN_REASON_LABEL_KEYS[row.reason] ??
                      row.reason
                  ),
              },
              {
                id: 'actions',
                header: t('Actions'),
                className: 'text-right',
                cellClassName: 'text-right',
                cell: (row: AiBridgeAuthCooldown) => {
                  const kind = AI_BRIDGE_COOLDOWN_KIND_BY_SCOPE[row.scope]
                  if (!kind) return null
                  return (
                    <Button
                      variant='outline'
                      size='sm'
                      onClick={() => setPending({ kind, id: row.id })}
                    >
                      {t('Clear')}
                    </Button>
                  )
                },
              },
            ]}
            data={authCooldowns.cooldowns}
            getRowKey={(row) => `${row.scope}:${row.id}`}
            emptyContent={t('No active authentication cooldowns')}
          />
        </div>
      </TitledCard>

      <TitledCard
        title={t('Retry cooldowns')}
        description={t(
          'Keys paused by upstream rate limits, capacity or gateway errors.'
        )}
        icon={<Timer />}
        iconTone='chart-2'
      >
        <div className='space-y-3'>
          {retryCooldowns.truncated && (
            <TruncatedNotice
              shown={retryCooldowns.entries.length}
              total={retryCooldowns.total}
            />
          )}
          <StaticDataTable
            columns={[
              {
                id: 'fingerprint',
                header: t('Key fingerprint'),
                cell: (row: AiBridgeRetryCooldown) => (
                  <TruncatedText text={row.fingerprint} className='font-mono' />
                ),
              },
              {
                id: 'remaining',
                header: t('Remaining'),
                className: 'whitespace-nowrap',
                cell: (row: AiBridgeRetryCooldown) => (
                  <span className='tabular-nums'>
                    {formatDuration(row.remaining_secs)}
                  </span>
                ),
              },
              {
                id: 'reason',
                header: t('Reason'),
                cell: (row: AiBridgeRetryCooldown) =>
                  t(
                    AI_BRIDGE_COOLDOWN_REASON_LABEL_KEYS[row.reason] ??
                      row.reason
                  ),
              },
              {
                id: 'actions',
                header: t('Actions'),
                className: 'text-right',
                cellClassName: 'text-right',
                cell: (row: AiBridgeRetryCooldown) => (
                  <Button
                    variant='outline'
                    size='sm'
                    onClick={() => setPending({ kind: 'retry', id: row.id })}
                  >
                    {t('Clear')}
                  </Button>
                ),
              },
            ]}
            data={retryCooldowns.entries}
            getRowKey={(row) => row.id}
            emptyContent={t('No active retry cooldowns')}
          />
        </div>
      </TitledCard>

      <ConfirmDialog
        open={pending != null}
        onOpenChange={(open) => {
          if (!open) setPending(null)
        }}
        title={t('Clear this cooldown?')}
        desc={t(
          'The blocked identity can be retried immediately after the cooldown is cleared.'
        )}
        confirmText={t('Clear')}
        destructive
        isLoading={clearCooldown.isPending}
        handleConfirm={handleConfirm}
      />
    </div>
  )
}
