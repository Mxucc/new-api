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
import { Loader2, Play } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { GroupBadge } from '@/components/group-badge'
import { Button } from '@/components/ui/button'
import {
  HoverCard,
  HoverCardContent,
  HoverCardTrigger,
} from '@/components/ui/hover-card'
import { toIntlLocale } from '@/i18n/languages'
import { formatTimestampRelative } from '@/lib/format'
import { cn } from '@/lib/utils'

import {
  buildIqRows,
  iqCellClass,
  iqDegradedRate,
  iqStripCellClass,
  iqVerdictLabelKey,
  pelicanTargetLabel,
} from '../lib/iq'
import type { ModelIqStatus, PelicanTarget } from '../types'
import { ModelIqHoverCard } from './model-iq-hover-card'

export interface ModelIqGridProps {
  models: ModelIqStatus[]
  isAdmin: boolean
  onPreview: (status: ModelIqStatus) => void
  onHistory: (status: ModelIqStatus) => void
  onRun: (target: PelicanTarget) => void
}

/**
 * 格子里的小格子：最近 24 小时按巡视间隔切片（30 分钟 48 格、1 小时 24 格）。
 * 采到样的格子显示那次的判定；没采到样的格子沿用上一次成功的结果（默认是绿的），
 * 只有真正拿到输出的降智才变色——所以上游 502 不会把整条染红。
 */
function ModelIqStrip(props: { status: ModelIqStatus }) {
  const { t, i18n } = useTranslation()
  const locale = toIntlLocale(i18n.resolvedLanguage || i18n.language)
  const cells = props.status.strip
  if (cells.length === 0) {
    return null
  }
  return (
    <span
      className='flex min-w-0 flex-1 items-stretch gap-[2px]'
      data-testid={`model-iq-strip-${props.status.group}-${props.status.model_name}`}
    >
      {cells.map((cell) => {
        const when = formatTimestampRelative(cell.start, 'seconds', locale)
        const verdict =
          cell.verdict === 'pending'
            ? t('Not tested')
            : t(iqVerdictLabelKey(cell.verdict))
        const detail = cell.sampled
          ? `${when} · ${verdict}`
          : `${when} · ${verdict} · ${t('Carried over from the last successful run')}`
        return (
          <span
            key={cell.start}
            title={detail}
            className={cn(
              'h-4 min-w-0 flex-1 rounded-[2px]',
              iqStripCellClass(cell.verdict),
              cell.sampled ? undefined : 'opacity-60'
            )}
          />
        )
      })}
    </span>
  )
}

interface ModelIqRowCellProps {
  status: ModelIqStatus
  isAdmin: boolean
  onPreview: (status: ModelIqStatus) => void
  onHistory: (status: ModelIqStatus) => void
  onRun: (target: PelicanTarget) => void
}

/**
 * 一格 = 一个分组的探测项。每个分组只配一个模型，所以没有模型列：
 * 模型名写在格子里，右侧标降智率，下面一条 24 小时的时间条。
 */
