package controller

import (
	"context"
	"errors"
	"net/http"
	"slices"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/QuantumNous/new-api/common"
	"github.com/QuantumNous/new-api/model"
	"github.com/QuantumNous/new-api/service"
	"github.com/gin-gonic/gin"
)

// 鹈鹕测智要模型交付一整段 SVG 动画，输出上限远高于健康检查用的 “hi”。
const pelicanProbeMaxTokens = 8000

// 流式探针最多读回多少字节：整段动画源码要拼回来打分，8KB 的日志上限远远不够。
const pelicanProbeStreamCaptureBytes = 2 << 20

// 一次鹈鹕探测的单次耗时上限，超时按失败记录，避免整轮巡视卡在一个组合上。
const pelicanProbeTimeout = 3 * time.Minute

// pelicanNonChatMarkers 排除没有对话输出的模型：它们无法回答这条题面，
// 探测只会得到无意义的失败记录。
var pelicanNonChatMarkers = []string{
	"embedding",
	"bge-",
	"m3e",
	"rerank",
	"whisper",
	"tts",
	"audio",
	"image",
	"dall-e",
	"midjourney",
	"mj_",
	"suno",
	"kling",
	"vidu",
	"jimeng",
	"doubao-video",
	"video",
}

type pelicanModelStatus struct {
	Group     string `json:"group"`
	ModelName string `json:"model_name"`
	Available bool   `json:"available"`
	Verdict   string `json:"verdict"`
	Score     int    `json:"score"`
	LatencyMs int64  `json:"latency_ms"`
	TestedAt  int64  `json:"tested_at"`
	ResultId  int64  `json:"result_id"`
	// ChannelId 是这次结果来自哪条渠道：上游报 502 之类时据此定位是哪条渠道。
	ChannelId  int      `json:"channel_id"`
	Dimensions []string `json:"dimensions"`
	LastError  string   `json:"last_error,omitempty"`
	// Source 是这条结果的来源：空表示本机测的，magpie-ops 表示直接取了它的结果
	// （没有重复花钱）；ObservedAt 是它真正被测出来的时刻。
	Source     string `json:"source,omitempty"`
	ObservedAt int64  `json:"observed_at,omitempty"`
	// Magpie 联动状态：Group 是映射到的 magpie 分组，AdjustedAt 是上次请求调整的时间，
	// Error 是那次请求失败的原因（判定与处置由 magpie-ops 自己做，这里只记「请过了」）。
	MagpieGroup      string `json:"magpie_group,omitempty"`
	MagpieAdjustedAt int64  `json:"magpie_adjusted_at,omitempty"`
	// MagpieActions 是上一次调整的动作明细（动了谁、做了什么、当时智商）。
	MagpieActions []model.PelicanMagpieActionResult `json:"magpie_actions,omitempty"`
	// MagpieResult 是没有结构化动作时的状态文本（dry run / 无需调整 / 读不到动作记录）。
	MagpieResult string `json:"magpie_result,omitempty"`
	MagpieError  string `json:"magpie_error,omitempty"`
	// Testing 表示这个组合此刻正有一次探测在跑：页面据此显示加载态并保持轮询。
	Testing bool `json:"testing"`
	// Strip 是最近 24 小时的时间格子：判定取该时段最新的一次结果，
	// 没有观测的时段用最近的一次结果补上并标记 filled。
	Strip []pelicanStripCell `json:"strip"`
}

// pelicanStripCell 是智商时间条上的一小格：一格对应一次巡视间隔。
// Sampled 为真表示这一格真的跑过一次并拿到了输出；否则判定沿用上一次成功的结果。
type pelicanStripCell struct {
	Start    int64  `json:"start"`
	Verdict  string `json:"verdict"`
	Score    int    `json:"score"`
	TestedAt int64  `json:"tested_at"`
	Sampled  bool   `json:"sampled"`
}

// isPelicanChatModel 判断模型是否属于可以回答题面的对话模型。
func isPelicanChatModel(modelName string) bool {
	lower := strings.ToLower(modelName)
	for _, marker := range pelicanNonChatMarkers {
		if strings.Contains(lower, marker) {
			return false
		}
	}
	return true
}

// splitPelicanDimensions 还原记录里的要素列表。
func splitPelicanDimensions(raw string) []string {
	dimensions := make([]string, 0, 5)
	for part := range strings.SplitSeq(raw, ",") {
		if trimmed := strings.TrimSpace(part); trimmed != "" {
			dimensions = append(dimensions, trimmed)
		}
	}
	return dimensions
}

// pelicanAvailableTargets 筛出当前真正可测的（分组, 模型）组合：有启用渠道，
// 且能回答题面的对话模型。
func pelicanAvailableTargets(probes []model.PelicanProbeTarget) []model.PelicanProbeTarget {
	available := make([]model.PelicanProbeTarget, 0, len(probes))
	for _, target := range probes {
		if isPelicanChatModel(target.Model) {
			available = append(available, target)
		}
	}
	return available
}

// pelicanSweepTargets 返回本轮要测的组合：后台配置是白名单，只有管理员配过、且当前
// 仍有启用渠道的组合才会被探测。没配置就是一个都不测，不再自动覆盖全部可用组合。
func pelicanSweepTargets(configured []model.PelicanProbeTarget, available []model.PelicanProbeTarget) []model.PelicanProbeTarget {
	serveable := make(map[model.PelicanProbeTarget]struct{}, len(available))
	for _, target := range available {
		serveable[target] = struct{}{}
	}
	targets := make([]model.PelicanProbeTarget, 0, len(configured))
	for _, target := range configured {
		if _, ok := serveable[target]; !ok {
			continue
		}
		targets = append(targets, target)
	}
	return targets
}

