package controller

import (
	"bytes"
	"context"
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
)

const (
	// pelicanMagpieTimeout 是一次联动请求的上限：magpie-ops 就在回环上跑，
	// 10 秒还没回就是它出了问题，不能把巡视收尾拖住。
	pelicanMagpieTimeout = 10 * time.Second
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
	payload, err := io.ReadAll(io.LimitReader(response.Body, 256<<10))
	if err != nil {
		return nil, err
	}
	if response.StatusCode < 200 || response.StatusCode > 299 {
		return nil, fmt.Errorf("magpie-ops %s: %s", response.Status, strings.TrimSpace(string(payload)))
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
		return nil, fmt.Errorf("magpie-ops reply: %w", err)
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

// pelicanMagpieAfterSweep 在一轮探测结束后联动 magpie-ops：把这一轮里降智的分组去重，
// 对配了映射且不在冷却期的分组各请 magpie 调整一次（一个分组一轮最多一次）。
// 异步执行，magpie 不可用只记日志与状态，不影响探测结果落库。
func pelicanMagpieAfterSweep(ctx context.Context, results []*model.PelicanTestResult) {
	cfg := model.GetPelicanTestConfig()
	if !cfg.MagpieLinked() {
		return
	}
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
	if !cfg.MagpieLinked() {
		return pelicanMagpieReused{}, false
	}
	window := cfg.MagpieReuseWindow()
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
