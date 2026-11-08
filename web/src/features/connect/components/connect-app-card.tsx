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
import { Pencil, Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'

import type { ConnectClient } from '../types'
import { ConnectAppLogo } from './connect-app-logo'
import { ConnectCredentials } from './connect-credentials'

interface ConnectAppCardProps {
  client: ConnectClient
  /** Administrators see every application, so each card names its owner. */
  showOwner: boolean
  onEdit: (client: ConnectClient) => void
  onDelete: (client: ConnectClient) => void
}

export function ConnectAppCard(props: ConnectAppCardProps) {
  const { t } = useTranslation()
  const client = props.client

  return (
    <Card>
      <CardHeader>
        <div className='flex min-w-0 items-start gap-3'>
          <ConnectAppLogo name={client.name} logoUrl={client.logo_url} />
          <div className='min-w-0 flex-1 space-y-1'>
            <div className='flex flex-wrap items-center gap-2'>
              <CardTitle className='truncate'>{client.name}</CardTitle>
              {props.showOwner && (
                <Badge variant='secondary'>
                  {client.owner_name || `#${client.owner_id}`}
                </Badge>
              )}
            </div>
            <CardDescription>
              {t('Created at')} {client.created_at}
            </CardDescription>
          </div>
        </div>
      </CardHeader>
      <CardContent className='space-y-4'>
        <ConnectCredentials
          clientId={client.id}
          clientSecret={client.client_secret}
        />
        <dl className='grid gap-2 text-sm'>
          <div className='flex flex-wrap gap-x-2'>
            <dt className='text-muted-foreground w-24 shrink-0 text-xs'>
              {t('Homepage')}
            </dt>
            <dd className='min-w-0'>
              <a
                href={client.homepage_url}
                target='_blank'
                rel='noreferrer'
                className='text-primary break-all hover:underline'
              >
                {client.homepage_url}
              </a>
            </dd>
          </div>
          <div className='flex flex-wrap gap-x-2'>
            <dt className='text-muted-foreground w-24 shrink-0 text-xs'>
              {t('Redirect URI')}
            </dt>
            <dd className='min-w-0'>
              <code className='font-mono text-xs break-all'>
                {client.redirect_uri}
              </code>
            </dd>
          </div>
          {client.description !== '' && (
            <div className='flex flex-wrap gap-x-2'>
              <dt className='text-muted-foreground w-24 shrink-0 text-xs'>
                {t('Description')}
              </dt>
              <dd className='min-w-0'>{client.description}</dd>
            </div>
          )}
        </dl>
      </CardContent>
      <CardFooter className='justify-end gap-2'>
        <Button
          type='button'
          variant='outline'
          size='sm'
          onClick={() => props.onEdit(client)}
        >
          <Pencil className='size-4' aria-hidden='true' />
          {t('Edit')}
        </Button>
        <Button
          type='button'
          variant='outline'
          size='sm'
          className='text-destructive'
          onClick={() => props.onDelete(client)}
        >
          <Trash2 className='size-4' aria-hidden='true' />
          {t('Delete')}
        </Button>
      </CardFooter>
    </Card>
  )
}