// pelicanSweepPlan 读取巡视设置与当前可用组合，算出这一轮实际要测的组合。
func pelicanSweepPlan() (model.PelicanTestConfig, []model.PelicanProbeTarget, error) {
	cfg := model.GetPelicanTestConfig()
	// 绑定 magpie 分组之后，模型以 magpie-ops 正在测的为准：
	// new-api 不再挑模型，也不再自己探测，只把它的结果取回来显示。
	if pelicanMagpieReadOnly(cfg) {
		targets, err := magpieProbeTargets(context.Background(), cfg)
		return cfg, targets, err
	}
	if len(cfg.Targets) == 0 {
		return cfg, nil, nil
	}
	probes, err := model.GetPelicanProbeTargets()
	if err != nil {
		return cfg, nil, err
	}
	return cfg, pelicanSweepTargets(cfg.Targets, pelicanAvailableTargets(probes)), nil
}

func pelicanStatusOf(target model.PelicanProbeTarget, available bool, result *model.PelicanTestResult) pelicanModelStatus {
	status := pelicanModelStatus{
		Group:      target.Group,
		ModelName:  target.Model,
		Available:  available,
		Verdict:    service.PelicanVerdictPending,
		Dimensions: []string{},
	}
	if result == nil {
		return status
	}
	status.Verdict = result.Verdict
	status.Score = result.Score
	status.LatencyMs = result.LatencyMs
	status.TestedAt = result.CreatedAt
	status.ResultId = result.Id
	status.ChannelId = result.ChannelId
	status.Dimensions = splitPelicanDimensions(result.Dimensions)
	status.LastError = result.Error
	// 这条结果是从 magpie-ops 取来的：页面要能说出来，而不是假装本机测过。
	status.Source = result.Source
	status.ObservedAt = result.ObservedAt
	return status
}

// pelicanStatusBeats 决定同一个分组里谁代表这一行：先看有没有测出结论，
// 再比智商分。两边都没测过时留第一个，页面照样能显示「未测试」。
func pelicanStatusBeats(candidate, current pelicanModelStatus) bool {
	candidateTested := candidate.Verdict != service.PelicanVerdictPending
	currentTested := current.Verdict != service.PelicanVerdictPending
	if candidateTested != currentTested {
		return candidateTested
	}
	return candidate.Score > current.Score
}

// pelicanStripWindows 按巡视间隔把最近 24 小时切成小格（30 分钟 = 48 格，1 小时 = 24 格）。
// 一格对应一次巡视：采到了就显示那次判定；没采到（含上游 502 这类传输失败）就沿用
// 上一次成功的结果——默认是绿的，只有真正拿到输出的降智才变色。
func pelicanStripWindows(targets []model.PelicanProbeTarget, intervalMinutes int, successes []model.PelicanTestResult, latestSuccess map[model.PelicanProbeTarget]*model.PelicanTestResult, now int64) map[model.PelicanProbeTarget][]pelicanStripCell {
	cellCount := model.PelicanStripCellsFor(intervalMinutes)
	windowSeconds := int64(model.PelicanStripHours * 3600)
	cellSeconds := windowSeconds / int64(cellCount)
	windowStart := now - windowSeconds
	slots := make(map[model.PelicanProbeTarget][]*model.PelicanTestResult, len(targets))
	for i := range successes {
		record := &successes[i]
		index := int((record.CreatedAt - windowStart) / cellSeconds)
		if index < 0 || index >= cellCount {
			continue
		}
		target := model.PelicanProbeTarget{Group: record.Group, Model: record.ModelName}
		// 查询按 id 倒序：先落到某个时段的记录就是该时段最新的一条。
		if slots[target] == nil {
			slots[target] = make([]*model.PelicanTestResult, cellCount)
		}
		if slots[target][index] == nil {
			slots[target][index] = record
		}
	}
	strips := make(map[model.PelicanProbeTarget][]pelicanStripCell, len(targets))
	for _, target := range targets {
		observed := slots[target]
		cells := make([]pelicanStripCell, cellCount)
		// 从左往右走一遍，记住「截至这一格最近一次成功」，用来补没采样的格子。
		carry := latestSuccess[target]
		for i := range cells {
			cells[i] = pelicanStripCell{
				Start:   windowStart + int64(i)*cellSeconds,
				Verdict: service.PelicanVerdictPending,
			}
			if observed != nil && observed[i] != nil {
				// 这一格真的采到了：观测到的成功优先，并成为后面格子的沿用值。
				carry = observed[i]
				cells[i].Verdict = observed[i].Verdict
				cells[i].Score = observed[i].Score
				cells[i].TestedAt = observed[i].CreatedAt
				cells[i].Sampled = true
				continue
			}
			if carry == nil {
				continue
			}
			cells[i].Verdict = carry.Verdict
			cells[i].Score = carry.Score
			cells[i].TestedAt = carry.CreatedAt
		}
		strips[target] = cells
	}
	return strips
}

