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
import { formatNumber } from '@/lib/format'
import { cn } from '@/lib/utils'

import {
  buildIqGrid,
  iqCellClass,
  iqHasScore,
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
  /** 这一格所在的分组（行）；借用数据时它与 status.group 不同。 */
  rowGroup: string
  status: ModelIqStatus
  /** 有值表示这一格是借用「旁边」的最近一份结果补上的，只做视觉补齐。 */
  borrowedFrom?: string
  isAdmin: boolean
  onPreview: (status: ModelIqStatus) => void
  onHistory: (status: ModelIqStatus) => void
  onRun: (target: PelicanTarget) => void
}

/** 一个（分组, 模型）格子：显示分数，悬停看概要与最近趋势，点击看完整历史。 */
function ModelIqCell(props: ModelIqCellProps) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const status = props.status
  const scored = iqHasScore(status.verdict)

  let cellContent = <span>—</span>
  if (status.testing) {
    cellContent = <Loader2 className='size-4 animate-spin' aria-hidden='true' />
  } else if (scored) {
    cellContent = <span>{formatNumber(status.score)}</span>
  } else if (status.verdict === 'failed') {
    cellContent = <CircleX className='size-4' aria-hidden='true' />
  }

  // 借来的格子只补视觉：不弹卡片、不跳历史，免得把别的分组的数据当成自己的。
  if (props.borrowedFrom) {
    return (
      <div
        data-testid={`model-iq-borrowed-${props.rowGroup}-${status.model_name}`}
        title={t('Copied from {{group}}', { group: props.borrowedFrom })}
        className={cn(
          'flex h-10 min-w-0 items-center justify-center gap-1 rounded-md border border-dashed text-sm font-semibold tabular-nums opacity-45',
          iqCellClass(status.verdict)
        )}
      >
        {scored ? <span>{formatNumber(status.score)}</span> : cellContent}
      </div>
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
              'flex h-10 min-w-0 items-center justify-center gap-1 rounded-md border text-sm font-semibold tabular-nums transition-colors',
              'hover:brightness-105 focus-visible:ring-ring focus-visible:ring-2 focus-visible:outline-none',
              iqCellClass(status.verdict)
            )}
          >
            {cellContent}
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
          // 每行只配一个模型：动作按钮落在这个分组自己配置的目标上，
          // 即使这一行的格子显示的是借来的数据。
          const own = row.own
          return (
            <div
              key={row.group}
              className='grid items-center gap-1'
              style={{ gridTemplateColumns: templateColumns }}
              data-testid={`model-iq-row-${row.group}`}
            >
              <GroupBadge group={row.group} copyable={false} />
              {row.cells.map((cell, index) =>
                cell ? (
                  <ModelIqCell
                    key={`${cell.borrowedFrom ?? cell.status.group}-${cell.status.model_name}`}
                    rowGroup={row.group}
                    status={cell.status}
                    borrowedFrom={cell.borrowedFrom}
                    isAdmin={props.isAdmin}
                    onPreview={props.onPreview}
                    onHistory={props.onHistory}
                    onRun={props.onRun}
                  />
                ) : (
                  // 这一格既没有自己的结果，旁边也借不到数据：留一个空槽，列才对得上。
                  <span
                    key={grid.columns[index]}
                    aria-hidden='true'
                    className='border-border/50 h-10 rounded-md border border-dashed'
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
