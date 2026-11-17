package controller

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"

	"github.com/QuantumNous/new-api/common"
	"github.com/QuantumNous/new-api/model"
	"github.com/QuantumNous/new-api/service"
	"github.com/gin-gonic/gin"
)

// pelicanMagpieReadOnlyWindowMinutes 是只读模式取结果的窗口：magpie-ops 只留
// 24 小时的结果，这里就按一天取，超出的它也没有。
const pelicanMagpieReadOnlyWindowMinutes = 24 * 60

const (
	// pelicanMagpieTimeout 是一次联动请求的上限：magpie-ops 就在回环上跑，
	// 10 秒还没回就是它出了问题，不能把巡视收尾拖住。
	pelicanMagpieTimeout = 20 * time.Second
	// pelicanMagpieBodyLimit 是一次联动回复的上限。线上一个 magpie-ops
	// 有 260 个分组、每个分组十几个成员，/api/groups 就有 700KB 左右；
	// 之前这里只有 256KB，读满就停，JSON 自然是半截的
	// （"unexpected end of JSON input" 就是这么来的）。
	pelicanMagpieBodyLimit = 32 << 20
	// pelicanMagpieDetailMaxRunes 是写进状态与日志的失败原因长度上限。
	pelicanMagpieDetailMaxRunes = 200
)

// 调整在 magpie-ops 那边是后台任务：给它一点时间跑完，再用 /api/actions 对账。
// 变量而不是常量，测试里可以把它压到毫秒级。
var (
	pelicanMagpieReconcileAttempts = 4
	pelicanMagpieReconcileInterval = 8 * time.Second
)

// magpieAdjustReply 是 magpie-ops POST /api/adjust 的回执：它立刻返回，工作在后台跑。
// Since 是这次调整开始的时刻，拿它去 /api/actions 就能看到这次到底动了谁。
type magpieAdjustReply struct {
	Started bool   `json:"started"`
	Group   string `json:"group"`
	Since   string `json:"since"`
}

// magpieGroupMember 是 magpie-ops GET /api/groups 里的一个成员，设置页只用它做展示。
type magpieGroupMember struct {
	Ref          string `json:"ref"`
	Provider     string `json:"provider"`
	ProviderName string `json:"providerName"`
	Model        string `json:"model"`
	Verdict      string `json:"verdict"`
	Score        int    `json:"score"`
	Disabled     bool   `json:"disabled"`
	// DisabledByUs 区分「本工具关的（会自己恢复）」与「管理员手工关的」。
	DisabledByUs bool    `json:"disabledByUs"`
	Rate         float64 `json:"rate"`
	TtftMs       int64   `json:"ttftMs"`
	LastProbedAt string  `json:"lastProbedAt"`
	// Excluded 是 magpie-ops 那边「不列入自动化」的渠道：它不会被排序、
	// 也不会被处置；Probe 说明排除时是否仍然测智。
	Excluded bool `json:"excluded"`
	Probe    bool `json:"probe"`
}

// magpieBestOf 是 magpie-ops GET /api/best 里的最佳结果：一个分组在窗口内
// 拿到的最高分。降智也算——只要分最高，new-api 要的是这个分组「最好什么样」。
type magpieBestOf struct {
	Ref     string  `json:"ref"`
	Model   string  `json:"model"`
	Score   int     `json:"score"`
	Verdict string  `json:"verdict"`
	At      string  `json:"at"`
	Rate    float64 `json:"rate"`
}

// magpieBestAnswer 是 GET /api/best 的回复。
type magpieBestAnswer struct {
	Group      string        `json:"group"`
	Since      string        `json:"since"`
	Hours      int           `json:"hours"`
	Considered int           `json:"considered"`
	Best       *magpieBestOf `json:"best"`
}

func fetchMagpieBest(ctx context.Context, cfg model.PelicanTestConfig, magpieGroup string, hours int) (magpieBestAnswer, error) {
	var best magpieBestAnswer
	if hours <= 0 {
		hours = 24
	}
	path := fmt.Sprintf("/api/best?group=%s&hours=%d", url.QueryEscape(magpieGroup), hours)
	body, err := pelicanMagpieRequest(ctx, cfg, http.MethodGet, path, nil)
	if err != nil {
		return best, err
	}
	if err := json.Unmarshal(body, &best); err != nil {
		return best, fmt.Errorf("magpie best: %w", err)
	}
	return best, nil
}