// GetPelicanOverview 返回管理员配置的每个（分组, 模型）探测项的状态与智商，登录用户可见。
// 探测由系统自己跑，所有用户读到的是同一份结果。
func GetPelicanOverview(c *gin.Context) {
	cfg := model.GetPelicanTestConfig()
	probes, err := model.GetPelicanProbeTargets()
	if err != nil {
		common.ApiError(c, err)
		return
	}
	latest, err := model.GetLatestPelicanResults()
	if err != nil {
		common.ApiError(c, err)
		return
	}
	now := time.Now().Unix()
	// 时间条只认成功的结果：上游 502 之类算「这次没采到样」，不算降智。
	successes, err := model.GetPelicanSuccessfulResults(now - int64(model.PelicanStripHours*3600))
	if err != nil {
		common.ApiError(c, err)
		return
	}
	latestSuccess, err := model.GetLatestPelicanSuccesses()
	if err != nil {
		common.ApiError(c, err)
		return
	}
	// 只读联动（绑定了 magpie 分组）时模型的取舍以 magpie-ops 为准：页面要显示它
	// 真正在测的那些成员，否则巡视把结果写进结果表也没人看得到。
	readOnly := pelicanMagpieReadOnly(cfg)
	targets := cfg.Targets
	if readOnly {
		magpieTargets, magpieErr := magpieProbeTargets(c.Request.Context(), cfg)
		if magpieErr != nil {
			common.SysLog(common.LogText("pelican magpie: cannot list the measured members: %s", magpieErr.Error()))
		} else {
			targets = magpieTargets
		}
	}
	serveable := make(map[model.PelicanProbeTarget]struct{}, len(targets))
	for _, target := range pelicanAvailableTargets(probes) {
		serveable[target] = struct{}{}
	}
	strips := pelicanStripWindows(targets, cfg.IntervalMinutes, successes, latestSuccess, now)
	// 联动状态只在动作发生时写，读一次就够所有格子用。
	magpieStates := model.GetPelicanMagpieStates()
	// 只上报要显示的探测项：只读联动时是 magpie 正在测的成员，否则是管理员配置的分组。
	statuses := make([]pelicanModelStatus, 0, len(targets))
	for _, target := range targets {
		_, served := serveable[target]
		// 只读联动的组合由 magpie-ops 测，本机有没有渠道不影响它是否被测。
		if readOnly {
			served = true
		}
		status := pelicanStatusOf(target, served, latestSuccess[target])
		// 最近一次如果只是传输失败（502 之类），不改变判定，只把错误留给排查用。
		if newest := latest[target]; newest != nil && newest.Status == "failed" {
			status.LastError = newest.Error
		}
		status.Testing = pelicanProbeActive(target)
		status.Strip = strips[target]
		// 联动状态按 new-api 分组记：一个分组一个 magpie 组，不按（分组, 模型）分开。
		// 还没请 magpie 调整过时，先显示配置里映射到的分组，否则页面上看不到联动开着。
		if mapped, ok := pelicanMagpieGroupFor(cfg, target.Group); ok {
			status.MagpieGroup = mapped
		}
		if state, ok := magpieStates[target.Group]; ok {
			if state.MagpieGroup != "" {
				status.MagpieGroup = state.MagpieGroup
			}
			status.MagpieAdjustedAt = state.AdjustedAt
			status.MagpieActions = state.Actions
			status.MagpieResult = state.Result
			status.MagpieError = state.Error
		}
		statuses = append(statuses, status)
	}
	// 一个分组只留智商最高的那个成员：magpie-ops 只往高分的渠道调度，低分的它自己会
	// 关掉，把所有成员都列出来只会把页面变成渠道清单。
	leading := make(map[string]pelicanModelStatus, len(statuses))
	for _, status := range statuses {
		if current, ok := leading[status.Group]; !ok || pelicanStatusBeats(status, current) {
			leading[status.Group] = status
		}
	}
	statuses = statuses[:0]
	for _, status := range leading {
		statuses = append(statuses, status)
	}
	slices.SortFunc(statuses, func(a, b pelicanModelStatus) int {
		return strings.Compare(a.Group, b.Group)
	})
	latestTestedAt := int64(0)
	for _, status := range statuses {
		if status.TestedAt > latestTestedAt {
			latestTestedAt = status.TestedAt
		}
	}
	sweeping, sweepingSince := pelicanSweepState()
	c.JSON(http.StatusOK, gin.H{
		"success": true,
		"data": gin.H{
			"models":           statuses,
			"total":            len(statuses),
			"latest_tested_at": latestTestedAt,
			"prompt":           cfg.EffectivePrompt(),
			// 说明原样返回：为空时前端用自己翻译的默认文案，避免中文默认值出现在英文界面。
			"description": cfg.Description,
			// 页面据此决定是否继续轮询：整轮巡视或任一个格子还在测就每 5 秒刷一次。
			"sweeping":       sweeping,
			"sweeping_since": sweepingSince,
		},
	})
}

// GetPelicanResult 返回一条记录，供页面在沙箱里预览模型交付的动画。
func GetPelicanResult(c *gin.Context) {
	id, err := strconv.ParseInt(c.Param("id"), 10, 64)
	if err != nil || id <= 0 {
		common.ApiErrorMsg(c, "invalid result id")
		return
	}
	result, err := model.GetPelicanTestResultById(id)
	if err != nil {
		if model.IsPelicanResultMissing(err) {
			common.ApiErrorMsg(c, "result not found")
			return
		}
		common.ApiError(c, err)
		return
	}
	// 复用的结论本机只有分数，动画留在 magpie-ops 那边：预览时回去取一次，
	// 取不到就说「这条动画没加载出来」，不把联动细节甩给普通用户。
	output, outputErr := result.Output, result.Error
	if output == "" && result.MagpieResultId != "" {
		cfg := model.GetPelicanTestConfig()
		if cfg.MagpieURL != "" && cfg.MagpieToken != "" {
			fetched, fetchErr := fetchMagpieResult(c.Request.Context(), cfg, result.MagpieResultId)
			switch {
			case fetchErr != nil:
				common.SysLog(common.LogText("pelican preview: cannot fetch result %s: %s", result.MagpieResultId, fetchErr.Error()))
				outputErr = "The animation for this run could not be loaded."
			case fetched.Error != "":
				outputErr = fetched.Error
			default:
				output = fetched.HTML
			}
		}
	}
	c.JSON(http.StatusOK, gin.H{
		"success": true,
		"data": gin.H{
			"id":          result.Id,
			"group":       result.Group,
			"model_name":  result.ModelName,
			"status":      result.Status,
			"verdict":     result.Verdict,
			"score":       result.Score,
			"latency_ms":  result.LatencyMs,
			"channel_id":  result.ChannelId,
			"dimensions":  splitPelicanDimensions(result.Dimensions),
			"output":      output,
			"error":       outputErr,
			"created_at":  result.CreatedAt,
			"source":      result.Source,
			"observed_at": result.ObservedAt,
		},
	})
}

