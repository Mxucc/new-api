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
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { handleServerError } from '@/lib/handle-server-error'
import { useAuthStore } from '@/stores/auth-store'

import {
  checkPelicanMagpieLink,
  getPelicanConfig,
  updatePelicanConfig,
} from '../api'
import { iqVerdictLabelKey } from '../lib/iq'
import type {
  ModelIqVerdict,
  PelicanMagpieCheck,
  PelicanConfig,
  PelicanMagpieGroup,
  PelicanMagpieGroupOption,
  PelicanTarget,
} from '../types'

const MAX_INTERVAL_MINUTES = 1440

/** 「这个分组不探测」在下拉里的取值：空值当选项值在部分选择器上不可靠，单独给个哨兵。 */
const NO_MODEL = '__none__'

/** 「不调整」在下拉里的哨兵：空值当选项值在部分选择器上不可靠。 */
const NO_MAGPIE_GROUP = '__none__'

/** 冷却上限跟后端一致（一周），超出范围保存会被服务端拒绝。 */
const MAX_MAGPIE_COOLDOWN_MINUTES = 10080
// 与服务端 model.PelicanMagpieDefaultReuse 一致：联动开着时默认复用一小时内的结果。
const DEFAULT_MAGPIE_REUSE_MINUTES = 60

type PelicanConfigDraft = {
  /** 分组 -> 模型；不在表里的分组就是不探测。 */
  models: Record<string, string>
  intervalMinutes: string
  /** 题面、交付约定与页面说明；空字符串表示用内置默认。 */
  prompt: string
  promptContract: string
  description: string
  /** magpie 联动：地址、token（留空表示沿用已保存的）与分组映射。 */
  magpieUrl: string
  magpieToken: string
  magpieTokenClear: boolean
  magpieCooldownMinutes: string
  magpieDryRun: boolean
  /** 复用 magpie-ops 结果的窗口；空字符串表示跟随默认。 */
  magpieReuseMinutes: string
  /** new-api 分组 -> magpie 分组 id；空字符串表示这个分组不调整。 */
  magpieGroups: Record<string, string>
}

export interface PelicanConfigDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

function draftFromConfig(
  config: PelicanConfig | undefined
): PelicanConfigDraft {
  const models: Record<string, string> = {}
  for (const target of config?.targets ?? []) {
    models[target.group] = target.model
  }
  const magpieGroups: Record<string, string> = {}
  for (const mapping of config?.magpie_groups ?? []) {
    magpieGroups[mapping.group] = mapping.magpie_group
  }
  return {
    models,
    intervalMinutes: String(config?.interval_minutes ?? 0),
    prompt: config?.prompt ?? '',
    promptContract: config?.prompt_contract ?? '',
    description: config?.description ?? '',
    magpieUrl: config?.magpie_url ?? '',
    // token 不回显：输入框永远从空开始，留空表示沿用已保存的那个。
    magpieToken: '',
    magpieTokenClear: false,
    magpieCooldownMinutes: String(config?.magpie_cooldown_minutes ?? 30),
    magpieDryRun: config?.magpie_dry_run ?? false,
    magpieReuseMinutes:
      config?.magpie_reuse_minutes === null ||
      config?.magpie_reuse_minutes === undefined
        ? ''
        : String(config.magpie_reuse_minutes),
    magpieGroups,
  }
}

/**
 * 管理员设置自动巡视：每个分组探测一个模型，以及多久跑一轮。
 * 分组与模型取自后端给出的当前可用列表，避免存进永远测不出结果的条目；
 * 用量记在保存这份设置的管理员名下，弹窗里写明记给谁。
 */
