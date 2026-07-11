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
import {
  Activity,
  AlertTriangle,
  Boxes,
  ChevronRight,
  CircleHelp,
  Info,
  RefreshCw,
} from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  StaticDataTable,
  staticDataTableClassNames as tableStyles,
} from '@/components/data-table'
import { Button } from '@/components/design-system/button'
import {
  ToggleGroup,
  ToggleGroupItem,
} from '@/components/design-system/toggle-group'
import { StatusBadge, type StatusVariant } from '@/components/status-badge'
import {
  Alert,
  AlertAction,
  AlertDescription,
  AlertTitle,
} from '@/components/ui/alert'
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty'
import { Skeleton } from '@/components/ui/skeleton'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { getPerfMetricsSummary } from '@/features/performance-metrics/api'
import {
  formatLatency,
  formatThroughput,
  formatUptimePct,
  getSuccessRateDotClass,
  getSuccessRateLevel,
} from '@/features/performance-metrics/lib/format'
import type { PerfModelSummary } from '@/features/performance-metrics/types'
import { toIntlLocale } from '@/i18n/languages'
import { getLobeIcon } from '@/lib/lobe-icon'
import { cn } from '@/lib/utils'

import { filterBySearch } from '../lib/filters'
import type { PricingModel } from '../types'

type StatusScope = 'recent' | 'all'

type ModelStatusRow = {
  model: PricingModel
  perf?: PerfModelSummary
}

const STATUS_BAR_SLOTS = ['oldest', 'middle', 'latest'] as const
const SUMMARY_SKELETON_KEYS = ['models', 'recent', 'unknown'] as const
const ROW_SKELETON_KEYS = [
  'status-row-1',
  'status-row-2',
  'status-row-3',
  'status-row-4',
  'status-row-5',
  'status-row-6',
  'status-row-7',
  'status-row-8',
] as const

export interface ModelStatusViewProps {
  models: PricingModel[]
  searchQuery: string
  onClearSearch: () => void
  onModelClick: (modelName: string) => void
}

