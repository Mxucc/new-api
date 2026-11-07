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
import { beforeEach, describe, expect, test, vi, type Mock } from 'vitest'

import { getFreshAuthHeaders } from '@/lib/auth-session'

import {
  exchangeExternalAuthForRedirect,
  externalAuthExchangePath,
} from './external-auth-exchange'

vi.mock('@/lib/auth-session', () => ({
  getFreshAuthHeaders: vi.fn(),
}))

const origin = 'https://dashboard.example.com'
const mockedGetFreshAuthHeaders = getFreshAuthHeaders as Mock

describe('external authentication exchange', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
    vi.clearAllMocks()
    mockedGetFreshAuthHeaders.mockResolvedValue({
      Authorization: 'Bearer dashboard-access-token',
    })
  })

  test.each([
    ['/connect', '/connect/auth/exchange'],
    ['/connect/authorize?client_id=test', '/connect/auth/exchange'],
    ['/ai-bridge/', '/ai-bridge/auth/exchange'],
    ['/jobs-files/', '/jobs-files/auth/exchange'],
    ['/jobs-flies/api/me', '/jobs-flies/auth/exchange'],
  ])('maps %s to its fixed exchange endpoint', (target, expected) => {
    expect(externalAuthExchangePath(target, origin)).toBe(expected)
  })

  test('rejects cross-origin and lookalike paths', () => {
    expect(
      externalAuthExchangePath('https://attacker.example/connect', origin)
    ).toBe(null)
    expect(externalAuthExchangePath('/connect-evil', origin)).toBe(null)
    expect(externalAuthExchangePath('/dashboard', origin)).toBe(null)
  })

  test('exchanges the fresh bearer before navigation can continue', async () => {
    const fetchMock = vi
      .spyOn(globalThis, 'fetch')
      .mockResolvedValue(new Response(null, { status: 204 }))

    await exchangeExternalAuthForRedirect('/connect/dashboard', origin)

    expect(mockedGetFreshAuthHeaders).toHaveBeenCalledOnce()
    expect(fetchMock).toHaveBeenCalledWith('/connect/auth/exchange', {
      method: 'POST',
      headers: { Authorization: 'Bearer dashboard-access-token' },
      credentials: 'same-origin',
      cache: 'no-store',
    })
  })

  test('does nothing for ordinary internal routes', async () => {
    const fetchMock = vi.spyOn(globalThis, 'fetch')

    await exchangeExternalAuthForRedirect('/dashboard', origin)

    expect(mockedGetFreshAuthHeaders).not.toHaveBeenCalled()
    expect(fetchMock).not.toHaveBeenCalled()
  })

  test('fails closed when the exchange endpoint rejects the token', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response(null, { status: 401 })
    )

    await expect(
      exchangeExternalAuthForRedirect('/ai-bridge', origin)
    ).rejects.toThrow('External authentication exchange failed (401)')
  })
})