// runPelicanProbe 通过站点自己的渠道链路向一个模型问一次鹈鹕题面，并写好记录。
// testUserID 是记账身份：探测照常写一条消费日志（令牌名 pelican-probe），记在
// 保存巡视设置的管理员名下，管理员据此把巡视开销与真实调用分开核对。
func runPelicanProbe(ctx context.Context, channel *model.Channel, modelName string, testUserID int, group string, prompt string) (*model.PelicanTestResult, error) {
	probeCtx, cancel := context.WithTimeout(ctx, pelicanProbeTimeout)
	defer cancel()

	startedAt := time.Now()
	// 按用户的方式请求：流式。上游网关对长时间无输出的非流式请求更容易超时并回 502，
	// 而流式只要开头有数据就不会被判超时；整段输出由 service.ChatCompletionOutput 拼回来。
	result := testChannelWithOptions(probeCtx, channel, testUserID, modelName, "", true, channelTestOptions{
		Prompt:             prompt,
		MaxTokens:          pelicanProbeMaxTokens,
		LogTokenName:       pelicanProbeLogTokenName,
		StreamCaptureBytes: pelicanProbeStreamCaptureBytes,
	})
	latencyMs := time.Since(startedAt).Milliseconds()

	record := &model.PelicanTestResult{
		ModelName: modelName,
		Group:     group,
		ChannelId: channel.Id,
		LatencyMs: latencyMs,
		CreatedAt: time.Now().Unix(),
	}
	switch {
	case result.newAPIError != nil:
		record.Status = "failed"
		record.Verdict = service.PelicanVerdictFailed
		record.Error = result.newAPIError.Error()
	case result.localErr != nil:
		record.Status = "failed"
		record.Verdict = service.PelicanVerdictFailed
		record.Error = result.localErr.Error()
	default:
		output, err := service.ChatCompletionOutput(result.responseBody)
		if err != nil {
			record.Status = "failed"
			record.Verdict = service.PelicanVerdictFailed
			record.Error = err.Error()
			break
		}
		score := service.ScorePelicanOutput(output)
		record.Status = "success"
		record.Verdict = score.Verdict
		record.Score = score.Score
		record.Dimensions = strings.Join(score.Dimensions, ",")
		record.Output = score.HTML
	}
	if err := model.SavePelicanTestResult(record); err != nil {
		return nil, err
	}
	// 联动不在这里做：一次巡视要汇总整轮结果才决定请 magpie 调整哪个分组。
	return record, nil
}

type pelicanTestRequest struct {
	Group string `json:"group"`
	Model string `json:"model"`
}

// pelicanProbeRunning 登记正在探测的（分组, 模型）组合，值是开始时间（Unix 秒）。
// 单组合触发与整轮巡视都先在这里登记，同一组合不会被并发探测两次；
// overview 也用它告诉页面哪些格子正在测。
var pelicanProbeRunning sync.Map

// pelicanProbeStaleAfter 是一次探测登记的失效时间：探测本身有 pelicanProbeTimeout
// 上限，超过它还在登记表里只可能是 goroutine 卡死或进程被重启打断。登记过期后
// 既不再让页面转圈，也不再挡住下一次触发。
const pelicanProbeStaleAfter = pelicanProbeTimeout + time.Minute

// pelicanProbeRun 是一条探测登记。token 让卡死的旧 goroutine 结束时不会把
// 已经接管它的新登记删掉。
type pelicanProbeRun struct {
	startedAt int64
	token     int64
}

func (run pelicanProbeRun) stale() bool {
	return run.startedAt > 0 && time.Since(time.Unix(run.startedAt, 0)) > pelicanProbeStaleAfter
}

// pelicanProbeBegin 登记一个组合。ok 为 false 表示该组合已经在跑；否则返回本次登记
// 的 token，结束时必须用同一个 token 注销。已有的登记已经过期时直接接管，
// 否则这个组合会永远测不动。
func pelicanProbeBegin(target model.PelicanProbeTarget) (token int64, ok bool) {
	if current, loaded := pelicanProbeRunning.Load(target); loaded {
		if run, isRun := current.(pelicanProbeRun); !isRun || !run.stale() {
			return 0, false
		}
		pelicanProbeRunning.Delete(target)
	}
	run := pelicanProbeRun{startedAt: time.Now().Unix(), token: time.Now().UnixNano()}
	if _, loaded := pelicanProbeRunning.LoadOrStore(target, run); loaded {
		return 0, false
	}
	return run.token, true
}

// pelicanProbeEnd 注销自己那条登记；被别的登记接管时什么都不做。
func pelicanProbeEnd(target model.PelicanProbeTarget, token int64) {
	current, ok := pelicanProbeRunning.Load(target)
	if !ok {
		return
	}
	run, isRun := current.(pelicanProbeRun)
	if !isRun || run.token != token {
		return
	}
	pelicanProbeRunning.CompareAndDelete(target, run)
}

// pelicanProbeActive 报告该组合此刻是否有一次探测在跑；过期的登记视为没在跑并清掉，
// 免得页面上一格永远转圈。
func pelicanProbeActive(target model.PelicanProbeTarget) bool {
	current, ok := pelicanProbeRunning.Load(target)
	if !ok {
		return false
	}
	run, isRun := current.(pelicanProbeRun)
	if !isRun {
		pelicanProbeRunning.Delete(target)
		return false
	}
	if run.stale() {
		pelicanProbeRunning.CompareAndDelete(target, run)
		return false
	}
	return true
}

// triggerPelicanTest 是单组合探测本身的入口：校验组合、选渠道、跑一次、落库。
// ctx 由调用方给出：异步触发与整轮巡视都用独立上下文，免得请求结束就打断探测。
func triggerPelicanTest(ctx context.Context, target model.PelicanProbeTarget, testUserID int) (*model.PelicanTestResult, error) {
	group := strings.TrimSpace(target.Group)
	modelName := strings.TrimSpace(target.Model)
	if group == "" || modelName == "" {
		return nil, errors.New("group and model are required")
	}
	if !isPelicanChatModel(modelName) {
		return nil, errors.New("model does not accept chat completions")
	}
	channel, err := model.GetChannel(group, modelName, 0, nil)
	if err != nil {
		return nil, err
	}
	if channel == nil {
		return nil, errors.New("no enabled channel serves this group/model: " + group + "/" + modelName)
	}
	cfg := model.GetPelicanTestConfig()
	if pelicanMagpieReadOnly(cfg) {
		// 只读：拿 magpie-ops 24 小时内的答案写进自己的结果表，一次探测都不发。
		reused, ok := reuseMagpieResultWithin(ctx, cfg, group, modelName, 0, pelicanMagpieReadOnlyWindowMinutes)
		if !ok {
			return nil, errors.New("magpie-ops 还没有 " + group + "/" + modelName + " 的近期结果（已绑定 magpie 分组，new-api 不再自己探测）")
		}
		if err := model.SavePelicanTestResult(reused.Record); err != nil {
			return nil, err
		}
		return reused.Record, nil
	}
	// 联动开着时先问一句「这个分组里的这个模型，magpie-ops 刚测过吗」：
	// 测智是整段回答，重复测就是重复花钱，而它给出的就是同一件事的答案。
	if reused, ok := reuseMagpieResult(ctx, cfg, group, modelName, channel.Id); ok {
		if err := model.SavePelicanTestResult(reused.Record); err != nil {
			return nil, err
		}
		common.SysLog(common.LogText(
			"pelican probe for %s/%s reused a magpie-ops result (%s, score %d, %s ago)",
			group, modelName, reused.Member.Ref, reused.Member.Score, time.Since(reused.Probed).Round(time.Second)))
		return reused.Record, nil
	}
	// 记录本身要往上带：一次巡视结束时用它汇总哪些分组降智了。
	return runPelicanProbe(ctx, channel, modelName, testUserID, group, cfg.EffectivePrompt())
}