function ModelIqRowCell(props: ModelIqRowCellProps) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const status = props.status
  const rate = iqDegradedRate(status.strip)

  let summary = <span>—</span>
  if (status.testing) {
    summary = <Loader2 className='size-3 animate-spin' aria-hidden='true' />
  } else if (rate !== null) {
    summary = (
      <span className={rate > 0 ? 'text-warning' : undefined}>
        {t('Degraded {{percent}}', { percent: `${Math.round(rate * 100)}%` })}
      </span>
    )
  }

  return (
    <HoverCard open={open} onOpenChange={setOpen}>
      <HoverCardTrigger
        // 悬停立刻打开；关闭沿用 Base UI 的 300ms 宽限，指针才移得进卡片点按钮。
        delay={0}
        render={
          <button
            type='button'
            data-testid={`model-iq-cell-${status.group}-${status.model_name}`}
            aria-label={`${pelicanTargetLabel({
              group: status.group,
              model: status.model_name,
            })} ${t(iqVerdictLabelKey(status.verdict))}`}
            onClick={() => props.onHistory(status)}
            className={cn(
              'flex w-full min-w-0 items-center gap-3 rounded-md border px-2 py-1.5 text-left transition-colors',
              'hover:brightness-105 focus-visible:ring-ring focus-visible:ring-2 focus-visible:ring-offset-1 focus-visible:outline-none',
              iqCellClass(status.verdict)
            )}
          >
            <span className='min-w-0 flex-1 truncate text-xs font-medium'>
              {status.model_name}
            </span>
            <ModelIqStrip status={status} />
            <span className='shrink-0 text-xs tabular-nums'>{summary}</span>
          </button>
        }
      />
      {open ? (
        <HoverCardContent className='w-64' side='bottom' align='start'>
          <ModelIqHoverCard
            status={status}
            isAdmin={props.isAdmin}
            onPreview={props.onPreview}
            onRun={props.onRun}
          />
        </HoverCardContent>
      ) : null}
    </HoverCard>
  )
}

/**
 * 一行 = 一个分组（每行只配一个模型，所以没有模型列）。铺成列表而不是表格，
 * 是为了让一屏内的对比靠颜色就够，细节留给悬停与点击；
 * 管理员每行末尾还有一个「立即测试」按钮，点了就在后台跑一次，不用等整轮巡视。
 */
export function ModelIqGrid(props: ModelIqGridProps) {
  const { t } = useTranslation()
  const rows = useMemo(() => buildIqRows(props.models), [props.models])
  // 列宽随模型名伸缩，网格模板只能按当前列数动态生成；管理员多一列动作。
  const templateColumns = `minmax(5.5rem, max-content) minmax(0, 1fr)${props.isAdmin ? ' minmax(2rem, max-content)' : ''}`

  // 一个探测项都没有时不留空列表：空壳子会被误读成「全挂了」。
  if (rows.length === 0) {
    return (
      <p
        className='text-muted-foreground bg-muted/40 rounded-xl border px-3 py-2 text-xs'
        data-testid='model-iq-grid-empty'
      >
        {t('No test results yet. Results appear here after the next sweep.')}
      </p>
    )
  }

  return (
    <div className='space-y-1' data-testid='model-iq-grid'>
      <div
        className='grid items-end gap-2 px-1'
        style={{ gridTemplateColumns: templateColumns }}
      >
        <span className='text-muted-foreground text-xs'>{t('Group')}</span>
        <span className='text-muted-foreground text-center text-xs'>
          {t('Last 24 hours')}
        </span>
        {props.isAdmin ? <span aria-hidden='true' /> : null}
      </div>

      {rows.map((row) => (
        <div
          key={`${row.group}/${row.status.model_name}`}
          className='grid items-center gap-2'
          style={{ gridTemplateColumns: templateColumns }}
          data-testid={`model-iq-row-${row.group}`}
        >
          <GroupBadge group={row.group} copyable={false} />
          <ModelIqRowCell
            status={row.status}
            isAdmin={props.isAdmin}
            onPreview={props.onPreview}
            onHistory={props.onHistory}
            onRun={props.onRun}
          />
          {props.isAdmin ? (
            <div className='flex justify-end'>
              <Button
                variant='ghost'
                size='icon-sm'
                title={t('Test {{group}} now', { group: row.group })}
                aria-label={t('Test {{group}} now', { group: row.group })}
                disabled={row.status.testing}
                onClick={() =>
                  props.onRun({
                    group: row.group,
                    model: row.status.model_name,
                  })
                }
              >
                {row.status.testing ? (
                  <Loader2
                    className='size-3.5 animate-spin'
                    aria-hidden='true'
                  />
                ) : (
                  <Play className='size-3.5' aria-hidden='true' />
                )}
              </Button>
            </div>
          ) : null}
        </div>
      ))}
    </div>
  )
}
