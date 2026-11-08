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
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { AxiosError } from 'axios'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { api } from '@/lib/api'

import { AiBridgeConsole } from '../index'
import type {
  AiBridgeControl,
  AiBridgeIdentity,
  AiBridgeSnapshot,
} from '../types'

const router = vi.hoisted(() => ({
  search: {} as Record<string, unknown>,
  navigate: vi.fn(),
}))

vi.mock('@tanstack/react-router', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@tanstack/react-router')>()),
  useSearch: () => router.search,
  useNavigate: () => router.navigate,
}))

const IDENTITY: AiBridgeIdentity = {
  id: 7,
  username: 'admin',
  role: 100,
  csrf_token: 'csrf-token-1',
}

function snapshot(overrides: Partial<AiBridgeSnapshot> = {}): AiBridgeSnapshot {
  return {
    generated_at_ms: 1_700_000_000_000,
    uptime_secs: 3_720,
    concurrency: { active: 3, tracked_keys: 12 },
    retry_cooldowns: { total: 0, truncated: false, entries: [] },
    auth_cooldowns: { total: 0, truncated: false, cooldowns: [] },
    blacklist: {
      total_api_keys: 0,
      total_client_ips: 0,
      api_keys_truncated: false,
      client_ips_truncated: false,
      api_keys: [],
      client_ips: [],
    },
    whitelist: { total_api_keys: 0, api_keys_truncated: false, api_keys: [] },
    observability: {
      requests_total: 4_321,
      responses_2xx: 4_000,
      responses_4xx: 300,
      responses_5xx: 21,
      upstream_retries: 5,
      admin_mutations: 9,
      reload_successes: 2,
      reload_failures: 1,
      event_capacity: 1_000,
      events: [],
    },
    ...overrides,
  }
}

const CONTROL: AiBridgeControl = {
  generated_at_ms: 1_700_000_000_000,
  override_file: {
    path: '/etc/ai-bridge/admin.env',
    exists: true,
    writable: true,
    overridden_fields: 1,
    issue: null,
  },
  fields: [
    {
      key: 'AI_BRIDGE_UPSTREAM_BASE_URL',
      label: 'New API 上游地址',
      group: '上游',
      kind: 'text',
      value: 'https://upstream.test/v1',
      overridden: true,
      min: null,
      max: null,
      options: [],
    },
    {
      key: 'AI_BRIDGE_CONNECT_TIMEOUT_SECS',
      label: '连接超时（秒）',
      group: '上游',
      kind: 'number',
      value: '15',
      overridden: false,
      min: 1,
      max: 3_600,
      options: [],
    },
  ],
  policies: {
    cyber_policy_ban_enabled: true,
    auth_failure_cooldown_enabled: false,
    auth_failure_policy: 'balanced',
    upstream_error_cooldown_enabled: true,
    upstream_502_cooldown_enabled: true,
    stream_event_stall_retry_enabled: false,
  },
  files: [],
}

function axiosError(status: number, code: string, message: string): AxiosError {
  return new AxiosError(message, String(status), undefined, undefined, {
    data: { error: { code, message } },
    status,
    statusText: message,
    headers: {},
    config: { headers: {} },
  } as never)
}

interface ReadOptions {
  me?: AiBridgeIdentity | Error
  snapshot?: AiBridgeSnapshot
  control?: AiBridgeControl
}

function mockReads(options: ReadOptions = {}) {
  const get = vi.spyOn(api, 'get')
  get.mockImplementation(((url: string) => {
    if (url === '/ai-bridge/api/me') {
      const me = options.me ?? IDENTITY
      return me instanceof Error
        ? Promise.reject(me)
        : Promise.resolve({ data: me })
    }
    if (url === '/ai-bridge/api/snapshot') {
      return Promise.resolve({ data: options.snapshot ?? snapshot() })
    }
    if (url === '/ai-bridge/api/control') {
      return Promise.resolve({ data: options.control ?? CONTROL })
    }
    return Promise.reject(new Error(`Unexpected request: ${url}`))
  }) as never)
  return get
}

function renderConsole() {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false },
    },
  })
  return render(
    <QueryClientProvider client={client}>
      <AiBridgeConsole />
    </QueryClientProvider>
  )
}

function statCard(label: string): HTMLElement {
  const card = screen.getByText(label).closest('[data-slot="card"]')
  if (!card) throw new Error(`Missing stat card for ${label}`)
  return card as HTMLElement
}

beforeEach(() => {
  router.search = {}
  router.navigate.mockReset()
})

