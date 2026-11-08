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
import { AlertTriangle } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Dialog } from '@/components/dialog'
import { Button } from '@/components/ui/button'

import type { CreatedConnectClient } from '../types'
import { ConnectCredentials } from './connect-credentials'

interface ConnectCreatedClientDialogProps {
  /** `null` closes the dialog. */
  client: CreatedConnectClient | null
  onOpenChange: (open: boolean) => void
}

/** Shows the one-time client secret returned by a successful registration. */
export function ConnectCreatedClientDialog(
  props: ConnectCreatedClientDialogProps
) {
  const { t } = useTranslation()

  return (
    <Dialog
      open={props.client !== null}
      onOpenChange={props.onOpenChange}
      title={t('Application created')}
      description={t('Keep these credentials to finish the OAuth2 setup.')}
      footer={
        <Button type='button' onClick={() => props.onOpenChange(false)}>
          {t('Done')}
        </Button>
      }
    >
      {props.client !== null && (
        <div className='space-y-4'>
          <div className='flex items-start gap-2 rounded-lg border border-amber-500/40 bg-amber-500/10 p-3 text-sm'>
            <AlertTriangle
              className='mt-0.5 size-4 shrink-0 text-amber-600'
              aria-hidden='true'
            />
            <p>{t('Save the client secret now; it is shown only once.')}</p>
          </div>
          <ConnectCredentials
            clientId={props.client.client_id}
            clientSecret={props.client.client_secret}
            revealSecret
          />
          <dl className='grid gap-2 text-sm'>
            <div className='flex flex-wrap gap-x-2'>
              <dt className='text-muted-foreground w-24 shrink-0 text-xs'>
                {t('Application name')}
              </dt>
              <dd className='min-w-0'>{props.client.name}</dd>
            </div>
            <div className='flex flex-wrap gap-x-2'>
              <dt className='text-muted-foreground w-24 shrink-0 text-xs'>
                {t('Homepage')}
              </dt>
              <dd className='min-w-0 break-all'>{props.client.homepage_url}</dd>
            </div>
            <div className='flex flex-wrap gap-x-2'>
              <dt className='text-muted-foreground w-24 shrink-0 text-xs'>
                {t('Redirect URI')}
              </dt>
              <dd className='min-w-0'>
                <code className='font-mono text-xs break-all'>
                  {props.client.redirect_uri}
                </code>
              </dd>
            </div>
          </dl>
        </div>
      )}
    </Dialog>
  )
}
