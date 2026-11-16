package model

import (
	"testing"

	"github.com/QuantumNous/new-api/common"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func resetPelicanResults(t *testing.T) {
	t.Helper()
	require.NoError(t, DB.Exec("DELETE FROM pelican_test_results").Error)
}

// setPelicanOption 直接在内存配置里写一个键，测试结束后还原。
func setPelicanOption(t *testing.T, key, value string) {
	t.Helper()
	common.OptionMapRWMutex.Lock()
	previous, existed := common.OptionMap[key]
	if common.OptionMap == nil {
		common.OptionMap = map[string]string{}
	}
	common.OptionMap[key] = value
	common.OptionMapRWMutex.Unlock()
	t.Cleanup(func() {
		common.OptionMapRWMutex.Lock()
		defer common.OptionMapRWMutex.Unlock()
		if !existed {
			delete(common.OptionMap, key)
			return
		}
		common.OptionMap[key] = previous
	})
}

func TestSavePelicanTestResultKeepsTheNewestPerPair(t *testing.T) {
	resetPelicanResults(t)

	// 单模型保留 pelicanRetentionPerModel 条，写多一条把最旧的挤掉；
	// 同一模型的另一个分组仍然各自保留最近一次。
	for i := range pelicanRetentionPerModel + 5 {
		require.NoError(t, SavePelicanTestResult(&PelicanTestResult{
			ModelName: "gpt-4.1",
			Group:     "default",
			Status:    "success",
			Verdict:   "healthy",
			Score:     i,
			CreatedAt: int64(1700000000 + i),
		}))
	}
	require.NoError(t, SavePelicanTestResult(&PelicanTestResult{
		ModelName: "gpt-4.1",
		Group:     "vip",
		Status:    "success",
		Verdict:   "poor",
		Score:     30,
		CreatedAt: 1700000800,
	}))
	require.NoError(t, SavePelicanTestResult(&PelicanTestResult{
		ModelName: "claude-opus",
		Group:     "default",
		Status:    "failed",
		Verdict:   "failed",
		Error:     "upstream 502",
		CreatedAt: 1700000900,
	}))

	var stored int64
	require.NoError(t, DB.Model(&PelicanTestResult{}).Where("model_name = ?", "gpt-4.1").Count(&stored).Error)
	assert.Equal(t, int64(pelicanRetentionPerModel), stored)

	latest, err := GetLatestPelicanResults()
	require.NoError(t, err)
	require.Len(t, latest, 3)
	// default 分组的最新一次是最后写入的那条，分数等于最后一次的序号。
	assert.Equal(t, pelicanRetentionPerModel+4, latest[PelicanProbeTarget{Group: "default", Model: "gpt-4.1"}].Score)
	// 另一个分组只留自己的记录，不会被同模型的大批记录带走。
	assert.Equal(t, 30, latest[PelicanProbeTarget{Group: "vip", Model: "gpt-4.1"}].Score)
	claude := latest[PelicanProbeTarget{Group: "default", Model: "claude-opus"}]
	require.NotNil(t, claude)
	assert.Equal(t, "failed", claude.Verdict)
	assert.Equal(t, "upstream 502", claude.Error)
}

func TestGetPelicanProbeTargetsReturnsEnabledPairsInPriorityOrder(t *testing.T) {
	require.NoError(t, DB.Exec("DELETE FROM abilities").Error)
	t.Cleanup(func() { DB.Exec("DELETE FROM abilities") })

	priority := func(value int64) *int64 { return &value }
	abilities := []Ability{
		{Group: "default", Model: "gpt-4.1", ChannelId: 1, Enabled: true, Priority: priority(10)},
		{Group: "vip", Model: "gpt-4.1", ChannelId: 1, Enabled: true, Priority: priority(10)},
		// 同一组合的第二个渠道只算一条。
		{Group: "default", Model: "gpt-4.1", ChannelId: 2, Enabled: true, Priority: priority(5)},
		{Group: "default", Model: "claude-opus", ChannelId: 1, Enabled: true, Priority: priority(1)},
		{Group: "default", Model: "text-embedding-3-large", ChannelId: 1, Enabled: false, Priority: priority(20)},
	}
	for i := range abilities {
		require.NoError(t, DB.Create(&abilities[i]).Error)
	}

	targets, err := GetPelicanProbeTargets()
	require.NoError(t, err)
	assert.Equal(t, []PelicanProbeTarget{
		{Group: "default", Model: "gpt-4.1"},
		{Group: "vip", Model: "gpt-4.1"},
		{Group: "default", Model: "claude-opus"},
	}, targets)
}

func TestGetPelicanTestConfigKeepsOneModelPerGroup(t *testing.T) {
	setPelicanOption(t, PelicanTestConfigOption, `{"targets":[{"group":" vip ","model":"gpt-4.1"},{"group":"vip","model":"claude-opus"},{"group":"","model":"  "}],"interval_minutes":30,"owner_user_id":7}`)
	cfg := GetPelicanTestConfig()
	// 同一分组只留第一个模型，缺分组或模型的条目被丢掉。
	assert.Equal(t, []PelicanProbeTarget{{Group: "vip", Model: "gpt-4.1"}}, cfg.Targets)
	assert.Equal(t, 30, cfg.IntervalMinutes)
	assert.Equal(t, 7, cfg.OwnerUserId)

	// 默认空：没有配置就不探测任何组合。
	setPelicanOption(t, PelicanTestConfigOption, `{}`)
	assert.Empty(t, GetPelicanTestConfig().Targets)

	// 内容损坏时按未配置处理，而不是退回到「全都测」。
	setPelicanOption(t, PelicanTestConfigOption, `not-json`)
	assert.Empty(t, GetPelicanTestConfig().Targets)
}

func TestSavePelicanTestConfigRoundTripsTargetsAndOwner(t *testing.T) {
	require.NoError(t, DB.AutoMigrate(&Option{}))
	t.Cleanup(func() { assert.NoError(t, SavePelicanTestConfig(PelicanTestConfig{})) })

	require.NoError(t, SavePelicanTestConfig(PelicanTestConfig{
		Targets: []PelicanProbeTarget{
			{Group: " default ", Model: " gpt-4.1 "},
			{Group: "default", Model: "claude-opus"},
			{Group: "vip", Model: "gpt-4.1"},
		},
		IntervalMinutes: 15,
		OwnerUserId:     9,
	}))

	cfg := GetPelicanTestConfig()
	assert.Equal(t, []PelicanProbeTarget{
		{Group: "default", Model: "gpt-4.1"},
		{Group: "vip", Model: "gpt-4.1"},
	}, cfg.Targets)
	assert.Equal(t, 15, cfg.IntervalMinutes)
	assert.Equal(t, 9, cfg.OwnerUserId)
}

func TestGetPelicanTestHistoryFiltersByGroup(t *testing.T) {
	resetPelicanResults(t)

	for i := range 3 {
		require.NoError(t, SavePelicanTestResult(&PelicanTestResult{
			ModelName: "gpt-4.1",
			Group:     "default",
			Status:    "success",
			Verdict:   "healthy",
			Score:     90 + i,
			CreatedAt: int64(1700000000 + i),
		}))
	}
	require.NoError(t, SavePelicanTestResult(&PelicanTestResult{
		ModelName: "gpt-4.1",
		Group:     "vip",
		Status:    "success",
		Verdict:   "poor",
		Score:     20,
		CreatedAt: 1700000100,
	}))

	history, err := GetPelicanTestHistory("default", "gpt-4.1", 0)
	require.NoError(t, err)
	require.Len(t, history, 3)
	// 返回按 id 倒序，最新的在最前。
	assert.Equal(t, 92, history[0].Score)
	for _, record := range history {
		assert.Equal(t, "default", record.Group)
	}

	vipHistory, err := GetPelicanTestHistory("vip", "gpt-4.1", 5)
	require.NoError(t, err)
	require.Len(t, vipHistory, 1)
	assert.Equal(t, 20, vipHistory[0].Score)
}

func TestGetPelicanTestResultByIdReportsMissingRows(t *testing.T) {
	resetPelicanResults(t)

	saved := &PelicanTestResult{
		ModelName:  "gpt-4.1",
		Group:      "default",
		Status:     "success",
		Verdict:    "degraded",
		Score:      65,
		Dimensions: "drawing,subject",
		Output:     "<svg></svg>",
	}
	require.NoError(t, SavePelicanTestResult(saved))
	require.NotZero(t, saved.Id)

	found, err := GetPelicanTestResultById(saved.Id)
	require.NoError(t, err)
	assert.Equal(t, "<svg></svg>", found.Output)

	_, err = GetPelicanTestResultById(saved.Id + 999)
	require.Error(t, err)
	assert.True(t, IsPelicanResultMissing(err))
}