// runPelicanProbeTarget 跑一次探测并把失败原因写进系统日志：探测改成异步之后
// 已经没有调用方能收到错误，日志是唯一还能查到原因的地方。
func runPelicanProbeTarget(ctx context.Context, target model.PelicanProbeTarget, testUserID int) (*model.PelicanTestResult, error) {
	record, err := triggerPelicanTest(ctx, target, testUserID)
	if err != nil {
		common.SysLog(common.LogText("pelican probe failed for %s/%s: %s", target.Group, target.Model, err))
	}
	return record, err
}

// RunPelicanTest 让管理员按需给一个（分组, 模型）组合测智商。
// 一次探测最长三分钟，所以这里只登记后就返回 202，结果由页面轮询 overview 取回。
// 记账身份取巡视设置里保存配置的管理员，与定时巡视记在同一本账上。
func RunPelicanTest(c *gin.Context) {
	var request pelicanTestRequest
	if err := c.ShouldBindJSON(&request); err != nil {
		common.ApiErrorMsg(c, "invalid request body")
		return
	}
	// 分组由请求给出，而不是取触发者自己的分组：管理员所在分组没有该组合时
	// 应当明确报错，而不是悄悄测到别的分组上。空值属于请求本身有误，同步返回。
	target := model.PelicanProbeTarget{
		Group: strings.TrimSpace(request.Group),
		Model: strings.TrimSpace(request.Model),
	}
	if target.Group == "" || target.Model == "" {
		common.ApiErrorMsg(c, "group and model are required")
		return
	}
	token, started := pelicanProbeBegin(target)
	if !started {
		c.JSON(http.StatusOK, gin.H{
			"success": true,
			"data":    gin.H{"started": false, "running": true},
		})
		return
	}
	probeUserId := pelicanProbeUserId(model.GetPelicanTestConfig())
	go func() {
		defer pelicanProbeEnd(target, token)
		record, err := runPelicanProbeTarget(context.Background(), target, probeUserId)
		if err != nil {
			return
		}
		// 单次测试就当作一轮只有这一条结果的巡视：降智才请 magpie 调整。
		pelicanMagpieAfterSweep(context.Background(), []*model.PelicanTestResult{record})
	}()
	c.JSON(http.StatusAccepted, gin.H{
		"success": true,
		"data":    gin.H{"started": true},
	})
}

// pelicanSweepConcurrency 是一次巡视的并发数：串行在组合多时要占住几分钟。
const pelicanSweepConcurrency = 3

// runPelicanSweep 巡视一轮，返回成功与失败的组合数。
func runPelicanSweep(ctx context.Context, testUserID int, targets []model.PelicanProbeTarget) (int, int) {
	var counters struct {
		sync.Mutex
		tested  int
		failed  int
		records []*model.PelicanTestResult
	}
	var wg sync.WaitGroup
	slots := make(chan struct{}, pelicanSweepConcurrency)
	for _, target := range targets {
		slots <- struct{}{}
		wg.Go(func() {
			defer func() { <-slots }()
			// 同一组合已经被单次触发抢占时不重复探测，也不计为失败。
			token, started := pelicanProbeBegin(target)
			if !started {
				return
			}
			defer pelicanProbeEnd(target, token)
			record, err := runPelicanProbeTarget(ctx, target, testUserID)
			counters.Lock()
			defer counters.Unlock()
			if err != nil {
				counters.failed++
				return
			}
			counters.tested++
			if record != nil {
				counters.records = append(counters.records, record)
			}
		})
	}
	wg.Wait()
	// 整轮结束后才联动：按分组去重，一个分组一轮最多请 magpie 调整一次。
	go pelicanMagpieAfterSweep(context.WithoutCancel(ctx), counters.records)
	return counters.tested, counters.failed
}

// RunAllPelicanTests 让管理员立刻巡视后台配置的组合。没配置时一个组合都不测。
// 整轮要好几分钟，所以这里和单组合一样只做登记，结果由页面轮询 overview 取回。
func RunAllPelicanTests(c *gin.Context) {
	cfg, targets, err := pelicanSweepPlan()
	if err != nil {
		common.ApiError(c, err)
		return
	}
	if len(targets) == 0 {
		// 没有配置，或配置的模型都下线了：不启动一轮空巡视，由页面提示去设置。
		c.JSON(http.StatusOK, gin.H{
			"success": true,
			"data":    gin.H{"started": false, "running": false, "targets": 0},
		})
		return
	}
	if !pelicanSweepBegin() {
		c.JSON(http.StatusOK, gin.H{
			"success": true,
			"data":    gin.H{"started": false, "running": true, "targets": len(targets)},
		})
		return
	}
	// 巡视用独立上下文：管理员关掉页面不应该把一轮巡视打断在半路。
	go func() {
		defer pelicanSweepEnd()
		tested, failed := runPelicanSweep(context.Background(), pelicanProbeUserId(cfg), targets)
		common.SysLog(common.LogText("pelican sweep finished: %d tested, %d failed", tested, failed))
	}()
	c.JSON(http.StatusAccepted, gin.H{
		"success": true,
		"data":    gin.H{"started": true, "targets": len(targets)},
	})
}

