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
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'

import { SettingsSection } from '../components/settings-section'
import { useUpdateOption } from '../hooks/use-update-option'

type FrontendTheme = 'default' | 'classic'

type FrontendSectionProps = {
  defaultValue: FrontendTheme
}

export function FrontendSection({ defaultValue }: FrontendSectionProps) {
  const { t } = useTranslation()
  const updateOption = useUpdateOption()
  const [value, setValue] = useState<FrontendTheme>(defaultValue)

  const handleValueChange = async (nextValue: FrontendTheme) => {
    if (nextValue === value) return

    setValue(nextValue)
    const response = await updateOption.mutateAsync({
      key: 'theme.frontend',
      value: nextValue,
    })
    if (response.success) {
      window.location.replace('/')
      return
    }
    setValue(value)
  }

  return (
    <SettingsSection title={t('Frontend')}>
      <div className='grid gap-1.5'>
        <Label className='text-sm font-medium'>{t('Theme')}</Label>
        <Select
          items={[
            { value: 'default', label: t('Default') },
            { value: 'classic', label: t('Classic') },
          ]}
          value={value}
          disabled={updateOption.isPending}
          onValueChange={(nextValue) => {
            if (nextValue === 'default' || nextValue === 'classic') {
              void handleValueChange(nextValue)
            }
          }}
        >
          <SelectTrigger className='w-full sm:w-56'>
            <SelectValue />
          </SelectTrigger>
          <SelectContent alignItemWithTrigger={false}>
            <SelectGroup>
              <SelectItem value='default'>{t('Default')}</SelectItem>
              <SelectItem value='classic'>{t('Classic')}</SelectItem>
            </SelectGroup>
          </SelectContent>
        </Select>
      </div>
    </SettingsSection>
  )
}
