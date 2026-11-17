package controller

import (
	"context"
	"io"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuantumNous/new-api/common"
	"github.com/QuantumNous/new-api/model"
	"github.com/QuantumNous/new-api/service"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// magpieCall 是假 magpie-ops 收到的一次请求。
type magpieCall struct {
	Path          string
	Group         string
	Reason        string
	Authorization string
}

// setupPelicanMagpieDB 给联动测试准备一份独立的 SQLite：配置与联动状态都写在 option 里。
func setupPelicanMagpieDB(t *testing.T) {
	t.Helper()
	previousDB, previousLogDB := model.DB, model.LOG_DB
	previousType, previousLogType := common.MainDatabaseType(), common.LogDatabaseType()
	previousMaster, previousCache, previousRedis, previousSQLite := common.IsMasterNode, common.MemoryCacheEnabled, common.RedisEnabled, common.SQLitePath
	common.OptionMapRWMutex.Lock()
	previousOptions := common.OptionMap
	common.OptionMap = map[string]string{}
	common.OptionMapRWMutex.Unlock()
	t.Cleanup(func() {
		model.DB, model.LOG_DB = previousDB, previousLogDB
		common.SetDatabaseTypes(previousType, previousLogType)
		common.IsMasterNode, common.MemoryCacheEnabled, common.RedisEnabled, common.SQLitePath = previousMaster, previousCache, previousRedis, previousSQLite
		common.OptionMapRWMutex.Lock()
		common.OptionMap = previousOptions
		common.OptionMapRWMutex.Unlock()
	})
	t.Setenv("SQL_DSN", "")
	t.Setenv("LOG_SQL_DSN", "")
	common.IsMasterNode, common.MemoryCacheEnabled, common.RedisEnabled = false, false, false
	common.SQLitePath = filepath.Join(t.TempDir(), "pelican-magpie.db")
	require.NoError(t, model.InitDB())
	sqlDB, err := model.DB.DB()
	require.NoError(t, err)
	t.Cleanup(func() { require.NoError(t, sqlDB.Close()) })
	model.LOG_DB = model.DB
	common.SetLogDatabaseType(common.MainDatabaseType())
	require.NoError(t, model.DB.AutoMigrate(&model.PelicanTestResult{}, &model.Option{}))
	// 默认关掉对账轮询：它是对账用例自己的主题，别让后台 goroutine 在收尾之后写库。
	previousAttempts, previousInterval := pelicanMagpieReconcileAttempts, pelicanMagpieReconcileInterval
	pelicanMagpieReconcileAttempts, pelicanMagpieReconcileInterval = 0, time.Millisecond
	t.Cleanup(func() {
		pelicanMagpieReconcileAttempts, pelicanMagpieReconcileInterval = previousAttempts, previousInterval
	})
}

// newMagpieOpsServer 起一个假的 magpie-ops：/api/adjust 按 status 回包，/api/groups 回固定清单。
func newMagpieOpsServer(t *testing.T, status int, payload string) (*httptest.Server, func() []magpieCall) {
	t.Helper()
	var mutex sync.Mutex
	calls := make([]magpieCall, 0)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, request *http.Request) {
		body, _ := io.ReadAll(io.LimitReader(request.Body, 4096))
		var act struct {
			Group  string `json:"group"`
			Reason string `json:"reason"`
		}
		_ = common.Unmarshal(body, &act)
		mutex.Lock()
		calls = append(calls, magpieCall{
			Path:          request.URL.Path,
			Group:         act.Group,
			Reason:        act.Reason,
			Authorization: request.Header.Get("Authorization"),
		})
		mutex.Unlock()
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(status)
		_, _ = w.Write([]byte(payload))
	}))
	t.Cleanup(server.Close)
	return server, func() []magpieCall {
		mutex.Lock()
		defer mutex.Unlock()
		return append([]magpieCall(nil), calls...)
	}
}

