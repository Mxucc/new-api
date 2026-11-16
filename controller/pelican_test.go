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

func TestPelicanStripCellsFollowTheSweepInterval(t *testing.T) {
	// 设置 30 分钟就是 48 格，设置 1 小时就是 24 格。
	assert.Equal(t, 48, model.PelicanStripCellsFor(30))
	assert.Equal(t, 24, model.PelicanStripCellsFor(60))
	assert.Equal(t, 12, model.PelicanStripCellsFor(120))
	// 不自动巡视时按 30 分钟切。
	assert.Equal(t, 48, model.PelicanStripCellsFor(0))
	assert.Equal(t, 1, model.PelicanStripCellsFor(model.PelicanMaxIntervalMinutes))
}

func TestPelicanStripWindowsCarriesTheLastSuccessIntoUnsampledCells(t *testing.T) {
	now := int64(1_800_000_000)
	interval := 60 // 24 格
	cellCount := model.PelicanStripCellsFor(interval)
	cellSeconds := int64(model.PelicanStripHours*3600) / int64(cellCount)
	target := model.PelicanProbeTarget{Group: "default", Model: "gpt-4.1"}
	success := model.PelicanTestResult{
		Id:        1,
		Group:     "default",
		ModelName: "gpt-4.1",
		Status:    "success",
		Verdict:   "healthy",
		Score:     92,
		CreatedAt: now - cellSeconds/2,
	}

	cells := pelicanStripWindows(
		[]model.PelicanProbeTarget{target},
		interval,
		[]model.PelicanTestResult{success},
		map[model.PelicanProbeTarget]*model.PelicanTestResult{target: &success},
		now,
	)[target]

	require.Len(t, cells, cellCount)
	for i, cell := range cells {
		// 只有最后一格真采到了样，其余沿用这一次成功的结果：整条是绿的，不是红的。
		assert.Equal(t, "healthy", cell.Verdict, "cell %d", i)
		assert.Equal(t, i == cellCount-1, cell.Sampled, "cell %d", i)
	}
}

func TestPelicanStripWindowsKeepsTransportFailuresOutOfTheStrip(t *testing.T) {
	now := int64(1_800_000_000)
	target := model.PelicanProbeTarget{Group: "default", Model: "gpt-4.1"}
	// 只有传输失败（上游 502 之类）时没有任何成功结果可沿用：格子留空，
	// 界面按默认绿渲染，不会因为一次 502 把整条染红。
	cells := pelicanStripWindows([]model.PelicanProbeTarget{target}, 30, nil, nil, now)[target]
	require.Len(t, cells, model.PelicanStripCellsFor(30))
	for i, cell := range cells {
		assert.Equal(t, "pending", cell.Verdict, "cell %d", i)
		assert.False(t, cell.Sampled, "cell %d", i)
	}
}
