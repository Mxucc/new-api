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
import { useTranslation } from 'react-i18next'

import { StaticDataTable } from '@/components/data-table'
import { EmptyState } from '@/components/empty-state'
import { StatusBadge, type StatusVariant } from '@/components/status-badge'
import { TruncatedText } from '@/components/truncated-text'
import { formatTimestampToDate } from '@/lib/format'

import { formatEventSummary } from '../lib/format'
import type { AiBridgeEvent, AiBridgeSnapshot } from '../types'

const EVENT_LEVEL_VARIANTS: Record<string, StatusVariant> = {
  info: 'info',
  warn: 'warning',
  error: 'danger',
}

export function LogsPanel(props: { snapshot: AiBridgeSnapshot }) {
  const { t } = useTranslation()
  const observability = props.snapshot.observability
  // The service already sends newest first; keep the guarantee on the client.
  const events = [...observability.events].sort(
    (left, right) => right.timestamp_ms - left.timestamp_ms
  )

  if (events.length === 0) {
    return (
      <EmptyState
        title={t('No events recorded yet')}
        description={t(
          'Requests, upstream retries, administrator changes and configuration reloads will appear here.'
        )}
      />
    )
  }

  return (
    <div className='space-y-3'>
      <p className='text-muted-foreground text-xs'>
        {t('Showing {{shown}} of the last {{capacity}} buffered events.', {
          shown: events.length,
          capacity: observability.event_capacity,
        })}
      </p>
      <StaticDataTable<AiBridgeEvent>
        columns={[
          {
            id: 'time',
            header: t('Time'),
            className: 'whitespace-nowrap',
            cell: (event) => (
              <span className='tabular-nums'>
                {formatTimestampToDate(event.timestamp_ms, 'milliseconds')}
              </span>
            ),
          },
          {
            id: 'level',
            header: t('Level'),
            cell: (event) => (
              <StatusBadge
                label={event.level}
                variant={EVENT_LEVEL_VARIANTS[event.level] ?? 'neutral'}
                copyable={false}
              />
            ),
          },
          {
            id: 'event',
            header: t('Event'),
            cell: (event) => (
              <span className='font-mono text-xs'>{event.event}</span>
            ),
          },
          {
            id: 'fields',
            header: t('Details'),
            cell: (event) => (
              <TruncatedText
                text={formatEventSummary(event)}
                maxWidth='max-w-[420px]'
                className='font-mono text-xs'
              />
            ),
          },
        ]}
        data={events}
        getRowKey={(event) => event.id}
        emptyContent={t('No events recorded yet')}
      />
    </div>
  )
}
