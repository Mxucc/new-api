package controller

import (
	"context"
	"errors"
	"net/http"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/QuantumNous/new-api/common"
	"github.com/QuantumNous/new-api/model"
	"github.com/QuantumNous/new-api/service"
	"github.com/gin-gonic/gin"
)

// 鹈鹕测智要模型交付一整段 SVG 动画，输出上限远高于健康检查用的 “hi”。
const pelicanProbeMaxTokens = 8000

// 一次鹈鹕探测的单次耗时上限，超时按失败记录，避免整轮巡视卡在一个模型上。
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
	ModelName  string   `json:"model_name"`
	Available  bool     `json:"available"`
	Verdict    string   `json:"verdict"`
	Score      int      `json:"score"`
	LatencyMs  int64    `json:"latency_ms"`
	TestedAt   int64    `json:"tested_at"`
	ResultId   int64    `json:"result_id"`
	Dimensions []string `json:"dimensions"`
	LastError  string   `json:"last_error,omitempty"`
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

// pelicanCandidateModels 返回参与巡视的模型：站点当前在售的对话模型，
// 加上曾经测过但已下架的模型（它们的状态对用户同样有意义）。
func pelicanCandidateModels(latest map[string]*model.PelicanTestResult) []string {
	names := make([]string, 0, len(latest))
	seen := make(map[string]struct{}, len(latest))
	for _, name := range model.GetEnabledModels() {
		if !isPelicanChatModel(name) {
			continue
		}
		if _, ok := seen[name]; ok {
			continue
		}
		seen[name] = struct{}{}
		names = append(names, name)
	}
	for name := range latest {
		if _, ok := seen[name]; ok {
			continue
		}
		seen[name] = struct{}{}
		names = append(names, name)
	}
	sort.Strings(names)
	return names
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

func pelicanStatusOf(name string, available bool, result *model.PelicanTestResult) pelicanModelStatus {
	status := pelicanModelStatus{
		ModelName:  name,
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

// GetPelicanOverview 返回站点模型状态与各自的智商，登录用户可见。
func GetPelicanOverview(c *gin.Context) {
	latest, err := model.GetLatestPelicanResults()
	if err != nil {
		common.ApiError(c, err)
		return
	}
	enabled := make(map[string]struct{})
	for _, name := range model.GetEnabledModels() {
		enabled[name] = struct{}{}
	}
	names := pelicanCandidateModels(latest)
	models := make([]pelicanModelStatus, 0, len(names))
	latestTestedAt := int64(0)
	for _, name := range names {
		_, available := enabled[name]
		status := pelicanStatusOf(name, available, latest[name])
		if status.TestedAt > latestTestedAt {
			latestTestedAt = status.TestedAt
		}
		models = append(models, status)
	}
	c.JSON(http.StatusOK, gin.H{
		"success": true,
		"data": gin.H{
			"models":           models,
			"total":            len(models),
			"latest_tested_at": latestTestedAt,
			"prompt":           service.PelicanPrompt,
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
	Model string `json:"model"`
}

// triggerPelicanTest 是 RunPelicanTest 与 RunAllPelicanTests 共用的单模型入口。
// ctx 由调用方给出：单模型测试跟随请求，整轮巡视则用独立上下文，
// 免得管理员关掉页面就把巡视打断在半路。
func triggerPelicanTest(ctx context.Context, modelName string, group string, testUserID int) (pelicanModelStatus, error) {
	modelName = strings.TrimSpace(modelName)
	if modelName == "" {
		return pelicanModelStatus{}, errors.New("model is required")
	}
	if !isPelicanChatModel(modelName) {
		return pelicanModelStatus{}, errors.New("model does not accept chat completions")
	}
	channel, err := model.GetChannel(group, modelName, 0, nil)
	if err != nil {
		return pelicanModelStatus{}, err
	}
	if channel == nil {
		return pelicanModelStatus{}, errors.New("no enabled channel serves this model")
	}
	record, err := runPelicanProbe(ctx, channel, modelName, testUserID, group)
	if err != nil {
		return pelicanModelStatus{}, err
	}
	return pelicanStatusOf(modelName, true, record), nil
}

// RunPelicanTest 让管理员按需给一个模型测智商。
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
	group, _ := model.GetUserGroup(testUserID, false)
	status, err := triggerPelicanTest(c.Request.Context(), request.Model, group, testUserID)
	if err != nil {
		common.ApiError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"success": true, "data": status})
}

// RunAllPelicanTests 让管理员一次巡视站点当前在售的对话模型。
func RunAllPelicanTests(c *gin.Context) {
	testUserID := c.GetInt("id")
	if testUserID <= 0 {
		common.ApiErrorMsg(c, "invalid request")
		return
	}
	latest, err := model.GetLatestPelicanResults()
	if err != nil {
		common.ApiError(c, err)
		return
	}
	group, _ := model.GetUserGroup(testUserID, false)
	enabled := make(map[string]struct{})
	for _, name := range model.GetEnabledModels() {
		enabled[name] = struct{}{}
	}
	// 巡视按固定并发跑：逐个串行在模型多时要占住请求几分钟。
	const pelicanSweepConcurrency = 3
	var counters struct {
		sync.Mutex
		tested int
		failed int
	}
	var wg sync.WaitGroup
	slots := make(chan struct{}, pelicanSweepConcurrency)
	sweepCtx := context.Background()
	for _, name := range pelicanCandidateModels(latest) {
		if _, ok := enabled[name]; !ok {
			continue
		}
		slots <- struct{}{}
		wg.Go(func() {
			defer func() { <-slots }()
			_, err := triggerPelicanTest(sweepCtx, name, group, testUserID)
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
	c.JSON(http.StatusOK, gin.H{
		"success": true,
		"data": gin.H{
			"tested": counters.tested,
			"failed": counters.failed,
		},
	})
}
