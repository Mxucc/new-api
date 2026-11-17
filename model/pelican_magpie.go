package model

import (
	"strings"
	"sync"

	"github.com/QuantumNous/new-api/common"
)

// PelicanMagpieStateOption 保存上一次「请 magpie 调整这个分组」的结果：
// 时间、对应的 magpie 分组、成功与否与失败原因，页面与系统日志靠它对账。
// 放在 option 里而不是新建表：只在动作发生时写，量极小，也不用为三种数据库做迁移。
const PelicanMagpieStateOption = "PelicanMagpieState"

// PelicanMagpieState 是一个 new-api 分组的联动状态。
type PelicanMagpieState struct {
	// MagpieGroup 是上次调用时映射到的 magpie 分组 id：映射改过之后旧状态就不再适用。
	MagpieGroup string `json:"magpie_group,omitempty"`
	// AdjustedAt 是上次发起调整的时间（Unix 秒），不管成功与否都记，冷却按它算。
	AdjustedAt int64 `json:"adjusted_at,omitempty"`
	// OK 表示上次调整请求真的被 magpie-ops 接受了。
	OK bool `json:"ok,omitempty"`
	// DryRun 表示上次只是记录、没有真的发请求。
	DryRun bool `json:"dry_run,omitempty"`
	// Error 是上次失败的原因，成功后清空。
	Error string `json:"error,omitempty"`
	// Since 是上次调整在 magpie-ops 那边的开始时刻，用它去 /api/actions 对账。
	Since string `json:"since,omitempty"`
	// Actions 是对账得到的动作明细：动了谁、做了什么、当时智商多少。
	Actions []PelicanMagpieActionResult `json:"actions,omitempty"`
	// Result 是没法结构化时的状态文本（dry run / 无需调整 / 读不到动作记录）。
	Result string `json:"result,omitempty"`
}

// pelicanMagpieStateLock 让读改写一条状态成为原子操作：巡视与单次测试会并发收尾。
var pelicanMagpieStateLock sync.Mutex

// GetPelicanMagpieStates 读出全部联动状态；内容损坏时按「没有状态」处理。
func GetPelicanMagpieStates() map[string]PelicanMagpieState {
	states := make(map[string]PelicanMagpieState)
	raw := strings.TrimSpace(pelicanOption(PelicanMagpieStateOption))
	if raw == "" {
		return states
	}
	if err := common.UnmarshalJsonStr(raw, &states); err != nil {
		return make(map[string]PelicanMagpieState)
	}
	return states
}

// UpdatePelicanMagpieState 在锁里读改写一个分组的联动状态，避免并发收尾互相覆盖。
func UpdatePelicanMagpieState(group string, mutate func(state *PelicanMagpieState)) error {
	pelicanMagpieStateLock.Lock()
	defer pelicanMagpieStateLock.Unlock()
	states := GetPelicanMagpieStates()
	state := states[group]
	mutate(&state)
	states[group] = state
	encoded, err := common.Marshal(states)
	if err != nil {
		return err
	}
	return UpdateOptionsBulk(map[string]string{PelicanMagpieStateOption: string(encoded)})
}

// PelicanMagpieActionResult 是 magpie-ops 动作记录里我们关心的一小部分。
type PelicanMagpieActionResult struct {
	Ref  string `json:"ref"`
	Kind string `json:"kind"`
	// Score 是驱动这次动作的智商。旧记录没有这个字段，所以用指针区分「没有」与 0 分。
	Score *int `json:"score,omitempty"`
}
