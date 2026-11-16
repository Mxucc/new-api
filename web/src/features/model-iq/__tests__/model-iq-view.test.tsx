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

import { ROLE } from '@/lib/roles'
import { useAuthStore } from '@/stores/auth-store'

import { ModelIq } from '../index'
import type { ModelIqOverview, ModelIqStatus } from '../types'

const api = vi.hoisted(() => ({
  getModelIqOverview: vi.fn(),
  getPelicanResult: vi.fn(),
  getPelicanHistory: vi.fn(),
  getPelicanConfig: vi.fn(),
  updatePelicanConfig: vi.fn(),
  runPelicanTest: vi.fn(),
  runAllPelicanTests: vi.fn(),
}))

vi.mock('../api', () => api)

function modelStatus(overrides: Partial<ModelIqStatus> = {}): ModelIqStatus {
  return {
    group: 'default',
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
      // 同一个模型在另一个分组：分数与历史必须分开。
      modelStatus({
        group: 'vip',
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
    group: 'default',
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
  api.getPelicanHistory.mockResolvedValue({
    group: 'vip',
    model_name: 'gpt-4.1',
    history: [
      {
        id: 12,
        group: 'vip',
        status: 'success',
        verdict: 'degraded',
        score: 65,
        latency_ms: 900,
        dimensions: ['drawing'],
        error: '',
        created_at: 1700000000,
      },
    ],
  })
  api.getPelicanConfig.mockResolvedValue({
    targets: [{ group: 'default', model: 'gpt-4.1' }],
    interval_minutes: 30,
    available_targets: [
      { group: 'default', model: 'gpt-4.1' },
      { group: 'vip', model: 'gpt-4.1' },
    ],
    missing_targets: [],
  })
  api.updatePelicanConfig.mockResolvedValue({
    targets: [{ group: 'default', model: 'gpt-4.1' }],
    interval_minutes: 30,
  })
  api.runPelicanTest.mockResolvedValue(modelStatus())
})

afterEach(() => {
  client.clear()
  useAuthStore.setState({ auth: originalAuth })
})

describe('model IQ page', () => {
  it('shows every group and model with its own status and IQ score', async () => {
    renderPage(ROLE.USER)

    await waitFor(() => {
      expect(
        screen.getAllByTestId('model-iq-score-default-gpt-4.1').length
      ).toBeGreaterThan(0)
    })
    expect(
      screen.getAllByTestId('model-iq-score-default-gpt-4.1')[0]
    ).toHaveTextContent('100')
    // 同一模型在 vip 分组是另一行，分数不同。
    expect(
      screen.getAllByTestId('model-iq-score-vip-gpt-4.1')[0]
    ).toHaveTextContent('65')
    expect(screen.getAllByText('default').length).toBeGreaterThan(0)
    expect(screen.getAllByText('vip').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Healthy').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Degraded').length).toBeGreaterThan(0)
  })

  it('leaves an unprobed combination without a fake score', async () => {
    renderPage(ROLE.USER)

    await waitFor(() => {
      expect(
        screen.getAllByTestId('model-iq-score-default-never-probed').length
      ).toBeGreaterThan(0)
    })
    expect(
      screen.getAllByTestId('model-iq-score-default-never-probed')[0]
    ).toHaveTextContent('—')
    expect(screen.getAllByText('Not tested').length).toBeGreaterThan(0)
  })

  it('hides the run and sweep controls from a regular user', async () => {
    renderPage(ROLE.USER)

    await waitFor(() => {
      expect(
        screen.getAllByTestId('model-iq-score-default-gpt-4.1').length
      ).toBeGreaterThan(0)
    })
    expect(screen.queryByRole('button', { name: 'Test all models' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Sweep settings' })).toBeNull()
    expect(screen.queryAllByRole('button', { name: 'Run test' })).toHaveLength(
      0
    )
  })

  it('offers the run controls to an administrator', async () => {
    renderPage(ROLE.ADMIN)

    await waitFor(() => {
      expect(
        screen.getAllByTestId('model-iq-score-default-gpt-4.1').length
      ).toBeGreaterThan(0)
    })
    expect(screen.getByRole('button', { name: 'Test all models' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Sweep settings' })).toBeTruthy()
    expect(
      screen.getAllByRole('button', { name: 'Run test' }).length
    ).toBeGreaterThan(0)
  })

  it('runs the pelican test for the clicked group and model', async () => {
    const user = userEvent.setup()
    renderPage(ROLE.ADMIN)

    const runButtons = await screen.findAllByRole('button', {
      name: 'Run test',
    })
    await user.click(runButtons[0])

    await waitFor(() => {
      expect(api.runPelicanTest).toHaveBeenCalledWith(
        { group: 'default', model: 'gpt-4.1' },
        expect.anything()
      )
    })
  })

  it('opens the history of the clicked group and model', async () => {
    const user = userEvent.setup()
    renderPage(ROLE.USER)

    const historyButtons = await screen.findAllByRole('button', {
      name: 'History',
    })
    // 第二行是 vip 分组：历史请求必须带上它自己的分组。
    await user.click(historyButtons[1])

    await waitFor(() => {
      expect(api.getPelicanHistory).toHaveBeenCalledWith('vip', 'gpt-4.1')
    })
    expect(await screen.findByText('Test history')).toBeVisible()
  })

  it('loads the sweep settings for an administrator', async () => {
    const user = userEvent.setup()
    renderPage(ROLE.ADMIN)

    const sweepButton = await screen.findByRole('button', {
      name: 'Sweep settings',
    })
    await user.click(sweepButton)

    await waitFor(() => {
      expect(api.getPelicanConfig).toHaveBeenCalled()
    })
    expect(
      await screen.findByLabelText('Sweep interval (minutes)')
    ).toHaveValue(30)
  })

  it('disables preview for a combination that was never probed', async () => {
    renderPage(ROLE.USER)

    await waitFor(() => {
      expect(
        screen.getAllByTestId('model-iq-card-default-never-probed').length
      ).toBeGreaterThan(0)
    })
    const cards = screen.getAllByTestId('model-iq-card-default-never-probed')
    const previewButtons = cards[0].querySelectorAll('button')
    // 第一个按钮是历史（永远可用），第二个是预览。
    expect(previewButtons[0]).toBeEnabled()
    expect(previewButtons[1]).toBeDisabled()
  })
})
