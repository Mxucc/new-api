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
import { CircleX, Loader2, Play } from 'lucide-react'
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
import { formatNumber, formatTimestampRelative } from '@/lib/format'
import { cn } from '@/lib/utils'

import {
  buildIqGrid,
  iqCellClass,
  iqHasScore,
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

interface ModelIqCellProps {
  status: ModelIqStatus
  isAdmin: boolean
  onPreview: (status: ModelIqStatus) => void
  onHistory: (status: ModelIqStatus) => void
  onRun: (target: PelicanTarget) => void
}

/**
 * 格子里的小格子：最近 24 小时的时间条，一段一格，和状态页一样。
 * 没有观测的段落用最近一次结果补上（淡显），所以只测过一次的组合也是一整条。
 */
function ModelIqCellStrip(props: { status: ModelIqStatus }) {
  const { t, i18n } = useTranslation()
  const locale = toIntlLocale(i18n.resolvedLanguage || i18n.language)
  const cells = props.status.strip
  if (cells.length === 0) {
    return null
  }
  return (
    <span
      className='flex w-full items-stretch gap-[2px]'
      data-testid={`model-iq-strip-${props.status.group}-${props.status.model_name}`}
    >
      {cells.map((cell) => {
        const when = formatTimestampRelative(cell.start, 'seconds', locale)
        const verdict = t(iqVerdictLabelKey(cell.verdict))
        return (
          <span
            key={cell.start}
            title={
              cell.filled
                ? `${when} · ${verdict} · ${t('Based on the latest result')}`
                : `${when} · ${verdict}`
            }
            className={cn(
              'h-3 min-w-[3px] flex-1 rounded-[2px]',
              iqStripCellClass(cell.verdict, cell.filled)
            )}
          />
        )
      })}
    </span>
  )
}

/** 一个（分组, 模型）格子：时间条 + 分数，悬停看概要，点击看完整历史。 */
function ModelIqCell(props: ModelIqCellProps) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const status = props.status
  const scored = iqHasScore(status.verdict)

  let cellContent = <span>—</span>
  if (status.testing) {
    cellContent = (
      <Loader2 className='size-3.5 animate-spin' aria-hidden='true' />
    )
  } else if (scored) {
    cellContent = <span>{formatNumber(status.score)}</span>
  } else if (status.verdict === 'failed') {
    cellContent = <CircleX className='size-3.5' aria-hidden='true' />
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
              'flex h-11 min-w-0 flex-col items-stretch justify-center gap-1 rounded-md border px-1 py-1 text-sm font-semibold tabular-nums transition-colors',
              'hover:brightness-105 focus-visible:ring-ring focus-visible:ring-2 focus-visible:outline-none',
              iqCellClass(status.verdict)
            )}
          >
            <ModelIqCellStrip status={status} />
            <span className='text-center leading-none'>{cellContent}</span>
          </button>
        }
      />
      {open ? (
        <HoverCardContent className='w-64'>
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
 * 行 = 分组，列 = 分组配置的模型。铺开成网格而不是表格，
 * 是为了让一屏内的对比靠颜色就够，细节留给悬停与点击；
 * 管理员每行末尾还有一个「立即测试」按钮，点了就在后台跑一次，不用等整轮巡视。
 */
export function ModelIqGrid(props: ModelIqGridProps) {
  const { t } = useTranslation()
  const grid = useMemo(() => buildIqGrid(props.models), [props.models])
  // 列宽随模型名伸缩，网格模板只能按当前列数动态生成；管理员多一列动作。
  const templateColumns = `minmax(5.5rem, max-content) repeat(${grid.columns.length}, minmax(4.5rem, 1fr))${props.isAdmin ? ' minmax(2rem, max-content)' : ''}`

  // 一个结果都没有时不留空网格：空壳子会被误读成「全挂了」。
  if (grid.total === 0) {
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
    <div className='overflow-x-auto' data-testid='model-iq-grid'>
      <div className='min-w-max space-y-1'>
        <div
          className='grid items-end gap-1'
          style={{ gridTemplateColumns: templateColumns }}
        >
          <span className='text-muted-foreground px-1 text-xs'>
            {t('Group')}
          </span>
          {grid.columns.map((modelName) => (
            <span
              key={modelName}
              title={modelName}
              className='text-muted-foreground truncate px-1 text-center text-xs'
            >
              {modelName}
            </span>
          ))}
          {props.isAdmin ? <span aria-hidden='true' /> : null}
        </div>

        {grid.rows.map((row) => {
          // 每行只配一个模型：动作按钮落在这个分组自己配置的目标上。
          const own = row.own
          return (
            <div
              key={row.group}
              className='grid items-center gap-1'
              style={{ gridTemplateColumns: templateColumns }}
              data-testid={`model-iq-row-${row.group}`}
            >
              <GroupBadge group={row.group} copyable={false} />
              {row.cells.map((status, index) =>
                status ? (
                  <ModelIqCell
                    key={status.model_name}
                    status={status}
                    isAdmin={props.isAdmin}
                    onPreview={props.onPreview}
                    onHistory={props.onHistory}
                    onRun={props.onRun}
                  />
                ) : (
                  // 这一行没有这个模型：留一个空槽，列才对得上。
                  <span
                    key={grid.columns[index]}
                    aria-hidden='true'
                    className='border-border/50 h-11 rounded-md border border-dashed'
                  />
                )
              )}
              {props.isAdmin ? (
                <div className='flex justify-end'>
                  {own ? (
                    <Button
                      variant='ghost'
                      size='icon-sm'
                      title={t('Test {{group}} now', { group: row.group })}
                      aria-label={t('Test {{group}} now', {
                        group: row.group,
                      })}
                      disabled={own.testing}
                      onClick={() =>
                        props.onRun({
                          group: own.group,
                          model: own.model_name,
                        })
                      }
                    >
                      {own.testing ? (
                        <Loader2
                          className='size-3.5 animate-spin'
                          aria-hidden='true'
                        />
                      ) : (
                        <Play className='size-3.5' aria-hidden='true' />
                      )}
                    </Button>
                  ) : null}
                </div>
              ) : null}
            </div>
          )
        })}
      </div>
    </div>
  )
}