// newMagpieOpsRoutes 起一个按路径回包的假 magpie-ops：对账用例要 adjust 与 actions 各回一份。
func newMagpieOpsRoutes(t *testing.T, routes map[string]string) (*httptest.Server, func() []magpieCall) {
	t.Helper()
	var mutex sync.Mutex
	calls := make([]magpieCall, 0)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, request *http.Request) {
		body, _ := io.ReadAll(io.LimitReader(request.Body, 4096))
		var act struct {
			Group  string `json:"group"`
			Reason string `json:"reason"`
		}
		_ = common.Unmarshal(body, &act)
		mutex.Lock()
		calls = append(calls, magpieCall{
			Path:          request.URL.Path,
			Group:         act.Group,
			Reason:        act.Reason,
			Authorization: request.Header.Get("Authorization"),
		})
		mutex.Unlock()
		payload, ok := routes[request.URL.Path]
		if !ok {
			w.WriteHeader(http.StatusNotFound)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(payload))
	}))
	t.Cleanup(server.Close)
	return server, func() []magpieCall {
		mutex.Lock()
		defer mutex.Unlock()
		return append([]magpieCall(nil), calls...)
	}
}

// saveMagpieConfig 写一份开启联动的巡视设置：两个 new-api 分组各映射一个 magpie 分组。
func saveMagpieConfig(t *testing.T, url string, groups []model.PelicanMagpieGroup) {
	t.Helper()
	require.NoError(t, model.SavePelicanTestConfig(model.PelicanTestConfig{
		Targets: []model.PelicanProbeTarget{
			{Group: "default", Model: "gpt-4.1"},
			{Group: "vip", Model: "gpt-4.1"},
		},
		IntervalMinutes: 30,
		OwnerUserId:     1,
		MagpieURL:       url,
		MagpieToken:     "secret-token",
		MagpieGroups:    groups,
	}))
}

// probeResult 造一条本轮巡视写下的记录。
func probeResult(group string, status string, verdict string, score int) *model.PelicanTestResult {
	return &model.PelicanTestResult{
		ModelName: "gpt-4.1",
		Group:     group,
		ChannelId: 7,
		Status:    status,
		Verdict:   verdict,
		Score:     score,
		CreatedAt: time.Now().Unix(),
	}
}

func TestPelicanMagpieGroupForFollowsTheConfiguredMapping(t *testing.T) {
	cfg := model.PelicanTestConfig{MagpieGroups: []model.PelicanMagpieGroup{
		{Group: "default", MagpieGroup: "never-stuck"},
		{Group: "vip", MagpieGroup: "fast-lane"},
	}}
	magpieGroup, ok := pelicanMagpieGroupFor(cfg, "vip")
	require.True(t, ok)
	assert.Equal(t, "fast-lane", magpieGroup)

	// 没配映射的分组不联动：new-api 无从知道它对应 magpie 的哪个组。
	_, ok = pelicanMagpieGroupFor(cfg, "svip")
	assert.False(t, ok)
}

func TestPelicanMagpieAfterSweepAdjustsEachDegradedGroupOnce(t *testing.T) {
	setupPelicanMagpieDB(t)
	server, calls := newMagpieOpsServer(t, http.StatusAccepted, `{"started":true,"group":"never-stuck"}`)
	saveMagpieConfig(t, server.URL, []model.PelicanMagpieGroup{
		{Group: "default", MagpieGroup: "never-stuck"},
		{Group: "vip", MagpieGroup: "fast-lane"},
	})

	pelicanMagpieAfterSweep(context.Background(), []*model.PelicanTestResult{
		probeResult("default", "success", service.PelicanVerdictPoor, 30),
		// 同一分组的两条降智记录只该触发一次 adjust。
		probeResult("default", "success", service.PelicanVerdictDegraded, 50),
		probeResult("vip", "success", service.PelicanVerdictDegraded, 60),
		// 满血与传输失败都不动手。
		probeResult("default", "success", service.PelicanVerdictHealthy, 100),
		probeResult("vip", "failed", service.PelicanVerdictFailed, 0),
	})

	sent := calls()
	require.Len(t, sent, 2)
	assert.Equal(t, "/api/adjust", sent[0].Path)
	assert.Equal(t, "never-stuck", sent[0].Group)
	assert.Equal(t, "Bearer secret-token", sent[0].Authorization)
	assert.Contains(t, sent[0].Reason, "default")
	assert.Equal(t, "fast-lane", sent[1].Group)

	state := model.GetPelicanMagpieStates()["default"]
	assert.Equal(t, "never-stuck", state.MagpieGroup)
	assert.True(t, state.OK)
	assert.NotZero(t, state.AdjustedAt)
	assert.Empty(t, state.Error)
}

