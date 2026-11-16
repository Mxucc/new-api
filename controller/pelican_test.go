package controller

import (
	"testing"
	"time"

	"github.com/QuantumNous/new-api/model"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestPelicanSweepTargetsOnlyProbesConfiguredPairsWithAChannel(t *testing.T) {
	available := []model.PelicanProbeTarget{
		{Group: "default", Model: "gpt-4.1"},
		{Group: "vip", Model: "gpt-4.1"},
	}
	cases := []struct {
		name       string
		configured []model.PelicanProbeTarget
		want       []model.PelicanProbeTarget
	}{
		// 没配置就是一个都不测：巡视不再是「自动覆盖全部可用组合」。
		{name: "no configured target", configured: nil, want: []model.PelicanProbeTarget{}},
		{name: "configured pairs", configured: available, want: available},
		// 配置的模型在该分组已经没有启用渠道时跳过，不去测一个必然失败的组合。
		{
			name:       "configured model lost its channel",
			configured: []model.PelicanProbeTarget{{Group: "vip", Model: "claude-opus"}},
			want:       []model.PelicanProbeTarget{},
		},
		// 只测配置里的组合，可测但没配置的分组不会被顺手带上。
		{
			name:       "unconfigured pairs stay untested",
			configured: []model.PelicanProbeTarget{{Group: "vip", Model: "gpt-4.1"}},
			want:       []model.PelicanProbeTarget{{Group: "vip", Model: "gpt-4.1"}},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			assert.Equal(t, tc.want, pelicanSweepTargets(tc.configured, available))
		})
	}
}

func TestPelicanProbeUserIdUsesTheAdminWhoSavedTheConfig(t *testing.T) {
	// 谁保存配置，探测的用量就记在谁名下。
	assert.Equal(t, 7, pelicanProbeUserId(model.PelicanTestConfig{OwnerUserId: 7}))
	// 旧配置没有记账身份时退回初始管理员，探测不会因为缺账主而停摆。
	assert.Equal(t, pelicanInitialAdminUserId, pelicanProbeUserId(model.PelicanTestConfig{}))
}

func TestPelicanProbeRegistrationExpiresAndIsTakenOver(t *testing.T) {
	target := model.PelicanProbeTarget{Group: "default", Model: "gpt-4.1"}
	t.Cleanup(func() { pelicanProbeRunning.Delete(target) })

	token, started := pelicanProbeBegin(target)
	require.True(t, started)
	assert.True(t, pelicanProbeActive(target))

	// 同一组合在跑时不会被再测一次。
	_, again := pelicanProbeBegin(target)
	assert.False(t, again)

	pelicanProbeEnd(target, token)
	assert.False(t, pelicanProbeActive(target))

	// 超过探测上限还留在登记表里（goroutine 卡死或被重启打断）：
	// 页面不该继续转圈，这个组合也要能重新触发。
	pelicanProbeRunning.Store(target, pelicanProbeRun{
		startedAt: time.Now().Add(-pelicanProbeStaleAfter - time.Minute).Unix(),
		token:     1,
	})
	assert.False(t, pelicanProbeActive(target))

	staleToken, restarted := pelicanProbeBegin(target)
	require.True(t, restarted)
	// 卡死的旧 goroutine 结束时不能把已经接管它的新登记删掉。
	pelicanProbeEnd(target, 1)
	assert.True(t, pelicanProbeActive(target))

	pelicanProbeEnd(target, staleToken)
	assert.False(t, pelicanProbeActive(target))
}

func TestPelicanStripWindowsFillsTheWholeStripFromOneResult(t *testing.T) {
	now := int64(1_800_000_000)
	cellSeconds := int64(model.PelicanStripHours*3600) / model.PelicanStripCells
	target := model.PelicanProbeTarget{Group: "default", Model: "gpt-4.1"}
	record := model.PelicanTestResult{
		Id:        1,
		Group:     "default",
		ModelName: "gpt-4.1",
		Verdict:   "healthy",
		Score:     92,
		CreatedAt: now - cellSeconds/2,
	}
	latest := map[model.PelicanProbeTarget]*model.PelicanTestResult{target: &record}

	cells := pelicanStripWindows([]model.PelicanProbeTarget{target}, latest, []model.PelicanTestResult{record}, now)[target]
	require.Len(t, cells, model.PelicanStripCells)
	// 一条结果也铺满整条：观测到的那一段是真实结果，其余沿用同一判定。
	for i, cell := range cells {
		assert.Equal(t, "healthy", cell.Verdict, "cell %d", i)
		assert.Equal(t, 92, cell.Score, "cell %d", i)
		assert.Equal(t, i != model.PelicanStripCells-1, cell.Filled, "cell %d", i)
	}
}

func TestPelicanStripWindowsFallsBackToTheLatestResultOutsideTheWindow(t *testing.T) {
	now := int64(1_800_000_000)
	target := model.PelicanProbeTarget{Group: "default", Model: "gpt-4.1"}
	record := model.PelicanTestResult{
		Id:        9,
		Group:     "default",
		ModelName: "gpt-4.1",
		Verdict:   "degraded",
		Score:     60,
		CreatedAt: now - 72*3600,
	}
	latest := map[model.PelicanProbeTarget]*model.PelicanTestResult{target: &record}

	// 窗口里没有记录：仍用最后一次结果把整条补满，而不是留一片灰。
	cells := pelicanStripWindows([]model.PelicanProbeTarget{target}, latest, nil, now)[target]
	require.Len(t, cells, model.PelicanStripCells)
	for i, cell := range cells {
		assert.Equal(t, "degraded", cell.Verdict, "cell %d", i)
		assert.True(t, cell.Filled, "cell %d", i)
	}
}

func TestPelicanStripWindowsKeepsAnUntestedPairEmpty(t *testing.T) {
	now := int64(1_800_000_000)
	target := model.PelicanProbeTarget{Group: "vip", Model: "claude"}

	cells := pelicanStripWindows([]model.PelicanProbeTarget{target}, nil, nil, now)[target]
	require.Len(t, cells, model.PelicanStripCells)
	for i, cell := range cells {
		// 一次都没测过就没有「好的」可补：格子留空，等第一次探测。
		assert.Equal(t, "pending", cell.Verdict, "cell %d", i)
		assert.False(t, cell.Filled, "cell %d", i)
	}
}