// magpieGroup 是 magpie-ops GET /api/groups 里的一个分组。
// PolicyEnabled/IntervalMinutes 是 magpie-ops 自己的巡检状态：两边都跑就是重复探测。
type magpieGroup struct {
	Id              string              `json:"id"`
	Name            string              `json:"name"`
	Routing         string              `json:"routing"`
	PolicyEnabled   bool                `json:"policyEnabled"`
	IntervalMinutes int                 `json:"intervalMinutes"`
	Members         []magpieGroupMember `json:"members"`
	// ProbeRefs 是这一组里 magpie-ops 真正会测的成员：一个渠道只测一个
	// 模型，所以成员数往往比它多。设置页用它说明「有几个是真的在测的」。
	ProbeRefs []string `json:"probeRefs"`
}

// magpieAction 是 magpie-ops GET /api/actions 里的一条动作记录。
// Score 是驱动这次动作的智商，旧记录没有它（nil），显示时按「没有就只写 ref + kind」处理。
type magpieAction struct {
	At     string `json:"at"`
	Ref    string `json:"ref"`
	Group  string `json:"group"`
	Kind   string `json:"kind"`
	Score  *int   `json:"score"`
	Reason string `json:"reason"`
	OK     bool   `json:"ok"`
	Auto   bool   `json:"auto"`
}

// pelicanMagpieReadOnly 说 new-api 是否把测智完全交给 magpie-ops：联动了，
// 而且至少有一个分组绑定到 magpie 分组。这时不再按 new-api 自己的
// 「分组 → 模型」探测，模型以 magpie-ops 正在测的为准。
func pelicanMagpieReadOnly(cfg model.PelicanTestConfig) bool {
	if !cfg.MagpieLinked() {
		return false
	}
	for _, mapping := range cfg.MagpieGroups {
		if strings.TrimSpace(mapping.MagpieGroup) != "" {
			return true
		}
	}
	return false
}

// magpieProbeTargets 是只读模式下的巡视目标：每个绑定分组的 magpie 分组里，
// magpie-ops 真正在测的那些成员（probeRefs；老版本没有这个字段就退回全部成员）。
// new-api 只按它去取结果，不再自己挑模型。
func magpieProbeTargets(ctx context.Context, cfg model.PelicanTestConfig) ([]model.PelicanProbeTarget, error) {
	groups, err := cachedMagpieGroups(ctx, cfg)
	if err != nil {
		return nil, err
	}
	byId := make(map[string]magpieGroup, len(groups))
	for _, group := range groups {
		byId[group.Id] = group
	}
	var out []model.PelicanProbeTarget
	seen := map[string]bool{}
	for _, mapping := range cfg.MagpieGroups {
		group, ok := byId[mapping.MagpieGroup]
		if !ok {
			continue
		}
		measured := group.ProbeRefs
		if len(measured) == 0 {
			for _, member := range group.Members {
				measured = append(measured, member.Ref)
			}
		}
		for _, ref := range measured {
			_, modelName, ok := strings.Cut(ref, "/")
			if !ok || modelName == "" {
				continue
			}
			key := mapping.Group + "\x00" + modelName
			if seen[key] {
				continue
			}
			seen[key] = true
			out = append(out, model.PelicanProbeTarget{Group: mapping.Group, Model: modelName})
		}
	}
	return out, nil
}

// pelicanMagpieGroupFor 找出这个 new-api 分组对应的 magpie 分组；没配映射就不联动。
func pelicanMagpieGroupFor(cfg model.PelicanTestConfig, group string) (string, bool) {
	for _, mapping := range cfg.MagpieGroups {
		if mapping.Group == group {
			return mapping.MagpieGroup, true
		}
	}
	return "", false
}

// pelicanMagpieDetail 把失败原因压成一行短文本，供状态与系统日志使用。
func pelicanMagpieDetail(err error) string {
	if err == nil {
		return ""
	}
	text := strings.Join(strings.Fields(err.Error()), " ")
	if runes := []rune(text); len(runes) > pelicanMagpieDetailMaxRunes {
		return string(runes[:pelicanMagpieDetailMaxRunes])
	}
	return text
}