func TestPelicanMagpieAfterSweepSkipsUnmappedHealthyAndCoolingGroups(t *testing.T) {
	setupPelicanMagpieDB(t)
	server, calls := newMagpieOpsServer(t, http.StatusAccepted, `{"started":true}`)
	saveMagpieConfig(t, server.URL, []model.PelicanMagpieGroup{{Group: "default", MagpieGroup: "never-stuck"}})

	// 没有映射的分组与只有满血的分组都不触发。
	pelicanMagpieAfterSweep(context.Background(), []*model.PelicanTestResult{
		probeResult("vip", "success", service.PelicanVerdictPoor, 30),
		probeResult("default", "success", service.PelicanVerdictHealthy, 100),
	})
	assert.Empty(t, calls())

	// 冷却期内不再重复请求：调整会让 magpie 把整个分组现场测一遍。
	require.NoError(t, model.UpdatePelicanMagpieState("default", func(state *model.PelicanMagpieState) {
		state.MagpieGroup = "never-stuck"
		state.AdjustedAt = time.Now().Add(-5 * time.Minute).Unix()
	}))
	pelicanMagpieAfterSweep(context.Background(), []*model.PelicanTestResult{
		probeResult("default", "success", service.PelicanVerdictPoor, 30),
	})
	assert.Empty(t, calls())

	// 冷却过期后重试。
	require.NoError(t, model.UpdatePelicanMagpieState("default", func(state *model.PelicanMagpieState) {
		state.AdjustedAt = time.Now().Add(-2 * time.Hour).Unix()
	}))
	pelicanMagpieAfterSweep(context.Background(), []*model.PelicanTestResult{
		probeResult("default", "success", service.PelicanVerdictPoor, 30),
	})
	assert.Len(t, calls(), 1)
}

func TestPelicanMagpieAfterSweepDryRunOnlyRecordsTheIntent(t *testing.T) {
	setupPelicanMagpieDB(t)
	server, calls := newMagpieOpsServer(t, http.StatusAccepted, `{"started":true}`)
	saveMagpieConfig(t, server.URL, []model.PelicanMagpieGroup{{Group: "default", MagpieGroup: "never-stuck"}})
	cfg := model.GetPelicanTestConfig()
	cfg.MagpieDryRun = true
	require.NoError(t, model.SavePelicanTestConfig(cfg))

	pelicanMagpieAfterSweep(context.Background(), []*model.PelicanTestResult{
		probeResult("default", "success", service.PelicanVerdictPoor, 30),
	})

	// dry run 只记状态，magpie 那边一条请求都收不到。
	assert.Empty(t, calls())
	state := model.GetPelicanMagpieStates()["default"]
	assert.Equal(t, "never-stuck", state.MagpieGroup)
	assert.True(t, state.OK)
	assert.True(t, state.DryRun)
	assert.NotZero(t, state.AdjustedAt)
}

func TestPelicanMagpieAfterSweepKeepsAFailedAdjustInTheState(t *testing.T) {
	setupPelicanMagpieDB(t)
	server, calls := newMagpieOpsServer(t, http.StatusUnauthorized, `{"error":"invalid token"}`)
	saveMagpieConfig(t, server.URL, []model.PelicanMagpieGroup{{Group: "default", MagpieGroup: "never-stuck"}})

	pelicanMagpieAfterSweep(context.Background(), []*model.PelicanTestResult{
		probeResult("default", "success", service.PelicanVerdictPoor, 30),
	})

	require.Len(t, calls(), 1)
	state := model.GetPelicanMagpieStates()["default"]
	assert.False(t, state.OK)
	assert.Contains(t, state.Error, "401")
	assert.Contains(t, state.Error, "invalid token")

	// magpie 不可达时同样只记状态：探测记录已经落库，联动失败不影响它。
	deadServer, _ := newMagpieOpsServer(t, http.StatusOK, `{"started":true}`)
	deadURL := deadServer.URL
	deadServer.Close()
	saveMagpieConfig(t, deadURL, []model.PelicanMagpieGroup{{Group: "vip", MagpieGroup: "fast-lane"}})
	require.NoError(t, model.UpdatePelicanMagpieState("vip", func(state *model.PelicanMagpieState) {
		state.AdjustedAt = 0
	}))
	require.NotPanics(t, func() {
		pelicanMagpieAfterSweep(context.Background(), []*model.PelicanTestResult{
			probeResult("vip", "success", service.PelicanVerdictPoor, 30),
		})
	})
	assert.NotEmpty(t, model.GetPelicanMagpieStates()["vip"].Error)
}

