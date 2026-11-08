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
import {
  Activity,
  AlertTriangle,
  CheckCircle2,
  Clock,
  Gauge,
  KeyRound,
  RefreshCw,
  ServerCrash,
  UserCog,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { StatusBadge, type StatusVariant } from '@/components/status-badge'
import { TitledCard } from '@/components/ui/titled-card'
import { toIntlLocale } from '@/i18n/languages'
import { formatNumber, formatTimestampToDate } from '@/lib/format'

import {
  AI_BRIDGE_AUTH_POLICY_LABEL_KEYS,
  AI_BRIDGE_POLICY_LABEL_KEYS,
} from '../constants'
import { formatDuration } from '../lib/format'
import type { AiBridgeControl, AiBridgeSnapshot } from '../types'
import { AiBridgeStatCard } from './stat-card'

function policyState(enabled: boolean): {
  value: string
  variant: StatusVariant
} {
  return enabled
    ? { value: 'Enabled', variant: 'success' }
    : { value: 'Disabled', variant: 'neutral' }
}

interface PolicyRow {
  label: string
  value: string
  variant: StatusVariant
}

function buildPolicyRows(policies: AiBridgeControl['policies']): PolicyRow[] {
  const authPolicy =
    AI_BRIDGE_AUTH_POLICY_LABEL_KEYS[policies.auth_failure_policy]
  return [
    {
      label: AI_BRIDGE_POLICY_LABEL_KEYS.cyber_policy_ban_enabled,
      ...policyState(policies.cyber_policy_ban_enabled),
    },
    {
      label: AI_BRIDGE_POLICY_LABEL_KEYS.auth_failure_cooldown_enabled,
      ...policyState(policies.auth_failure_cooldown_enabled),
    },
    {
      label: AI_BRIDGE_POLICY_LABEL_KEYS.auth_failure_policy,
      value: authPolicy ?? policies.auth_failure_policy,
      variant: 'info',
    },
    {
      label: AI_BRIDGE_POLICY_LABEL_KEYS.upstream_error_cooldown_enabled,
      ...policyState(policies.upstream_error_cooldown_enabled),
    },
    {
      label: AI_BRIDGE_POLICY_LABEL_KEYS.upstream_502_cooldown_enabled,
      ...policyState(policies.upstream_502_cooldown_enabled),
    },
    {
      label: AI_BRIDGE_POLICY_LABEL_KEYS.stream_event_stall_retry_enabled,
      ...policyState(policies.stream_event_stall_retry_enabled),
    },
  ]
}

interface OverviewPanelProps {
  snapshot: AiBridgeSnapshot
  control?: AiBridgeControl
}

export function OverviewPanel(props: OverviewPanelProps) {
  const { t, i18n } = useTranslation()
  const locale = toIntlLocale(i18n.resolvedLanguage || i18n.language)
  const observability = props.snapshot.observability
  const cards = [
    {
      label: t('Uptime'),
      value: formatDuration(props.snapshot.uptime_secs),
      icon: Clock,
    },
    {
      label: t('Active concurrency'),
      value: formatNumber(props.snapshot.concurrency.active, locale),
      icon: Gauge,
    },
    {
      label: t('Tracked keys'),
      value: formatNumber(props.snapshot.concurrency.tracked_keys, locale),
      icon: KeyRound,
    },
    {
      label: t('Total requests'),
      value: formatNumber(observability.requests_total, locale),
      icon: Activity,
    },
    {
      label: t('Successful responses (2xx)'),
      value: formatNumber(observability.responses_2xx, locale),
      icon: CheckCircle2,
    },
    {
      label: t('Client error responses (4xx)'),
      value: formatNumber(observability.responses_4xx, locale),
      icon: AlertTriangle,
    },
    {
      label: t('Server error responses (5xx)'),
      value: formatNumber(observability.responses_5xx, locale),
      icon: ServerCrash,
    },
    {
      label: t('Upstream retries'),
      value: formatNumber(observability.upstream_retries, locale),
      icon: RefreshCw,
    },
    {
      label: t('Administrator changes'),
      value: formatNumber(observability.admin_mutations, locale),
      icon: UserCog,
    },
    {
      label: t('Configuration reloads succeeded'),
      value: formatNumber(observability.reload_successes, locale),
      icon: CheckCircle2,
    },
    {
      label: t('Configuration reloads failed'),
      value: formatNumber(observability.reload_failures, locale),
      icon: AlertTriangle,
    },
  ]
  const policyRows = props.control
    ? buildPolicyRows(props.control.policies)
    : []

  return (
    <div className='space-y-6'>
      <div className='grid gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4'>
        {cards.map((card) => (
          <AiBridgeStatCard
            key={card.label}
            label={card.label}
            value={card.value}
            icon={card.icon}
          />
        ))}
      </div>

      <TitledCard
        title={t('Policies')}
        description={t('Runtime switches reported by the AI Bridge service')}
      >
        {policyRows.length === 0 ? (
          <p className='text-muted-foreground text-sm'>{t('Loading...')}</p>
        ) : (
          <dl className='grid gap-3 sm:grid-cols-2'>
            {policyRows.map((row) => (
              <div
                key={row.label}
                className='flex items-center justify-between gap-3 rounded-lg border px-3 py-2'
              >
                <dt className='text-sm'>{t(row.label)}</dt>
                <dd>
                  <StatusBadge
                    label={t(row.value)}
                    variant={row.variant}
                    copyable={false}
                  />
                </dd>
              </div>
            ))}
          </dl>
        )}
      </TitledCard>

      <p className='text-muted-foreground text-xs'>
        {t('Snapshot generated at')}{' '}
        <span className='tabular-nums'>
          {formatTimestampToDate(
            props.snapshot.generated_at_ms,
            'milliseconds'
          )}
        </span>
      </p>
    </div>
  )
}