// pelicanMagpieRequest 发一个带 token 的请求给 magpie-ops。地址与 token 由管理员在设置里填。
// 不跟跳转：带着 token 追到别的主机等于把凭据送出去。
func pelicanMagpieRequest(ctx context.Context, cfg model.PelicanTestConfig, method, path string, body []byte) ([]byte, error) {
	requestCtx, cancel := context.WithTimeout(ctx, pelicanMagpieTimeout)
	defer cancel()
	var reader io.Reader
	if body != nil {
		reader = bytes.NewReader(body)
	}
	request, err := http.NewRequestWithContext(requestCtx, method, cfg.MagpieURL+path, reader)
	if err != nil {
		return nil, err
	}
	request.Header.Set("Authorization", "Bearer "+cfg.MagpieToken)
	if body != nil {
		request.Header.Set("Content-Type", "application/json")
	}
	client := &http.Client{
		Timeout:       pelicanMagpieTimeout,
		CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse },
	}
	response, err := client.Do(request)
	if err != nil {
		return nil, err
	}
	defer response.Body.Close()
	payload, err := io.ReadAll(io.LimitReader(response.Body, pelicanMagpieBodyLimit))
	if err != nil {
		return nil, fmt.Errorf("magpie-ops %s %s: 读回复失败：%w", method, cfg.MagpieURL+path, err)
	}
	if int64(len(payload)) >= pelicanMagpieBodyLimit {
		// 读满上限说明回复比上限还大：报「太大」，不要让它伪装成 JSON 语法错
		return nil, fmt.Errorf("magpie-ops %s %s 的回复超过 %d 字节上限（可能分组太多）", method, cfg.MagpieURL+path, pelicanMagpieBodyLimit)
	}
	if response.StatusCode < 200 || response.StatusCode > 299 {
		detail := strings.TrimSpace(string(payload))
		if detail == "" {
			detail = "（没有响应体）"
		}
		return nil, fmt.Errorf("magpie-ops %s %s: %s", method, cfg.MagpieURL+path, response.Status+" "+detail)
	}
	if len(payload) == 0 {
		// 200 但 0 字节：地址接到了别处（反向代理吞了请求、或者指到了静态服务），
		// 这个错必须说清，否则只看到 "unexpected end of JSON input"
		return nil, fmt.Errorf("magpie-ops %s %s 返回 HTTP %d 但内容为空（0 字节）：地址可能不是 magpie-ops，或中间的反向代理没把请求转过去", method, cfg.MagpieURL+path, response.StatusCode)
	}
	return payload, nil
}

// callMagpieAdjust 请 magpie-ops 现场给这个 magpie 分组测一遍并处理最差的那个成员。
// 判定与处置都由它做：new-api 只知道自己分组降智了，对不上它里面的渠道。
// 返回它的 since，用来在稍后对账这次到底动了谁。
func callMagpieAdjust(ctx context.Context, cfg model.PelicanTestConfig, magpieGroup, reason string) (string, error) {
	body, err := common.Marshal(map[string]string{"group": magpieGroup, "reason": reason})
	if err != nil {
		return "", err
	}
	payload, err := pelicanMagpieRequest(ctx, cfg, http.MethodPost, "/api/adjust", body)
	if err != nil {
		return "", err
	}
	var reply magpieAdjustReply
	if err := common.Unmarshal(payload, &reply); err != nil {
		return "", fmt.Errorf("magpie-ops reply: %w", err)
	}
	if !reply.Started {
		return "", fmt.Errorf("magpie-ops did not start the adjustment for %s", magpieGroup)
	}
	return reply.Since, nil
}

// listMagpieActions 读动作记录：配合 adjust 的 since，只取那次调整之后的事。
func listMagpieActions(ctx context.Context, cfg model.PelicanTestConfig, magpieGroup, since string) ([]magpieAction, error) {
	path := "/api/actions?group=" + url.QueryEscape(magpieGroup) + "&limit=20"
	if since != "" {
		path += "&since=" + url.QueryEscape(since)
	}
	payload, err := pelicanMagpieRequest(ctx, cfg, http.MethodGet, path, nil)
	if err != nil {
		return nil, err
	}
	var reply struct {
		Actions []magpieAction `json:"actions"`
	}
	if err := common.Unmarshal(payload, &reply); err != nil {
		return nil, fmt.Errorf("magpie-ops %s/api/groups 的回复不是 JSON（%d 字节，开头 %q）：%w",
			cfg.MagpieURL, len(payload), snippet(payload), err)
	}
	return reply.Actions, nil
}