// GetPelicanHistory 返回一个（分组, 模型）组合的历史记录，供页面画趋势图。用户只读。
func GetPelicanHistory(c *gin.Context) {
	group := strings.TrimSpace(c.Query("group"))
	modelName := strings.TrimSpace(c.Query("model"))
	if group == "" || modelName == "" {
		common.ApiErrorMsg(c, "group and model are required")
		return
	}
	limit, _ := strconv.Atoi(c.Query("limit"))
	results, err := model.GetPelicanTestHistory(group, modelName, limit)
	if err != nil {
		common.ApiError(c, err)
		return
	}
	// 查询按 id 倒序取最近若干条，这里翻成时间正序方便直接画线。
	history := make([]gin.H, 0, len(results))
	for i := len(results) - 1; i >= 0; i-- {
		record := results[i]
		history = append(history, gin.H{
			"id":         record.Id,
			"group":      record.Group,
			"status":     record.Status,
			"verdict":    record.Verdict,
			"score":      record.Score,
			"latency_ms": record.LatencyMs,
			"channel_id": record.ChannelId,
			"dimensions": splitPelicanDimensions(record.Dimensions),
			"error":      record.Error,
			"created_at": record.CreatedAt,
		})
	}
	c.JSON(http.StatusOK, gin.H{
		"success": true,
		"data": gin.H{
			"group":      group,
			"model_name": modelName,
			"history":    history,
		},
	})
}

// DeletePelicanResult 删掉一条历史：管理员用它清掉写错的结论，图表与时间条跟着变。
func DeletePelicanResult(c *gin.Context) {
	id, err := strconv.ParseInt(c.Param("id"), 10, 64)
	if err != nil || id <= 0 {
		common.ApiErrorMsg(c, "invalid result id")
		return
	}
	if err := model.DeletePelicanTestResult(id); err != nil {
		common.ApiError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"success": true, "message": ""})
}

// ClearPelicanHistory 清掉一个（分组, 模型）的历史：两个参数都要给，不做「全清」。
// 删除不影响巡视设置与正在跑的探测。
func ClearPelicanHistory(c *gin.Context) {
	group := strings.TrimSpace(c.Query("group"))
	modelName := strings.TrimSpace(c.Query("model"))
	if group == "" || modelName == "" {
		common.ApiErrorMsg(c, "group and model are required")
		return
	}
	removed, err := model.DeletePelicanTestResults(group, modelName)
	if err != nil {
		common.ApiError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"success": true, "data": gin.H{"removed": removed}})
}

type pelicanConfigRequest struct {
	Targets         []model.PelicanProbeTarget `json:"targets"`
	IntervalMinutes int                        `json:"interval_minutes"`
	// 留空表示用内置默认：题面、交付约定与页面说明都可以整体替换。
	Prompt         string `json:"prompt"`
	PromptContract string `json:"prompt_contract"`
	Description    string `json:"description"`
	// magpie 联动：地址与 token 为空表示不联动，映射把 new-api 的分组接到 magpie 的路由组。
	MagpieURL             string                     `json:"magpie_url"`
	MagpieToken           string                     `json:"magpie_token"`
	MagpieClear           bool                       `json:"magpie_token_clear"`
	MagpieGroups          []model.PelicanMagpieGroup `json:"magpie_groups"`
	MagpieCooldownMinutes int                        `json:"magpie_cooldown_minutes"`
	MagpieDryRun          bool                       `json:"magpie_dry_run"`
	// MagpieReuseMinutes 是复用 magpie-ops 结果的窗口：null 表示跟随默认
	// （联动开着就复用），0 表示关掉复用、永远自己测。
	MagpieReuseMinutes *int `json:"magpie_reuse_minutes"`
}

// GetPelicanConfig 返回巡视设置与可选的（分组, 模型）组合，供后台设置页使用。
func GetPelicanConfig(c *gin.Context) {
	probes, err := model.GetPelicanProbeTargets()
	if err != nil {
		common.ApiError(c, err)
		return
	}
	available := pelicanAvailableTargets(probes)
	serveable := make(map[model.PelicanProbeTarget]struct{}, len(available))
	for _, target := range available {
		serveable[target] = struct{}{}
	}
	cfg := model.GetPelicanTestConfig()
	missing := make([]model.PelicanProbeTarget, 0)
	for _, target := range cfg.Targets {
		if _, ok := serveable[target]; !ok {
			missing = append(missing, target)
		}
	}
	// 记账身份是保存这份配置的管理员：设置页要写明用量记在谁名下。
	ownerUsername := ""
	if cfg.OwnerUserId > 0 {
		if username, nameErr := model.GetUsernameById(cfg.OwnerUserId, true); nameErr == nil {
			ownerUsername = username
		}
	}
	// magpie 的分组清单给设置页填下拉；拿不到时给一个空数组与错误原因，页面仍可手填 id。
	magpieGroupsAvailable := []magpieGroup{}
	magpieGroupsError := ""
	if cfg.MagpieURL != "" && cfg.MagpieToken != "" {
		if groups, listErr := listMagpieGroups(c.Request.Context(), cfg); listErr != nil {
			magpieGroupsError = pelicanMagpieDetail(listErr)
		} else {
			magpieGroupsAvailable = groups
		}
	}
	c.JSON(http.StatusOK, gin.H{
		"success": true,
		"data": gin.H{
			"targets":          cfg.Targets,
			"interval_minutes": cfg.IntervalMinutes,
			"owner_user_id":    cfg.OwnerUserId,
			"owner_username":   ownerUsername,
			// 原样返回管理员填的题面（可能为空）：设置页要能看出哪些是自定义的。
			"prompt":              cfg.Prompt,
			"prompt_contract":     cfg.PromptContract,
			"description":         cfg.Description,
			"default_prompt":      model.DefaultPelicanPrompt,
			"default_contract":    model.DefaultPelicanContract,
			"default_description": model.DefaultPelicanDescription,
			"available_targets":   available,
			"missing_targets":     missing,
			// magpie 联动：token 只回显「有没有设置」，绝不把明文发回浏览器。
			"magpie_url":              cfg.MagpieURL,
			"magpie_token_set":        cfg.MagpieToken != "",
			"magpie_groups":           cfg.MagpieGroups,
			"magpie_cooldown_minutes": cfg.MagpieCooldownMinutes,
			"magpie_dry_run":          cfg.MagpieDryRun,
			"magpie_reuse_minutes":    cfg.MagpieReuseMinutes,
			"magpie_groups_available": magpieGroupsAvailable,
			"magpie_groups_error":     magpieGroupsError,
			// 每个映射分组最近一次拿到的「最佳结果」（24 小时窗口）。
			// 绑定了 magpie 分组就是只读：模型以 magpie-ops 为准，设置页不再让选模型
			"magpie_read_only": pelicanMagpieReadOnly(cfg),
			"magpie_best":      magpieBest(),
			"magpie_best_at":   magpieBestAt(),
		},
	})
}

