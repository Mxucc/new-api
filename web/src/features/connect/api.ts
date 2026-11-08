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
  ConnectAuthorization,
  ConnectClient,
  ConnectClientPayload,
  ConnectMe,
  CreatedConnectClient,
  UpdateConnectClientData,
} from './types'

const BASE_PATH = '/connect'

/**
 * The Connect service returns a bare JSON body (`{"error": "..."}` on failure)
 * and owns its own session handling: a 401 here means "not signed in for this
 * deployment", not "refresh and redirect". Keep the shared interceptor from
 * toasting, retrying or redirecting these calls and handle every status at the
 * call site instead.
 */
const connectRequestConfig: ApiRequestConfig = {
  skipErrorHandler: true,
  skipAuthRefresh: true,
}

export const CONNECT_ME_QUERY_KEY = ['connect', 'me'] as const
export const CONNECT_CLIENTS_QUERY_KEY = ['connect', 'clients'] as const
export const CONNECT_AUTHORIZATIONS_QUERY_KEY = [
  'connect',
  'authorizations',
] as const

export async function getConnectMe(): Promise<ConnectMe> {
  const res = await api.get<ConnectMe>(
    `${BASE_PATH}/api/me`,
    connectRequestConfig
  )
  return res.data
}

export async function getConnectClients(): Promise<ConnectClient[]> {
  const res = await api.get<ConnectClient[]>(
    `${BASE_PATH}/api/clients`,
    connectRequestConfig
  )
  return res.data
}

export async function createConnectClient(
  data: ConnectClientPayload
): Promise<CreatedConnectClient> {
  const res = await api.post<CreatedConnectClient>(
    `${BASE_PATH}/clients`,
    data,
    connectRequestConfig
  )
  return res.data
}

export async function updateConnectClient(
  id: string,
  data: UpdateConnectClientData
): Promise<void> {
  await api.put(
    `${BASE_PATH}/api/clients/${encodeURIComponent(id)}`,
    data,
    connectRequestConfig
  )
}

export async function deleteConnectClient(id: string): Promise<void> {
  await api.delete(
    `${BASE_PATH}/api/clients/${encodeURIComponent(id)}`,
    connectRequestConfig
  )
}

export async function getConnectAuthorizations(): Promise<
  ConnectAuthorization[]
> {
  const res = await api.get<ConnectAuthorization[]>(
    `${BASE_PATH}/api/authorizations`,
    connectRequestConfig
  )
  return res.data
}

export async function revokeConnectAuthorization(
  clientId: string
): Promise<void> {
  await api.delete(
    `${BASE_PATH}/api/authorizations/${encodeURIComponent(clientId)}`,
    connectRequestConfig
  )
}