// pelicanMagpieSaveState 写一个分组的联动状态；写失败只记系统日志，不影响调用方。
func pelicanMagpieSaveState(group string, mutate func(state *model.PelicanMagpieState)) {
	if err := model.UpdatePelicanMagpieState(group, mutate); err != nil {
		common.SysError(common.LogText("pelican magpie: save state for %s: %s", group, err))
	}
}

// pelicanMagpieReconcile 在调整之后短轮询 /api/actions，把「这次到底动了谁」写进状态。
// magpie-ops 的调整是后台任务：几秒内通常已经出结果，等不到就记「已检查，无需调整」。
func pelicanMagpieReconcile(ctx context.Context, cfg model.PelicanTestConfig, group, magpieGroup, since string) {
	for attempt := range pelicanMagpieReconcileAttempts {
		if attempt > 0 {
			select {
			case <-ctx.Done():
				return
			case <-time.After(pelicanMagpieReconcileInterval):
			}
		}
		actions, err := listMagpieActions(ctx, cfg, magpieGroup, since)
		if err != nil {
			if attempt == pelicanMagpieReconcileAttempts-1 {
				detail := pelicanMagpieDetail(err)
				common.SysError(common.LogText("pelican magpie: read actions for %s failed: %s", magpieGroup, detail))
				pelicanMagpieSaveState(group, func(state *model.PelicanMagpieState) {
					state.Result = "actions unreadable: " + detail
				})
			}
			continue
		}
		if len(actions) == 0 {
			if attempt == pelicanMagpieReconcileAttempts-1 {
				pelicanMagpieSaveState(group, func(state *model.PelicanMagpieState) {
					state.Result = "checked, nothing to adjust"
				})
			}
			continue
		}
		results := make([]model.PelicanMagpieActionResult, 0, len(actions))
		for _, action := range actions {
			results = append(results, model.PelicanMagpieActionResult{
				Ref:   action.Ref,
				Kind:  action.Kind,
				Score: action.Score,
			})
		}
		pelicanMagpieSaveState(group, func(state *model.PelicanMagpieState) {
			state.Actions = results
			state.Result = ""
		})
		return
	}
}

// listMagpieGroups 读 magpie-ops 的分组清单，设置页用它填映射下拉。
func listMagpieGroups(ctx context.Context, cfg model.PelicanTestConfig) ([]magpieGroup, error) {
	payload, err := pelicanMagpieRequest(ctx, cfg, http.MethodGet, "/api/groups", nil)
	if err != nil {
		return nil, err
	}
	var reply struct {
		Groups []magpieGroup `json:"groups"`
	}
	if err := common.Unmarshal(payload, &reply); err != nil {
		return nil, fmt.Errorf("magpie-ops reply: %w", err)
	}
	if reply.Groups == nil {
		reply.Groups = []magpieGroup{}
	}
	return reply.Groups, nil
}

// pelicanMagpieCooldownFree 报告这个分组是否已经过了冷却期：调整会让 magpie 现场
// 把分组里每个成员都测一遍，同一分组不该被一次巡视打很多次。
func pelicanMagpieCooldownFree(cfg model.PelicanTestConfig, group string) bool {
	state := model.GetPelicanMagpieStates()[group]
	if state.AdjustedAt <= 0 {
		return true
	}
	return time.Since(time.Unix(state.AdjustedAt, 0)) >= time.Duration(cfg.MagpieCooldownMinutes)*time.Minute
}

// CheckMagpieLink 让设置页当场验证一次联动：地址通不通、token 认不认、
// 能不能列出分组、magpie-ops 是哪个版本、有没有 /api/best。
// 只发 GET，不改 magpie 任何东西。
func CheckMagpieLink(c *gin.Context) {
	cfg := model.GetPelicanTestConfig()
	if strings.TrimSpace(cfg.MagpieURL) == "" {
		c.JSON(http.StatusOK, gin.H{"success": true, "data": gin.H{"linked": false, "error": "还没填 magpie-ops 地址"}})
		return
	}
	if strings.TrimSpace(cfg.MagpieToken) == "" {
		c.JSON(http.StatusOK, gin.H{"success": true, "data": gin.H{"linked": false, "error": "还没填 magpie-ops token"}})
		return
	}
	out := gin.H{"linked": true, "url": cfg.MagpieURL}
	ctx := c.Request.Context()
	if body, err := pelicanMagpieRequest(ctx, cfg, http.MethodGet, "/api/health", nil); err != nil {
		out["error"] = pelicanMagpieDetail(err)
	} else {
		var health struct {
			OK      bool   `json:"ok"`
			Version string `json:"version"`
			Magpie  string `json:"magpie"`
		}
		_ = common.Unmarshal(body, &health)
		out["ok"] = health.OK
		out["version"] = health.Version
	}
	if groups, err := listMagpieGroups(ctx, cfg); err != nil {
		out["groups_error"] = pelicanMagpieDetail(err)
	} else {
		out["groups"] = len(groups)
	}
	// 有 /api/best 才算 magpie-ops 够新：分组最佳结果靠它
	if _, err := pelicanMagpieRequest(ctx, cfg, http.MethodGet, "/api/best", nil); err != nil {
		out["best_error"] = pelicanMagpieDetail(err)
	} else {
		out["best"] = true
	}
	c.JSON(http.StatusOK, gin.H{"success": true, "data": out})
}