// UpdatePelicanConfig 保存巡视设置：每个分组只测一个模型，间隔 0–1440 分钟。
// 记账身份一律取会话里的管理员，请求体不能指定，免得伪造探测的账主。
func UpdatePelicanConfig(c *gin.Context) {
	adminUserId := c.GetInt("id")
	if adminUserId <= 0 {
		common.ApiErrorMsg(c, "invalid request")
		return
	}
	var request pelicanConfigRequest
	if err := c.ShouldBindJSON(&request); err != nil {
		common.ApiErrorMsg(c, "invalid request body")
		return
	}
	if request.IntervalMinutes < 0 || request.IntervalMinutes > model.PelicanMaxIntervalMinutes {
		common.ApiErrorMsg(c, "interval_minutes must be between 0 and 1440")
		return
	}
	// magpie 联动：地址必须是 http(s)，开联动时地址与 token 都得有。
	magpieURL := strings.TrimSpace(request.MagpieURL)
	if magpieURL != "" && !model.PelicanMagpieURLValid(magpieURL) {
		common.ApiErrorMsg(c, "magpie_url must start with http:// or https://")
		return
	}
	probes, err := model.GetPelicanProbeTargets()
	if err != nil {
		common.ApiError(c, err)
		return
	}
	serveable := make(map[model.PelicanProbeTarget]struct{})
	for _, target := range pelicanAvailableTargets(probes) {
		serveable[target] = struct{}{}
	}
	groups := make(map[string]struct{}, len(request.Targets))
	cleaned := make([]model.PelicanProbeTarget, 0, len(request.Targets))
	for _, target := range request.Targets {
		group := strings.TrimSpace(target.Group)
		modelName := strings.TrimSpace(target.Model)
		if group == "" || modelName == "" || !isPelicanChatModel(modelName) {
			continue
		}
		// 一个分组只配一个模型：同组第二个条目说明前端传错了，直接拒绝而不是悄悄丢掉。
		if _, ok := groups[group]; ok {
			common.ApiErrorMsg(c, "group already has a probe model: "+group)
			return
		}
		pair := model.PelicanProbeTarget{Group: group, Model: modelName}
		if _, ok := serveable[pair]; !ok {
			common.ApiErrorMsg(c, "no enabled channel serves this group/model: "+group+"/"+modelName)
			return
		}
		groups[group] = struct{}{}
		cleaned = append(cleaned, pair)
	}
	// token 只写不读：请求里留空表示沿用已保存的那个，要清除必须显式带 magpie_token_clear。
	previous := model.GetPelicanTestConfig()
	magpieToken := strings.TrimSpace(request.MagpieToken)
	if request.MagpieClear {
		magpieToken = ""
	} else if magpieToken == "" {
		magpieToken = previous.MagpieToken
	}
	// 联动有两套用法：
	//   * 只读（填了地址与 token）：模型由 magpie-ops 决定，映射的分组不需要
	//     先选模型，巡视目标也从那边取——所以下面的「必须是探测分组」不适用；
	//   * 复用 + 调整（没有地址/token）：映射只能指向真正会探测的分组，否则
	//     永远触发不了。
	magpieReadOnlySetup := magpieURL != ""
	magpieGroups := make([]model.PelicanMagpieGroup, 0, len(request.MagpieGroups))
	for _, mapping := range request.MagpieGroups {
		group := strings.TrimSpace(mapping.Group)
		magpieGroup := strings.TrimSpace(mapping.MagpieGroup)
		if group == "" && magpieGroup == "" {
			continue
		}
		if !magpieReadOnlySetup && !slices.ContainsFunc(cleaned, func(target model.PelicanProbeTarget) bool { return target.Group == group }) {
			common.ApiErrorMsg(c, "magpie group is not a probe group: "+group)
			return
		}
		if !model.PelicanMagpieGroupIDValid(magpieGroup) {
			common.ApiErrorMsg(c, "magpie_group is required for group: "+group)
			return
		}
		magpieGroups = append(magpieGroups, model.PelicanMagpieGroup{Group: group, MagpieGroup: magpieGroup})
	}
	// 开了自动调整就必须填全：地址、token 与至少一个分组映射，缺一个都跑不起来。
	if len(magpieGroups) > 0 && (magpieURL == "" || magpieToken == "") {
		common.ApiErrorMsg(c, "magpie_url and magpie_token are required to adjust magpie groups")
		return
	}
	if err := model.SavePelicanTestConfig(model.PelicanTestConfig{
		Targets:               cleaned,
		IntervalMinutes:       request.IntervalMinutes,
		OwnerUserId:           adminUserId,
		Prompt:                request.Prompt,
		PromptContract:        request.PromptContract,
		Description:           request.Description,
		MagpieURL:             magpieURL,
		MagpieToken:           magpieToken,
		MagpieGroups:          magpieGroups,
		MagpieCooldownMinutes: request.MagpieCooldownMinutes,
		MagpieDryRun:          request.MagpieDryRun,
		MagpieReuseMinutes:    request.MagpieReuseMinutes,
	}); err != nil {
		common.ApiError(c, err)
		return
	}
	// 回显读回落库后的值：次数与冷却在保存时会被归一化，页面要看到真正生效的数字。
	saved := model.GetPelicanTestConfig()
	c.JSON(http.StatusOK, gin.H{
		"success": true,
		"data": gin.H{
			"targets":                 cleaned,
			"interval_minutes":        request.IntervalMinutes,
			"owner_user_id":           adminUserId,
			"owner_username":          c.GetString("username"),
			"prompt":                  saved.Prompt,
			"prompt_contract":         saved.PromptContract,
			"description":             saved.Description,
			"magpie_url":              saved.MagpieURL,
			"magpie_token_set":        saved.MagpieToken != "",
			"magpie_groups":           saved.MagpieGroups,
			"magpie_cooldown_minutes": saved.MagpieCooldownMinutes,
			"magpie_dry_run":          saved.MagpieDryRun,
			"magpie_reuse_minutes":    saved.MagpieReuseMinutes,
		},
	})
}

