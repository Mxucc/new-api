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
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import {
  clearAiBridgeCooldown,
  createAiBridgeListEntry,
  deleteAiBridgeListEntry,
  fetchAiBridgeControl,
  fetchAiBridgeIdentity,
  fetchAiBridgeSnapshot,
  reloadAiBridgeConfig,
  saveAiBridgeConfig,
} from '../api'
import { AI_BRIDGE_SNAPSHOT_REFETCH_INTERVAL_MS } from '../constants'
import {
  readAiBridgeError,
  resolveAiBridgeErrorMessageKey,
} from '../lib/errors'
import type { AiBridgeCooldownKind, AiBridgeListKind } from '../types'

export const aiBridgeQueryKeys = {
  identity: ['ai-bridge', 'me'] as const,
  snapshot: ['ai-bridge', 'snapshot'] as const,
  control: ['ai-bridge', 'control'] as const,
}

/**
 * Identity is requested once: 401/403/503 are page states, not retryable errors.
 * The sidebar passes `enabled` so only administrators probe the console.
 */
export function useAiBridgeIdentity(enabled = true) {
  return useQuery({
    queryKey: aiBridgeQueryKeys.identity,
    queryFn: fetchAiBridgeIdentity,
    enabled,
    retry: false,
    staleTime: 60_000,
    gcTime: 5 * 60_000,
    meta: { errorToast: false },
  })
}

export function useAiBridgeSnapshot(enabled: boolean) {
  return useQuery({
    queryKey: aiBridgeQueryKeys.snapshot,
    queryFn: fetchAiBridgeSnapshot,
    enabled,
    retry: false,
    refetchInterval: AI_BRIDGE_SNAPSHOT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: false,
    meta: { errorToast: false },
  })
}

export function useAiBridgeControl(enabled: boolean) {
  return useQuery({
    queryKey: aiBridgeQueryKeys.control,
    queryFn: fetchAiBridgeControl,
    enabled,
    retry: false,
    refetchInterval: AI_BRIDGE_SNAPSHOT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: false,
    meta: { errorToast: false },
  })
}

interface CooldownInput {
  csrfToken: string
  kind: AiBridgeCooldownKind
  id: string
}

export function useClearAiBridgeCooldown() {
  const client = useQueryClient()
  const { t } = useTranslation()
  return useMutation({
    mutationFn: async (input: CooldownInput) => {
      try {
        return await clearAiBridgeCooldown(
          input.csrfToken,
          input.kind,
          input.id
        )
      } catch (error) {
        // An already expired cooldown is reported as 404; refreshing is enough.
        if (readAiBridgeError(error).code === 'cooldown_not_found') {
          return { ok: true, result: 'cleared' }
        }
        throw error
      }
    },
    onSuccess: () => {
      toast.success(t('Cooldown cleared'))
      void client.invalidateQueries({ queryKey: aiBridgeQueryKeys.snapshot })
    },
    onError: (error) => {
      toast.error(
        t(
          resolveAiBridgeErrorMessageKey(error, 'Unable to clear the cooldown.')
        )
      )
    },
    meta: { errorToast: false },
  })
}

interface ListEntryInput {
  csrfToken: string
  kind: AiBridgeListKind
  value: string
}

export function useCreateAiBridgeListEntry() {
  const client = useQueryClient()
  const { t } = useTranslation()
  return useMutation({
    mutationFn: (input: ListEntryInput) =>
      createAiBridgeListEntry(input.csrfToken, input.kind, input.value),
    onSuccess: () => {
      toast.success(t('List entry added'))
      void client.invalidateQueries({ queryKey: aiBridgeQueryKeys.snapshot })
      void client.invalidateQueries({ queryKey: aiBridgeQueryKeys.control })
    },
    onError: (error) => {
      toast.error(
        t(
          resolveAiBridgeErrorMessageKey(error, 'Unable to add the list entry.')
        )
      )
    },
    meta: { errorToast: false },
  })
}

export function useDeleteAiBridgeListEntry() {
  const client = useQueryClient()
  const { t } = useTranslation()
  return useMutation({
    mutationFn: (input: {
      csrfToken: string
      kind: AiBridgeListKind
      id: string
    }) => deleteAiBridgeListEntry(input.csrfToken, input.kind, input.id),
    onSuccess: () => {
      toast.success(t('List entry removed'))
      void client.invalidateQueries({ queryKey: aiBridgeQueryKeys.snapshot })
      void client.invalidateQueries({ queryKey: aiBridgeQueryKeys.control })
    },
    onError: (error) => {
      toast.error(
        t(
          resolveAiBridgeErrorMessageKey(
            error,
            'Unable to remove the list entry.'
          )
        )
      )
    },
    meta: { errorToast: false },
  })
}

export function useSaveAiBridgeConfig() {
  const client = useQueryClient()
  const { t } = useTranslation()
  return useMutation({
    mutationFn: (input: {
      csrfToken: string
      values: Record<string, string>
    }) => saveAiBridgeConfig(input.csrfToken, input.values),
    onSuccess: (control) => {
      client.setQueryData(aiBridgeQueryKeys.control, control)
      void client.invalidateQueries({ queryKey: aiBridgeQueryKeys.snapshot })
      toast.success(t('Configuration saved'))
    },
    onError: (error) => {
      toast.error(
        t(
          resolveAiBridgeErrorMessageKey(
            error,
            'Unable to save the configuration.'
          )
        )
      )
    },
    meta: { errorToast: false },
  })
}

export function useReloadAiBridgeConfig() {
  const client = useQueryClient()
  const { t } = useTranslation()
  return useMutation({
    mutationFn: (csrfToken: string) => reloadAiBridgeConfig(csrfToken),
    onSuccess: (control) => {
      client.setQueryData(aiBridgeQueryKeys.control, control)
      toast.success(t('Configuration reloaded'))
    },
    onError: (error) => {
      toast.error(
        t(
          resolveAiBridgeErrorMessageKey(
            error,
            'Unable to reload the configuration.'
          )
        )
      )
    },
    meta: { errorToast: false },
  })
}