// magpieBestCache 记住每个映射分组最近一次拿到的「分组最佳结果」。
// 巡视结束后刷一次，设置页读缓存，不在页面请求里等 magpie。
var magpieBestCache = struct {
	mu   sync.Mutex
	at   time.Time
	best map[string]magpieBestAnswer
	err  string
}{best: map[string]magpieBestAnswer{}}

// refreshMagpieBest 给每个映射到的 magpie 分组取一次 24 小时内的最佳结果。
// 一个分组一次请求：拿的是 magpie-ops 已经测过的数，不再花探测费。
func refreshMagpieBest(ctx context.Context, cfg model.PelicanTestConfig) {
	groups := map[string]struct{}{}
	for _, mapping := range cfg.MagpieGroups {
		if strings.TrimSpace(mapping.MagpieGroup) != "" {
			groups[mapping.MagpieGroup] = struct{}{}
		}
	}
	if len(groups) == 0 {
		return
	}
	best := make(map[string]magpieBestAnswer, len(groups))
	errText := ""
	for group := range groups {
		got, err := fetchMagpieBest(ctx, cfg, group, 24)
		if err != nil {
			errText = pelicanMagpieDetail(err)
			continue
		}
		best[group] = got
	}
	magpieBestCache.mu.Lock()
	magpieBestCache.at, magpieBestCache.best, magpieBestCache.err = time.Now(), best, errText
	magpieBestCache.mu.Unlock()
}

// magpieBestSnapshot 是给设置页看的一份拷贝。
func magpieBestSnapshot() (map[string]magpieBestAnswer, time.Time, string) {
	magpieBestCache.mu.Lock()
	defer magpieBestCache.mu.Unlock()
	out := make(map[string]magpieBestAnswer, len(magpieBestCache.best))
	for k, v := range magpieBestCache.best {
		out[k] = v
	}
	return out, magpieBestCache.at, magpieBestCache.err
}

// pelicanMagpieAfterSweep 在一轮探测结束后联动 magpie-ops：把这一轮里降智的分组去重，
// 对配了映射且不在冷却期的分组各请 magpie 调整一次（一个分组一轮最多一次）。
// 异步执行，magpie 不可用只记日志与状态，不影响探测结果落库。
func pelicanMagpieAfterSweep(ctx context.Context, results []*model.PelicanTestResult) {
	cfg := model.GetPelicanTestConfig()
	if !cfg.MagpieLinked() {
		return
	}
	// 每轮测完都问一次「这些分组各自的最好成绩是多少」：new-api 要的是分组
	// 的能力上限，不是某条渠道这一次的分数。
	refreshMagpieBest(ctx, cfg)
	seen := make(map[string]struct{}, len(results))
	for _, record := range results {
		if record == nil || record.Status != "success" {
			continue
		}
		// 这条结果本来就是从 magpie 取来的：那个分组刚被它自己测过，
		// 再请一次 adjust 等于第三次掏钱，而且它已经知道该怎么处置了。
		if record.Source == model.PelicanSourceMagpie {
			continue
		}
		if record.Verdict != service.PelicanVerdictDegraded && record.Verdict != service.PelicanVerdictPoor {
			continue
		}
		if _, done := seen[record.Group]; done {
			continue
		}
		magpieGroup, mapped := pelicanMagpieGroupFor(cfg, record.Group)
		if !mapped {
			continue
		}
		if !pelicanMagpieCooldownFree(cfg, record.Group) {
			common.SysLog(common.LogText("pelican magpie: skip %s, still cooling down", record.Group))
			continue
		}
		// 先占住冷却再发请求：并发的两轮巡视不会给同一个分组各打一次 adjust。
		seen[record.Group] = struct{}{}
		pelicanMagpieAdjust(ctx, cfg, record, magpieGroup)
	}
}

