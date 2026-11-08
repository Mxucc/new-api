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
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { StatusBadge } from '@/components/status-badge'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'

import { AI_BRIDGE_FIELD_LABEL_KEYS } from '../constants'
import type { AiBridgeConfigField } from '../types'

function fieldLabel(
  t: (key: string) => string,
  field: AiBridgeConfigField
): string {
  return t(AI_BRIDGE_FIELD_LABEL_KEYS[field.key] ?? field.key)
}

interface ConfigFieldControlProps {
  field: AiBridgeConfigField
  value: string
  disabled?: boolean
  onValueChange: (value: string) => void
}

export function ConfigFieldControl(props: ConfigFieldControlProps) {
  const { t } = useTranslation()
  const field = props.field
  const label = fieldLabel(t, field)
  const inputId = `ai-bridge-config-${field.key}`
  let control: ReactNode

  if (field.kind === 'boolean') {
    control = (
      <Switch
        id={inputId}
        checked={props.value === 'true'}
        disabled={props.disabled}
        onCheckedChange={(checked) =>
          props.onValueChange(checked ? 'true' : 'false')
        }
      />
    )
  } else if (field.kind === 'select') {
    control = (
      <Select
        items={field.options.map((option) => ({
          value: option,
          label: option,
        }))}
        value={props.value}
        disabled={props.disabled}
        onValueChange={props.onValueChange}
      >
        <SelectTrigger className='w-full max-w-72' aria-label={label}>
          <SelectValue placeholder={label} />
        </SelectTrigger>
        <SelectContent alignItemWithTrigger={false}>
          {field.options.map((option) => (
            <SelectItem key={option} value={option}>
              {option}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    )
  } else if (field.kind === 'json') {
    control = (
      <Textarea
        id={inputId}
        className='font-mono text-xs'
        rows={4}
        spellCheck={false}
        value={props.value}
        disabled={props.disabled}
        onChange={(event) => props.onValueChange(event.target.value)}
      />
    )
  } else {
    control = (
      <Input
        id={inputId}
        type={field.kind === 'number' ? 'number' : 'text'}
        min={field.min ?? undefined}
        max={field.max ?? undefined}
        spellCheck={false}
        value={props.value}
        disabled={props.disabled}
        onChange={(event) => props.onValueChange(event.target.value)}
      />
    )
  }

  const rangeEnabled =
    field.kind === 'number' && field.min != null && field.max != null

  return (
    <div className='space-y-2'>
      <div className='flex flex-wrap items-center gap-2'>
        <Label htmlFor={field.kind === 'select' ? undefined : inputId}>
          {label}
        </Label>
        {field.overridden && (
          <StatusBadge
            label={t('Overridden')}
            variant='info'
            copyable={false}
          />
        )}
      </div>
      {control}
      {rangeEnabled && (
        <p className='text-muted-foreground text-xs'>
          {t('Allowed range: {{min}} to {{max}}', {
            min: field.min,
            max: field.max,
          })}
        </p>
      )}
    </div>
  )
}
