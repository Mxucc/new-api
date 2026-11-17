package service

import (
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestServiceStatusOfSuccessRate(t *testing.T) {
	tests := []struct {
		name    string
		rate    float64
		reports int64
		want    string
	}{
		// 服务状态只有可用与不可用：没人用不算挂了。
		{name: "no traffic is available", rate: 0, reports: 0, want: ServiceStatusAvailable},
		{name: "99% is available", rate: 99, reports: 1000, want: ServiceStatusAvailable},
		{name: "100% is available", rate: 100, reports: 1000, want: ServiceStatusAvailable},
		{name: "just below 99% is unavailable", rate: 98.99, reports: 1000, want: ServiceStatusUnavailable},
		{name: "95% is unavailable", rate: 95, reports: 1000, want: ServiceStatusUnavailable},
		{name: "half failing is unavailable", rate: 50, reports: 1000, want: ServiceStatusUnavailable},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			assert.Equal(t, tt.want, ServiceStatusOfSuccessRate(tt.rate, tt.reports))
		})
	}
}

func TestBuildServiceStatusPlacesSamplesOnTheCellGrid(t *testing.T) {
	now := time.Unix(1_700_000_000, 0)
	// 24 小时窗口切成 48 格，每格 1800 秒。
	samples := []ServiceStatusSample{
		{Model: "gpt-4.1", BucketTs: now.Unix() - 1800, Requests: 90, Successes: 90, LatencyMs: 90 * 1000},
		{Model: "gpt-4.1", BucketTs: now.Unix() - 3600, Requests: 10, Successes: 5, LatencyMs: 10 * 4000},
		{Model: "claude", BucketTs: now.Unix() - 1800, Requests: 10, Successes: 10, LatencyMs: 10 * 500},
	}
	view := BuildServiceStatus(samples, 24, now)

	require.Equal(t, 48, len(view.Cells))
	assert.Equal(t, int64(1800), view.CellSeconds)
	assert.Equal(t, now.Unix(), view.WindowEnd)
	assert.Equal(t, now.Unix()-24*3600, view.WindowStart)
	assert.Equal(t, now.Unix()-1800, view.DataThrough)

	// 最近一格：100 请求 / 100 成功 (gpt 90 + claude 10)，可用率 100%。
	last := view.Cells[len(view.Cells)-1]
	assert.Equal(t, int64(100), last.Requests)
	assert.InDelta(t, 100.0, last.SuccessRate, 0.001)
	assert.Equal(t, ServiceStatusAvailable, last.Status)

	// 倒数第二格：只有 gpt-4.1 的 10 请求 5 成功 → 50%。
	second := view.Cells[len(view.Cells)-2]
	assert.Equal(t, int64(10), second.Requests)
	assert.InDelta(t, 50.0, second.SuccessRate, 0.001)
	assert.Equal(t, ServiceStatusUnavailable, second.Status)

	// 没有流量的格子按可用处理，时间轴完整且不会画成故障。
	assert.Equal(t, ServiceStatusAvailable, view.Cells[0].Status)
	assert.Zero(t, view.Cells[0].Requests)

	assert.Equal(t, int64(110), view.Requests)
	assert.InDelta(t, 95.45, view.SuccessRate, 0.01)
	assert.Equal(t, ServiceStatusUnavailable, view.Status)
}

func TestBuildServiceStatusAggregatesPerModel(t *testing.T) {
	now := time.Unix(1_700_000_000, 0)
	samples := []ServiceStatusSample{
		{Model: "gpt-4.1", BucketTs: now.Unix() - 60, Requests: 60, Successes: 60, LatencyMs: 60 * 800},
		{Model: "gpt-4.1", BucketTs: now.Unix() - 7200, Requests: 40, Successes: 38, LatencyMs: 40 * 1700},
		{Model: "claude", BucketTs: now.Unix() - 60, Requests: 1, Successes: 1, LatencyMs: 300},
		// 窗口外采样必须被丢掉。
		{Model: "claude", BucketTs: now.Unix() - 25*3600, Requests: 500, Successes: 0},
	}
	view := BuildServiceStatus(samples, 24, now)

	require.Len(t, view.Models, 2)
	// 按请求量倒序，主力模型在前。
	assert.Equal(t, "gpt-4.1", view.Models[0].ModelName)
	assert.Equal(t, "claude", view.Models[1].ModelName)

	gpt := view.Models[0]
	assert.Equal(t, int64(100), gpt.Requests)
	assert.InDelta(t, 98.0, gpt.SuccessRate, 0.001)
	assert.Equal(t, ServiceStatusUnavailable, gpt.Status)
	// 延迟按请求数加权：(60*800 + 40*1700) / 100 = 1160。
	assert.Equal(t, int64(1160), gpt.AvgLatencyMs)
	assert.Equal(t, 48, len(gpt.Cells))
	assert.Equal(t, ServiceStatusAvailable, gpt.Cells[47].Status)
	assert.Equal(t, ServiceStatusUnavailable, gpt.Cells[44].Status)

	// 窗口外的 500 次失败不能污染 claude 的状态。
	assert.False(t, view.Models[1].ModelName == "claude" && view.Models[1].Requests > 1)
	assert.Equal(t, int64(1), view.Models[1].Requests)
	assert.Equal(t, ServiceStatusAvailable, view.Models[1].Status)
}

func TestBuildServiceStatusWithoutTrafficReportsAvailable(t *testing.T) {
	now := time.Unix(1_700_000_000, 0)
	view := BuildServiceStatus(nil, 72, now)

	// 没有任何流量：状态是可用（不是故障），时间轴按可用铺满。
	assert.Equal(t, ServiceStatusAvailable, view.Status)
	assert.Zero(t, view.Requests)
	assert.Zero(t, view.DataThrough)
	assert.Empty(t, view.Models)
	assert.Equal(t, 72, view.WindowHours)
	// 72 小时 / 48 格 = 5400 秒。
	assert.Equal(t, int64(5400), view.CellSeconds)
	require.Len(t, view.Cells, 48)
	for _, cell := range view.Cells {
		assert.Equal(t, ServiceStatusAvailable, cell.Status)
	}
}
