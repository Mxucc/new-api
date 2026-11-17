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
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Navigate } from '@tanstack/react-router'
import { Brain, Loader2, Play, RefreshCw, Settings2 } from 'lucide-react'
import { useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import { EmptyState } from '@/components/empty-state'
import { ErrorState } from '@/components/error-state'
import { SectionPageLayout } from '@/components/layout'
import { LoadingState } from '@/components/loading-state'
import { Button } from '@/components/ui/button'
import { toIntlLocale } from '@/i18n/languages'
import { formatTimestampRelative } from '@/lib/format'
import { handleServerError } from '@/lib/handle-server-error'
import { ROLE } from '@/lib/roles'
import { useAuthStore } from '@/stores/auth-store'

import { getModelIqOverview, runAllPelicanTests, runPelicanTest } from './api'
import { ModelIqGrid } from './components/model-iq-grid'
import { PelicanConfigDialog } from './components/pelican-config-dialog'
import { PelicanHistoryDialog } from './components/pelican-history-dialog'
import { PelicanPreviewDialog } from './components/pelican-preview-dialog'
import { iqNeedsPolling, pelicanTargetLabel, summarizeModelIq } from './lib/iq'
import type { PelicanTarget } from './types'

/**
 * 探测在后台跑，页面只在有活干时每 5 秒看一次进度；全部空闲就停，
 * 不留一个永远转的定时器。
 */
const MODEL_IQ_POLL_INTERVAL_MS = 5000

/**
 * 预览弹窗需要的东西：哪个组合的第几条结果。
 * 格子上的「预览」和历史上任意一条记录都能打开它。
 */
type PreviewTarget = {
  group: string
  modelName: string
  resultId: number
}

/**
 * 模型智商页：把每个（分组, 模型）组合的状态和鹈鹕测智（SVG 画一只骑自行车的鹈鹕）
 * 的分数放在一起，登录用户都能看到；只有管理员能触发新的测试与调整巡视设置。
 * 触发即返回，进度和结果都来自 overview 的轮询。
 */
export function ModelIq() {
  const { t, i18n } = useTranslation()
  const locale = toIntlLocale(i18n.resolvedLanguage || i18n.language)
  const queryClient = useQueryClient()
  const role = useAuthStore((state) => state.auth.user?.role ?? 0)
  const isAdmin = role >= ROLE.ADMIN
  const [preview, setPreview] = useState<PreviewTarget | null>(null)
  const [historyTarget, setHistoryTarget] = useState<PelicanTarget | null>(null)
  const [configOpen, setConfigOpen] = useState(false)

  const overview = useQuery({
    queryKey: ['model-iq', 'overview'],
    queryFn: getModelIqOverview,
    // 整轮巡视在跑，或任一格子还在测时继续轮询；两件都停了就不再请求。
    refetchInterval: (query) =>
      iqNeedsPolling(query.state.data) ? MODEL_IQ_POLL_INTERVAL_MS : false,
  })

  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ['model-iq', 'overview'] })

  const singleRun = useMutation({
    mutationFn: runPelicanTest,
    onSuccess: (ack, target) => {
      void invalidate()
      toast.success(
        ack.started
          ? t('Test started for {{target}}', {
              target: pelicanTargetLabel(target),
            })
          : t('The test for this group and model is already running')
      )
    },
    onError: (error) => handleServerError(error, t('Failed to run the test')),
  })

  const allRun = useMutation({
    mutationFn: runAllPelicanTests,
    onSuccess: (ack) => {
      void invalidate()
      if (ack.started) {
        toast.success(
          t('Sweep started for {{count}} models', {
            count: ack.targets ?? 0,
          })
        )
        return
      }
      // running 为真说明已有一轮在跑；否则是没配置探测项，得先去设置里配。
      toast.info(
        ack.running
          ? t('The sweep is already running')
          : t('No probe targets are configured yet')
      )
    },
    onError: (error) => handleServerError(error, t('Failed to run the test')),
  })

  const models = overview.data?.models ?? []
  const summary = summarizeModelIq(models)
  const sweeping = overview.data?.sweeping ?? false
  const sweepingSince = overview.data?.sweeping_since ?? 0

  // 管理员没配探测目标时这页对普通用户没有内容：直接回首页，别把空页露出来。
  if (!isAdmin && !overview.isLoading && models.length === 0) {
    return <Navigate to='/dashboard/models' replace />
  }
  const handleRun = (target: PelicanTarget) => {
    singleRun.mutate(target)
  }

  // 加载、失败、空与正常四种状态按顺序判定，避免嵌套三元。
  let listContent: ReactNode
  if (overview.isLoading) {
    listContent = <LoadingState />
  } else if (overview.isError) {
    listContent = (
      <ErrorState
        title={t('Failed to load the model IQ results')}
        onRetry={() => void overview.refetch()}
      />
    )
  } else if (models.length === 0) {
    listContent = (
      <EmptyState
        icon={Brain}
        title={t('No probe targets configured')}
        description={t(
          'An administrator picks one model per group in the sweep settings. Everyone sees the same results here.'
        )}
      />
    )
  } else {
    listContent = (
      <ModelIqGrid
        models={models}
        isAdmin={isAdmin}
        onPreview={(status) =>
          setPreview({
            group: status.group,
            modelName: status.model_name,
            resultId: status.result_id,
          })
        }
        onHistory={(status) =>
          setHistoryTarget({ group: status.group, model: status.model_name })
        }
        onRun={handleRun}
      />
    )
  }

  return (
    <>
      <SectionPageLayout>
        <SectionPageLayout.Title>{t('Model IQ')}</SectionPageLayout.Title>
        <SectionPageLayout.Actions>
          {isAdmin ? (
            <>
              <Button
                variant='ghost'
                size='sm'
                onClick={() => setConfigOpen(true)}
              >
                <Settings2 data-icon='inline-start' />
                {t('Sweep settings')}
              </Button>
              <Button
                variant='outline'
                size='sm'
                disabled={sweeping || allRun.isPending || !models.length}
                onClick={() => allRun.mutate()}
              >
                <Play data-icon='inline-start' />
                {sweeping ? t('Testing…') : t('Test all models')}
              </Button>
            </>
          ) : null}
          <Button
            variant='ghost'
            size='sm'
            disabled={overview.isFetching}
            onClick={() => void overview.refetch()}
          >
            <RefreshCw
              data-icon='inline-start'
              className={overview.isFetching ? 'animate-spin' : undefined}
            />
            {t('Refresh')}
          </Button>
        </SectionPageLayout.Actions>
        <SectionPageLayout.Content>
          <div className='space-y-4'>
            <section className='bg-card rounded-xl border p-4'>
              <p className='text-sm font-medium'>{t('Pelican IQ test')}</p>
              <p className='text-muted-foreground mt-1 text-sm whitespace-pre-line'>
                {overview.data?.description ||
                  t(
                    'Every model gets the same request: an SVG animation of a pelican riding a bicycle. The score shows whether it can turn a multi-constraint instruction into working code.'
                  )}
              </p>
              <p className='bg-muted mt-3 rounded-lg px-3 py-2 font-mono text-xs break-all whitespace-pre-line'>
                {overview.data?.prompt ||
                  t('An SVG animation of a pelican riding a bicycle.')}
              </p>
            </section>

            {models.length ? (
              <div className='text-muted-foreground flex flex-wrap gap-x-4 gap-y-1 text-xs'>
                <span>{t('{{count}} models', { count: models.length })}</span>
                <span>{t('{{count}} tested', { count: summary.tested })}</span>
                <span>
                  {t('{{count}} healthy', { count: summary.healthy })}
                </span>
                {summary.unavailable ? (
                  <span>
                    {t('{{count}} unavailable', {
                      count: summary.unavailable,
                    })}
                  </span>
                ) : null}
                {sweeping ? (
                  <span
                    className='text-info inline-flex items-center gap-1'
                    data-testid='model-iq-sweeping'
                  >
                    <Loader2
                      className='size-3 animate-spin'
                      aria-hidden='true'
                    />
                    {sweepingSince
                      ? t('Sweep in progress since {{time}}', {
                          time: formatTimestampRelative(
                            sweepingSince,
                            'seconds',
                            locale
                          ),
                        })
                      : t('Sweep in progress')}
                  </span>
                ) : null}
              </div>
            ) : null}

            {listContent}
          </div>
        </SectionPageLayout.Content>
      </SectionPageLayout>

      <PelicanHistoryDialog
        target={historyTarget}
        isAdmin={isAdmin}
        onPreview={(resultId) => {
          // 历史里点一条就看它当时的画面：先关掉历史，别把两个弹窗叠在一起。
          setPreview({
            group: historyTarget?.group ?? '',
            modelName: historyTarget?.model ?? '',
            resultId,
          })
          setHistoryTarget(null)
        }}
        onOpenChange={(open) => {
          if (!open) setHistoryTarget(null)
        }}
      />

      {isAdmin ? (
        <PelicanConfigDialog open={configOpen} onOpenChange={setConfigOpen} />
      ) : null}

      <PelicanPreviewDialog
        open={preview !== null}
        group={preview?.group ?? ''}
        modelName={preview?.modelName ?? ''}
        resultId={preview?.resultId ?? 0}
        onOpenChange={(open) => {
          if (!open) setPreview(null)
        }}
      />
    </>
  )
}
