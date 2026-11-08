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
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { api } from '@/lib/api'

import { Connect } from '../index'
import type {
  ConnectAuthorization,
  ConnectClient,
  ConnectMe,
  CreatedConnectClient,
} from '../types'

const { navigate, searchState } = vi.hoisted(() => ({
  navigate: vi.fn(),
  searchState: { current: {} as Record<string, unknown> },
}))

vi.mock('@tanstack/react-router', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@tanstack/react-router')>()),
  useNavigate: () => navigate,
  useSearch: () => searchState.current,
}))

function connectMe(overrides: Partial<ConnectMe> = {}): ConnectMe {
  return {
    id: 7,
    username: 'alice',
    role: 1,
    status: 1,
    group: 'default',
    is_admin: false,
    ...overrides,
  }
}

function connectClient(overrides: Partial<ConnectClient> = {}): ConnectClient {
  return {
    id: 'client-abc',
    name: 'Example App',
    homepage_url: 'https://example.com',
    description: 'A demo application',
    logo_url: '',
    redirect_uri: 'https://example.com/callback',
    client_secret: 'cs_secret_value',
    owner_id: 7,
    owner_name: 'alice',
    created_at: '2026-01-02 03:04:05',
    ...overrides,
  }
}

function connectAuthorization(
  overrides: Partial<ConnectAuthorization> = {}
): ConnectAuthorization {
  return {
    client_id: 'client-abc',
    name: 'Example App',
    description: 'A demo application',
    homepage_url: 'https://example.com',
    logo_url: '',
    scope: 'profile',
    first_granted_at: '2026-01-02 03:04:05',
    last_used_at: '2026-01-03 04:05:06',
    ...overrides,
  }
}

function httpFailure(status: number, message: string): AxiosError {
  return new AxiosError(message, String(status), undefined, undefined, {
    data: { error: message },
    status,
    statusText: '',
    headers: {},
    config: { headers: {} },
  } as never)
}

interface ConnectApiRoutes {
  me: ConnectMe | Error
  clients?: ConnectClient[]
  authorizations?: ConnectAuthorization[]
}

function mockConnectApi(routes: ConnectApiRoutes) {
  return vi.spyOn(api, 'get').mockImplementation((url) => {
    if (url === '/connect/api/me') {
      if (routes.me instanceof Error) return Promise.reject(routes.me)
      return Promise.resolve({ data: routes.me })
    }
    if (url === '/connect/api/clients') {
      return Promise.resolve({ data: routes.clients ?? [] })
    }
    if (url === '/connect/api/authorizations') {
      return Promise.resolve({ data: routes.authorizations ?? [] })
    }
    return Promise.reject(new Error(`Unexpected GET ${url}`))
  })
}

let queryClient: QueryClient

beforeEach(() => {
  searchState.current = {}
  queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false },
    },
  })
})

afterEach(() => {
  queryClient.clear()
  navigate.mockReset()
})

function renderConnect() {
  render(
    <QueryClientProvider client={queryClient}>
      <Connect />
    </QueryClientProvider>
  )
}

describe('connect service availability', () => {
  it('explains that the service is not deployed instead of retrying a 404', async () => {
    const get = vi
      .spyOn(api, 'get')
      .mockRejectedValue(httpFailure(404, 'Not Found'))

    renderConnect()

    expect(
      await screen.findByText(
        'The Connect service is not available on this deployment.'
      )
    ).toBeVisible()
    expect(get).toHaveBeenCalledTimes(1)
    expect(get).toHaveBeenCalledWith('/connect/api/me', expect.any(Object))
    expect(screen.queryByRole('tab')).not.toBeInTheDocument()
  })

  it('asks for a new session when the service answers 401', async () => {
    vi.spyOn(api, 'get').mockRejectedValue(httpFailure(401, 'Unauthorized'))

    renderConnect()

    expect(
      await screen.findByText(
        'Sign in again to manage your Connect applications.'
      )
    ).toBeVisible()
  })
})