func TestListMagpieGroupsReadsTheGroupPicker(t *testing.T) {
	setupPelicanMagpieDB(t)
	server, _ := newMagpieOpsServer(t, http.StatusOK, `{"groups":[
		{"id":"never-stuck","name":"永不卡住","routing":"order","policyEnabled":true,"intervalMinutes":30,"members":[
			{"ref":"dli/gpt-6-astra","provider":"dli","providerName":"嘀哩哩","model":"gpt-6-astra",
			 "verdict":"healthy","score":100,"disabled":false,"disabledByUs":true,
			 "rate":0.1648,"ttftMs":3900,"lastProbedAt":"2026-11-17T11:52:48+08:00"}
		]}
	]}`)

	groups, err := listMagpieGroups(context.Background(), model.PelicanTestConfig{
		MagpieURL:   server.URL,
		MagpieToken: "secret-token",
	})
	require.NoError(t, err)
	require.Len(t, groups, 1)
	assert.Equal(t, "never-stuck", groups[0].Id)
	assert.Equal(t, "永不卡住", groups[0].Name)
	// 两边都在巡检就是重复探测：设置页要靠这两个字段提示。
	assert.True(t, groups[0].PolicyEnabled)
	assert.Equal(t, 30, groups[0].IntervalMinutes)
	require.Len(t, groups[0].Members, 1)
	assert.Equal(t, "dli/gpt-6-astra", groups[0].Members[0].Ref)
	assert.Equal(t, "嘀哩哩", groups[0].Members[0].ProviderName)
	assert.True(t, groups[0].Members[0].DisabledByUs)
	assert.Equal(t, "2026-11-17T11:52:48+08:00", groups[0].Members[0].LastProbedAt)

	// 地址与 token 都不对时返回错误，设置页据此显示「列不出分组」而不是空下拉。
	badGroups, err := listMagpieGroups(context.Background(), model.PelicanTestConfig{
		MagpieURL:   strings.Replace(server.URL, "http://", "http://127.0.0.1:1", 1),
		MagpieToken: "secret-token",
	})
	require.Error(t, err)
	assert.Empty(t, badGroups)
}

func TestPelicanMagpieReconcileRecordsWhatMagpieChanged(t *testing.T) {
	setupPelicanMagpieDB(t)
	// 对账用例自己要跑轮询：把等待压到毫秒级，只试两次。
	pelicanMagpieReconcileAttempts = 2
	pelicanMagpieReconcileInterval = time.Millisecond
	server, calls := newMagpieOpsRoutes(t, map[string]string{
		"/api/adjust":  `{"started":true,"group":"never-stuck","since":"2026-11-17T12:57:22+08:00"}`,
		"/api/actions": `{"actions":[{"ref":"gptpro/gpt-6-astra","group":"never-stuck","kind":"disable","score":0,"ok":true}]}`,
	})
	saveMagpieConfig(t, server.URL, []model.PelicanMagpieGroup{{Group: "default", MagpieGroup: "never-stuck"}})

	pelicanMagpieAdjust(context.Background(), model.GetPelicanTestConfig(),
		probeResult("default", "success", service.PelicanVerdictPoor, 30), "never-stuck")

	// 对账是后台 goroutine：等它把「动了谁」写回状态。
	require.Eventually(t, func() bool {
		return len(model.GetPelicanMagpieStates()["default"].Actions) > 0
	}, 2*time.Second, 5*time.Millisecond)

	state := model.GetPelicanMagpieStates()["default"]
	require.Len(t, state.Actions, 1)
	assert.Equal(t, "gptpro/gpt-6-astra", state.Actions[0].Ref)
	assert.Equal(t, "disable", state.Actions[0].Kind)
	// 0 分是合法智商：指针区分「0 分」与「旧记录没有这个字段」。
	require.NotNil(t, state.Actions[0].Score)
	assert.Equal(t, 0, *state.Actions[0].Score)
	assert.Equal(t, "2026-11-17T12:57:22+08:00", state.Since)
	assert.True(t, state.OK)
	// 先请求调整、再拿 since 去 /api/actions 对账。
	paths := make([]string, 0, 2)
	for _, call := range calls() {
		paths = append(paths, call.Path)
	}
	require.NotEmpty(t, paths)
	assert.Equal(t, "/api/adjust", paths[0])
	assert.Contains(t, paths, "/api/actions")
}

