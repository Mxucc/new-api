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
import { getFreshAuthHeaders } from '@/lib/auth-session'

const externalAuthExchangePaths = [
  ['/connect', '/connect/auth/exchange'],
  ['/ai-bridge', '/ai-bridge/auth/exchange'],
  ['/jobs-files', '/jobs-files/auth/exchange'],
  ['/jobs-flies', '/jobs-flies/auth/exchange'],
] as const

export function externalAuthExchangePath(
  target: string,
  origin: string
): string | null {
  let trustedOrigin: URL
  let targetURL: URL
  try {
    trustedOrigin = new URL(origin)
    targetURL = new URL(target, trustedOrigin.origin)
  } catch {
    return null
  }

  if (
    !['http:', 'https:'].includes(trustedOrigin.protocol) ||
    targetURL.origin !== trustedOrigin.origin
  ) {
    return null
  }

  const match = externalAuthExchangePaths.find(
    ([prefix]) =>
      targetURL.pathname === prefix || targetURL.pathname.startsWith(`${prefix}/`)
  )
  return match?.[1] ?? null
}

export async function exchangeExternalAuthForRedirect(
  target: string,
  origin = window.location.origin
): Promise<void> {
  const exchangePath = externalAuthExchangePath(target, origin)
  if (!exchangePath) return

  const headers = await getFreshAuthHeaders()
  const response = await fetch(exchangePath, {
    method: 'POST',
    headers,
    credentials: 'same-origin',
    cache: 'no-store',
  })
  if (!response.ok) {
    throw new Error(`External authentication exchange failed (${response.status})`)
  }
}