export function ModelStatusView(props: ModelStatusViewProps) {
  const { t, i18n } = useTranslation()
  const [scope, setScope] = useState<StatusScope>('recent')
  const metricsQuery = useQuery({
    queryKey: ['perf-metrics-summary', 24],
    queryFn: () => getPerfMetricsSummary(24),
    staleTime: 2 * 60 * 1000,
    retry: false,
    refetchOnWindowFocus: false,
  })

  const metrics = useMemo(
    () => metricsQuery.data?.data?.models ?? [],
    [metricsQuery.data]
  )
  const searchedModels = useMemo(
    () => filterBySearch(props.models, props.searchQuery.trim()),
    [props.models, props.searchQuery]
  )
  const catalogModelNames = useMemo(
    () => new Set(props.models.map((model) => model.model_name)),
    [props.models]
  )
  const activeModelNames = useMemo(
    () =>
      new Set(
        metrics
          .filter((perf) => catalogModelNames.has(perf.model_name))
          .map((perf) => perf.model_name)
      ),
    [catalogModelNames, metrics]
  )
  const rows = useMemo(() => {
    const searchedByName = new Map(
      searchedModels.map((model) => [model.model_name, model])
    )
    const withMetrics: ModelStatusRow[] = []
    const included = new Set<string>()

    for (const perf of metrics) {
      const model = searchedByName.get(perf.model_name)
      if (!model || included.has(perf.model_name)) continue
      included.add(perf.model_name)
      withMetrics.push({ model, perf })
    }

    if (scope === 'recent') return withMetrics

    const withoutMetrics = searchedModels
      .filter((model) => !included.has(model.model_name))
      .sort((a, b) => a.model_name.localeCompare(b.model_name))
      .map((model) => ({ model }))

    return [...withMetrics, ...withoutMetrics]
  }, [metrics, scope, searchedModels])

  const updatedAt = useMemo(() => {
    if (!metricsQuery.dataUpdatedAt) return null
    return new Intl.DateTimeFormat(
      toIntlLocale(i18n.resolvedLanguage || i18n.language),
      {
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
      }
    ).format(new Date(metricsQuery.dataUpdatedAt))
  }, [i18n.language, i18n.resolvedLanguage, metricsQuery.dataUpdatedAt])

  const handleScopeChange = (values: string[]) => {
    const next = values.find((value) => value !== scope)
    if (next === 'recent' || next === 'all') {
      setScope(next)
    }
  }

  const noDataCount = Math.max(props.models.length - activeModelNames.size, 0)

  let statusContent: React.ReactNode
  if (metricsQuery.isLoading) {
    statusContent = <StatusLoadingState />
  } else if (metricsQuery.isError) {
    statusContent = (
      <Alert variant='destructive'>
        <AlertTriangle />
        <AlertTitle>{t('Unable to load performance data')}</AlertTitle>
        <AlertDescription>{t('Please try again later.')}</AlertDescription>
        <AlertAction>
          <Button
            type='button'
            variant='outline'
            onClick={() => void metricsQuery.refetch()}
          >
            {t('Retry')}
          </Button>
        </AlertAction>
      </Alert>
    )
  } else {
    statusContent = (
      <>
        <div className='divide-y border-y sm:grid sm:grid-cols-3 sm:divide-x sm:divide-y-0'>
          <SummaryMetric
            icon={Boxes}
            label={t('Models')}
            value={props.models.length}
          />
          <SummaryMetric
            icon={Activity}
            label={t('Models with recent data')}
            value={activeModelNames.size}
          />
          <SummaryMetric
            icon={CircleHelp}
            label={t('No recent data')}
            value={noDataCount}
          />
        </div>

        {updatedAt && (
          <p className='text-muted-foreground text-right text-xs tabular-nums'>
            {t('Last updated:')} {updatedAt}
          </p>
        )}

        {rows.length === 0 ? (
          <StatusEmptyState
            hasSearch={Boolean(props.searchQuery.trim())}
            scope={scope}
            onClearSearch={props.onClearSearch}
            onShowAll={() => setScope('all')}
          />
        ) : (
          <StatusRows rows={rows} onModelClick={props.onModelClick} />
        )}
      </>
    )
  }

  return (
    <section className='space-y-4'>
      <div className='flex flex-col gap-4 border-b pb-4 sm:flex-row sm:items-end sm:justify-between'>
        <div className='min-w-0 space-y-1'>
          <h2 className='text-lg font-semibold'>
            {t('Model performance metrics')}
          </h2>
          <p className='text-muted-foreground text-sm'>
            {t('Performance metrics for the last 24 hours')}
          </p>
          <p className='text-muted-foreground/70 flex max-w-2xl items-start gap-1.5 text-xs leading-relaxed'>
            <Info className='mt-0.5 size-3.5 shrink-0' />
            <span>
              {t(
                'Based on site-wide real requests, not active availability probes.'
              )}
            </span>
          </p>
        </div>

        <div className='flex items-center gap-2 self-stretch sm:self-auto'>
          <ToggleGroup
            value={[scope]}
            onValueChange={handleScopeChange}
            aria-label={t('Models')}
            variant='outline'
            spacing={0}
            className='min-w-0 flex-1 sm:flex-none'
          >
            <ToggleGroupItem
              value='recent'
              className='min-w-0 flex-1 gap-1.5 px-2.5 sm:flex-none'
            >
              <Activity className='size-3.5' />
              <span className='truncate'>{t('Models with recent data')}</span>
            </ToggleGroupItem>
            <ToggleGroupItem
              value='all'
              className='min-w-0 flex-1 gap-1.5 px-2.5 sm:flex-none'
            >
              <Boxes className='size-3.5' />
              <span className='truncate'>{t('All Models')}</span>
            </ToggleGroupItem>
          </ToggleGroup>

          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  type='button'
                  variant='outline'
                  size='icon'
                  aria-label={t('Refresh')}
                  disabled={metricsQuery.isFetching}
                  onClick={() => void metricsQuery.refetch()}
                >
                  <RefreshCw
                    className={cn(
                      'size-4',
                      metricsQuery.isFetching && 'animate-spin'
                    )}
                  />
                </Button>
              }
            />
            <TooltipContent>{t('Refresh')}</TooltipContent>
          </Tooltip>
        </div>
      </div>

      {statusContent}
    </section>
  )
}

function SummaryMetric(props: {
  icon: React.ComponentType<{ className?: string }>
  label: string
  value: number
}) {
  const Icon = props.icon
  return (
    <div className='flex min-w-0 items-center gap-3 px-3 py-3 sm:px-4'>
      <Icon className='text-muted-foreground size-4 shrink-0' />
      <div className='min-w-0'>
        <div className='text-muted-foreground text-xs leading-snug'>
          {props.label}
        </div>
        <div className='text-lg font-semibold tabular-nums'>{props.value}</div>
      </div>
    </div>
  )
}

