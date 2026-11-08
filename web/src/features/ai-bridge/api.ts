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
import { api, type ApiRequestConfig } from '@/lib/api'

import type {
  AiBridgeCooldownKind,
  AiBridgeControl,
  AiBridgeIdentity,
  AiBridgeListKind,
  AiBridgeListMutationResult,
  AiBridgeMutationResult,
  AiBridgeSnapshot,
} from './types'

const BASE_PATH = '/ai-bridge/api'

/**
 * Every ai-bridge call is same-origin (the service is mounted under
 * `/ai-bridge`), so the session cookie is sent automatically. The console never
 * retries the service's own 401/403 responses and never toasts them: the page
 * renders the access notice instead.
 */
function requestConfig(
  csrfToken?: string,
  withJsonBody = false
): ApiRequestConfig {
  const headers: Record<string, string> = {
    Accept: 'application/json',
    'Cache-Control': 'no-store',
  }
  if (withJsonBody) headers['Content-Type'] = 'application/json'
  if (csrfToken) headers['X-AI-Bridge-CSRF'] = csrfToken
  return {
    skipErrorHandler: true,
    skipAuthRefresh: true,
    headers,
  }
}

export async function fetchAiBridgeIdentity(): Promise<AiBridgeIdentity> {
  const response = await api.get(`${BASE_PATH}/me`, requestConfig())
  return response.data as AiBridgeIdentity
}

export async function fetchAiBridgeSnapshot(): Promise<AiBridgeSnapshot> {
  const response = await api.get(`${BASE_PATH}/snapshot`, requestConfig())
  return response.data as AiBridgeSnapshot
}

export async function fetchAiBridgeControl(): Promise<AiBridgeControl> {
  const response = await api.get(`${BASE_PATH}/control`, requestConfig())
  return response.data as AiBridgeControl
}

export async function saveAiBridgeConfig(
  csrfToken: string,
  values: Record<string, string>
): Promise<AiBridgeControl> {
  const response = await api.put(
    `${BASE_PATH}/config`,
    { values },
    requestConfig(csrfToken, true)
  )
  return response.data as AiBridgeControl
}

export async function reloadAiBridgeConfig(
  csrfToken: string
): Promise<AiBridgeControl> {
  const response = await api.post(
    `${BASE_PATH}/reload`,
    undefined,
    requestConfig(csrfToken, true)
  )
  return response.data as AiBridgeControl
}

export async function clearAiBridgeCooldown(
  csrfToken: string,
  kind: AiBridgeCooldownKind,
  id: string
): Promise<AiBridgeMutationResult> {
  const response = await api.delete(`${BASE_PATH}/cooldowns/${kind}`, {
    ...requestConfig(csrfToken, true),
    data: { id },
  })
  return response.data as AiBridgeMutationResult
}

export async function createAiBridgeListEntry(
  csrfToken: string,
  kind: AiBridgeListKind,
  value: string
): Promise<AiBridgeListMutationResult> {
  const response = await api.post(
    `${BASE_PATH}/lists/${kind}`,
    { value },
    requestConfig(csrfToken, true)
  )
  return response.data as AiBridgeListMutationResult
}

export async function deleteAiBridgeListEntry(
  csrfToken: string,
  kind: AiBridgeListKind,
  id: string
): Promise<AiBridgeListMutationResult> {
  const response = await api.delete(`${BASE_PATH}/lists/${kind}`, {
    ...requestConfig(csrfToken, true),
    data: { id },
  })
  return response.data as AiBridgeListMutationResult
}
