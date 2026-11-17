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
import { Play, Sparkles } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { GroupBadge } from '@/components/group-badge'
import { Button } from '@/components/ui/button'
import { formatLatency } from '@/features/performance-metrics/lib/format'
import { toIntlLocale } from '@/i18n/languages'
import { formatTimestampRelative } from '@/lib/format'

import { getPelicanHistory } from '../api'
import type { ModelIqStatus, PelicanTarget } from '../types'
import { ModelIqScore } from './model-iq-score'

/** 悬停卡片里只画最近几次，趋势本身交给点击后的历史弹窗。 */
const HOVER_HISTORY_LIMIT = 5

/** 悬停历史是懒加载的：卡片第一次打开才请求，之后由 react-query 缓存复用。 */
function RecentScoreSparkline(props: { group: string; modelName: string }) {
  const { t } = useTranslation()
  const query = useQuery({
    queryKey: [
      'model-iq',
      'history',
      props.group,
      props.modelName,
      HOVER_HISTORY_LIMIT,
    ],
    queryFn: () =>
      getPelicanHistory(props.group, props.modelName, HOVER_HISTORY_LIMIT),
    staleTime: 30 * 1000,
    retry: false,
    meta: { errorToast: false },
  })

  if (query.isLoading) {
    return (
      <div
        className='bg-muted h-8 w-full animate-pulse rounded'
        data-testid={`model-iq-sparkline-loading-${props.group}-${props.modelName}`}
      />
    )
  }

  const points = query.data?.history ?? []
  if (points.length === 0) {
    return null
  }
  // 分数固定按 0–100 映射到卡片高度，失败的一次（0 分）会落到基线，看得见。
  const polyline = points
    .map((point, index) => {
      const x = points.length > 1 ? (index * 100) / (points.length - 1) : 50
      const score = Math.min(100, Math.max(0, point.score))
      return `${x.toFixed(1)},${(26 - (score / 100) * 24).toFixed(1)}`
    })
    .join(' ')

  return (
    <div className='space-y-1'>
      <span className='text-muted-foreground text-[10px]'>
        {t('Recent scores')}
      </span>
      <svg
        viewBox='0 0 100 28'
        preserveAspectRatio='none'
        aria-hidden='true'
        className='text-chart-1 h-8 w-full'
        data-testid={`model-iq-sparkline-${props.group}-${props.modelName}`}
      >
        <polyline
          points={polyline}
          fill='none'
          stroke='currentColor'
          strokeWidth={1.5}
          strokeLinecap='round'
          vectorEffect='non-scaling-stroke'
        />
      </svg>
    </div>
  )
}

export interface ModelIqHoverCardProps {
  status: ModelIqStatus
  isAdmin: boolean
  onPreview: (status: ModelIqStatus) => void
  onRun: (target: PelicanTarget) => void
}

/** 悬停卡片：最近一次成绩、最近五次的迷你折线，以及针对该组合的动作。 */
export function ModelIqHoverCard(props: ModelIqHoverCardProps) {
  const { t, i18n } = useTranslation()
  const locale = toIntlLocale(i18n.resolvedLanguage || i18n.language)
  const status = props.status

  return (
    <div
      className='space-y-2'
      data-testid={`model-iq-hover-${status.group}-${status.model_name}`}
    >
      <div className='flex min-w-0 items-center gap-1.5'>
        <GroupBadge group={status.group} copyable={false} />
        <span className='min-w-0 truncate text-xs font-medium'>
          {status.model_name}
        </span>
      </div>

      <div className='flex items-center justify-between gap-2'>
        <ModelIqScore status={status} />
        <span className='text-muted-foreground text-xs tabular-nums'>
          {status.latency_ms
            ? formatLatency(status.latency_ms)
            : t('Not tested')}
        </span>
      </div>

      {status.tested_at ? (
        <p className='text-muted-foreground text-xs'>
          {formatTimestampRelative(status.tested_at, 'seconds', locale)}
        </p>
      ) : null}

      {/* 从 magpie-ops 取来的结果：说清来源，以及那个答案是什么时候测出来的。 */}
      {status.source === 'magpie-ops' ? (
        <p className='text-muted-foreground text-xs'>
          {t('Reused from magpie-ops')}
          {status.observed_at
            ? ` · ${formatTimestampRelative(status.observed_at, 'seconds', locale)}`
            : ''}
        </p>
      ) : null}

      {/* 上游报 502 之类的错误时，这一行说明是哪条渠道测出来的。 */}
      {status.channel_id > 0 ? (
        <p className='text-muted-foreground text-xs'>
          {t('Channel {{id}}', { id: status.channel_id })}
        </p>
      ) : null}

      {/* 联动状态：这个分组映射到的 magpie 分组，以及上次请求调整失败的原因。 */}
      {status.magpie_group ? (
        <p className='text-muted-foreground text-xs'>
          {t('magpie group {{group}}', { group: status.magpie_group })}
        </p>
      ) : null}
      {(status.magpie_actions ?? []).map((action) => (
        <p
          key={`${action.ref}-${action.kind}`}
          className='text-muted-foreground text-xs'
        >
          {t('magpie adjusted: {{result}}', {
            result:
              action.score === undefined
                ? `${action.ref} ${action.kind}`
                : `${action.ref} ${action.kind} · ${action.score}`,
          })}
        </p>
      ))}
      {status.magpie_result ? (
        <p className='text-muted-foreground text-xs'>
          {t('magpie adjusted: {{result}}', { result: status.magpie_result })}
        </p>
      ) : null}
      {status.magpie_error ? (
        <p className='text-destructive text-xs'>
          {t('magpie: {{message}}', { message: status.magpie_error })}
        </p>
      ) : null}

      <RecentScoreSparkline
        group={status.group}
        modelName={status.model_name}
      />

      <div className='flex items-center justify-end gap-1'>
        <Button
          variant='ghost'
          size='sm'
          disabled={status.result_id <= 0}
          onClick={() => props.onPreview(status)}
        >
          <Sparkles data-icon='inline-start' />
          {t('Preview')}
        </Button>
        {props.isAdmin ? (
          <Button
            variant='outline'
            size='sm'
            disabled={status.testing}
            onClick={() =>
              props.onRun({
                group: status.group,
                model: status.model_name,
              })
            }
          >
            <Play data-icon='inline-start' />
            {status.testing ? t('Testing…') : t('Run test')}
          </Button>
        ) : null}
      </div>
    </div>
  )
}
