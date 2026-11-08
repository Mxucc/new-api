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

import { CopyButton } from '@/components/copy-button'
import { MaskedValueDisplay } from '@/components/masked-value-display'

import { maskConnectSecret } from '../lib/secret'

interface ConnectCredentialsProps {
  clientId: string
  clientSecret: string
  /** The create response is the only time the secret may be shown in full. */
  revealSecret?: boolean
}

/** Client ID and Client Secret with copy support and a masked secret. */
export function ConnectCredentials(props: ConnectCredentialsProps) {
  const { t } = useTranslation()

  let secretContent: ReactNode
  if (!props.clientSecret) {
    secretContent = (
      <span className='text-muted-foreground text-xs'>
        {t('Legacy app, please register it again')}
      </span>
    )
  } else if (props.revealSecret) {
    secretContent = (
      <span className='flex min-w-0 items-start gap-1'>
        <code className='min-w-0 font-mono text-xs break-all'>
          {props.clientSecret}
        </code>
        <CopyButton
          value={props.clientSecret}
          className='size-7'
          iconClassName='size-3.5'
          tooltip={t('Copy Client Secret')}
          aria-label={t('Copy Client Secret')}
        />
      </span>
    )
  } else {
    secretContent = (
      <MaskedValueDisplay
        label={t('Client Secret')}
        fullValue={props.clientSecret}
        maskedValue={maskConnectSecret(props.clientSecret)}
        copyTooltip={t('Copy Client Secret')}
        copyAriaLabel={t('Copy Client Secret')}
      />
    )
  }

  return (
    <dl className='grid gap-3 sm:grid-cols-2'>
      <div className='min-w-0 space-y-1'>
        <dt className='text-muted-foreground text-xs font-medium'>
          {t('Client ID')}
        </dt>
        <dd className='flex min-w-0 items-center gap-1'>
          <code className='truncate font-mono text-xs'>{props.clientId}</code>
          <CopyButton
            value={props.clientId}
            className='size-7'
            iconClassName='size-3.5'
            tooltip={t('Copy Client ID')}
            aria-label={t('Copy Client ID')}
          />
        </dd>
      </div>
      <div className='min-w-0 space-y-1'>
        <dt className='text-muted-foreground text-xs font-medium'>
          {t('Client Secret')}
        </dt>
        <dd className='min-w-0'>{secretContent}</dd>
      </div>
    </dl>
  )
}