// pelicanInitialAdminUserId 是配置里还没有记账管理员时的兜底账号：探测走站点自己的
// 渠道链路，需要一个真实用户来取分组与额度上下文，这里用安装时创建的初始管理员。
const pelicanInitialAdminUserId = 1

// pelicanProbeLogTokenName 是写进消费日志的令牌名：管理员按它就能把巡视开销与真实
// 业务调用分开核对。
const pelicanProbeLogTokenName = "pelican-probe"

var (
	pelicanSweepRunning   atomic.Bool
	pelicanSweepStartedAt atomic.Int64
	pelicanLastSweepAt    atomic.Int64
)

// pelicanSweepBegin 抢占巡视守卫并记下开始时间：手动巡视与定时巡视共用这把锁，
// 两个入口不会把同一组合同时铺开跑两遍。
func pelicanSweepBegin() bool {
	if !pelicanSweepRunning.CompareAndSwap(false, true) {
		return false
	}
	pelicanSweepStartedAt.Store(time.Now().Unix())
	return true
}

func pelicanSweepEnd() {
	pelicanSweepStartedAt.Store(0)
	pelicanSweepRunning.Store(false)
}

// pelicanSweepState 报告整轮巡视是否在跑，以及开始时间（0 表示没在跑）。
func pelicanSweepState() (bool, int64) {
	if !pelicanSweepRunning.Load() {
		return false, 0
	}
	return true, pelicanSweepStartedAt.Load()
}

// pelicanProbeUserId 返回探测的记账身份：保存巡视设置的那个管理员。
func pelicanProbeUserId(cfg model.PelicanTestConfig) int {
	if cfg.OwnerUserId > 0 {
		return cfg.OwnerUserId
	}
	return pelicanInitialAdminUserId
}

// StartPelicanScheduler 按后台配置的间隔自动巡视。只在主节点上跑：
// 从节点跟着主节点的数据库同步结果即可，不需要各跑一遍。
func StartPelicanScheduler() {
	if !common.IsMasterNode {
		return
	}
	go func() {
		ticker := time.NewTicker(time.Minute)
		defer ticker.Stop()
		for range ticker.C {
			// 每分钟先补测当前不可用的目标，再按配置间隔跑常规巡视。
			runPelicanRecoverySweep()
			runPelicanScheduledSweep()
		}
	}()
}

// pelicanRecoveryTargets 挑出需要立刻补测的目标：上一次成功的结果不是「可用」，
// 或者根本还没有成功记录。可用率是二值的：只要最近一次成功是健康的就算可用。
func pelicanRecoveryTargets(targets []model.PelicanProbeTarget, latestSuccess map[model.PelicanProbeTarget]*model.PelicanTestResult) []model.PelicanProbeTarget {
	pending := make([]model.PelicanProbeTarget, 0, len(targets))
	for _, target := range targets {
		record := latestSuccess[target]
		if record != nil && record.Verdict == service.PelicanVerdictHealthy {
			continue
		}
		pending = append(pending, target)
	}
	return pending
}

// runPelicanRecoverySweep 把「上一次测试不可用」的目标每分钟补测一次，直到测回可用：
// 掉智或失败的组合不必等到下一个巡视间隔。每个组合各自有登记锁，不会和常规巡视重复测。
func runPelicanRecoverySweep() {
	cfg, targets, err := pelicanSweepPlan()
	if err != nil || len(targets) == 0 {
		return
	}
	latestSuccess, err := model.GetLatestPelicanSuccesses()
	if err != nil {
		return
	}
	pending := pelicanRecoveryTargets(targets, latestSuccess)
	if len(pending) == 0 {
		return
	}
	tested, failed := runPelicanSweep(context.Background(), pelicanProbeUserId(cfg), pending)
	common.SysLog(common.LogText("pelican recovery sweep finished: %d tested, %d failed", tested, failed))
}

// runPelicanScheduledSweep 到点就跑一轮；上一轮没跑完时直接跳过，避免叠加。
// 没配置探测项时什么都不做：不配置就不探测。
func runPelicanScheduledSweep() {
	cfg, targets, err := pelicanSweepPlan()
	if err != nil {
		common.SysLog(common.LogText("pelican sweep skipped: %s", err.Error()))
		return
	}
	if cfg.IntervalMinutes <= 0 || len(cfg.Targets) == 0 {
		return
	}
	if time.Since(time.Unix(pelicanLastSweepAt.Load(), 0)) < time.Duration(cfg.IntervalMinutes)*time.Minute {
		return
	}
	// 先记下这一轮的时间：配置的模型都下线时，跳过原因每个间隔只记一次，不刷屏。
	pelicanLastSweepAt.Store(time.Now().Unix())
	if len(targets) == 0 {
		common.SysLog(common.LogText("pelican sweep skipped: none of the configured targets has an enabled channel"))
		return
	}
	if !pelicanSweepBegin() {
		return
	}
	defer pelicanSweepEnd()
	tested, failed := runPelicanSweep(context.Background(), pelicanProbeUserId(cfg), targets)
	common.SysLog(common.LogText("pelican sweep finished: %d tested, %d failed", tested, failed))
}