// pelicanMagpieAdjust 发一次调整请求并记录结果。dry run 只写日志，不真的改 magpie。
func pelicanMagpieAdjust(ctx context.Context, cfg model.PelicanTestConfig, record *model.PelicanTestResult, magpieGroup string) {
	reason := fmt.Sprintf("new-api group %s degraded: %s %d points", record.Group, record.Verdict, record.Score)
	now := time.Now().Unix()
	if cfg.MagpieDryRun {
		common.SysLog(common.LogText("pelican magpie: dry run, would adjust %s for group %s (%s %d)", magpieGroup, record.Group, record.Verdict, record.Score))
		pelicanMagpieSaveState(record.Group, func(state *model.PelicanMagpieState) {
			state.MagpieGroup = magpieGroup
			state.AdjustedAt = now
			state.OK = true
			state.DryRun = true
			state.Error = ""
			state.Result = "dry run"
		})
		return
	}

	since, err := callMagpieAdjust(ctx, cfg, magpieGroup, reason)
	if err != nil {
		detail := pelicanMagpieDetail(err)
		common.SysError(common.LogText("pelican magpie: adjust %s for group %s failed: %s", magpieGroup, record.Group, detail))
		// 失败也记时间、也占用冷却：恢复巡视每分钟都会补测不可用的组合，
		// 不让失败占冷却的话，magpie 挂掉时会被每分钟打一次 adjust。
		pelicanMagpieSaveState(record.Group, func(state *model.PelicanMagpieState) {
			state.MagpieGroup = magpieGroup
			state.AdjustedAt = now
			state.OK = false
			state.DryRun = false
			state.Error = detail
			state.Result = ""
			state.Since = ""
			state.Actions = nil
		})
		return
	}

	common.SysLog(common.LogText("pelican magpie: asked %s to adjust %s", magpieGroup, record.Group))
	pelicanMagpieSaveState(record.Group, func(state *model.PelicanMagpieState) {
		state.MagpieGroup = magpieGroup
		state.AdjustedAt = now
		state.OK = true
		state.DryRun = false
		state.Error = ""
		// 调整还在后台跑：先清掉上一次的对账结果，等 reconcile 写回这次动了谁。
		state.Result = ""
		state.Actions = nil
		state.Since = since
	})
	// 调整是后台任务：隔几秒去看一看到底动了谁。独立超时，不跟着请求或被调用方的收尾走。
	go pelicanMagpieReconcile(context.WithoutCancel(ctx), cfg, record.Group, magpieGroup, since)
}

// pelicanMagpieGroupsCache 让一轮巡视只读一次 magpie-ops 的分组清单：
// 一个分组一个目标，10 个目标就是 10 次 HTTP，没必要。
// 变量而不是常量，测试里可以直接清空。
var (
	pelicanMagpieGroupsMu       sync.Mutex
	pelicanMagpieGroupsAt       time.Time
	pelicanMagpieGroupsSnapshot []magpieGroup
	pelicanMagpieGroupsTTL      = 20 * time.Second
)

// cachedMagpieGroups 读 magpie-ops 的分组清单，短时间内的重复调用复用同一份。
// 读不到就返回错误，调用方按「这次自己测」处理：拿不到别人的答案，自己测才是对的。
func cachedMagpieGroups(ctx context.Context, cfg model.PelicanTestConfig) ([]magpieGroup, error) {
	pelicanMagpieGroupsMu.Lock()
	if !pelicanMagpieGroupsAt.IsZero() && time.Since(pelicanMagpieGroupsAt) < pelicanMagpieGroupsTTL {
		groups := pelicanMagpieGroupsSnapshot
		pelicanMagpieGroupsMu.Unlock()
		return groups, nil
	}
	pelicanMagpieGroupsMu.Unlock()

	groups, err := listMagpieGroups(ctx, cfg)
	if err != nil {
		return nil, err
	}
	pelicanMagpieGroupsMu.Lock()
	pelicanMagpieGroupsSnapshot, pelicanMagpieGroupsAt = groups, time.Now()
	pelicanMagpieGroupsMu.Unlock()
	return groups, nil
}

