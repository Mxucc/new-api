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
	Group      string   `json:"group"`
	ModelName  string   `json:"model_name"`
	Available  bool     `json:"available"`
	Verdict    string   `json:"verdict"`
	Score      int      `json:"score"`
	LatencyMs  int64    `json:"latency_ms"`
	TestedAt   int64    `json:"tested_at"`
	ResultId   int64    `json:"result_id"`
	Dimensions []string `json:"dimensions"`
	LastError  string   `json:"last_error,omitempty"`
	// Testing 表示这个组合此刻正有一次探测在跑：页面据此显示加载态并保持轮询。
	Testing bool `json:"testing"`
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
	status.Dimensions = splitPelicanDimensions(result.Dimensions)
	status.LastError = result.Error
	return status
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
	serveable := make(map[model.PelicanProbeTarget]struct{}, len(cfg.Targets))
	for _, target := range pelicanAvailableTargets(probes) {
		serveable[target] = struct{}{}
	}
	// 只上报配置里的探测项：用户看到的行数就等于管理员配置的分组数。
	statuses := make([]pelicanModelStatus, 0, len(cfg.Targets))
	for _, target := range cfg.Targets {
		_, served := serveable[target]
		status := pelicanStatusOf(target, served, latest[target])
		status.Testing = pelicanProbeActive(target)
		statuses = append(statuses, status)
	}
	slices.SortFunc(statuses, func(a, b pelicanModelStatus) int {
		if byGroup := strings.Compare(a.Group, b.Group); byGroup != 0 {
			return byGroup
		}
		return strings.Compare(a.ModelName, b.ModelName)
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
			"prompt":           service.PelicanPrompt,
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
	c.JSON(http.StatusOK, gin.H{
		"success": true,
		"data": gin.H{
			"id":         result.Id,
			"group":      result.Group,
			"model_name": result.ModelName,
			"status":     result.Status,
			"verdict":    result.Verdict,
			"score":      result.Score,
			"latency_ms": result.LatencyMs,
			"dimensions": splitPelicanDimensions(result.Dimensions),
			"output":     result.Output,
			"error":      result.Error,
			"created_at": result.CreatedAt,
		},
	})
}

// runPelicanProbe 通过站点自己的渠道链路向一个模型问一次鹈鹕题面，并写好记录。
// testUserID 是记账身份：探测照常写一条消费日志（令牌名 pelican-probe），记在
// 保存巡视设置的管理员名下，管理员据此把巡视开销与真实调用分开核对。
func runPelicanProbe(ctx context.Context, channel *model.Channel, modelName string, testUserID int, group string) (*model.PelicanTestResult, error) {
	probeCtx, cancel := context.WithTimeout(ctx, pelicanProbeTimeout)
	defer cancel()

	startedAt := time.Now()
	result := testChannelWithOptions(probeCtx, channel, testUserID, modelName, "", false, channelTestOptions{
		Prompt:       service.PelicanPrompt + "\n\n" + service.PelicanPromptContract,
		MaxTokens:    pelicanProbeMaxTokens,
		LogTokenName: pelicanProbeLogTokenName,
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
func triggerPelicanTest(ctx context.Context, target model.PelicanProbeTarget, testUserID int) (pelicanModelStatus, error) {
	group := strings.TrimSpace(target.Group)
	modelName := strings.TrimSpace(target.Model)
	if group == "" || modelName == "" {
		return pelicanModelStatus{}, errors.New("group and model are required")
	}
	if !isPelicanChatModel(modelName) {
		return pelicanModelStatus{}, errors.New("model does not accept chat completions")
	}
	channel, err := model.GetChannel(group, modelName, 0, nil)
	if err != nil {
		return pelicanModelStatus{}, err
	}
	if channel == nil {
		return pelicanModelStatus{}, errors.New("no enabled channel serves this group/model: " + group + "/" + modelName)
	}
	record, err := runPelicanProbe(ctx, channel, modelName, testUserID, group)
	if err != nil {
		return pelicanModelStatus{}, err
	}
	return pelicanStatusOf(model.PelicanProbeTarget{Group: group, Model: modelName}, true, record), nil
}

// runPelicanProbeTarget 跑一次探测并把失败原因写进系统日志：探测改成异步之后
// 已经没有调用方能收到错误，日志是唯一还能查到原因的地方。
func runPelicanProbeTarget(ctx context.Context, target model.PelicanProbeTarget, testUserID int) error {
	_, err := triggerPelicanTest(ctx, target, testUserID)
	if err != nil {
		common.SysLog(common.LogText("pelican probe failed for %s/%s: %s", target.Group, target.Model, err))
	}
	return err
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
		runPelicanProbeTarget(context.Background(), target, probeUserId)
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
		tested int
		failed int
	}
	var wg sync.WaitGroup
	slots := make(chan struct{}, pelicanSweepConcurrency)
	for _, target := range targets {
		slots <- struct{}{}
		wg.Go(func() {
			defer func() { <-slots }()
			// 同一组合已经被单次触发抢占时不重复探测，也不计为失败。
			// 同一组合已经被单次触发抢占时不重复探测，也不计为失败。
			token, started := pelicanProbeBegin(target)
			if !started {
				return
			}
			defer pelicanProbeEnd(target, token)
			err := runPelicanProbeTarget(ctx, target, testUserID)
			counters.Lock()
			defer counters.Unlock()
			if err != nil {
				counters.failed++
				return
			}
			counters.tested++
		})
	}
	wg.Wait()
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

type pelicanConfigRequest struct {
	Targets         []model.PelicanProbeTarget `json:"targets"`
	IntervalMinutes int                        `json:"interval_minutes"`
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
	c.JSON(http.StatusOK, gin.H{
		"success": true,
		"data": gin.H{
			"targets":           cfg.Targets,
			"interval_minutes":  cfg.IntervalMinutes,
			"owner_user_id":     cfg.OwnerUserId,
			"owner_username":    ownerUsername,
			"available_targets": available,
			"missing_targets":   missing,
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
	if err := model.SavePelicanTestConfig(model.PelicanTestConfig{
		Targets:         cleaned,
		IntervalMinutes: request.IntervalMinutes,
		OwnerUserId:     adminUserId,
	}); err != nil {
		common.ApiError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{
		"success": true,
		"data": gin.H{
			"targets":          cleaned,
			"interval_minutes": request.IntervalMinutes,
			"owner_user_id":    adminUserId,
			"owner_username":   c.GetString("username"),
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
			runPelicanScheduledSweep()
		}
	}()
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
