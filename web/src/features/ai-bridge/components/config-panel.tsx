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
import { RotateCw, Save } from 'lucide-react'
import { useCallback, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { StaticDataTable } from '@/components/data-table'
import { StatusBadge, type StatusVariant } from '@/components/status-badge'
import { TruncatedText } from '@/components/truncated-text'
import { Button } from '@/components/ui/button'
import { TitledCard } from '@/components/ui/titled-card'
import { toIntlLocale } from '@/i18n/languages'
import { formatNumber, formatTimestampToDate } from '@/lib/format'

import {
  AI_BRIDGE_FILE_KIND_LABEL_KEYS,
  AI_BRIDGE_FILE_STATUS_LABEL_KEYS,
  AI_BRIDGE_GROUP_LABEL_KEYS,
} from '../constants'
import {
  useReloadAiBridgeConfig,
  useSaveAiBridgeConfig,
} from '../hooks/use-ai-bridge-console'
import type {
  AiBridgeConfigField,
  AiBridgeControl,
  AiBridgeFileDiagnostic,
} from '../types'
import { ConfigFieldControl } from './config-field-control'

function fileStatusVariant(status: string): StatusVariant {
  if (status === 'ok') return 'success'
  if (status === 'error') return 'danger'
  return 'warning'
}

function groupConfigFields(fields: AiBridgeConfigField[]) {
  const groups = new Map<string, AiBridgeConfigField[]>()
  for (const field of fields) {
    const existing = groups.get(field.group)
    if (existing) existing.push(field)
    else groups.set(field.group, [field])
  }
  return [...groups.entries()]
}

function collectChangedValues(
  fields: AiBridgeConfigField[],
  drafts: Record<string, string>
) {
  const values: Record<string, string> = {}
  for (const field of fields) {
    const draft = drafts[field.key]
    if (draft !== undefined && draft !== field.value) values[field.key] = draft
  }
  return values
}

interface ConfigPanelProps {
  control: AiBridgeControl
  csrfToken: string
}

export function ConfigPanel(props: ConfigPanelProps) {
  const { t, i18n } = useTranslation()
  const locale = toIntlLocale(i18n.resolvedLanguage || i18n.language)
  const saveConfig = useSaveAiBridgeConfig()
  const reloadConfig = useReloadAiBridgeConfig()
  const [drafts, setDrafts] = useState<Record<string, string>>({})
  const fields = props.control.fields
  const groups = useMemo(() => groupConfigFields(fields), [fields])
  const changedValues = useMemo(
    () => collectChangedValues(fields, drafts),
    [drafts, fields]
  )
  const changedKeys = Object.keys(changedValues)
  const overrideFile = props.control.override_file

  const handleValueChange = useCallback((key: string, value: string) => {
    setDrafts((previous) => ({ ...previous, [key]: value }))
  }, [])

  return (
    <div className='space-y-6'>
      <div className='flex flex-wrap items-center gap-2'>
        <Button
          disabled={changedKeys.length === 0 || saveConfig.isPending}
          onClick={() =>
            saveConfig.mutate(
              { csrfToken: props.csrfToken, values: changedValues },
              { onSuccess: () => setDrafts({}) }
            )
          }
        >
          <Save aria-hidden='true' />
          {t('Save changes')}
        </Button>
        <Button
          variant='outline'
          disabled={reloadConfig.isPending}
          onClick={() => reloadConfig.mutate(props.csrfToken)}
        >
          <RotateCw aria-hidden='true' />
          {t('Reload configuration')}
        </Button>
        <p className='text-muted-foreground text-xs'>
          {changedKeys.length === 0
            ? t('No unsaved changes')
            : t('{{count}} unsaved changes', { count: changedKeys.length })}
        </p>
      </div>

      <TitledCard
        title={t('Override file')}
        description={t(
          'Values saved here are written to the administrator override file.'
        )}
      >
        <dl className='grid gap-3 sm:grid-cols-2'>
          <div className='space-y-1'>
            <dt className='text-muted-foreground text-xs'>{t('Path')}</dt>
            <dd className='font-mono text-xs break-all'>{overrideFile.path}</dd>
          </div>
          <div className='space-y-1'>
            <dt className='text-muted-foreground text-xs'>
              {t('Overridden fields')}
            </dt>
            <dd className='text-sm tabular-nums'>
              {formatNumber(overrideFile.overridden_fields, locale)}
            </dd>
          </div>
          <div className='space-y-1'>
            <dt className='text-muted-foreground text-xs'>
              {t('File exists')}
            </dt>
            <dd className='text-sm'>
              {overrideFile.exists ? t('Yes') : t('No')}
            </dd>
          </div>
          <div className='space-y-1'>
            <dt className='text-muted-foreground text-xs'>{t('Writable')}</dt>
            <dd className='text-sm'>
              {overrideFile.writable ? t('Yes') : t('No')}
            </dd>
          </div>
        </dl>
        {overrideFile.issue != null && (
          <p className='text-destructive mt-3 text-xs' role='alert'>
            {overrideFile.issue}
          </p>
        )}
      </TitledCard>

      <TitledCard
        title={t('Policy files')}
        description={t('Status of the blacklist, whitelist and rules files.')}
      >
        <StaticDataTable<AiBridgeFileDiagnostic>
          columns={[
            {
              id: 'kind',
              header: t('File'),
              cell: (file) =>
                t(AI_BRIDGE_FILE_KIND_LABEL_KEYS[file.kind] ?? file.kind),
            },
            {
              id: 'status',
              header: t('Status'),
              cell: (file) => (
                <StatusBadge
                  label={t(
                    AI_BRIDGE_FILE_STATUS_LABEL_KEYS[file.status] ?? file.status
                  )}
                  variant={fileStatusVariant(file.status)}
                  copyable={false}
                />
              ),
            },
            {
              id: 'path',
              header: t('Path'),
              cell: (file) => (
                <TruncatedText
                  text={file.path}
                  maxWidth='max-w-[320px]'
                  className='font-mono text-xs'
                />
              ),
            },
            {
              id: 'entries',
              header: t('Cached entries'),
              cell: (file) => (
                <span className='tabular-nums'>
                  {formatNumber(file.cached_entries, locale)}
                </span>
              ),
            },
            {
              id: 'size',
              header: t('Size (bytes)'),
              cell: (file) => (
                <span className='tabular-nums'>
                  {formatNumber(file.size_bytes, locale)}
                </span>
              ),
            },
            {
              id: 'modified',
              header: t('Modified at'),
              cell: (file) =>
                file.modified_at_ms == null ? (
                  <span className='text-muted-foreground'>-</span>
                ) : (
                  <span className='tabular-nums'>
                    {formatTimestampToDate(file.modified_at_ms, 'milliseconds')}
                  </span>
                ),
            },
            {
              id: 'issue',
              header: t('Issue'),
              cell: (file) =>
                file.issue == null ? (
                  <span className='text-muted-foreground'>-</span>
                ) : (
                  <TruncatedText
                    text={file.issue}
                    maxWidth='max-w-[280px]'
                    className='text-destructive text-xs'
                  />
                ),
            },
          ]}
          data={props.control.files}
          getRowKey={(file) => file.kind}
          emptyContent={t('No policy files configured')}
        />
      </TitledCard>

      {groups.map(([group, groupFields]) => (
        <TitledCard
          key={group}
          title={t(AI_BRIDGE_GROUP_LABEL_KEYS[group] ?? 'Other')}
          description={t('{{count}} settings', { count: groupFields.length })}
        >
          <div className='grid gap-4 lg:grid-cols-2'>
            {groupFields.map((field) => (
              <ConfigFieldControl
                key={field.key}
                field={field}
                value={drafts[field.key] ?? field.value}
                disabled={saveConfig.isPending || reloadConfig.isPending}
                onValueChange={(value) => handleValueChange(field.key, value)}
              />
            ))}
          </div>
        </TitledCard>
      ))}
    </div>
  )
}