// pelicanMagpieReused 是一次「用 magpie-ops 的结果代替自己测」的判定结果。
type pelicanMagpieReused struct {
	Record  *model.PelicanTestResult
	Member  magpieGroupMember
	Probed  time.Time
	GroupId string
}

// reuseMagpieResult 看 magpie-ops 最近有没有替我们测过这个（分组, 模型）：
// 同一个 magpie 分组里，模型名相同的成员如果刚被测过，就把最差的那个答案拿过来用。
//
// 为什么取最差：magpie 的分组是多渠道轮询，用户实际可能落到任何一个成员上；
// 与 magpie-ops 自己的处置口径一致（它也是按最差的成员决定要不要动手）。
//
// 返回 ok=false 表示这次该自己测：没联动、没映射、窗口是 0、那边没有新鲜的答案。
func reuseMagpieResult(ctx context.Context, cfg model.PelicanTestConfig, group, modelName string, channelId int) (pelicanMagpieReused, bool) {
	return reuseMagpieResultWithin(ctx, cfg, group, modelName, channelId, cfg.MagpieReuseWindow())
}

// reuseMagpieResultWithin 与 reuseMagpieResult 一样，只是窗口由调用方给：
// 只读模式（绑定 magpie 分组）要的是「它 24 小时内测过什么」，与复用窗口无关。
func reuseMagpieResultWithin(ctx context.Context, cfg model.PelicanTestConfig, group, modelName string, channelId, windowMinutes int) (pelicanMagpieReused, bool) {
	if !cfg.MagpieLinked() {
		return pelicanMagpieReused{}, false
	}
	window := windowMinutes
	if window <= 0 {
		return pelicanMagpieReused{}, false
	}
	magpieGroup, mapped := pelicanMagpieGroupFor(cfg, group)
	if !mapped {
		return pelicanMagpieReused{}, false
	}
	groups, err := cachedMagpieGroups(ctx, cfg)
	if err != nil {
		common.SysLog(common.LogText("pelican magpie: cannot reuse a result for %s/%s: %s", group, modelName, err))
		return pelicanMagpieReused{}, false
	}
	var chosen magpieGroupMember
	chosenAt := time.Time{}
	chosenGroup := ""
	for _, candidate := range groups {
		if candidate.Id != magpieGroup {
			continue
		}
		for _, member := range candidate.Members {
			if !strings.EqualFold(strings.TrimSpace(member.Model), modelName) {
				continue
			}
			// 从没测过的成员（pending）不算答案
			if member.Verdict == "" || member.Verdict == "pending" {
				continue
			}
			probedAt, err := time.Parse(time.RFC3339Nano, member.LastProbedAt)
			if err != nil {
				continue
			}
			if time.Since(probedAt) > time.Duration(window)*time.Minute {
				continue
			}
			if chosenGroup == "" || member.Score < chosen.Score {
				chosen, chosenAt, chosenGroup = member, probedAt, candidate.Id
			}
		}
	}
	if chosenGroup == "" {
		return pelicanMagpieReused{}, false
	}
	record := &model.PelicanTestResult{
		ModelName:  modelName,
		Group:      group,
		ChannelId:  channelId,
		Status:     "success",
		Verdict:    chosen.Verdict,
		Score:      chosen.Score,
		LatencyMs:  chosen.TtftMs,
		CreatedAt:  time.Now().Unix(),
		Source:     model.PelicanSourceMagpie,
		ObservedAt: chosenAt.Unix(),
	}
	return pelicanMagpieReused{Record: record, Member: chosen, Probed: chosenAt, GroupId: chosenGroup}, true
}

// magpieBest 与 magpieBestAt 给设置页用：拿的是缓存，不在请求里等 magpie。
func magpieBest() map[string]magpieBestAnswer {
	best, _, _ := magpieBestSnapshot()
	return best
}

func magpieBestAt() string {
	_, at, err := magpieBestSnapshot()
	if at.IsZero() {
		return ""
	}
	if err != "" {
		return err
	}
	return at.UTC().Format(time.RFC3339)
}

// snippet 是响应体开头的一小段，放进错误信息里足够定位问题，又不会太长。
func snippet(b []byte) string {
	s := strings.TrimSpace(string(b))
	if len(s) > 80 {
		return s[:80] + "…"
	}
	return s
}
