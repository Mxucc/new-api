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
import { PlugZap } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { EmptyState } from '@/components/empty-state'
import { Button } from '@/components/ui/button'

interface ConnectUnavailableStateProps {
  /** A 401 means the visitor has no session for Connect, not that it is missing. */
  signedOut: boolean
  onRetry: () => void
}

/**
 * Shown when the Connect service cannot be reached at all. It is a deployment
 * state, so it stays a local empty state instead of a stream of error toasts.
 */
export function ConnectUnavailableState(props: ConnectUnavailableStateProps) {
  const { t } = useTranslation()

  const description = props.signedOut
    ? t('Sign in again to manage your Connect applications.')
    : t('The Connect service is not available on this deployment.')

  return (
    <EmptyState
      icon={PlugZap}
      title={t('Connect is unavailable')}
      description={description}
      action={
        <Button
          type='button'
          variant='outline'
          size='sm'
          onClick={props.onRetry}
        >
          {t('Retry')}
        </Button>
      }
    />
  )
}