describe('connect applications', () => {
  it('lists the applications and masks their client secrets', async () => {
    mockConnectApi({
      me: connectMe({ is_admin: true, username: 'root' }),
      clients: [connectClient({ owner_name: 'bob' })],
    })

    renderConnect()

    expect(await screen.findByText('Example App')).toBeVisible()
    expect(screen.getByText('client-abc')).toBeVisible()
    expect(screen.getByText('https://example.com/callback')).toBeVisible()
    expect(screen.getByRole('button', { name: 'Copy Client ID' })).toBeVisible()
    expect(screen.getByText('bob')).toBeVisible()
    expect(screen.queryByText('cs_secret_value')).not.toBeInTheDocument()
    expect(screen.getByText(/^cs_sec/)).toBeVisible()
  })

  it('hides the owner of an application from a non-admin user', async () => {
    mockConnectApi({
      me: connectMe(),
      clients: [connectClient({ owner_name: 'bob' })],
    })

    renderConnect()

    expect(await screen.findByText('Example App')).toBeVisible()
    expect(screen.queryByText('bob')).not.toBeInTheDocument()
  })

  it('explains that a legacy application without a stored secret must be registered again', async () => {
    mockConnectApi({
      me: connectMe(),
      clients: [connectClient({ client_secret: '' })],
    })

    renderConnect()

    expect(
      await screen.findByText('Legacy app, please register it again')
    ).toBeVisible()
    expect(
      screen.queryByRole('button', { name: 'Copy Client Secret' })
    ).not.toBeInTheDocument()
  })

  it('shows the one-time client secret after registering an application', async () => {
    const created: CreatedConnectClient = {
      client_id: 'client-new',
      client_secret: 'cs_new_secret',
      name: 'New App',
      homepage_url: 'https://new.example.com',
      description: '',
      logo_url: '',
      redirect_uri: 'https://new.example.com/callback',
    }
    mockConnectApi({ me: connectMe(), clients: [connectClient()] })
    const post = vi
      .spyOn(api, 'post')
      .mockImplementation(async () => ({ data: created }))
    const user = userEvent.setup()

    renderConnect()

    await user.click(
      await screen.findByRole('button', { name: 'New application' })
    )
    await user.type(screen.getByLabelText('Application name'), 'New App')
    await user.type(
      screen.getByLabelText('Application homepage'),
      'https://new.example.com'
    )
    await user.type(
      screen.getByLabelText('Redirect URI'),
      'https://new.example.com/callback'
    )
    await user.click(screen.getByRole('button', { name: 'Create application' }))

    expect(await screen.findByText('cs_new_secret')).toBeVisible()
    expect(screen.getByText('client-new')).toBeVisible()
    expect(
      screen.getByText('Save the client secret now; it is shown only once.')
    ).toBeVisible()
    expect(
      screen.getByRole('button', { name: 'Copy Client Secret' })
    ).toBeVisible()
    expect(post).toHaveBeenCalledWith(
      '/connect/clients',
      {
        name: 'New App',
        homepage_url: 'https://new.example.com',
        description: '',
        logo_url: '',
        redirect_uri: 'https://new.example.com/callback',
      },
      expect.any(Object)
    )
  })
})

describe('connect authorizations', () => {
  it('revokes an authorization only after confirmation', async () => {
    searchState.current = { tab: 'authorizations' }
    mockConnectApi({
      me: connectMe(),
      clients: [connectClient()],
      authorizations: [connectAuthorization()],
    })
    const remove = vi
      .spyOn(api, 'delete')
      .mockResolvedValue({ data: { success: true } })
    const user = userEvent.setup()

    renderConnect()

    expect(await screen.findByText('Example App')).toBeVisible()
    expect(
      screen.queryByRole('button', { name: 'New application' })
    ).not.toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Revoke' }))
    const dialog = await screen.findByRole('alertdialog', {
      name: 'Revoke authorization?',
    })
    expect(remove).not.toHaveBeenCalled()

    await user.click(
      within(dialog).getByRole('button', { name: 'Revoke authorization' })
    )

    await waitFor(() =>
      expect(remove).toHaveBeenCalledWith(
        '/connect/api/authorizations/client-abc',
        expect.any(Object)
      )
    )
  })

  it('writes the selected tab into the URL search', async () => {
    mockConnectApi({ me: connectMe(), clients: [connectClient()] })
    const user = userEvent.setup()

    renderConnect()

    await user.click(
      await screen.findByRole('tab', { name: 'My authorizations' })
    )

    expect(navigate).toHaveBeenCalledTimes(1)
    const options = navigate.mock.calls[0][0] as {
      to: string
      replace: boolean
      search: (prev: Record<string, unknown>) => Record<string, unknown>
    }
    expect(options.to).toBe('.')
    expect(options.replace).toBe(true)
    expect(options.search({ page: 2 })).toEqual({
      page: 2,
      tab: 'authorizations',
    })
  })
})