describe('ai-bridge console', () => {
  it('shows the administrator session notice when the identity request is unauthorized', async () => {
    mockReads({
      me: axiosError(
        401,
        'admin_auth_required',
        'Administrator authentication is required.'
      ),
    })

    renderConsole()

    expect(
      await screen.findByText('Administrator session required')
    ).toBeVisible()
    expect(screen.queryByRole('tab')).not.toBeInTheDocument()
    expect(api.get).toHaveBeenCalledTimes(1)
  })

  it('shows uptime, concurrency and request counters from the snapshot', async () => {
    mockReads()

    renderConsole()

    await screen.findByText('Uptime')
    expect(statCard('Uptime')).toHaveTextContent('1h 2m')
    expect(statCard('Active concurrency')).toHaveTextContent('3')
    expect(statCard('Tracked keys')).toHaveTextContent('12')
    expect(statCard('Total requests')).toHaveTextContent('4,321')
    expect(statCard('Upstream retries')).toHaveTextContent('5')
  })

  it('clears a cooldown with a DELETE request carrying the CSRF header', async () => {
    router.search = { tab: 'cooldowns' }
    const cooldownId = 'a'.repeat(64)
    mockReads({
      snapshot: snapshot({
        auth_cooldowns: {
          total: 1,
          truncated: false,
          cooldowns: [
            {
              id: cooldownId,
              scope: 'api_key',
              identifier: '0123456789abcdef',
              remaining_secs: 120,
              status: 403,
              reason: 'repeated_upstream_auth_failure',
            },
          ],
        },
      }),
    })
    const request = vi
      .spyOn(api, 'delete')
      .mockResolvedValue({ data: { ok: true, result: 'cleared' } } as never)
    const user = userEvent.setup()

    renderConsole()

    await user.click(await screen.findByRole('button', { name: 'Clear' }))
    const dialog = await screen.findByRole('alertdialog')
    await user.click(within(dialog).getByRole('button', { name: 'Clear' }))

    await waitFor(() => expect(request).toHaveBeenCalledTimes(1))
    expect(request).toHaveBeenCalledWith(
      '/ai-bridge/api/cooldowns/auth_api_key',
      expect.objectContaining({
        data: { id: cooldownId },
        headers: expect.objectContaining({
          'X-AI-Bridge-CSRF': 'csrf-token-1',
        }),
      })
    )
  })

  it('saves only the configuration keys that were edited', async () => {
    router.search = { tab: 'config' }
    mockReads()
    const request = vi
      .spyOn(api, 'put')
      .mockResolvedValue({ data: CONTROL } as never)
    const user = userEvent.setup()

    renderConsole()

    const input = await screen.findByLabelText('Upstream base URL')
    const saveButton = screen.getByRole('button', { name: 'Save changes' })
    expect(saveButton).toBeDisabled()
    await user.clear(input)
    await user.type(input, 'https://changed.test/v1')
    await user.click(saveButton)

    await waitFor(() => expect(request).toHaveBeenCalledTimes(1))
    expect(request).toHaveBeenCalledWith(
      '/ai-bridge/api/config',
      { values: { AI_BRIDGE_UPSTREAM_BASE_URL: 'https://changed.test/v1' } },
      expect.objectContaining({
        headers: expect.objectContaining({
          'Content-Type': 'application/json',
          'X-AI-Bridge-CSRF': 'csrf-token-1',
        }),
      })
    )
  })
  it('rejects an invalid IP address before adding it to the blacklist', async () => {
    router.search = { tab: 'access' }
    mockReads({
      snapshot: snapshot({
        blacklist: {
          total_api_keys: 0,
          total_client_ips: 3,
          api_keys_truncated: false,
          client_ips_truncated: true,
          api_keys: [],
          client_ips: ['203.0.113.9'],
        },
      }),
    })
    const createRequest = vi.spyOn(api, 'post')
    const user = userEvent.setup()

    renderConsole()

    expect(await screen.findByText('203.0.113.9')).toBeVisible()
    expect(screen.getByText('Showing 1 of 3 entries.')).toBeVisible()
    await user.type(
      screen.getByLabelText('Add to IP blacklist'),
      'not-an-ip{Enter}'
    )

    expect(
      screen.getByText('Enter a valid IPv4 or IPv6 address.')
    ).toBeVisible()
    expect(createRequest).not.toHaveBeenCalled()
  })

  it('shows an empty state when the service buffered no events', async () => {
    router.search = { tab: 'logs' }
    mockReads()

    renderConsole()

    expect(await screen.findByText('No events recorded yet')).toBeVisible()
  })
})
