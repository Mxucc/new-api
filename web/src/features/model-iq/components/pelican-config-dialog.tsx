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
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import { Dialog } from '@/components/dialog'
import { ErrorState } from '@/components/error-state'
import { LoadingState } from '@/components/loading-state'
import { MultiSelect } from '@/components/multi-select'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { handleServerError } from '@/lib/handle-server-error'

import { getPelicanConfig, updatePelicanConfig } from '../api'
import {
  pelicanTargetFromValue,
  pelicanTargetLabel,
  pelicanTargetValue,
} from '../lib/iq'

const MAX_INTERVAL_MINUTES = 1440

type PelicanConfigDraft = {
  selected: string[]
  intervalMinutes: string
}

export interface PelicanConfigDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

/**
 * 管理员设置自动巡视：要测哪些（分组, 模型）组合，以及多久跑一轮。
 * 组合取自后端给出的当前可用列表，避免存进永远测不出结果的条目。
 */
export function PelicanConfigDialog(props: PelicanConfigDialogProps) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [draft, setDraft] = useState<PelicanConfigDraft | null>(null)

  const query = useQuery({
    queryKey: ['model-iq', 'config'],
    queryFn: getPelicanConfig,
    enabled: props.open,
  })

  const config = query.data
  const options = useMemo(
    () =>
      (config?.available_targets ?? []).map((target) => ({
        value: pelicanTargetValue(target),
        label: pelicanTargetLabel(target),
      })),
    [config]
  )
  const configuredValues = useMemo(
    () => (config?.targets ?? []).map(pelicanTargetValue),
    [config]
  )
  const selected = draft?.selected ?? configuredValues
  const intervalMinutes =
    draft?.intervalMinutes ?? String(config?.interval_minutes ?? 0)
  const intervalNumber = Number(intervalMinutes)
  const intervalValid =
    intervalMinutes.trim() !== '' &&
    Number.isInteger(intervalNumber) &&
    intervalNumber >= 0 &&
    intervalNumber <= MAX_INTERVAL_MINUTES

  // 配置里可能留着已经没有渠道的组合：后端会拒绝它们，这里先要求删掉再保存。
  const blockingTargets = useMemo(() => {
    const available = new Set(options.map((option) => option.value))
    return selected
      .filter((value) => !available.has(value))
      .map((value) => pelicanTargetLabel(pelicanTargetFromValue(value)))
  }, [selected, options])

  const save = useMutation({
    mutationFn: updatePelicanConfig,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['model-iq'] })
      toast.success(t('Sweep settings saved'))
      setDraft(null)
      props.onOpenChange(false)
    },
    onError: (error) =>
      handleServerError(error, t('Failed to save the sweep settings')),
  })

  // 关掉弹窗时丢掉草稿，下次打开重新读服务端的配置。
  useEffect(() => {
    if (!props.open) {
      setDraft(null)
    }
  }, [props.open])

  const handleSave = () => {
    if (!intervalValid || blockingTargets.length > 0) {
      return
    }
    save.mutate({
      targets: selected.map(pelicanTargetFromValue),
      interval_minutes: intervalNumber,
    })
  }

  // 加载、失败与表单三种状态按顺序判定，避免嵌套三元。
  let body = null
  if (query.isLoading) {
    body = <LoadingState />
  } else if (query.isError) {
    body = (
      <ErrorState
        title={t('Failed to load the sweep settings')}
        onRetry={() => void query.refetch()}
      />
    )
  } else {
    body = (
      <div className='space-y-5'>
        <div className='space-y-2'>
          <Label htmlFor='pelican-sweep-targets'>
            {t('Targets to test')}
          </Label>
          <MultiSelect
            id='pelican-sweep-targets'
            options={options}
            selected={selected}
            onChange={(values) =>
              setDraft({ selected: values, intervalMinutes })
            }
            placeholder={t('Choose group / model pairs')}
            emptyText={t('No targets available yet')}
          />
          <p className='text-muted-foreground text-xs'>
            {t(
              'Pick the group and model pairs the scheduled sweep should cover. Leave empty to test everything that is available.'
            )}
          </p>
          {blockingTargets.length > 0 ? (
            <div
              className='text-destructive space-y-1 text-xs'
              data-testid='pelican-config-missing-targets'
            >
              <p>
                {t(
                  'These targets no longer have an enabled channel: remove them before saving.'
                )}
              </p>
              <ul className='list-disc ps-5'>
                {blockingTargets.map((label) => (
                  <li key={label}>{label}</li>
                ))}
              </ul>
            </div>
          ) : null}
        </div>

        <div className='space-y-2'>
          <Label htmlFor='pelican-sweep-interval'>
            {t('Sweep interval (minutes)')}
          </Label>
          <Input
            id='pelican-sweep-interval'
            type='number'
            min={0}
            max={MAX_INTERVAL_MINUTES}
            step={1}
            value={intervalMinutes}
            aria-invalid={intervalValid ? undefined : true}
            onChange={(event) =>
              setDraft({
                selected,
                intervalMinutes: event.target.value,
              })
            }
          />
          <p className='text-muted-foreground text-xs'>
            {intervalValid
              ? t('0 disables the scheduled sweep.')
              : t('Interval must be a whole number between 0 and 1440.')}
          </p>
        </div>
      </div>
    )
  }

  return (
    <Dialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      title={t('Sweep settings')}
      description={t(
        'Choose which group / model pairs the pelican sweep tests and how often it runs.'
      )}
      footer={
        <>
          <Button
            variant='ghost'
            size='sm'
            onClick={() => props.onOpenChange(false)}
          >
            {t('Cancel')}
          </Button>
          <Button
            size='sm'
            disabled={
              save.isPending ||
              !intervalValid ||
              blockingTargets.length > 0 ||
              query.isLoading
            }
            onClick={handleSave}
          >
            {save.isPending ? t('Saving…') : t('Save')}
          </Button>
        </>
      }
    >
      {body}
    </Dialog>
  )
}