export function PelicanConfigDialog(props: PelicanConfigDialogProps) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const currentUsername = useAuthStore(
    (state) => state.auth.user?.username ?? ''
  )
  const [editing, setEditing] = useState<PelicanConfigDraft | null>(null)

  const query = useQuery({
    queryKey: ['model-iq', 'config'],
    queryFn: getPelicanConfig,
    enabled: props.open,
  })

  const config = query.data
  const initialDraft = useMemo(() => draftFromConfig(config), [config])
  const draft = editing ?? initialDraft
  // 任何一处改动都在当前草稿上打补丁，别的字段不会被清空。
  const patchDraft = (patch: Partial<PelicanConfigDraft>) =>
    setEditing({ ...draft, ...patch })

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

  // 要联动的分组 = 这一轮真正会探测的分组：映射按分组逐行填，与模型无关。
  // 「测试连接」的结果：当场看清是地址不对、token 不对，还是版本太旧
  const [magpieCheck, setMagpieCheck] = useState<PelicanMagpieCheck | null>(null)
  const [magpieChecking, setMagpieChecking] = useState(false)

  const magpieRows = useMemo(() => {
    const rows: string[] = []
    for (const group of groups) {
      if ((draft.models[group] ?? '') !== '') {
        rows.push(group)
      }
    }
    return rows
  }, [draft.models, groups])

  // 下拉里的候选：magpie-ops 给出的分组，外加已经保存过、但这次没列出来的那个。
  const magpieOptions = useMemo(() => {
    const options = config?.magpie_groups_available ?? []
    const known = new Set(options.map((option) => option.id))
    const saved: PelicanMagpieGroup[] = config?.magpie_groups ?? []
    const extras = saved
      .filter((mapping) => !known.has(mapping.magpie_group))
      .map((mapping) => ({
        id: mapping.magpie_group,
        name: mapping.magpie_group,
        members: [],
      }))
    return [...options, ...extras]
  }, [config])

  // 两边都在巡检就是重复探测、重复花钱：选中的 magpie 分组开了策略就提示一句。
  const magpiePolicyInterval = useMemo(() => {
    const selected = new Set(
      magpieRows
        .map((group) => draft.magpieGroups[group] ?? '')
        .filter((id) => id !== '')
    )
    for (const option of config?.magpie_groups_available ?? []) {
      if (selected.has(option.id) && option.policyEnabled) {
        return option.intervalMinutes ?? 0
      }
    }
    return null
  }, [config, draft.magpieGroups, magpieRows])

  const magpieOptionById = useMemo(() => {
    const map = new Map<string, PelicanMagpieGroupOption>()
    for (const option of magpieOptions) {
      map.set(option.id, option)
    }
    return map
  }, [magpieOptions])

  const magpieReuseRaw = draft.magpieReuseMinutes.trim()
  const magpieReuseNumber = Number(magpieReuseRaw)
  const magpieReuseValid =
    magpieReuseRaw === '' ||
    (Number.isInteger(magpieReuseNumber) &&
      magpieReuseNumber >= 0 &&
      magpieReuseNumber <= MAX_MAGPIE_COOLDOWN_MINUTES)
  const magpieCooldownNumber = Number(draft.magpieCooldownMinutes)
  const magpieCooldownValid =
    Number.isInteger(magpieCooldownNumber) &&
    magpieCooldownNumber >= 1 &&
    magpieCooldownNumber <= MAX_MAGPIE_COOLDOWN_MINUTES
  const magpieUrlValue = draft.magpieUrl.trim()
  const magpieUrlValid =
    magpieUrlValue === '' ||
    magpieUrlValue.startsWith('http://') ||
    magpieUrlValue.startsWith('https://')
  // token 只写不读：输入框为空时靠「已保存」的那个，清除要显式勾选。
  const magpieTokenReady =
    draft.magpieToken.trim() !== '' ||
    (config?.magpie_token_set === true && !draft.magpieTokenClear)
  const magpieMappedGroups = magpieRows.filter(
    (group) => (draft.magpieGroups[group] ?? '') !== ''
  )
  // 开了自动调整就必须填全：地址、token 与至少一个分组映射，缺一个都跑不起来。
  const magpieReady =
    magpieMappedGroups.length === 0 ||
    (magpieUrlValue !== '' && magpieTokenReady)
  const canSave =
    intervalValid &&
    missing.length === 0 &&
    magpieCooldownValid &&
    magpieReuseValid &&
    magpieUrlValid &&
    magpieReady
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
    patchDraft({
      models: { ...draft.models, [group]: value === NO_MODEL ? '' : value },
    })
  }

  const handleMagpieGroupChange = (group: string, value: string) => {
    patchDraft({
      magpieGroups: {
        ...draft.magpieGroups,
        [group]: value === NO_MAGPIE_GROUP ? '' : value,
      },
    })
  }

  const handleSave = () => {
    if (!canSave) {
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
    // 只提交选好 magpie 分组的行：没选就是这个分组不调整。
    const magpieGroups: PelicanMagpieGroup[] = []
    for (const group of magpieMappedGroups) {
      magpieGroups.push({
        group,
        magpie_group: draft.magpieGroups[group] ?? '',
      })
    }
    save.mutate({
      targets,
      interval_minutes: intervalNumber,
      prompt: draft.prompt,
      prompt_contract: draft.promptContract,
      description: draft.description,
      magpie_url: draft.magpieUrl.trim(),
      magpie_token: draft.magpieToken.trim(),
      magpie_token_clear: draft.magpieTokenClear,
      magpie_groups: magpieGroups,
      magpie_cooldown_minutes: magpieCooldownNumber,
      magpie_dry_run: draft.magpieDryRun,
      magpie_reuse_minutes: magpieReuseRaw === '' ? null : magpieReuseNumber,
    })
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
          {config?.magpie_read_only ? (
            <p
              className='text-muted-foreground text-xs'
              data-testid='pelican-read-only-note'
            >
              {t(
                'Bound to magpie-ops: the model to test comes from magpie-ops, so there is nothing to pick here. new-api will not probe on its own.'
              )}
            </p>
          ) : (
            <p className='text-muted-foreground text-xs'>
              {t(
                'Pick one model per group. The sweep only tests the groups listed here.'
              )}
            </p>
          )}
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
              patchDraft({ intervalMinutes: event.target.value })
            }
          />
          <p className='text-muted-foreground text-xs'>
            {intervalValid
              ? t('0 disables the scheduled sweep.')
              : t('Interval must be a whole number between 0 and 1440.')}
          </p>
        </div>

        <div className='space-y-2'>
          <Label htmlFor='pelican-probe-prompt'>{t('Probe prompt')}</Label>
          <Textarea
            id='pelican-probe-prompt'
            rows={3}
            value={draft.prompt}
            placeholder={config?.default_prompt}
            onChange={(event) => patchDraft({ prompt: event.target.value })}
          />
          <p className='text-muted-foreground text-xs'>
            {t('Leave empty to use the built-in question.')}
          </p>
        </div>

        <div className='space-y-2'>
          <Label htmlFor='pelican-probe-contract'>
            {t('Delivery instruction')}
          </Label>
          <Textarea
            id='pelican-probe-contract'
            rows={2}
            value={draft.promptContract}
            placeholder={config?.default_contract}
            onChange={(event) =>
              patchDraft({ promptContract: event.target.value })
            }
          />
          <p className='text-muted-foreground text-xs'>
            {t(
              'Appended to the prompt. Leave empty to use the built-in instruction.'
            )}
          </p>
        </div>

        <div className='space-y-2'>
          <Label htmlFor='pelican-probe-description'>
            {t('Description shown to users')}
          </Label>
          <Textarea
            id='pelican-probe-description'
            rows={2}
            value={draft.description}
            placeholder={config?.default_description}
            onChange={(event) =>
              patchDraft({ description: event.target.value })
            }
          />
          <p className='text-muted-foreground text-xs'>
            {t('Leave empty to use the translated default description.')}
          </p>
        </div>

        {/* magpie 联动：巡视判出降智后，请 magpie-ops 现场调整映射到的那个分组。 */}
        <div className='space-y-2' data-testid='pelican-magpie-section'>
          <Label>{t('Magpie integration')}</Label>
          <p className='text-muted-foreground text-xs'>
            {t(
              'The sweep asks magpie-ops to adjust the mapped magpie group when a probe comes back degraded. Leave the URL empty to keep the two systems independent.'
            )}
          </p>
          <Input
            aria-label={t('magpie-ops URL')}
            placeholder='http://127.0.0.1:7788'
            value={draft.magpieUrl}
            aria-invalid={magpieUrlValid ? undefined : true}
            onChange={(event) => patchDraft({ magpieUrl: event.target.value })}
          />
          {magpieUrlValid ? null : (
            <p className='text-destructive text-xs'>
              {t('The address must start with http:// or https://.')}
            </p>
          )}
          <div className='flex flex-wrap items-center gap-2'>
            <Button
              type='button'
              variant='outline'
              size='sm'
              disabled={magpieChecking || !magpieUrlValid || !magpieTokenReady}
              onClick={async () => {
                setMagpieChecking(true)
                try {
                  setMagpieCheck(await checkPelicanMagpieLink())
                } catch (error) {
                  handleServerError(error, t)
                } finally {
                  setMagpieChecking(false)
                }
              }}
            >
              {magpieChecking ? t('Loading') : t('Test the magpie-ops link')}
            </Button>
            {magpieCheck ? (
              <span
                className={
                  magpieCheck.ok
                    ? 'text-muted-foreground text-xs'
                    : 'text-destructive text-xs'
                }
                data-testid='pelican-magpie-check'
              >
                {magpieCheck.ok
                  ? `${magpieCheck.version ?? ''} · ${
                      magpieCheck.groups ?? 0
                    } groups${magpieCheck.best ? ' · /api/best' : ' · no /api/best'}`
                  : (magpieCheck.error ??
                    magpieCheck.groups_error ??
                    magpieCheck.best_error ??
                    'failed')}
              </span>
            ) : null}
          </div>
          <Input
            aria-label={t('API token')}
            type='password'
            autoComplete='off'
            value={draft.magpieToken}
            placeholder={
              config?.magpie_token_set
                ? t('A token is saved. Leave empty to keep it.')
                : t('API token')
            }
            onChange={(event) =>
              patchDraft({ magpieToken: event.target.value })
            }
          />
          {config?.magpie_token_set ? (
            <div className='flex items-center gap-2'>
              <Checkbox
                id='pelican-magpie-token-clear'
                checked={draft.magpieTokenClear}
                onCheckedChange={(checked) =>
                  patchDraft({ magpieTokenClear: checked === true })
                }
              />
              <Label
                htmlFor='pelican-magpie-token-clear'
                className='text-muted-foreground text-xs font-normal'
              >
                {t('Clear the saved token')}
              </Label>
            </div>
          ) : null}
          {config?.magpie_groups_error ? (
            <p
              className='text-destructive text-xs'
              data-testid='pelican-magpie-groups-error'
            >
              {t('magpie groups could not be listed: {{message}}', {
                message: config.magpie_groups_error,
              })}
            </p>
          ) : null}
          {magpieRows.length > 0 ? (
            <div className='space-y-2'>
              <Label>{t('magpie group to adjust')}</Label>
              <p className='text-muted-foreground text-xs'>
                {t(
                  'Pick the magpie group each new-api group should adjust. magpie-ops decides which member to change.'
                )}
              </p>
              <p
                className='text-muted-foreground text-xs'
                data-testid='pelican-magpie-one-model'
              >
                {t(
                  'magpie-ops measures one model per channel: the number after a group is how many of its members are actually probed.'
                )}
              </p>
              <p
                className='text-muted-foreground text-xs'
                data-testid='pelican-magpie-excluded'
              >
                {t(
                  'A channel marked \u201cnot automated\u201d in magpie-ops is never adjusted here, and is only probed if magpie-ops is told to keep testing it.'
                )}
              </p>
              {magpieRows.map((group) => {
                const option = magpieOptionById.get(
                  draft.magpieGroups[group] ?? ''
                )
                return (
                  <div
                    key={group}
                    className='space-y-1'
                    data-testid={`pelican-magpie-group-${group}`}
                  >
                    <div className='flex items-center gap-2'>
                      <div className='w-28 min-w-0 shrink-0'>
                        <GroupBadge group={group} copyable={false} />
                      </div>
                      <Select
                        value={draft.magpieGroups[group] || NO_MAGPIE_GROUP}
                        onValueChange={(value) =>
                          handleMagpieGroupChange(group, value ?? '')
                        }
                      >
                        <SelectTrigger
                          className='w-full'
                          aria-label={t('magpie group for {{group}}', {
                            group,
                          })}
                        >
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value={NO_MAGPIE_GROUP}>
                            {t('Do not adjust')}
                          </SelectItem>
                          {magpieOptions.map((item) => (
                            <SelectItem key={item.id} value={item.id}>
                              {item.name === item.id
                                ? item.id
                                : `${item.name} (${item.id})`}
                              {item.probeRefs && item.probeRefs.length > 0
                                ? ` · ${item.probeRefs.length}/${item.members.length}`
                                : ''}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>
                    {/* 选中的分组里有什么：渠道、判定，以及是不是本工具关掉的。 */}
                    {option && option.members.length > 0 ? (
                      <ul className='text-muted-foreground space-y-0.5 ps-[7.5rem] text-[10px]'>
                        {option.members.slice(0, 4).map((member) => (
                          <li key={member.ref} className='truncate'>
                            {[
                              member.ref,
                              `${t(
                                iqVerdictLabelKey(
                                  (member.verdict ??
                                    'pending') as ModelIqVerdict
                                )
                              )}${
                                member.score === undefined
                                  ? ''
                                  : ` ${member.score}`
                              }`,
                              member.rate === undefined
                                ? ''
                                : `${t('Multiplier')} ${member.rate.toFixed(2)}×`,
                              member.disabled
                                ? t(
                                    member.disabledByUs
                                      ? 'closed (ours)'
                                      : 'closed (manual)'
                                  )
                                : '',
                            ]
                              .filter((part) => part !== '')
                              .join(' · ')}
                          </li>
                        ))}
                      </ul>
                    ) : null}
                    {/* magpie-ops 那边这个分组 24 小时内的最高分：降智也算，
                        只要分最高——new-api 要看的是这个分组最好能做到什么。 */}
                    {(() => {
                      const mapped = draft.magpieGroups[group] ?? ''
                      const best = config?.magpie_best?.[mapped]
                      if (!best || !best.best) return null
                      return (
                        <p
                          className='text-muted-foreground ps-[7.5rem] text-[10px]'
                          data-testid={`pelican-magpie-best-${group}`}
                        >
                          {`best ${best.best.score} · ${best.best.ref} · ${best.hours}h · n=${best.considered}`}
                        </p>
                      )
                    })()}
                  </div>
                )
              })}
            </div>
          ) : null}
          {magpiePolicyInterval === null ? null : (
            <p
              className='text-destructive text-xs'
              data-testid='pelican-magpie-policy-warning'
            >
              {magpiePolicyInterval > 0
                ? t(
                    'magpie is also sweeping this group every {{minutes}} minutes: both sides running will probe twice.',
                    { minutes: magpiePolicyInterval }
                  )
                : t(
                    'magpie is also sweeping this group on its own: both sides running will probe twice.'
                  )}
            </p>
          )}
          <div className='grid grid-cols-2 items-end gap-2'>
            <div className='space-y-1'>
              <Label
                htmlFor='pelican-magpie-cooldown'
                className='text-muted-foreground text-xs font-normal'
              >
                {t('Minutes between two adjustments of the same group')}
              </Label>
              <Input
                id='pelican-magpie-cooldown'
                type='number'
                min={1}
                max={MAX_MAGPIE_COOLDOWN_MINUTES}
                value={draft.magpieCooldownMinutes}
                aria-invalid={magpieCooldownValid ? undefined : true}
                onChange={(event) =>
                  patchDraft({ magpieCooldownMinutes: event.target.value })
                }
              />
            </div>
            <div className='flex items-center gap-2 pb-1.5'>
              <Checkbox
                id='pelican-magpie-dry-run'
                checked={draft.magpieDryRun}
                onCheckedChange={(checked) =>
                  patchDraft({ magpieDryRun: checked === true })
                }
              />
              <Label
                htmlFor='pelican-magpie-dry-run'
                className='text-muted-foreground text-xs font-normal'
              >
                {t('Dry run: only log what would be adjusted')}
              </Label>
            </div>
          </div>
          {magpieCooldownValid ? null : (
            <p className='text-destructive text-xs'>
              {t(
                'The cooldown must be a whole number from 1 to 10080 minutes.'
              )}
            </p>
          )}
          <div className='space-y-1'>
            <Label
              htmlFor='pelican-magpie-reuse'
              className='text-muted-foreground text-xs font-normal'
            >
              {t(
                'Reuse its result within (minutes); leave empty for the default, 0 to always test here'
              )}
            </Label>
            <Input
              id='pelican-magpie-reuse'
              type='number'
              min={0}
              max={MAX_MAGPIE_COOLDOWN_MINUTES}
              placeholder={String(DEFAULT_MAGPIE_REUSE_MINUTES)}
              value={draft.magpieReuseMinutes}
              aria-invalid={magpieReuseValid ? undefined : true}
              onChange={(event) =>
                patchDraft({ magpieReuseMinutes: event.target.value })
              }
            />
            <p className='text-muted-foreground text-xs'>
              {t(
                'When magpie-ops already probed this group a moment ago, the IQ test reuses its answer instead of paying for the same question twice.'
              )}
            </p>
          </div>
          {magpieReuseValid ? null : (
            <p className='text-destructive text-xs'>
              {t('The reuse window must be a whole number of minutes.')}
            </p>
          )}
          {magpieReady ? null : (
            <p className='text-destructive text-xs'>
              {t('Adjusting needs the URL and the token.')}
            </p>
          )}
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
            disabled={save.isPending || !canSave || query.isLoading}
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
