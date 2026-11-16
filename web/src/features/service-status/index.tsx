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
import { HeartPulse, RefreshCw } from 'lucide-react'
import { useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { EmptyState } from '@/components/empty-state'
import { ErrorState } from '@/components/error-state'
import { SectionPageLayout } from '@/components/layout'
import { LoadingState } from '@/components/loading-state'
import { StatusBadge } from '@/components/status-badge'
import { Button } from '@/components/ui/button'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { formatUptimePct } from '@/features/performance-metrics/lib/format'
import { toIntlLocale } from '@/i18n/languages'
import { formatNumber, formatTimestampRelative } from '@/lib/format'

import { getServiceStatus } from './api'
import { ServiceStatusBar } from './components/service-status-bar'
import { ServiceStatusModels } from './components/service-status-models'
import {
  SERVICE_STATUS_WINDOWS,
  serviceStatusHeadlineKey,
  serviceStatusIntervalKey,
  serviceStatusLabelKey,
  serviceStatusVariant,
} from './lib/status'

/**
 * 服务状态页是被动式的：数字全部来自真实请求的聚合（perf_metrics），
 * 这个页面不会为了探测而额外调用任何模型，所以「没有流量」就是「没有数据」，
 * 不会被当成故障。
 */
export function ServiceStatusPage() {
  const { t, i18n } = useTranslation()
  const locale = toIntlLocale(i18n.resolvedLanguage || i18n.language)
  const [hours, setHours] = useState<number>(24)

  const query = useQuery({
    queryKey: ['service-status', hours],
    queryFn: () => getServiceStatus(hours),
  })
  const view = query.data

  let content: ReactNode
  if (query.isLoading) {
    content = <LoadingState />
  } else if (query.isError || !view) {
    content = (
      <ErrorState
        title={t('Failed to load the service status')}
        onRetry={() => void query.refetch()}
      />
    )
  } else if (view.models.length === 0) {
    content = (
      <EmptyState
        icon={HeartPulse}
        title={t('No traffic in this window yet.')}
        description={t(
          'The next real requests will start filling this timeline; this page never sends probe requests.'
        )}
      />
    )
  } else {
    const interval = serviceStatusIntervalKey(view.cell_seconds)
    content = (
      <div className='space-y-4'>
        <section className='bg-card rounded-xl border p-4 sm:p-6'>
          <div className='flex flex-wrap items-start justify-between gap-4'>
            <div className='min-w-0'>
              <div className='flex flex-wrap items-center gap-2'>
                <StatusBadge
                  label={t(serviceStatusLabelKey(view.status))}
                  variant={serviceStatusVariant(view.status)}
                  copyable={false}
                />
                <span
                  className='text-muted-foreground text-sm'
                  data-testid='service-status-headline'
                >
                  {t(serviceStatusHeadlineKey(view.status))}
                </span>
              </div>
              <p className='text-muted-foreground mt-2 text-xs'>
                {t('Data through')}{' '}
                {view.data_through
                  ? formatTimestampRelative(
                      view.data_through,
                      'seconds',
                      locale
                    )
                  : '—'}{' '}
                · {t(interval.key, { count: interval.count })}
              </p>
            </div>
            <div className='text-end'>
              <div
                className='text-3xl font-bold tracking-tight tabular-nums'
                data-testid='service-status-availability'
              >
                {formatUptimePct(view.success_rate)}
              </div>
              <div className='text-muted-foreground text-xs'>
                {t('Availability')} ·{' '}
                {t('{{count}} requests', {
                  count: formatNumber(view.requests),
                })}
              </div>
            </div>
          </div>

          <div className='mt-4'>
            <ServiceStatusBar
              cells={view.cells}
              label={t('Availability over time')}
            />
          </div>

          <p className='text-muted-foreground mt-2 text-xs'>
            {formatTimestampRelative(view.window_start, 'seconds', locale)} –{' '}
            {formatTimestampRelative(view.window_end, 'seconds', locale)}
          </p>
        </section>

        <section className='space-y-3'>
          <div className='flex items-center justify-between gap-2'>
            <h3 className='text-sm font-medium'>{t('Per group and model')}</h3>
            <span className='text-muted-foreground text-xs'>
              {t('{{count}} models', { count: view.models.length })}
            </span>
          </div>
          <ServiceStatusModels models={view.models} />
        </section>
      </div>
    )
  }

  return (
    <SectionPageLayout>
      <SectionPageLayout.Title>{t('Service Status')}</SectionPageLayout.Title>
      <SectionPageLayout.Actions>
        <ToggleGroup
          value={[String(hours)]}
          onValueChange={(value: string[]) => {
            const next = Number(value[0])
            if (Number.isFinite(next) && next > 0) {
              setHours(next)
            }
          }}
          variant='outline'
          spacing={0}
          aria-label={t('Window')}
        >
          {SERVICE_STATUS_WINDOWS.map((option) => (
            <ToggleGroupItem
              key={option.hours}
              value={String(option.hours)}
              className='px-2.5 text-xs'
            >
              {t(option.labelKey)}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
        <Button
          variant='ghost'
          size='sm'
          disabled={query.isFetching}
          onClick={() => void query.refetch()}
        >
          <RefreshCw
            data-icon='inline-start'
            className={query.isFetching ? 'animate-spin' : undefined}
          />
          {t('Refresh')}
        </Button>
      </SectionPageLayout.Actions>
      <SectionPageLayout.Content>
        <div className='space-y-4'>
          <p
            className='bg-muted/40 rounded-xl border px-3 py-2 text-xs leading-relaxed'
            data-testid='service-status-passive-note'
          >
            {t(
              'Status is derived from real traffic only. This page never sends probe requests, so a model without traffic shows no data instead of a failure.'
            )}
          </p>
          {content}
        </div>
      </SectionPageLayout.Content>
    </SectionPageLayout>
  )
}
