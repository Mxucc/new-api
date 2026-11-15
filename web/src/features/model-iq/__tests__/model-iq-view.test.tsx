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
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ROLE } from '@/lib/roles'
import { useAuthStore } from '@/stores/auth-store'

import { ModelIq } from '../index'
import type { ModelIqOverview, ModelIqStatus } from '../types'

const api = vi.hoisted(() => ({
  getModelIqOverview: vi.fn(),
  getPelicanResult: vi.fn(),
  runPelicanTest: vi.fn(),
  runAllPelicanTests: vi.fn(),
}))

vi.mock('../api', () => api)

function modelStatus(overrides: Partial<ModelIqStatus> = {}): ModelIqStatus {
  return {
    model_name: 'gpt-4.1',
    available: true,
    verdict: 'healthy',
    score: 100,
    latency_ms: 1200,
    tested_at: 1700000000,
    result_id: 11,
    dimensions: ['drawing', 'subject'],
    ...overrides,
  }
}

function overview(models: ModelIqStatus[]): ModelIqOverview {
  return {
    models,
    total: models.length,
    latest_tested_at: 1700000000,
    prompt: '创建一个 HTML，内容是 SVG 绘制一个鹈鹕骑自行车的 2D 动画',
  }
}

let client: QueryClient
let originalAuth: ReturnType<typeof useAuthStore.getState>['auth']

function renderPage(role: number) {
  useAuthStore.setState({
    auth: {
      ...originalAuth,
      user: { id: 1, username: 'tester', role } as never,
    },
  })
  return render(
    <QueryClientProvider client={client}>
      <ModelIq />
    </QueryClientProvider>
  )
}

beforeEach(() => {
  client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  originalAuth = useAuthStore.getState().auth
  api.getModelIqOverview.mockResolvedValue(
    overview([
      modelStatus(),
      modelStatus({
        model_name: 'claude-opus',
        verdict: 'degraded',
        score: 65,
        result_id: 12,
      }),
      modelStatus({
        model_name: 'never-probed',
        verdict: 'pending',
        score: 0,
        result_id: 0,
        tested_at: 0,
        latency_ms: 0,
      }),
    ])
  )
  api.getPelicanResult.mockResolvedValue({
    id: 11,
    model_name: 'gpt-4.1',
    status: 'success',
    verdict: 'healthy',
    score: 100,
    latency_ms: 1200,
    dimensions: ['drawing'],
    output: '<svg><animate attributeName="x"/></svg>',
    error: '',
    created_at: 1700000000,
  })
})

afterEach(() => {
  client.clear()
  useAuthStore.setState({ auth: originalAuth })
})

describe('model IQ page', () => {
  it('shows every site model with its status and IQ score', async () => {
    renderPage(ROLE.USER)

    await waitFor(() => {
      expect(screen.getAllByText('gpt-4.1').length).toBeGreaterThan(0)
    })
    expect(screen.getAllByText('claude-opus').length).toBeGreaterThan(0)
    expect(
      screen.getAllByTestId('model-iq-score-gpt-4.1')[0]
    ).toHaveTextContent('100')
    expect(
      screen.getAllByTestId('model-iq-score-claude-opus')[0]
    ).toHaveTextContent('65')
    expect(screen.getAllByText('Healthy').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Degraded').length).toBeGreaterThan(0)
  })

  it('leaves an unprobed model without a fake score', async () => {
    renderPage(ROLE.USER)

    await waitFor(() => {
      expect(
        screen.getAllByTestId('model-iq-score-never-probed').length
      ).toBeGreaterThan(0)
    })
    expect(
      screen.getAllByTestId('model-iq-score-never-probed')[0]
    ).toHaveTextContent('—')
    expect(screen.getAllByText('Not tested').length).toBeGreaterThan(0)
  })

  it('hides the run controls from a regular user', async () => {
    renderPage(ROLE.USER)

    await waitFor(() => {
      expect(screen.getAllByText('gpt-4.1').length).toBeGreaterThan(0)
    })
    expect(screen.queryByRole('button', { name: 'Test all models' })).toBeNull()
    expect(screen.queryAllByRole('button', { name: 'Run test' })).toHaveLength(
      0
    )
  })

  it('offers the run controls to an administrator', async () => {
    renderPage(ROLE.ADMIN)

    await waitFor(() => {
      expect(screen.getAllByText('gpt-4.1').length).toBeGreaterThan(0)
    })
    expect(screen.getByRole('button', { name: 'Test all models' })).toBeTruthy()
    expect(
      screen.getAllByRole('button', { name: 'Run test' }).length
    ).toBeGreaterThan(0)
  })

  it('disables preview for a model that was never probed', async () => {
    renderPage(ROLE.USER)

    await waitFor(() => {
      expect(
        screen.getAllByTestId('model-iq-card-never-probed').length
      ).toBeGreaterThan(0)
    })
    const cards = screen.getAllByTestId('model-iq-card-never-probed')
    const previewButtons = cards[0].querySelectorAll('button')
    expect(previewButtons[0]).toBeDisabled()
  })
})
