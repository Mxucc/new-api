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
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ServiceStatusPage } from '../index'
import type { ServiceStatusCell, ServiceStatusView } from '../types'

const api = vi.hoisted(() => ({
  getServiceStatus: vi.fn(),
}))

vi.mock('../api', () => api)

const EMPTY_CELL: Partial<ServiceStatusCell> = {
  requests: 0,
  success_rate: 0,
  status: 'unknown',
}

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
        group: 'default',
        model_name: 'gpt-4.1',
        requests: 4000,
        success_rate: 99.7,
        avg_latency_ms: 900,
        status: 'operational',
        cells: [
          cell(),
          // 这一段没有流量：状态条不应该为它画灰块。
          cell({ start: 1_700_001_800, ...EMPTY_CELL }),
        ],
      },
      {
        group: 'vip',
        model_name: 'gpt-4.1',
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

  it('separates the same model in different groups into its own row', async () => {
    renderPage()

    const defaultRow = await screen.findByTestId(
      'service-status-model-default-gpt-4.1'
    )
    const vipRow = screen.getByTestId('service-status-model-vip-gpt-4.1')
    expect(within(defaultRow).getByText('default')).toBeVisible()
    expect(within(vipRow).getByText('vip')).toBeVisible()
    // 两个分组的可用率不同，说明没有把同一个模型的流量混在一起。
    expect(within(defaultRow).getByText('Availability 99.70%')).toBeVisible()
    expect(within(vipRow).getByText('Availability 92.50%')).toBeVisible()
  })

  it('renders a cell only for the time slices that carried traffic', async () => {
    renderPage()

    const row = await screen.findByTestId('service-status-model-default-gpt-4.1')
    expect(within(row).getByTestId('service-status-cell-0')).toBeVisible()
    // 第二格没有流量，因此不渲染任何格子，而不是画一个灰块。
    expect(within(row).queryByTestId('service-status-cell-1')).toBeNull()
    // 有数据的格子仍然按时间线条目渲染。
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
        cells: [cell({ ...EMPTY_CELL })],
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
