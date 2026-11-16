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
import { Brain, Play, RefreshCw, Settings2 } from 'lucide-react'
import { useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import { EmptyState } from '@/components/empty-state'
import { ErrorState } from '@/components/error-state'
import { SectionPageLayout } from '@/components/layout'
import { LoadingState } from '@/components/loading-state'
import { Button } from '@/components/ui/button'
import { handleServerError } from '@/lib/handle-server-error'
import { ROLE } from '@/lib/roles'
import { useAuthStore } from '@/stores/auth-store'

import { getModelIqOverview, runAllPelicanTests, runPelicanTest } from './api'
import { ModelIqTable } from './components/model-iq-table'
import { PelicanConfigDialog } from './components/pelican-config-dialog'
import { PelicanHistoryDialog } from './components/pelican-history-dialog'
import { PelicanPreviewDialog } from './components/pelican-preview-dialog'
import { pelicanTargetValue, summarizeModelIq } from './lib/iq'
import type { ModelIqStatus, PelicanTarget } from './types'

/**
 * 模型智商页：把每个（分组, 模型）组合的状态和鹈鹕测智（SVG 画一只骑自行车的鹈鹕）
 * 的分数放在一起，登录用户都能看到；只有管理员能触发新的测试与调整巡视设置。
 */
export function ModelIq() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const role = useAuthStore((state) => state.auth.user?.role ?? 0)
  const isAdmin = role >= ROLE.ADMIN
  const [busyTarget, setBusyTarget] = useState<string | null>(null)
  const [preview, setPreview] = useState<ModelIqStatus | null>(null)
  const [historyTarget, setHistoryTarget] = useState<PelicanTarget | null>(null)
  const [configOpen, setConfigOpen] = useState(false)

  const overview = useQuery({
    queryKey: ['model-iq', 'overview'],
    queryFn: getModelIqOverview,
  })

  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ['model-iq', 'overview'] })

  const singleRun = useMutation({
    mutationFn: runPelicanTest,
    onSuccess: (status) => {
      void invalidate()
      toast.success(
        t('{{group}} · {{model}} scored {{score}}', {
          group: status.group,
          model: status.model_name,
          score: status.score,
        })
      )
    },
    onError: (error) => handleServerError(error, t('Failed to run the test')),
    onSettled: () => setBusyTarget(null),
  })

  const allRun = useMutation({
    mutationFn: runAllPelicanTests,
    onSuccess: (summary) => {
      void invalidate()
      toast.success(
        t('{{tested}} models tested, {{failed}} failed', {
          tested: summary.tested,
          failed: summary.failed,
        })
      )
    },
    onError: (error) => handleServerError(error, t('Failed to run the test')),
  })

  const models = overview.data?.models ?? []
  const summary = summarizeModelIq(models)

  const handleRun = (status: ModelIqStatus) => {
    // 整轮巡视已在跑时不再接受单组合触发，避免同一组合被并发探两次。
    if (allRun.isPending) {
      return
    }
    const target = { group: status.group, model: status.model_name }
    setBusyTarget(pelicanTargetValue(target))
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
        title={t('No models to test yet')}
        description={t(
          'Add a chat model to an enabled channel, then let an administrator run the pelican test.'
        )}
      />
    )
  } else {
    listContent = (
      <ModelIqTable
        models={models}
        isAdmin={isAdmin}
        busyTarget={busyTarget}
        sweepRunning={allRun.isPending}
        onPreview={setPreview}
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
                disabled={
                  allRun.isPending || busyTarget !== null || !models.length
                }
                onClick={() => allRun.mutate()}
              >
                <Play data-icon='inline-start' />
                {allRun.isPending ? t('Testing…') : t('Test all models')}
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
              <p className='text-muted-foreground mt-1 text-sm'>
                {t(
                  'Every model gets the same request: an SVG animation of a pelican riding a bicycle. The score shows whether it can turn a multi-constraint instruction into working code.'
                )}
              </p>
              <p className='bg-muted mt-3 rounded-lg px-3 py-2 font-mono text-xs break-all'>
                {overview.data?.prompt ??
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
              </div>
            ) : null}

            {listContent}
          </div>
        </SectionPageLayout.Content>
      </SectionPageLayout>

      <PelicanHistoryDialog
        target={historyTarget}
        onOpenChange={(open) => {
          if (!open) setHistoryTarget(null)
        }}
      />

      {isAdmin ? (
        <PelicanConfigDialog
          open={configOpen}
          onOpenChange={setConfigOpen}
        />
      ) : null}

      <PelicanPreviewDialog
        open={preview !== null}
        group={preview?.group ?? ''}
        modelName={preview?.model_name ?? ''}
        resultId={preview?.result_id ?? 0}
        onOpenChange={(open) => {
          if (!open) setPreview(null)
        }}
      />
    </>
  )
}
