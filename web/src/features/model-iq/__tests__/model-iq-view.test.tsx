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
import type { ModelIqOverview, ModelIqStatus, ModelIqStripCell } from '../types'

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

/** 四格时间条：最后一格是真实采样，其余沿用上一次成功的结果。 */
function stripCells(verdict: ModelIqStatus['verdict']): ModelIqStripCell[] {
  return Array.from({ length: 4 }, (_, index) => {
    const sampled = verdict !== 'pending' && index === 3
    return {
      start: 1700000000 + index * 1800,
      // 没采到样的格子沿用上一次成功结果，判定与最后一次成功相同。
      verdict,
      score: verdict === 'pending' ? 0 : 100,
      tested_at: sampled ? 1700000000 + index * 1800 : 0,
      sampled,
    }
  })
}

function modelStatus(overrides: Partial<ModelIqStatus> = {}): ModelIqStatus {
  const verdict = overrides.verdict ?? 'healthy'
  return {
    group: 'default',
    model_name: 'gpt-4.1',
    available: true,
    verdict,
    score: 100,
    latency_ms: 1200,
    tested_at: 1700000000,
    result_id: 11,
    channel_id: 7,
    dimensions: ['drawing', 'subject'],
    testing: false,
    strip: stripCells(verdict),
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
    // 说明留空：页面用前端翻译好的默认文案。
    description: '',
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
    owner_user_id: 2,
    owner_username: 'admin',
    prompt: '',
    prompt_contract: '',
    description: '',
    default_prompt: 'Draw a pelican.',
    default_contract: 'Output HTML only.',
    default_description: 'Every model gets the same request.',
    available_targets: [
      { group: 'default', model: 'gpt-4.1' },
      { group: 'vip', model: 'gpt-4.1' },
    ],
    missing_targets: [],
    magpie_url: '',
    magpie_token_set: false,
    magpie_groups: [],
    magpie_cooldown_minutes: 30,
    magpie_dry_run: false,
    magpie_groups_available: [],
    magpie_groups_error: '',
  })
  api.updatePelicanConfig.mockResolvedValue({
    targets: [{ group: 'default', model: 'gpt-4.1' }],
    interval_minutes: 30,
    owner_user_id: 2,
    owner_username: 'admin',
    prompt: '',
    prompt_contract: '',
    description: '',
    magpie_url: '',
    magpie_token_set: false,
    magpie_groups: [],
    magpie_cooldown_minutes: 30,
    magpie_dry_run: false,
    magpie_groups_available: [],
    magpie_groups_error: '',
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
  it('shows one row per group with its model and degraded rate', async () => {
    renderPage(ROLE.USER)

    // 每个分组只测一个模型：模型名写在格子里，不再有模型列。
    const cell = await screen.findByTestId('model-iq-cell-default-gpt-4.1')
    expect(cell).toHaveTextContent('gpt-4.1')
    // 全健康的一组不可用率是 0。
    expect(cell).toHaveTextContent('0% unavailable')
    // vip 分组的判定是 degraded：整条时间片都不可用，不可用率 100%。
    expect(screen.getByTestId('model-iq-cell-vip-gpt-4.1')).toHaveTextContent(
      '100% unavailable'
    )
    expect(screen.getAllByText('default').length).toBeGreaterThan(0)
    expect(screen.getAllByText('vip').length).toBeGreaterThan(0)
  })

  it('tells an admin when a result was reused instead of paid for twice', async () => {
    const reused = modelStatus({
      verdict: 'degraded',
      score: 60,
      source: 'magpie-ops',
      observed_at: 1699999800,
    })
    api.getModelIqOverview.mockResolvedValue(overview([reused]))
    renderPage(ROLE.ADMIN)

    const note = await screen.findByTestId('model-iq-reused-default-gpt-4.1')
    expect(note).toHaveTextContent('Reused from magpie-ops')
  })

  it('keeps every magpie detail away from ordinary users', async () => {
    api.getModelIqOverview.mockResolvedValue(
      overview([
        modelStatus({
          verdict: 'poor',
          source: 'magpie-ops',
          observed_at: 1699999800,
          magpie_group: 'never-stuck',
          magpie_result: 'gptpro/gpt-6-astra disable',
        }),
      ])
    )
    renderPage(ROLE.USER)

    // 普通用户只需要知道「可用 / 不可用」：背后接了哪套调度工具跟他们无关。
    await screen.findByTestId('model-iq-cell-default-gpt-4.1')
    expect(screen.queryByTestId('model-iq-reused-default-gpt-4.1')).toBeNull()
    expect(screen.queryByText('Reused from magpie-ops')).toBeNull()
    expect(screen.queryByTestId('model-iq-magpie-default-gpt-4.1')).toBeNull()
  })

  it('shows what magpie changed for a degraded group', async () => {
    api.getModelIqOverview.mockResolvedValue(
      overview([
        modelStatus({
          verdict: 'poor',
          magpie_group: 'never-stuck',
          magpie_adjusted_at: 1700000000,
          magpie_result: 'gptpro/gpt-6-astra disable',
        }),
      ])
    )
    renderPage(ROLE.ADMIN)

    // 对账结果比对「已请求调整」更具体：格子里直接显示动了哪个成员。
    const note = await screen.findByTestId('model-iq-magpie-default-gpt-4.1')
    expect(note).toHaveTextContent('gptpro/gpt-6-astra disable')
  })

  it('shows the score behind each magpie action, and keeps old records readable', async () => {
    api.getModelIqOverview.mockResolvedValue(
      overview([
        modelStatus({
          verdict: 'poor',
          magpie_group: 'never-stuck',
          magpie_adjusted_at: 1700000000,
          // 0 分是合法智商：不能被当成「没有分数」吞掉。
          magpie_actions: [
            { ref: 'gptpro/gpt-6-astra', kind: 'disable', score: 0 },
            { ref: 'dli/gpt-6-astra', kind: 'enable' },
          ],
        }),
      ])
    )
    renderPage(ROLE.ADMIN)

    const note = await screen.findByTestId('model-iq-magpie-default-gpt-4.1')
    expect(note).toHaveTextContent('gptpro/gpt-6-astra disable · 0')
    // 旧记录没有 score：只写 ref + kind。
    expect(note).toHaveTextContent('dli/gpt-6-astra enable')
  })

  it('keeps a configured group that was never tested, with an empty cell', async () => {
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

    // 配置了但还没测过的分组照样留一格：它就是管理员要触发的那一行。
    const cell = await screen.findByTestId('model-iq-cell-default-never-probed')
    expect(cell).toHaveTextContent('—')
    expect(cell).toHaveAccessibleName('default · never-probed Not tested')
  })

  it('keeps an untested combination inside its group row', async () => {
    renderPage(ROLE.USER)

    await screen.findByTestId('model-iq-cell-default-gpt-4.1')
    expect(
      screen.getByTestId('model-iq-cell-default-never-probed')
    ).toBeInTheDocument()
  })

  it('paints the sampled square and carries the last success into the rest', async () => {
    renderPage(ROLE.USER)

    const strip = await screen.findByTestId('model-iq-strip-default-gpt-4.1')
    const squares = strip.querySelectorAll('span')
    expect(squares).toHaveLength(4)
    // 最后一格是真测过的；前面三格沿用上一次成功结果（绿），并标注沿用。
    expect(squares[3]?.className).toContain('bg-success')
    expect(squares[3]?.className).not.toContain('opacity-60')
    expect(squares[0]?.className).toContain('bg-success')
    expect(squares[0]?.className).toContain('opacity-60')
    expect(squares[0]).toHaveAttribute(
      'title',
      expect.stringContaining('Carried over from the last successful run')
    )
  })

  it('shows a loading cell, without a stale score, while a combination is being tested', async () => {
    api.getModelIqOverview.mockResolvedValue(
      overview([
        modelStatus({ model_name: 'claude', testing: true, score: 40 }),
      ])
    )
    renderPage(ROLE.USER)

    const cell = await screen.findByTestId('model-iq-cell-default-claude')
    // 正在测的格子不拿上一次的分数冒充结论：只显示模型名和加载态。
    expect(cell).toHaveTextContent('claude')
    expect(cell).not.toHaveTextContent('unavailable')
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
    expect(screen.queryAllByRole('button', { name: 'Run test' })).toHaveLength(
      0
    )
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

  it('starts one probe straight from the row button', async () => {
    const user = userEvent.setup()
    renderPage(ROLE.ADMIN)

    // 每行一个「立即测试」：不必等整轮巡视。
    await user.click(
      await screen.findByRole('button', { name: 'Test vip now' })
    )

    await waitFor(() => {
      expect(api.runPelicanTest).toHaveBeenCalledWith(
        { group: 'vip', model: 'gpt-4.1' },
        expect.anything()
      )
    })
  })

  it('does not offer the row button to a regular user', async () => {
    renderPage(ROLE.USER)

    await screen.findByTestId('model-iq-cell-default-gpt-4.1')
    expect(
      screen.queryByRole('button', { name: 'Test default now' })
    ).toBeNull()
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

  it('saves a custom question and page description', async () => {
    const user = userEvent.setup()
    renderPage(ROLE.ADMIN)

    await user.click(
      await screen.findByRole('button', { name: 'Sweep settings' })
    )
    const prompt = await screen.findByLabelText('Probe prompt')
    // 留空时用内置题面做占位提示，管理员一眼能看到默认值。
    expect(prompt).toHaveAttribute('placeholder', 'Draw a pelican.')
    await user.type(prompt, '画一只猫')
    await user.type(
      screen.getByLabelText('Description shown to users'),
      '看谁画得好'
    )
    await user.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => {
      expect(api.updatePelicanConfig).toHaveBeenCalledWith(
        expect.objectContaining({
          prompt: '画一只猫',
          description: '看谁画得好',
        }),
        expect.anything()
      )
    })
  })

  it('configures one model per group and records the usage on the saving administrator', async () => {
    const user = userEvent.setup()
    renderPage(ROLE.ADMIN)

    await user.click(
      await screen.findByRole('button', { name: 'Sweep settings' })
    )

    // 行 = 分组：两个可测分组各一行下拉，而不是一串（分组, 模型）多选。
    expect(
      await screen.findByTestId('pelican-config-group-default')
    ).toBeInTheDocument()
    expect(screen.getByTestId('pelican-config-group-vip')).toBeInTheDocument()
    // 用量记在保存配置的那个管理员名下。
    expect(screen.getByTestId('pelican-config-owner')).toHaveTextContent(
      'admin'
    )

    await user.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => {
      expect(api.updatePelicanConfig).toHaveBeenCalledWith(
        {
          // 每个分组最多一个模型：没配过模型的 vip 分组不提交。
          targets: [{ group: 'default', model: 'gpt-4.1' }],
          interval_minutes: 30,
          // 题面、交付约定与说明留空表示用内置默认。
          prompt: '',
          prompt_contract: '',
          description: '',
          // 没配 magpie 联动时字段照常提交，全部是「不调整」的默认值。
          magpie_url: '',
          magpie_token: '',
          magpie_token_clear: false,
          magpie_groups: [],
          magpie_cooldown_minutes: 30,
          magpie_dry_run: false,
          // 留空表示跟随默认（联动开着就复用），不是关掉。
          magpie_reuse_minutes: null,
        },
        expect.anything()
      )
    })
  })

  it('keeps the saved magpie token secret and sends the group mapping', async () => {
    // token 只写不读：能回显给浏览器的只有「已保存」这一个事实；
    // 映射是 new-api 分组 -> magpie 分组，动哪个成员由 magpie-ops 判定。
    api.getPelicanConfig.mockResolvedValue({
      targets: [{ group: 'default', model: 'gpt-4.1' }],
      interval_minutes: 30,
      owner_user_id: 2,
      owner_username: 'admin',
      prompt: '',
      prompt_contract: '',
      description: '',
      default_prompt: 'Draw a pelican.',
      default_contract: 'Output HTML only.',
      default_description: 'Every model gets the same request.',
      available_targets: [
        { group: 'default', model: 'gpt-4.1' },
        { group: 'vip', model: 'gpt-4.1' },
      ],
      missing_targets: [],
      magpie_url: 'http://127.0.0.1:7788',
      magpie_token_set: true,
      magpie_groups: [{ group: 'default', magpie_group: 'never-stuck' }],
      magpie_cooldown_minutes: 30,
      magpie_dry_run: true,
      magpie_groups_available: [
        { id: 'never-stuck', name: '永不卡住', routing: 'order', members: [] },
      ],
      magpie_groups_error: '',
    })
    const user = userEvent.setup()
    renderPage(ROLE.ADMIN)

    await user.click(
      await screen.findByRole('button', { name: 'Sweep settings' })
    )
    const token = await screen.findByLabelText('API token')
    expect(token).toHaveValue('')
    expect(token).toHaveAttribute(
      'placeholder',
      'A token is saved. Leave empty to keep it.'
    )
    // 映射行 = 一个探测分组一行，下拉里是 magpie-ops 给出的分组。
    expect(
      screen.getByTestId('pelican-magpie-group-default')
    ).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => {
      expect(api.updatePelicanConfig).toHaveBeenCalledWith(
        expect.objectContaining({
          magpie_url: 'http://127.0.0.1:7788',
          // 输入框留空就沿用已保存的 token，绝不把明文写回请求体。
          magpie_token: '',
          magpie_token_clear: false,
          magpie_groups: [{ group: 'default', magpie_group: 'never-stuck' }],
          magpie_cooldown_minutes: 30,
          magpie_dry_run: true,
          magpie_reuse_minutes: null,
        }),
        expect.anything()
      )
    })
  })

  it('lists the members of the mapped magpie group and who closed them', async () => {
    api.getPelicanConfig.mockResolvedValue({
      targets: [{ group: 'default', model: 'gpt-4.1' }],
      interval_minutes: 30,
      owner_user_id: 2,
      owner_username: 'admin',
      prompt: '',
      prompt_contract: '',
      description: '',
      default_prompt: 'Draw a pelican.',
      default_contract: 'Output HTML only.',
      default_description: 'Every model gets the same request.',
      available_targets: [
        { group: 'default', model: 'gpt-4.1' },
        { group: 'vip', model: 'gpt-4.1' },
      ],
      missing_targets: [],
      magpie_url: 'http://127.0.0.1:7788',
      magpie_token_set: true,
      magpie_groups: [{ group: 'default', magpie_group: 'never-stuck' }],
      magpie_cooldown_minutes: 30,
      magpie_dry_run: false,
      magpie_groups_available: [
        {
          id: 'never-stuck',
          name: '永不卡住',
          routing: 'order',
          policyEnabled: true,
          intervalMinutes: 30,
          members: [
            {
              ref: 'dli/gpt-6-astra',
              providerName: '嘀哩哩',
              verdict: 'healthy',
              score: 100,
              disabled: false,
              disabledByUs: false,
              rate: 1.1592,
            },
            {
              ref: 'gptpro/gpt-6-astra',
              verdict: 'poor',
              score: 30,
              disabled: true,
              disabledByUs: true,
            },
          ],
        },
      ],
      magpie_groups_error: '',
    })
    const user = userEvent.setup()
    renderPage(ROLE.ADMIN)

    await user.click(
      await screen.findByRole('button', { name: 'Sweep settings' })
    )
    const row = await screen.findByTestId('pelican-magpie-group-default')
    // 成员当天状态直接写在映射行下面：谁被关、是不是本工具关的。
    expect(row).toHaveTextContent('dli/gpt-6-astra')
    expect(row).toHaveTextContent('Multiplier 1.16×')
    expect(row).toHaveTextContent('closed (ours)')
    // magpie 侧也在巡检时还要提示重复探测。
    expect(
      await screen.findByTestId('pelican-magpie-policy-warning')
    ).toBeInTheDocument()
  })

  it('tells a reader that nothing is configured yet', async () => {
    api.getModelIqOverview.mockResolvedValue(overview([]))
    renderPage(ROLE.USER)

    expect(
      await screen.findByText('No probe targets configured')
    ).toBeInTheDocument()
    expect(screen.queryByTestId('model-iq-grid')).toBeNull()
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

  it('previews a run straight from the history list', async () => {
    const user = userEvent.setup()
    renderPage(ROLE.USER)

    await user.click(await screen.findByTestId('model-iq-cell-default-gpt-4.1'))
    // 历史里每条记录都能点开当时的画面，不用先猜是哪一条。
    await user.click(await screen.findByTestId('pelican-history-run-12'))

    await waitFor(() => {
      expect(api.getPelicanResult).toHaveBeenCalledWith(12)
    })
  })

  it('names the channel behind the result so an upstream error can be traced', async () => {
    const user = userEvent.setup()
    renderPage(ROLE.USER)

    await user.hover(await screen.findByTestId('model-iq-cell-default-gpt-4.1'))
    expect(await screen.findByText('Channel 7')).toBeVisible()
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