func TestPelicanMagpieReconcileReportsNothingToAdjust(t *testing.T) {
	setupPelicanMagpieDB(t)
	pelicanMagpieReconcileAttempts = 1
	pelicanMagpieReconcileInterval = time.Millisecond
	server, _ := newMagpieOpsRoutes(t, map[string]string{
		"/api/adjust":  `{"started":true,"group":"never-stuck","since":"2026-11-17T12:57:22+08:00"}`,
		"/api/actions": `{"actions":[]}`,
	})
	saveMagpieConfig(t, server.URL, []model.PelicanMagpieGroup{{Group: "default", MagpieGroup: "never-stuck"}})

	pelicanMagpieAdjust(context.Background(), model.GetPelicanTestConfig(),
		probeResult("default", "success", service.PelicanVerdictHealthy, 100), "never-stuck")

	require.Eventually(t, func() bool {
		return model.GetPelicanMagpieStates()["default"].Result != ""
	}, 2*time.Second, 5*time.Millisecond)
	assert.Equal(t, "checked, nothing to adjust", model.GetPelicanMagpieStates()["default"].Result)
}

// magpieGroupsPayload 造一份 magpie-ops /api/groups 的回复：同一分组里两个成员
// 服务同一个模型，其中一个刚测过、分数更低。
func magpieGroupsPayload(t *testing.T, probedAt time.Time) string {
	t.Helper()
	payload, err := common.Marshal(map[string]any{
		"groups": []map[string]any{{
			"id":   "never-stuck",
			"name": "永不卡住",
			"members": []map[string]any{
				{"ref": "dli/gpt-6-astra", "model": "gpt-6-astra", "verdict": "healthy", "score": 100,
					"ttftMs": 4200, "lastProbedAt": probedAt.Format(time.RFC3339Nano)},
				{"ref": "gptpro/gpt-6-astra", "model": "gpt-6-astra", "verdict": "degraded", "score": 60,
					"ttftMs": 9100, "lastProbedAt": probedAt.Format(time.RFC3339Nano)},
				{"ref": "other/gpt-legacy", "model": "gpt-legacy", "verdict": "poor", "score": 20,
					"ttftMs": 1200, "lastProbedAt": probedAt.Format(time.RFC3339Nano)},
			},
		}},
	})
	require.NoError(t, err)
	return string(payload)
}

// linkedConfig 是一份完整的联动配置，指向假 magpie-ops。
func linkedConfig(url string, reuse *int) model.PelicanTestConfig {
	return model.PelicanTestConfig{
		MagpieURL:             url,
		MagpieToken:           "secret",
		MagpieGroups:          []model.PelicanMagpieGroup{{Group: "vip", MagpieGroup: "never-stuck"}},
		MagpieCooldownMinutes: 30,
		MagpieReuseMinutes:    reuse,
	}
}

