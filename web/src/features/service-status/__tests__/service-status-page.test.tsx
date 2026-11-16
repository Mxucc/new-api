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
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ServiceStatusPage } from '../index'
import type { ServiceStatusCell, ServiceStatusView } from '../types'

const api = vi.hoisted(() => ({
  getServiceStatus: vi.fn(),
}))

vi.mock('../api', () => api)

function cell(overrides: Partial<ServiceStatusCell> = {}): ServiceStatusCell {
  return {
    start: 1_700_000_000,
    requests: 100,
    success_rate: 100,
    status: 'operational',
    ...overrides,
  }
}

function view(overrides: Partial<ServiceStatusView> = {}): ServiceStatusView {
  return {
    generated_at: 1_700_086_400,
    window_hours: 24,
    window_start: 1_700_000_000,
    window_end: 1_700_086_400,
    cell_seconds: 1800,
    data_through: 1_700_085_000,
    requests: 4200,
    success_rate: 99.52,
    status: 'operational',
    cells: [cell(), cell({ start: 1_700_001_800, status: 'degraded' })],
    models: [
      {
        model_name: 'gpt-4.1',
        requests: 4000,
        success_rate: 99.7,
        avg_latency_ms: 900,
        status: 'operational',
        cells: [cell()],
      },
      {
        model_name: 'claude-opus',
        requests: 200,
        success_rate: 92.5,
        avg_latency_ms: 2100,
        status: 'partial',
        cells: [cell({ status: 'partial', success_rate: 92.5 })],
      },
    ],
    ...overrides,
  }
}

let client: QueryClient

function renderPage() {
  return render(
    <QueryClientProvider client={client}>
      <ServiceStatusPage />
    </QueryClientProvider>
  )
}

beforeEach(() => {
  client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  api.getServiceStatus.mockResolvedValue(view())
})

afterEach(() => {
  client.clear()
})

describe('service status page', () => {
  it('shows the overall availability and the passive-only note', async () => {
    renderPage()

    await waitFor(() => {
      expect(screen.getByTestId('service-status-availability')).toBeVisible()
    })
    expect(
      screen.getByTestId('service-status-availability').textContent
    ).toContain('99.52')
    expect(screen.getByTestId('service-status-headline').textContent).toBe(
      'Requests are being served normally.'
    )
    expect(screen.getByTestId('service-status-passive-note')).toBeVisible()
    expect(api.getServiceStatus).toHaveBeenCalledWith(24)
  })

  it('lists per-model availability with its own timeline', async () => {
    renderPage()

    await waitFor(() => {
      expect(
        screen.getAllByTestId('service-status-model-claude-opus').length
      ).toBeGreaterThan(0)
    })
    expect(screen.getAllByText('gpt-4.1').length).toBeGreaterThan(0)
    // 时间线条目按格子渲染，橙红格说明那段真实请求有失败。
    expect(
      screen.getAllByTestId('service-status-cell-1').length
    ).toBeGreaterThan(0)
  })

  it('switches the window and asks for the new range', async () => {
    const user = userEvent.setup()
    renderPage()

    await waitFor(() => {
      expect(screen.getByTestId('service-status-availability')).toBeVisible()
    })
    api.getServiceStatus.mockResolvedValue(view({ window_hours: 168 }))
    await user.click(screen.getByText('Last 7 days'))

    await waitFor(() => {
      expect(api.getServiceStatus).toHaveBeenCalledWith(168)
    })
  })

  it('explains an empty window instead of reporting an outage', async () => {
    api.getServiceStatus.mockResolvedValue(
      view({
        requests: 0,
        success_rate: 0,
        status: 'unknown',
        data_through: 0,
        models: [],
        cells: [cell({ requests: 0, success_rate: 0, status: 'unknown' })],
      })
    )
    renderPage()

    await waitFor(() => {
      expect(screen.getByText('No traffic in this window yet.')).toBeVisible()
    })
    expect(screen.queryByTestId('service-status-availability')).toBeNull()
  })

  it('offers a retry when the request fails', async () => {
    api.getServiceStatus.mockRejectedValue(new Error('boom'))
    renderPage()

    await waitFor(() => {
      expect(
        screen.getByText('Failed to load the service status')
      ).toBeVisible()
    })
  })
})
