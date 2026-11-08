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
import { AppWindow, Plus } from 'lucide-react'
import { useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import { ConfirmDialog } from '@/components/confirm-dialog'
import { EmptyState } from '@/components/empty-state'
import { ErrorState } from '@/components/error-state'
import { LoadingState } from '@/components/loading-state'
import { Button } from '@/components/ui/button'

import {
  CONNECT_CLIENTS_QUERY_KEY,
  deleteConnectClient,
  getConnectClients,
} from '../api'
import { getConnectErrorMessage } from '../lib/error'
import type { ConnectClient, CreatedConnectClient } from '../types'
import { ConnectAppCard } from './connect-app-card'
import { ConnectAppFormDialog } from './connect-app-form-dialog'
import { ConnectCreatedClientDialog } from './connect-created-client-dialog'

interface ConnectAppsTabProps {
  /** Administrators see every application and each card names its owner. */
  showOwner: boolean
}

export function ConnectAppsTab(props: ConnectAppsTabProps) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [formOpen, setFormOpen] = useState(false)
  const [editing, setEditing] = useState<ConnectClient | null>(null)
  const [deleting, setDeleting] = useState<ConnectClient | null>(null)
  const [created, setCreated] = useState<CreatedConnectClient | null>(null)

  const clientsQuery = useQuery({
    queryKey: CONNECT_CLIENTS_QUERY_KEY,
    queryFn: getConnectClients,
    meta: { errorToast: false },
  })

  const deleteMutation = useMutation({
    mutationFn: (client: ConnectClient) => deleteConnectClient(client.id),
    onSuccess: () => {
      toast.success(t('Application deleted'))
      setDeleting(null)
      queryClient.invalidateQueries({ queryKey: CONNECT_CLIENTS_QUERY_KEY })
    },
    onError: (error: unknown) => {
      toast.error(
        getConnectErrorMessage(error, t('Failed to delete the application'))
      )
    },
  })

  const openCreate = () => {
    setEditing(null)
    setFormOpen(true)
  }
  const openEdit = (client: ConnectClient) => {
    setEditing(client)
    setFormOpen(true)
  }

  const clients = clientsQuery.data ?? []
  let content: ReactNode
  if (clientsQuery.isPending) {
    content = <LoadingState />
  } else if (clientsQuery.isError) {
    content = (
      <ErrorState
        description={getConnectErrorMessage(
          clientsQuery.error,
          t('Failed to load applications')
        )}
        onRetry={() => {
          void clientsQuery.refetch()
        }}
      />
    )
  } else if (clients.length === 0) {
    content = (
      <EmptyState
        icon={AppWindow}
        title={t('No applications yet')}
        description={t(
          'Register an application to sign users in with Connect.'
        )}
        action={
          <Button type='button' size='sm' onClick={openCreate}>
            <Plus className='size-4' aria-hidden='true' />
            {t('New application')}
          </Button>
        }
      />
    )
  } else {
    content = (
      <div className='grid gap-3'>
        {clients.map((client) => (
          <ConnectAppCard
            key={client.id}
            client={client}
            showOwner={props.showOwner}
            onEdit={openEdit}
            onDelete={setDeleting}
          />
        ))}
      </div>
    )
  }

  return (
    <div className='space-y-4'>
      <div className='flex flex-wrap items-center justify-between gap-3'>
        <p className='text-muted-foreground text-sm'>
          {t('Applications you registered for Connect sign-in.')}
        </p>
        <Button type='button' size='sm' onClick={openCreate}>
          <Plus className='size-4' aria-hidden='true' />
          {t('New application')}
        </Button>
      </div>

      {content}

      <ConnectAppFormDialog
        open={formOpen}
        onOpenChange={setFormOpen}
        client={editing}
        onCreated={setCreated}
      />

      <ConnectCreatedClientDialog
        client={created}
        onOpenChange={(open) => {
          if (!open) setCreated(null)
        }}
      />

      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(open) => {
          if (!open && !deleteMutation.isPending) setDeleting(null)
        }}
        title={t('Delete application?')}
        desc={
          deleting === null
            ? ''
            : t(
                'The application "{{name}}" stops working immediately and its authorizations are removed.',
                { name: deleting.name }
              )
        }
        confirmText={t('Delete')}
        destructive
        isLoading={deleteMutation.isPending}
        handleConfirm={() => {
          if (deleting !== null) deleteMutation.mutate(deleting)
        }}
      />
    </div>
  )
}