function StatusRows(props: {
  rows: ModelStatusRow[]
  onModelClick: (modelName: string) => void
}) {
  const { t } = useTranslation()

  return (
    <>
      <StaticDataTable
        className='hidden md:block'
        tableClassName='min-w-[720px] text-sm'
        headerRowClassName={tableStyles.compactHeaderRow}
        data={props.rows}
        getRowKey={(row) => row.model.id || row.model.model_name}
        columns={[
          {
            id: 'model',
            header: t('Model'),
            className: cn(tableStyles.compactHeaderCell, 'min-w-[240px]'),
            cellClassName: tableStyles.compactCell,
            cell: (row) => <ModelIdentity model={row.model} />,
          },
          {
            id: 'status',
            header: t('Status'),
            className: cn(tableStyles.compactHeaderCell, 'min-w-[190px]'),
            cellClassName: tableStyles.compactCell,
            cell: (row) => <ModelStatusCell perf={row.perf} />,
          },
          {
            id: 'latency',
            header: t('Average latency'),
            className: tableStyles.compactHeaderCellRight,
            cellClassName: tableStyles.compactMutedNumericCell,
            cell: (row) =>
              row.perf ? formatLatency(row.perf.avg_latency_ms) : '—',
          },
          {
            id: 'throughput',
            header: t('Throughput'),
            className: tableStyles.compactHeaderCellRight,
            cellClassName: tableStyles.compactMutedNumericCell,
            cell: (row) =>
              row.perf ? formatThroughput(row.perf.avg_tps) : '—',
          },
          {
            id: 'actions',
            header: <span className='sr-only'>{t('Details')}</span>,
            className: tableStyles.actionHeaderCell,
            cellClassName: tableStyles.actionCell,
            cell: (row) => (
              <Tooltip>
                <TooltipTrigger
                  render={
                    <Button
                      type='button'
                      variant='ghost'
                      size='icon'
                      aria-label={`${t('Details')}: ${row.model.model_name}`}
                      onClick={() => props.onModelClick(row.model.model_name)}
                    >
                      <ChevronRight className='size-4' />
                    </Button>
                  }
                />
                <TooltipContent>{t('Details')}</TooltipContent>
              </Tooltip>
            ),
          },
        ]}
      />

      <div className='overflow-hidden rounded-lg border md:hidden'>
        {props.rows.map((row) => (
          <div
            key={row.model.id || row.model.model_name}
            className='border-b p-4 last:border-b-0'
          >
            <div className='flex min-w-0 items-start justify-between gap-3'>
              <ModelIdentity model={row.model} />
              <ModelStatusBadge perf={row.perf} />
            </div>

            <div className='mt-3 grid grid-cols-2 gap-x-4 gap-y-3'>
              <MetricValue
                label={t('Average latency')}
                value={row.perf ? formatLatency(row.perf.avg_latency_ms) : '—'}
              />
              <MetricValue
                label={t('Throughput')}
                value={row.perf ? formatThroughput(row.perf.avg_tps) : '—'}
              />
            </div>

            <div className='mt-3 flex items-center justify-between gap-3 border-t pt-3'>
              <RecentSuccessBars perf={row.perf} />
              <Button
                type='button'
                variant='ghost'
                onClick={() => props.onModelClick(row.model.model_name)}
              >
                {t('Details')}
                <ChevronRight data-icon='inline-end' className='size-4' />
              </Button>
            </div>
          </div>
        ))}
      </div>
    </>
  )
}

function ModelIdentity(props: { model: PricingModel }) {
  const iconKey = props.model.icon || props.model.vendor_icon
  return (
    <div className='flex min-w-0 items-center gap-2.5'>
      <div className='bg-muted/40 flex size-8 shrink-0 items-center justify-center rounded-lg'>
        {getLobeIcon(iconKey, 18)}
      </div>
      <div className='min-w-0'>
        <div className='truncate font-mono text-sm font-medium'>
          {props.model.model_name}
        </div>
        {props.model.vendor_name && (
          <div className='text-muted-foreground truncate text-xs'>
            {props.model.vendor_name}
          </div>
        )}
      </div>
    </div>
  )
}

function ModelStatusCell(props: { perf?: PerfModelSummary }) {
  return (
    <div className='flex items-center gap-3'>
      <ModelStatusBadge perf={props.perf} />
      <RecentSuccessBars perf={props.perf} />
    </div>
  )
}

function ModelStatusBadge(props: { perf?: PerfModelSummary }) {
  const { t } = useTranslation()
  return (
    <StatusBadge variant={getStatusVariant(props.perf)}>
      {props.perf
        ? formatUptimePct(props.perf.success_rate)
        : t('No recent data')}
    </StatusBadge>
  )
}

