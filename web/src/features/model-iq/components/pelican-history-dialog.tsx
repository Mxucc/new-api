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
import { Activity } from 'lucide-react'
import { useMemo, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { CartesianGrid, Line, LineChart, XAxis, YAxis } from 'recharts'

import { Dialog } from '@/components/dialog'
import { EmptyState } from '@/components/empty-state'
import { ErrorState } from '@/components/error-state'
import { LoadingState } from '@/components/loading-state'
import { StatusBadge } from '@/components/status-badge'
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from '@/components/ui/chart'
import { formatLatency } from '@/features/performance-metrics/lib/format'
import { toIntlLocale } from '@/i18n/languages'
import { formatTimestamp } from '@/lib/format'

import { getPelicanHistory } from '../api'
import {
  iqVerdictLabelKey,
  iqVerdictVariant,
  pelicanTargetLabel,
} from '../lib/iq'
import type { PelicanTarget } from '../types'

/** 图下方的近期记录只列最近几条，趋势本身交给折线。 */
const RECENT_RUNS = 5

export interface PelicanHistoryDialogProps {
  target: PelicanTarget | null
  onOpenChange: (open: boolean) => void
}

/** 一个（分组, 模型）组合的历史趋势：智商与延迟随时间的折线。 */
export function PelicanHistoryDialog(props: PelicanHistoryDialogProps) {
  const { t, i18n } = useTranslation()
  const locale = toIntlLocale(i18n.resolvedLanguage || i18n.language)
  const group = props.target?.group ?? ''
  const modelName = props.target?.model ?? ''

  const query = useQuery({
    queryKey: ['model-iq', 'history', group, modelName],
    queryFn: () => getPelicanHistory(group, modelName),
    enabled: props.target !== null,
  })

  const chartConfig = useMemo<ChartConfig>(
    () => ({
      score: { label: t('IQ score'), color: 'var(--chart-1)' },
      latency: { label: t('Latency'), color: 'var(--chart-3)' },
    }),
    [t]
  )

  const points = useMemo(() => {
    const formatter = new Intl.DateTimeFormat(locale, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    })
    return (query.data?.history ?? []).map((point) => ({
      id: point.id,
      label: formatter.format(new Date(point.created_at * 1000)),
      score: point.score,
      latency: point.latency_ms,
    }))
  }, [query.data, locale])

  const recentRuns = useMemo(
    () => (query.data?.history ?? []).slice(-RECENT_RUNS).reverse(),
    [query.data]
  )

  // 加载、失败、无历史与有数据四种状态按顺序判定，避免嵌套三元。
  let body: ReactNode
  if (query.isLoading) {
    body = <LoadingState />
  } else if (query.isError) {
    body = (
      <ErrorState
        title={t('Failed to load the test history')}
        onRetry={() => void query.refetch()}
      />
    )
  } else if (points.length === 0) {
    body = (
      <EmptyState
        icon={Activity}
        title={t('No history yet')}
        description={t('Run the pelican test to start a trend line.')}
      />
    )
  } else {
    body = (
      <div className='space-y-4'>
        <ChartContainer
          config={chartConfig}
          className='aspect-auto h-[240px] w-full'
        >
          <LineChart data={points} accessibilityLayer>
            <CartesianGrid strokeDasharray='3 3' />
            <XAxis dataKey='label' tick={{ fontSize: 11 }} tickMargin={8} />
            <YAxis
              yAxisId='score'
              domain={[0, 100]}
              tick={{ fontSize: 11 }}
              width={32}
            />
            <YAxis
              yAxisId='latency'
              orientation='right'
              tick={{ fontSize: 11 }}
              width={48}
              tickFormatter={(value: number) => formatLatency(value)}
            />
            <ChartTooltip content={<ChartTooltipContent labelKey='label' />} />
            <Line
              yAxisId='score'
              type='monotone'
              dataKey='score'
              stroke='var(--color-score)'
              strokeWidth={2}
              dot={{ r: 2 }}
            />
            <Line
              yAxisId='latency'
              type='monotone'
              dataKey='latency'
              stroke='var(--color-latency)'
              strokeWidth={2}
              strokeDasharray='4 2'
              dot={false}
            />
          </LineChart>
        </ChartContainer>

        <ul className='divide-y' data-testid='pelican-history-recent'>
          {recentRuns.map((run) => (
            <li
              key={run.id}
              className='flex flex-wrap items-center justify-between gap-2 py-1.5 text-xs'
            >
              <span className='text-muted-foreground'>
                {formatTimestamp(run.created_at)}
              </span>
              <span className='flex items-center gap-3'>
                <StatusBadge
                  label={t(iqVerdictLabelKey(run.verdict))}
                  variant={iqVerdictVariant(run.verdict)}
                  copyable={false}
                />
                <span className='tabular-nums'>
                  {t('IQ score')} {run.score}
                </span>
                <span className='text-muted-foreground tabular-nums'>
                  {formatLatency(run.latency_ms)}
                </span>
              </span>
            </li>
          ))}
        </ul>
      </div>
    )
  }

  return (
    <Dialog
      open={props.target !== null}
      onOpenChange={props.onOpenChange}
      title={t('Test history')}
      description={props.target ? pelicanTargetLabel(props.target) : undefined}
      contentClassName='sm:max-w-3xl'
    >
      {body}
    </Dialog>
  )
}
