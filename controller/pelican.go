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

// pelicanSweepTargets 返回本轮要测的组合：后台配置与当前可用组合取交集；
// 没配置时覆盖全部可用组合。旧格式配置（只有模型名、分组为空）会在该模型的
// 所有分组上展开，让升级后的行为与旧配置一致。
func pelicanSweepTargets(configured []model.PelicanProbeTarget, available []model.PelicanProbeTarget) []model.PelicanProbeTarget {
	if len(configured) == 0 {
		return available
	}
	pairs := make(map[model.PelicanProbeTarget]struct{}, len(configured))
	wildcardModels := make(map[string]struct{}, len(configured))
	for _, target := range configured {
		if target.Group == "" {
			wildcardModels[target.Model] = struct{}{}
			continue
		}
		pairs[target] = struct{}{}
	}
	targets := make([]model.PelicanProbeTarget, 0, len(configured))
	for _, target := range available {
		if _, ok := pairs[target]; ok {
			targets = append(targets, target)
			continue
		}
		if _, ok := wildcardModels[target.Model]; ok {
			targets = append(targets, target)
		}
	}
	return targets
}

// pelicanSweepPlan 读取当前可用组合，并算出这一轮实际要测的组合。
func pelicanSweepPlan() ([]model.PelicanProbeTarget, error) {
	probes, err := model.GetPelicanProbeTargets()
	if err != nil {
		return nil, err
	}
	return pelicanSweepTargets(model.GetPelicanTestTargets(), pelicanAvailableTargets(probes)), nil
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

// GetPelicanOverview 返回站点每个（分组, 模型）组合的状态与智商，登录用户可见。
func GetPelicanOverview(c *gin.Context) {
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
	available := pelicanAvailableTargets(probes)
	served := make(map[model.PelicanProbeTarget]struct{}, len(available))
	statuses := make([]pelicanModelStatus, 0, len(available)+len(latest))
	for _, target := range available {
		served[target] = struct{}{}
		status := pelicanStatusOf(target, true, latest[target])
		status.Testing = pelicanProbeActive(target)
		statuses = append(statuses, status)
	}
	// 曾经测过、但现在没有渠道的组合也要展示：历史分数对用户依然有意义。
	for target, result := range latest {
		if _, ok := served[target]; ok {
			continue
		}
		status := pelicanStatusOf(target, false, result)
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
func runPelicanProbe(ctx context.Context, channel *model.Channel, modelName string, testUserID int, group string) (*model.PelicanTestResult, error) {
	probeCtx, cancel := context.WithTimeout(ctx, pelicanProbeTimeout)
	defer cancel()

	startedAt := time.Now()
	result := testChannelWithOptions(probeCtx, channel, testUserID, modelName, "", false, channelTestOptions{
		Prompt:         service.PelicanPrompt + "\n\n" + service.PelicanPromptContract,
		MaxTokens:      pelicanProbeMaxTokens,
		SkipConsumeLog: true,
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

// pelicanProbeBegin 登记一个组合；返回 false 表示该组合已经在跑。
func pelicanProbeBegin(target model.PelicanProbeTarget) bool {
	_, loaded := pelicanProbeRunning.LoadOrStore(target, time.Now().Unix())
	return !loaded
}

func pelicanProbeEnd(target model.PelicanProbeTarget) {
	pelicanProbeRunning.Delete(target)
}

// pelicanProbeActive 报告该组合此刻是否有一次探测在跑。
func pelicanProbeActive(target model.PelicanProbeTarget) bool {
	_, ok := pelicanProbeRunning.Load(target)
	return ok
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
func RunPelicanTest(c *gin.Context) {
	testUserID := c.GetInt("id")
	if testUserID <= 0 {
		common.ApiErrorMsg(c, "invalid request")
		return
	}
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
	if !pelicanProbeBegin(target) {
		c.JSON(http.StatusOK, gin.H{
			"success": true,
			"data":    gin.H{"started": false, "running": true},
		})
		return
	}
	go func() {
		defer pelicanProbeEnd(target)
		runPelicanProbeTarget(context.Background(), target, testUserID)
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
			if !pelicanProbeBegin(target) {
				return
			}
			defer pelicanProbeEnd(target)
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

// RunAllPelicanTests 让管理员一次巡视后台配置的组合（未配置时为全部可测组合）。
// 整轮要好几分钟，所以这里和单组合一样只做登记，结果由页面轮询 overview 取回。
func RunAllPelicanTests(c *gin.Context) {
	testUserID := c.GetInt("id")
	if testUserID <= 0 {
		common.ApiErrorMsg(c, "invalid request")
		return
	}
	targets, err := pelicanSweepPlan()
	if err != nil {
		common.ApiError(c, err)
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
		tested, failed := runPelicanSweep(context.Background(), testUserID, targets)
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

// GetPelicanConfig 返回巡视配置与可选的（分组, 模型）组合，供后台设置页使用。
func GetPelicanConfig(c *gin.Context) {
	probes, err := model.GetPelicanProbeTargets()
	if err != nil {
		common.ApiError(c, err)
		return
	}
	available := pelicanAvailableTargets(probes)
	serveable := make(map[model.PelicanProbeTarget]struct{}, len(available))
	servedModels := make(map[string]struct{}, len(available))
	for _, target := range available {
		serveable[target] = struct{}{}
		servedModels[target.Model] = struct{}{}
	}
	targets := model.GetPelicanTestTargets()
	if targets == nil {
		targets = []model.PelicanProbeTarget{}
	}
	missing := make([]model.PelicanProbeTarget, 0)
	for _, target := range targets {
		// 旧格式配置没有分组，只要该模型还有任意分组可测就不算缺失。
		if target.Group == "" {
			if _, ok := servedModels[target.Model]; !ok {
				missing = append(missing, target)
			}
			continue
		}
		if _, ok := serveable[target]; !ok {
			missing = append(missing, target)
		}
	}
	c.JSON(http.StatusOK, gin.H{
		"success": true,
		"data": gin.H{
			"targets":           targets,
			"interval_minutes":  model.GetPelicanTestIntervalMinutes(),
			"available_targets": available,
			"missing_targets":   missing,
		},
	})
}

// UpdatePelicanConfig 保存要巡视的组合与间隔。只接受当前有渠道的对话模型组合，
// 免得配置里留下一批永远测不出结果的条目。
func UpdatePelicanConfig(c *gin.Context) {
	var request pelicanConfigRequest
	if err := c.ShouldBindJSON(&request); err != nil {
		common.ApiErrorMsg(c, "invalid request body")
		return
	}
	if request.IntervalMinutes < 0 || request.IntervalMinutes > 1440 {
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
	seen := make(map[model.PelicanProbeTarget]struct{}, len(request.Targets))
	cleaned := make([]model.PelicanProbeTarget, 0, len(request.Targets))
	for _, target := range request.Targets {
		group := strings.TrimSpace(target.Group)
		modelName := strings.TrimSpace(target.Model)
		if group == "" || modelName == "" || !isPelicanChatModel(modelName) {
			continue
		}
		pair := model.PelicanProbeTarget{Group: group, Model: modelName}
		if _, ok := serveable[pair]; !ok {
			common.ApiErrorMsg(c, "no enabled channel serves this group/model: "+group+"/"+modelName)
			return
		}
		if _, ok := seen[pair]; ok {
			continue
		}
		seen[pair] = struct{}{}
		cleaned = append(cleaned, pair)
	}
	if err := model.SavePelicanTestConfig(cleaned, request.IntervalMinutes); err != nil {
		common.ApiError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{
		"success": true,
		"data": gin.H{
			"targets":          cleaned,
			"interval_minutes": request.IntervalMinutes,
		},
	})
}

// pelicanSchedulerUserId 是自动巡视使用的账号：探测走站点自己的渠道链路，
// 需要一个真实用户来取分组与额度上下文，这里用初始管理员。
const pelicanSchedulerUserId = 1

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
func runPelicanScheduledSweep() {
	interval := model.GetPelicanTestIntervalMinutes()
	if interval <= 0 {
		return
	}
	if time.Since(time.Unix(pelicanLastSweepAt.Load(), 0)) < time.Duration(interval)*time.Minute {
		return
	}
	if !pelicanSweepBegin() {
		return
	}
	defer pelicanSweepEnd()
	targets, err := pelicanSweepPlan()
	if err != nil {
		common.SysLog(common.LogText("pelican sweep skipped: %s", err.Error()))
		return
	}
	pelicanLastSweepAt.Store(time.Now().Unix())
	tested, failed := runPelicanSweep(context.Background(), pelicanSchedulerUserId, targets)
	common.SysLog(common.LogText("pelican sweep finished: %d tested, %d failed", tested, failed))
}