function RecentSuccessBars(props: { perf?: PerfModelSummary }) {
  const { t } = useTranslation()
  const recentRates =
    props.perf?.recent_success_rates?.filter((rate) => Number.isFinite(rate)) ??
    []
  let statusRates: number[] = []
  if (props.perf) {
    statusRates =
      recentRates.length > 0 ? recentRates.slice(-3) : [props.perf.success_rate]
  }
  const paddedRates = [
    ...Array(Math.max(0, 3 - statusRates.length)).fill(null),
    ...statusRates,
  ].slice(-3) as Array<number | null>
  const bars = STATUS_BAR_SLOTS.map((slot, index) => ({
    slot,
    rate: paddedRates.at(index) ?? null,
  }))

  return (
    <div
      className='flex h-4 items-end gap-1'
      role='img'
      aria-label={
        props.perf
          ? `${t('Status')}: ${formatUptimePct(props.perf.success_rate)}`
          : t('No recent data')
      }
    >
      {bars.map(({ slot, rate }, index) => (
        <span
          key={slot}
          title={rate == null ? undefined : formatUptimePct(rate)}
          className={cn(
            'w-1.5 rounded-sm',
            index === 0 && 'h-2',
            index === 1 && 'h-3',
            index === 2 && 'h-4',
            rate == null
              ? 'bg-muted-foreground/15'
              : getSuccessRateDotClass(rate)
          )}
        />
      ))}
    </div>
  )
}

function MetricValue(props: { label: string; value: string }) {
  return (
    <div className='min-w-0'>
      <div className='text-muted-foreground text-xs'>{props.label}</div>
      <div className='mt-0.5 truncate font-mono text-sm tabular-nums'>
        {props.value}
      </div>
    </div>
  )
}

function getStatusVariant(perf?: PerfModelSummary): StatusVariant {
  if (!perf) return 'neutral'
  const level = getSuccessRateLevel(perf.success_rate)
  if (level === 'excellent' || level === 'good') return 'success'
  if (level === 'warning') return 'warning'
  if (level === 'critical') return 'destructive'
  return 'neutral'
}

function StatusEmptyState(props: {
  hasSearch: boolean
  scope: StatusScope
  onClearSearch: () => void
  onShowAll: () => void
}) {
  const { t } = useTranslation()
  let action: React.ReactNode = null
  if (props.hasSearch) {
    action = (
      <Button type='button' variant='outline' onClick={props.onClearSearch}>
        {t('Clear search')}
      </Button>
    )
  } else if (props.scope === 'recent') {
    action = (
      <Button type='button' variant='outline' onClick={props.onShowAll}>
        {t('All Models')}
      </Button>
    )
  }

  return (
    <Empty className='min-h-[260px] border'>
      <EmptyHeader>
        <EmptyMedia variant='icon'>
          {props.hasSearch ? <Boxes /> : <Activity />}
        </EmptyMedia>
        <EmptyTitle>
          {props.hasSearch
            ? t('No models found')
            : t('No performance data available')}
        </EmptyTitle>
        <EmptyDescription>
          {props.hasSearch
            ? t('Try adjusting your search')
            : t('Performance metrics for the last 24 hours')}
        </EmptyDescription>
      </EmptyHeader>
      <EmptyContent>{action}</EmptyContent>
    </Empty>
  )
}

function StatusLoadingState() {
  return (
    <div className='space-y-4'>
      <div className='grid grid-cols-1 divide-y border-y sm:grid-cols-3 sm:divide-x sm:divide-y-0'>
        {SUMMARY_SKELETON_KEYS.map((key) => (
          <div key={key} className='flex items-center gap-3 px-3 py-3 sm:px-4'>
            <Skeleton className='size-4 rounded-sm' />
            <div className='space-y-1.5'>
              <Skeleton className='h-3 w-28' />
              <Skeleton className='h-5 w-10' />
            </div>
          </div>
        ))}
      </div>
      <div className='overflow-hidden rounded-lg border'>
        {ROW_SKELETON_KEYS.map((key) => (
          <div
            key={key}
            className='flex items-center gap-4 border-b px-4 py-3 last:border-b-0'
          >
            <Skeleton className='size-8 shrink-0 rounded-lg' />
            <Skeleton className='h-4 min-w-0 flex-1' />
            <Skeleton className='hidden h-5 w-20 sm:block' />
            <Skeleton className='hidden h-4 w-16 md:block' />
            <Skeleton className='size-7 shrink-0' />
          </div>
        ))}
      </div>
    </div>
  )
}
