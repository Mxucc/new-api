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
import { act, render, screen, waitFor } from '@testing-library/react'
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
    testing: false,
    ...overrides,
  }
}

function overview(
  models: ModelIqStatus[],
  overrides: Partial<ModelIqOverview> = {}
): ModelIqOverview {
  return {
    models,
    total: models.length,
    latest_tested_at: 1700000000,
    prompt: '创建一个 HTML，内容是 SVG 绘制一个鹈鹕骑自行车的 2D 动画',
    sweeping: false,
    sweeping_since: 0,
    ...overrides,
  }
}

function historyPoint(overrides: Record<string, unknown> = {}) {
  return {
    id: 12,
    group: 'vip',
    status: 'success' as const,
    verdict: 'degraded' as const,
    score: 65,
    latency_ms: 900,
    dimensions: ['drawing'],
    error: '',
    created_at: 1700000000,
    ...overrides,
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
    history: [historyPoint(), historyPoint({ id: 13, score: 40 })],
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
  api.runPelicanTest.mockResolvedValue({ started: true })
  api.runAllPelicanTests.mockResolvedValue({ started: true, targets: 3 })
})

afterEach(() => {
  client.clear()
  useAuthStore.setState({ auth: originalAuth })
  vi.useRealTimers()
})

describe('model IQ grid', () => {
  it('shows a cell with its own score for every group and model pair', async () => {
    renderPage(ROLE.USER)

    const cell = await screen.findByTestId('model-iq-cell-default-gpt-4.1')
    expect(cell).toHaveTextContent('100')
    // 同一模型在 vip 分组是另一格，分数不同。
    expect(screen.getByTestId('model-iq-cell-vip-gpt-4.1')).toHaveTextContent(
      '65'
    )
    expect(screen.getAllByText('default').length).toBeGreaterThan(0)
    expect(screen.getAllByText('vip').length).toBeGreaterThan(0)
  })

  it('explains the empty grid when nothing has been tested yet', async () => {
    api.getModelIqOverview.mockResolvedValue(
      overview([
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
    renderPage(ROLE.USER)

    expect(await screen.findByTestId('model-iq-grid-empty')).toBeInTheDocument()
    expect(screen.queryByTestId('model-iq-grid')).toBeNull()
  })

  it('leaves a combination that was never probed out of the grid', async () => {
    renderPage(ROLE.USER)

    await screen.findByTestId('model-iq-cell-default-gpt-4.1')
    expect(screen.queryByTestId('model-iq-cell-default-never-probed')).toBeNull()
    expect(screen.queryByText('Not tested')).toBeNull()
  })

  it('shows a loading cell, without a stale score, while a combination is being tested', async () => {
    api.getModelIqOverview.mockResolvedValue(
      overview([
        modelStatus({ model_name: 'claude', testing: true, score: 40 }),
      ])
    )
    renderPage(ROLE.USER)

    const cell = await screen.findByTestId('model-iq-cell-default-claude')
    // 正在测的格子不拿上一次的分数冒充结论，只显示加载态。
    expect(cell).toHaveTextContent('')
    expect(cell.querySelector('svg')).not.toBeNull()
    expect(cell).toHaveAccessibleName('default · claude Healthy')
  })
})

describe('model IQ actions', () => {
  it('hides the sweep controls from a regular user', async () => {
    renderPage(ROLE.USER)

    await screen.findByTestId('model-iq-cell-default-gpt-4.1')
    expect(screen.queryByRole('button', { name: 'Test all models' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Sweep settings' })).toBeNull()
    expect(screen.queryAllByRole('button', { name: 'Run test' })).toHaveLength(0)
  })

  it('offers a run control to an administrator and starts one probe', async () => {
    const user = userEvent.setup()
    renderPage(ROLE.ADMIN)

    await screen.findByTestId('model-iq-cell-default-gpt-4.1')
    expect(screen.getByRole('button', { name: 'Test all models' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Sweep settings' })).toBeTruthy()

    await user.hover(screen.getByTestId('model-iq-cell-default-gpt-4.1'))
    await user.click(await screen.findByRole('button', { name: 'Run test' }))

    await waitFor(() => {
      expect(api.runPelicanTest).toHaveBeenCalledWith(
        { group: 'default', model: 'gpt-4.1' },
        expect.anything()
      )
    })
  })

  it('starts a sweep without waiting for its results', async () => {
    const user = userEvent.setup()
    renderPage(ROLE.ADMIN)

    await screen.findByTestId('model-iq-cell-default-gpt-4.1')
    await user.click(screen.getByRole('button', { name: 'Test all models' }))

    await waitFor(() => {
      expect(api.runAllPelicanTests).toHaveBeenCalled()
    })
  })

  it('marks a running sweep and refuses to start a second one', async () => {
    api.getModelIqOverview.mockResolvedValue(
      overview([modelStatus()], { sweeping: true, sweeping_since: 1700000000 })
    )
    renderPage(ROLE.ADMIN)

    expect(await screen.findByTestId('model-iq-sweeping')).toBeInTheDocument()
    const sweepButton = screen.getByRole('button', { name: 'Testing…' })
    expect(sweepButton).toBeDisabled()
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
})

describe('model IQ hover card', () => {
  it('lazily loads the last five runs of the hovered pair', async () => {
    const user = userEvent.setup()
    renderPage(ROLE.USER)

    const cell = await screen.findByTestId('model-iq-cell-vip-gpt-4.1')
    expect(api.getPelicanHistory).not.toHaveBeenCalled()

    await user.hover(cell)

    await waitFor(() => {
      expect(api.getPelicanHistory).toHaveBeenCalledWith('vip', 'gpt-4.1', 5)
    })
    expect(
      await screen.findByTestId('model-iq-sparkline-vip-gpt-4.1')
    ).toBeInTheDocument()
  })

  it('reuses the loaded history when the same pair is hovered again', async () => {
    const user = userEvent.setup()
    renderPage(ROLE.USER)

    const cell = await screen.findByTestId('model-iq-cell-default-gpt-4.1')
    await user.hover(cell)
    await screen.findByTestId('model-iq-hover-default-gpt-4.1')
    await user.unhover(cell)
    await user.hover(cell)

    await screen.findByTestId('model-iq-hover-default-gpt-4.1')
    const recentCalls = api.getPelicanHistory.mock.calls.filter(
      (call) => call[0] === 'default' && call[1] === 'gpt-4.1'
    )
    expect(recentCalls).toHaveLength(1)
  })

  it('opens the history dialog of the clicked cell', async () => {
    const user = userEvent.setup()
    renderPage(ROLE.USER)

    await user.click(await screen.findByTestId('model-iq-cell-vip-gpt-4.1'))

    await waitFor(() => {
      expect(api.getPelicanHistory.mock.calls).toContainEqual([
        'vip',
        'gpt-4.1',
      ])
    })
    expect(await screen.findByText('Test history')).toBeVisible()
  })
})

describe('model IQ polling', () => {
  it('refreshes the overview every five seconds while a sweep runs, then stops', async () => {
    vi.useFakeTimers()
    api.getModelIqOverview.mockResolvedValue(
      overview([modelStatus()], { sweeping: true })
    )
    const view = renderPage(ROLE.USER)

    await act(async () => {
      await vi.advanceTimersByTimeAsync(0)
    })
    expect(api.getModelIqOverview).toHaveBeenCalledTimes(1)

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000)
    })
    expect(api.getModelIqOverview.mock.calls.length).toBeGreaterThanOrEqual(2)

    // 巡视结束：下一轮不再排期，页面安静下来。
    api.getModelIqOverview.mockResolvedValue(overview([modelStatus()]))
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000)
    })
    const stoppedAt = api.getModelIqOverview.mock.calls.length
    await act(async () => {
      await vi.advanceTimersByTimeAsync(20000)
    })
    expect(api.getModelIqOverview).toHaveBeenCalledTimes(stoppedAt)

    view.unmount()
  })
})
