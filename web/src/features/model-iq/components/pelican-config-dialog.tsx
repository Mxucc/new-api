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
import { GroupBadge } from '@/components/group-badge'
import { LoadingState } from '@/components/loading-state'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { handleServerError } from '@/lib/handle-server-error'
import { useAuthStore } from '@/stores/auth-store'

import { getPelicanConfig, updatePelicanConfig } from '../api'
import type { PelicanConfig, PelicanTarget } from '../types'

const MAX_INTERVAL_MINUTES = 1440

/** 「这个分组不探测」在下拉里的取值：空值当选项值在部分选择器上不可靠，单独给个哨兵。 */
const NO_MODEL = '__none__'

type PelicanConfigDraft = {
  /** 分组 -> 模型；不在表里的分组就是不探测。 */
  models: Record<string, string>
  intervalMinutes: string
}

export interface PelicanConfigDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

function draftFromConfig(config: PelicanConfig | undefined): PelicanConfigDraft {
  const models: Record<string, string> = {}
  for (const target of config?.targets ?? []) {
    models[target.group] = target.model
  }
  return { models, intervalMinutes: String(config?.interval_minutes ?? 0) }
}

/**
 * 管理员设置自动巡视：每个分组探测一个模型，以及多久跑一轮。
 * 分组与模型取自后端给出的当前可用列表，避免存进永远测不出结果的条目；
 * 用量记在保存这份设置的管理员名下，弹窗里写明记给谁。
 */
export function PelicanConfigDialog(props: PelicanConfigDialogProps) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const currentUsername = useAuthStore((state) => state.auth.user?.username ?? '')
  const [editing, setEditing] = useState<PelicanConfigDraft | null>(null)

  const query = useQuery({
    queryKey: ['model-iq', 'config'],
    queryFn: getPelicanConfig,
    enabled: props.open,
  })

  const config = query.data
  const initialDraft = useMemo(() => draftFromConfig(config), [config])
  const draft = editing ?? initialDraft

  // 行 = 分组：可测的分组与配置里已有的分组都要出现，后者可能已经没有渠道了。
  const groups = useMemo(() => {
    const names = new Set<string>()
    for (const target of config?.available_targets ?? []) {
      names.add(target.group)
    }
    for (const target of config?.targets ?? []) {
      names.add(target.group)
    }
    return [...names].sort((a, b) => a.localeCompare(b))
  }, [config])

  const modelsByGroup = useMemo(() => {
    const map = new Map<string, string[]>()
    for (const target of config?.available_targets ?? []) {
      map.set(target.group, [...(map.get(target.group) ?? []), target.model])
    }
    // 配置里留着、但现在没有渠道的模型也要能显示出来，保存时后端会明确拒绝。
    for (const target of config?.targets ?? []) {
      const models = map.get(target.group) ?? []
      if (!models.includes(target.model)) {
        map.set(target.group, [...models, target.model])
      }
    }
    for (const group of map.keys()) {
      const models = map.get(group) ?? []
      map.set(
        group,
        models.sort((a, b) => a.localeCompare(b))
      )
    }
    return map
  }, [config])

  // 配置里可能留着已经没有渠道的组合：后端会拒绝它们，这里先要求改掉再保存。
  const missing = useMemo(() => {
    const available = new Set(
      (config?.available_targets ?? []).map(
        (target) => `${target.group}/${target.model}`
      )
    )
    const stale: PelicanTarget[] = []
    for (const group of groups) {
      const model = draft.models[group] ?? ''
      if (model !== '' && !available.has(`${group}/${model}`)) {
        stale.push({ group, model })
      }
    }
    return stale
  }, [config, draft.models, groups])

  const intervalNumber = Number(draft.intervalMinutes)
  const intervalValid =
    draft.intervalMinutes.trim() !== '' &&
    Number.isInteger(intervalNumber) &&
    intervalNumber >= 0 &&
    intervalNumber <= MAX_INTERVAL_MINUTES

  const save = useMutation({
    mutationFn: updatePelicanConfig,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['model-iq'] })
      toast.success(t('Sweep settings saved'))
      setEditing(null)
      props.onOpenChange(false)
    },
    onError: (error) =>
      handleServerError(error, t('Failed to save the sweep settings')),
  })

  // 关掉弹窗时丢掉草稿，下次打开重新读服务端的配置。
  useEffect(() => {
    if (!props.open) {
      setEditing(null)
    }
  }, [props.open])

  const handleModelChange = (group: string, value: string) => {
    setEditing({
      models: { ...draft.models, [group]: value === NO_MODEL ? '' : value },
      intervalMinutes: draft.intervalMinutes,
    })
  }

  const handleSave = () => {
    if (!intervalValid || missing.length > 0) {
      return
    }
    // 每个分组最多一个模型：这里按分组各取一个，空分组直接不提交。
    const targets: PelicanTarget[] = []
    for (const group of groups) {
      const model = draft.models[group] ?? ''
      if (model !== '') {
        targets.push({ group, model })
      }
    }
    save.mutate({ targets, interval_minutes: intervalNumber })
  }

  const ownerName = config?.owner_username || currentUsername

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
          <Label>{t('Groups to test')}</Label>
          <p className='text-muted-foreground text-xs'>
            {t(
              'Pick one model per group. The sweep only tests the groups listed here.'
            )}
          </p>
          {groups.length === 0 ? (
            <p
              className='text-muted-foreground text-xs'
              data-testid='pelican-config-no-groups'
            >
              {t('No group has an enabled chat model yet.')}
            </p>
          ) : (
            <div className='space-y-2'>
              {groups.map((group) => (
                <div
                  key={group}
                  className='flex items-center gap-2'
                  data-testid={`pelican-config-group-${group}`}
                >
                  <div className='w-28 min-w-0 shrink-0'>
                    <GroupBadge group={group} copyable={false} />
                  </div>
                  <Select
                    value={draft.models[group] || NO_MODEL}
                    onValueChange={(value) =>
                      handleModelChange(group, value ?? '')
                    }
                  >
                    <SelectTrigger
                      className='w-full'
                      aria-label={t('Probe model for {{group}}', { group })}
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value={NO_MODEL}>
                        {t('Do not test this group')}
                      </SelectItem>
                      {(modelsByGroup.get(group) ?? []).map((model) => (
                        <SelectItem key={model} value={model}>
                          {model}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              ))}
            </div>
          )}
          {missing.length > 0 ? (
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
                {missing.map((target) => (
                  <li key={`${target.group}/${target.model}`}>
                    {target.group} · {target.model}
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
          {ownerName ? (
            <p
              className='text-muted-foreground text-xs'
              data-testid='pelican-config-owner'
            >
              {t('Usage from the sweep is recorded on {{name}}.', {
                name: ownerName,
              })}
            </p>
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
            value={draft.intervalMinutes}
            aria-invalid={intervalValid ? undefined : true}
            onChange={(event) =>
              setEditing({
                models: draft.models,
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
        'Choose the model each group should be probed with, and how often the sweep runs.'
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
              missing.length > 0 ||
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
