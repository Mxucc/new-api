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
/** Signed-in user as reported by `GET /connect/api/me`. */
export interface ConnectMe {
  id: number
  username: string
  role: number
  status: number
  group: string
  is_admin: boolean
}

/** Fields a client sends when registering or editing an application. */
export interface ConnectClientPayload {
  name: string
  homepage_url: string
  description: string
  logo_url: string
  redirect_uri: string
}

/** Application row returned by `GET /connect/api/clients`. */
export interface ConnectClient extends ConnectClientPayload {
  id: string
  /** Plaintext secret; empty for applications registered before it was stored. */
  client_secret: string
  owner_id: number
  owner_name: string
  created_at: string
}

export type UpdateConnectClientData = Partial<ConnectClientPayload>

/** Response of `POST /connect/clients`; the secret is returned only once. */
export interface CreatedConnectClient extends ConnectClientPayload {
  client_id: string
  client_secret: string
}

/** Authorization granted to a third-party application. */
export interface ConnectAuthorization {
  client_id: string
  name: string
  description: string
  homepage_url: string
  logo_url: string
  scope: string
  first_granted_at: string
  last_used_at: string
}

export type ConnectTab = 'apps' | 'authorizations'

export const DEFAULT_CONNECT_TAB: ConnectTab = 'apps'

export function isConnectTab(value: unknown): value is ConnectTab {
  return value === 'apps' || value === 'authorizations'
}