// 联动开着时，同一个（分组, 模型）刚被 magpie-ops 测过：直接拿最差的那个答案，
// 不再自己花钱测一遍。
func TestReuseMagpieResultTakesTheWorstFreshMember(t *testing.T) {
	setupPelicanMagpieDB(t)
	server, _ := newMagpieOpsServer(t, http.StatusOK, magpieGroupsPayload(t, time.Now().Add(-2*time.Minute)))
	pelicanMagpieGroupsAt = time.Time{}

	reused, ok := reuseMagpieResult(context.Background(), linkedConfig(server.URL, nil), "vip", "gpt-6-astra", 7)
	require.True(t, ok, "a fresh result for the same group and model should be reused")
	assert.Equal(t, "gptpro/gpt-6-astra", reused.Member.Ref, "the worst member is what a user could land on")
	assert.Equal(t, 60, reused.Record.Score)
	assert.Equal(t, service.PelicanVerdictDegraded, reused.Record.Verdict)
	assert.Equal(t, model.PelicanSourceMagpie, reused.Record.Source)
	assert.Equal(t, 7, reused.Record.ChannelId)
	assert.Equal(t, "vip", reused.Record.Group)
	assert.True(t, reused.Record.ObservedAt > 0, "the moment it was really probed is kept")
}

// 窗口之外的答案不算数：那是旧答案，自己测才是对的。
func TestReuseMagpieResultIgnoresStaleAnswers(t *testing.T) {
	setupPelicanMagpieDB(t)
	server, _ := newMagpieOpsServer(t, http.StatusOK, magpieGroupsPayload(t, time.Now().Add(-3*time.Hour)))
	pelicanMagpieGroupsAt = time.Time{}

	_, ok := reuseMagpieResult(context.Background(), linkedConfig(server.URL, nil), "vip", "gpt-6-astra", 7)
	assert.False(t, ok, "a three hour old answer is not this minute's answer")
}

// 没联动、没映射、窗口为 0、模型对不上：都不该复用。
func TestReuseMagpieResultNeedsAFreshMappedModel(t *testing.T) {
	setupPelicanMagpieDB(t)
	server, _ := newMagpieOpsServer(t, http.StatusOK, magpieGroupsPayload(t, time.Now()))
	pelicanMagpieGroupsAt = time.Time{}

	unlinked := linkedConfig(server.URL, nil)
	unlinked.MagpieToken = ""
	_, ok := reuseMagpieResult(context.Background(), unlinked, "vip", "gpt-6-astra", 1)
	assert.False(t, ok, "without a token nothing may be reused")

	_, ok = reuseMagpieResult(context.Background(), linkedConfig(server.URL, nil), "unmapped", "gpt-6-astra", 1)
	assert.False(t, ok, "a group without a mapping is not reused")

	off := 0
	_, ok = reuseMagpieResult(context.Background(), linkedConfig(server.URL, &off), "vip", "gpt-6-astra", 1)
	assert.False(t, ok, "a zero window means: always probe")

	_, ok = reuseMagpieResult(context.Background(), linkedConfig(server.URL, nil), "vip", "gpt-5-mini", 1)
	assert.False(t, ok, "a model magpie never probed is not reused")
}

// magpie-ops 读不到时按「自己测」处理，不能因此把探测卡住。
func TestReuseMagpieResultFallsBackWhenMagpieIsDown(t *testing.T) {
	setupPelicanMagpieDB(t)
	server, _ := newMagpieOpsServer(t, http.StatusInternalServerError, `{"error":"boom"}`)
	pelicanMagpieGroupsAt = time.Time{}

	_, ok := reuseMagpieResult(context.Background(), linkedConfig(server.URL, nil), "vip", "gpt-6-astra", 1)
	assert.False(t, ok, "an unreachable magpie-ops means: probe it yourself")
}

// 复用来的结果不该再触发一次 adjust：那个分组刚被 magpie 自己测过。
func TestPelicanMagpieAfterSweepSkipsReusedResults(t *testing.T) {
	setupPelicanMagpieDB(t)
	server, calls := newMagpieOpsServer(t, http.StatusOK, `{"started":true,"group":"never-stuck","since":"2026-01-01T00:00:00Z"}`)
	cfg := linkedConfig(server.URL, nil)
	require.NoError(t, model.SavePelicanTestConfig(cfg))

	pelicanMagpieAfterSweep(context.Background(), []*model.PelicanTestResult{{
		Group: "vip", ModelName: "gpt-6-astra", Status: "success",
		Verdict: service.PelicanVerdictDegraded, Score: 60, Source: model.PelicanSourceMagpie,
	}})

	assert.Empty(t, calls(), "a reused result must not ask magpie to adjust again")
}
