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
import { ShieldCheck } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import { ConfirmDialog } from '@/components/confirm-dialog'
import { EmptyState } from '@/components/empty-state'
import { ErrorState } from '@/components/error-state'
import { LoadingState } from '@/components/loading-state'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'

import {
  CONNECT_AUTHORIZATIONS_QUERY_KEY,
  getConnectAuthorizations,
  revokeConnectAuthorization,
} from '../api'
import { getConnectErrorMessage } from '../lib/error'
import type { ConnectAuthorization } from '../types'
import { ConnectAppLogo } from './connect-app-logo'

export function ConnectAuthorizationsTab() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [revoking, setRevoking] = useState<ConnectAuthorization | null>(null)

  const authorizationsQuery = useQuery({
    queryKey: CONNECT_AUTHORIZATIONS_QUERY_KEY,
    queryFn: getConnectAuthorizations,
    meta: { errorToast: false },
  })

  const revokeMutation = useMutation({
    mutationFn: (authorization: ConnectAuthorization) =>
      revokeConnectAuthorization(authorization.client_id),
    onSuccess: () => {
      toast.success(t('Authorization revoked'))
      setRevoking(null)
      queryClient.invalidateQueries({
        queryKey: CONNECT_AUTHORIZATIONS_QUERY_KEY,
      })
    },
    onError: (error: unknown) => {
      toast.error(
        getConnectErrorMessage(error, t('Failed to revoke the authorization'))
      )
    },
  })

  const authorizations = authorizationsQuery.data ?? []
  if (authorizationsQuery.isPending) return <LoadingState />
  if (authorizationsQuery.isError) {
    return (
      <ErrorState
        description={getConnectErrorMessage(
          authorizationsQuery.error,
          t('Failed to load authorizations')
        )}
        onRetry={() => {
          void authorizationsQuery.refetch()
        }}
      />
    )
  }
  if (authorizations.length === 0) {
    return (
      <EmptyState
        icon={ShieldCheck}
        title={t('No authorizations yet')}
        description={t(
          'Applications you authorize with Connect are listed here.'
        )}
      />
    )
  }

  return (
    <div className='grid gap-3'>
      {authorizations.map((authorization) => (
        <Card key={authorization.client_id}>
          <CardHeader>
            <div className='flex min-w-0 items-start gap-3'>
              <ConnectAppLogo
                name={authorization.name}
                logoUrl={authorization.logo_url}
              />
              <div className='min-w-0 flex-1 space-y-1'>
                <div className='flex flex-wrap items-center gap-2'>
                  <CardTitle className='truncate'>
                    {authorization.name}
                  </CardTitle>
                  <Badge variant='outline'>{authorization.scope}</Badge>
                </div>
                {authorization.description !== '' && (
                  <p className='text-muted-foreground text-sm'>
                    {authorization.description}
                  </p>
                )}
                <p className='text-muted-foreground text-xs'>
                  {t('First authorized at')} {authorization.first_granted_at}
                </p>
                <p className='text-muted-foreground text-xs'>
                  {t('Last used at')} {authorization.last_used_at}
                </p>
              </div>
            </div>
          </CardHeader>
          <CardContent className='flex justify-end'>
            <Button
              type='button'
              variant='outline'
              size='sm'
              className='text-destructive'
              onClick={() => setRevoking(authorization)}
            >
              {t('Revoke')}
            </Button>
          </CardContent>
        </Card>
      ))}

      <ConfirmDialog
        open={revoking !== null}
        onOpenChange={(open) => {
          if (!open && !revokeMutation.isPending) setRevoking(null)
        }}
        title={t('Revoke authorization?')}
        desc={
          revoking === null
            ? ''
            : t('"{{name}}" will lose access to your account information.', {
                name: revoking.name,
              })
        }
        confirmText={t('Revoke authorization')}
        destructive
        isLoading={revokeMutation.isPending}
        handleConfirm={() => {
          if (revoking !== null) revokeMutation.mutate(revoking)
        }}
      />
    </div>
  )
}
