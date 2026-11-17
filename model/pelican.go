package model

import (
	"net/url"
	"strings"
	"time"

	"github.com/QuantumNous/new-api/common"
	"gorm.io/gorm"
)

// PelicanTestResult 是模型的一次鹈鹕测智记录。
// Output 保存抽出后的 HTML/SVG 源码，界面只在沙箱预览里渲染它。
type PelicanTestResult struct {
	Id         int64  `json:"id" gorm:"primaryKey;autoIncrement"`
	ModelName  string `json:"model_name" gorm:"size:128;index"`
	Group      string `json:"group" gorm:"column:group;size:64"`
	ChannelId  int    `json:"channel_id"`
	Status     string `json:"status" gorm:"size:16"`  // success / failed
	Verdict    string `json:"verdict" gorm:"size:16"` // healthy / degraded / poor / failed
	Score      int    `json:"score"`
	LatencyMs  int64  `json:"latency_ms"`
	Dimensions string `json:"dimensions" gorm:"type:text"`
	Output     string `json:"output" gorm:"type:text"`
	Error      string `json:"error" gorm:"type:text"`
	CreatedAt  int64  `json:"created_at" gorm:"bigint;index"`
}

func (PelicanTestResult) TableName() string {
	return "pelican_test_results"
}

// PelicanProbeTarget 是一次鹈鹕探测的目标：每个分组只挑一个模型，分组不同走的渠道也不同，
// 智商与延迟必须分开记录，否则一个分组的故障会污染另一个分组的历史。
type PelicanProbeTarget struct {
	Group string `json:"group"`
	Model string `json:"model"`
}

// PelicanMagpieGroup 是 new-api 的一个分组到 magpie 一个路由组的映射。
// 两边之间还隔着 sub2api，provider 名对不上，所以只映射到「组」，不映射到渠道。
type PelicanMagpieGroup struct {
	// Group 是 new-api 的分组名，和探测目标里的分组同一个写法。
	Group string `json:"group"`
	// MagpieGroup 是 magpie 的分组 id，例如 never-stuck。
	MagpieGroup string `json:"magpie_group"`
}

// PelicanTestConfig 是后台的巡视设置。探测由系统自己跑，所有用户看同一份结果：
// 测哪些分组由管理员决定，用量记在保存这份配置的管理员名下。
type PelicanTestConfig struct {
	// Targets 是探测白名单，每个分组最多一个模型；空表示不探测任何组合。
	Targets []PelicanProbeTarget `json:"targets"`
	// IntervalMinutes 是自动巡视间隔，0 表示不自动巡视。
	IntervalMinutes int `json:"interval_minutes"`
	// OwnerUserId 是保存这份配置的管理员，探测的用量日志记在他名下。
	OwnerUserId int `json:"owner_user_id"`
	// Prompt 是发给模型的题面，留空用内置默认。
	Prompt string `json:"prompt,omitempty"`
	// PromptContract 是追加在题面后的交付约定，留空用内置默认。
	PromptContract string `json:"prompt_contract,omitempty"`
	// Description 是页面上给用户看的说明，留空用前端翻译的默认说明。
	Description string `json:"description,omitempty"`
	// MagpieURL 是 magpie-ops 的地址（如 http://127.0.0.1:7788），空表示不联动。
	MagpieURL string `json:"magpie_url,omitempty"`
	// MagpieToken 是它的 API token：只在保存时写入，读取时绝不回显。
	MagpieToken string `json:"magpie_token,omitempty"`
	// MagpieGroups 是开了自动调整的 new-api 分组及其 magpie 分组，没列出的不联动。
	MagpieGroups []PelicanMagpieGroup `json:"magpie_groups,omitempty"`
	// MagpieCooldownMinutes 是同一分组两次 adjust 之间的最小间隔，默认 30。
	// adjust 会让 magpie 现场把分组里每个成员都测一遍，不该被一次巡视打很多次。
	MagpieCooldownMinutes int `json:"magpie_cooldown_minutes,omitempty"`
	// MagpieDryRun 只记录「本来会调谁」，不真的改 magpie，先观察用。
	MagpieDryRun bool `json:"magpie_dry_run,omitempty"`
}

// 题面、交付约定与页面说明的内置默认值。管理员可以整体替换它们，
// 于是同一套探测能换成别的题目，而不用改代码。
const (
	DefaultPelicanPrompt      = "创建一个 HTML，内容是 SVG 绘制一个鹈鹕骑自行车的 2D 动画，你不需要任何测试"
	DefaultPelicanContract    = "只输出 HTML，不要使用 Markdown 代码块，不要解释。"
	DefaultPelicanDescription = "每个模型收到同一条题面，分数衡量它把一条多约束指令落成可运行代码的能力。"
)

// 题面与交付约定的长度上限：题面是要发给上游的请求体，不能无限长。
const PelicanPromptMaxRunes = 4000

// PelicanMaxIntervalMinutes 是巡视间隔上限（一天）：再长就没有巡视的意义了。
const PelicanMaxIntervalMinutes = 1440

// 联动冷却的默认值与上限：默认 30 分钟与 magpie-ops 自己的巡检间隔一致，
// 上限一周，避免管理员填出一个永远等不到触发的数字。
const (
	PelicanMagpieDefaultCooldown   = 30
	PelicanMagpieMaxCooldownMinute = 10080
)

// 智商时间条按巡视间隔切片：24 小时里放 24*60/interval 格，一格就是一次巡视。
// 没采到样的格子沿用上一次成功的结果（默认是绿的），只有真失败才变色。
const PelicanStripHours = 24

// 没有配置间隔（不自动巡视）时按 30 分钟切片，一天 48 格。
const PelicanDefaultIntervalMinutes = 30

// PelicanStripCellsFor 返回 24 小时按给定间隔切出的格数。
func PelicanStripCellsFor(intervalMinutes int) int {
	if intervalMinutes <= 0 {
		intervalMinutes = PelicanDefaultIntervalMinutes
	}
	cells := PelicanStripHours * 60 / intervalMinutes
	if cells < 1 {
		return 1
	}
	return cells
}

// EffectivePrompt 返回实际发给模型的题面（含交付约定）。
func (cfg PelicanTestConfig) EffectivePrompt() string {
	prompt := strings.TrimSpace(cfg.Prompt)
	if prompt == "" {
		prompt = DefaultPelicanPrompt
	}
	contract := strings.TrimSpace(cfg.PromptContract)
	if contract == "" {
		contract = DefaultPelicanContract
	}
	return prompt + "\n\n" + contract
}

// pelicanConfigText 归一化管理员填写的长文本：去两侧空白并限制长度。
func pelicanConfigText(value string) string {
	trimmed := strings.TrimSpace(value)
	if runes := []rune(trimmed); len(runes) > PelicanPromptMaxRunes {
		return string(runes[:PelicanPromptMaxRunes])
	}
	return trimmed
}

// pelicanRetentionPerModel 限制单个模型保留的历史记录条数：趋势图靠这些元数据。
const pelicanRetentionPerModel = 200

// pelicanPreviewRetention 只为最近的若干条保留 HTML 源码，更早的记录只留元数据，
// 免得每个模型几十 KB 的动画源码把库撑大。
const pelicanPreviewRetention = 20

// PelicanTestConfigOption 保存整份巡视设置；没有这个键就是「没有配置」，不探测。
const PelicanTestConfigOption = "PelicanTestConfig"

func pelicanOption(key string) string {
	common.OptionMapRWMutex.RLock()
	defer common.OptionMapRWMutex.RUnlock()
	return common.OptionMap[key]
}

// pelicanConfigTargets 归一化探测目标：去掉两侧空白、丢掉缺分组或模型的条目，
// 并保证每个分组只留一个模型——同一分组测两个模型只会让用户看到两行一样的格子。
func pelicanConfigTargets(targets []PelicanProbeTarget) []PelicanProbeTarget {
	cleaned := make([]PelicanProbeTarget, 0, len(targets))
	groups := make(map[string]struct{}, len(targets))
	for _, target := range targets {
		group := strings.TrimSpace(target.Group)
		modelName := strings.TrimSpace(target.Model)
		if group == "" || modelName == "" {
			continue
		}
		if _, ok := groups[group]; ok {
			continue
		}
		groups[group] = struct{}{}
		cleaned = append(cleaned, PelicanProbeTarget{Group: group, Model: modelName})
	}
	return cleaned
}

// PelicanMagpieGroupIDValid 判断 magpie 分组 id 是否可用：id 会原样进 URL 与请求体，
// 带空白或斜杠的写法在 magpie-ops 那边只会被当成未知分组。
func PelicanMagpieGroupIDValid(id string) bool {
	return id != "" && !strings.ContainsAny(id, " \t/\\")
}

// PelicanMagpieURLValid 判断联动地址是否可用：只接受 http/https 的绝对地址。
func PelicanMagpieURLValid(rawURL string) bool {
	parsed, err := url.Parse(rawURL)
	if err != nil {
		return false
	}
	if parsed.Scheme != "http" && parsed.Scheme != "https" {
		return false
	}
	return parsed.Host != ""
}

// pelicanMagpieGroups 归一化分组映射：去掉空条目与非法 id，同一 new-api 分组只留一条。
func pelicanMagpieGroups(groups []PelicanMagpieGroup) []PelicanMagpieGroup {
	cleaned := make([]PelicanMagpieGroup, 0, len(groups))
	seen := make(map[string]struct{}, len(groups))
	for _, mapping := range groups {
		group := strings.TrimSpace(mapping.Group)
		magpieGroup := strings.TrimSpace(mapping.MagpieGroup)
		if group == "" || !PelicanMagpieGroupIDValid(magpieGroup) {
			continue
		}
		if _, ok := seen[group]; ok {
			continue
		}
		seen[group] = struct{}{}
		cleaned = append(cleaned, PelicanMagpieGroup{Group: group, MagpieGroup: magpieGroup})
	}
	return cleaned
}

// normalizePelicanMagpie 归一化联动配置：地址去尾斜杠、token 去空白、冷却落回安全区间，
// 映射只留合法的分组。地址与 token 缺失时不抹掉映射——保存时已经拒绝过一次，
// 这里再改只会让管理员看到自己填的东西消失。
func (cfg *PelicanTestConfig) normalizePelicanMagpie() {
	cfg.MagpieURL = strings.TrimRight(strings.TrimSpace(cfg.MagpieURL), "/")
	cfg.MagpieToken = strings.TrimSpace(cfg.MagpieToken)
	if cfg.MagpieCooldownMinutes <= 0 {
		cfg.MagpieCooldownMinutes = PelicanMagpieDefaultCooldown
	}
	if cfg.MagpieCooldownMinutes > PelicanMagpieMaxCooldownMinute {
		cfg.MagpieCooldownMinutes = PelicanMagpieMaxCooldownMinute
	}
	cfg.MagpieGroups = pelicanMagpieGroups(cfg.MagpieGroups)
}

// GetPelicanTestConfig 读取巡视设置；内容损坏时按「未配置」处理，一个组合都不探测。
func GetPelicanTestConfig() PelicanTestConfig {
	cfg := PelicanTestConfig{Targets: []PelicanProbeTarget{}}
	raw := strings.TrimSpace(pelicanOption(PelicanTestConfigOption))
	if raw == "" {
		return cfg
	}
	if err := common.UnmarshalJsonStr(raw, &cfg); err != nil {
		return PelicanTestConfig{Targets: []PelicanProbeTarget{}}
	}
	if cfg.IntervalMinutes < 0 || cfg.IntervalMinutes > PelicanMaxIntervalMinutes {
		cfg.IntervalMinutes = 0
	}
	cfg.Targets = pelicanConfigTargets(cfg.Targets)
	cfg.Prompt = pelicanConfigText(cfg.Prompt)
	cfg.PromptContract = pelicanConfigText(cfg.PromptContract)
	cfg.Description = pelicanConfigText(cfg.Description)
	cfg.normalizePelicanMagpie()
	return cfg
}

// SavePelicanTestConfig 整体写入巡视设置：目标、间隔、题面与记账身份一次落库，避免只写一半。
func SavePelicanTestConfig(cfg PelicanTestConfig) error {
	cfg.Targets = pelicanConfigTargets(cfg.Targets)
	cfg.Prompt = pelicanConfigText(cfg.Prompt)
	cfg.PromptContract = pelicanConfigText(cfg.PromptContract)
	cfg.Description = pelicanConfigText(cfg.Description)
	cfg.normalizePelicanMagpie()
	encoded, err := common.Marshal(cfg)
	if err != nil {
		return err
	}
	return UpdateOptionsBulk(map[string]string{PelicanTestConfigOption: string(encoded)})
}

// GetPelicanProbeTargets 返回所有可测的（分组, 模型）组合：abilities 里真正启用
// 该模型的分组。巡视必须按这些分组探测，而不是按触发者的分组——管理员所在分组
// 没有该模型时，逐个组合都会报「无可用渠道」，整轮巡视就全军覆没。
func GetPelicanProbeTargets() ([]PelicanProbeTarget, error) {
	var abilities []Ability
	err := DB.Select("model", "group", "priority").Where("enabled = ?", true).
		Order("priority DESC, " + commonGroupCol + " ASC, model ASC").
		Find(&abilities).Error
	if err != nil {
		return nil, err
	}
	targets := make([]PelicanProbeTarget, 0, len(abilities))
	seen := make(map[PelicanProbeTarget]struct{}, len(abilities))
	for _, ability := range abilities {
		target := PelicanProbeTarget{Group: ability.Group, Model: ability.Model}
		if _, ok := seen[target]; ok {
			continue
		}
		seen[target] = struct{}{}
		targets = append(targets, target)
	}
	return targets, nil
}

// SavePelicanTestResult 写入一条记录，并裁剪该模型的元数据与动画源码。
func SavePelicanTestResult(result *PelicanTestResult) error {
	if result == nil || result.ModelName == "" {
		return nil
	}
	if result.CreatedAt == 0 {
		result.CreatedAt = time.Now().Unix()
	}
	if err := DB.Create(result).Error; err != nil {
		return err
	}
	// 派生表别名让 MySQL 允许在 DELETE 的子查询里引用被删除的表。
	if err := DB.Exec(
		"DELETE FROM pelican_test_results WHERE model_name = ? AND id NOT IN ("+
			"SELECT id FROM (SELECT id FROM pelican_test_results WHERE model_name = ? ORDER BY id DESC LIMIT ?) AS keep_rows)",
		result.ModelName, result.ModelName, pelicanRetentionPerModel,
	).Error; err != nil {
		return err
	}
	// 只有最近的记录需要动画源码用于预览，更早的只留分数与耗时。
	return DB.Exec(
		"UPDATE pelican_test_results SET output = '' WHERE model_name = ? AND output <> '' AND id NOT IN ("+
			"SELECT id FROM (SELECT id FROM pelican_test_results WHERE model_name = ? ORDER BY id DESC LIMIT ?) AS keep_rows)",
		result.ModelName, result.ModelName, pelicanPreviewRetention,
	).Error
}

// GetLatestPelicanResults 返回每个（分组, 模型）组合最近一次记录，组合作为键。
// 用 MAX(id) 取样，MySQL、PostgreSQL 与 SQLite 都支持。
func GetLatestPelicanResults() (map[PelicanProbeTarget]*PelicanTestResult, error) {
	latestIds := DB.Model(&PelicanTestResult{}).
		Select("MAX(id)").
		Group("model_name").
		Group(commonGroupCol)
	var results []PelicanTestResult
	err := DB.Model(&PelicanTestResult{}).
		Where("id IN (?)", latestIds).
		Find(&results).Error
	if err != nil {
		return nil, err
	}
	latest := make(map[PelicanProbeTarget]*PelicanTestResult, len(results))
	for i := range results {
		key := PelicanProbeTarget{Group: results[i].Group, Model: results[i].ModelName}
		latest[key] = &results[i]
	}
	return latest, nil
}

// GetPelicanSuccessfulResults 返回 since 之后的成功记录（不含动画源码），最新的在前。
// 时间条只认成功的结果：上游 502 之类的传输失败算「这次没采到样」，不算降智。
func GetPelicanSuccessfulResults(since int64) ([]PelicanTestResult, error) {
	var results []PelicanTestResult
	err := DB.Model(&PelicanTestResult{}).
		Select("id, model_name, "+commonGroupCol+", channel_id, status, verdict, score, latency_ms, created_at").
		Where("created_at >= ? AND status = ?", since, "success").
		Order("id DESC").
		Find(&results).Error
	return results, err
}

// GetLatestPelicanSuccesses 返回每个（分组, 模型）组合最近一次成功的结果。
func GetLatestPelicanSuccesses() (map[PelicanProbeTarget]*PelicanTestResult, error) {
	latestIds := DB.Model(&PelicanTestResult{}).
		Select("MAX(id)").
		Where("status = ?", "success").
		Group("model_name").
		Group(commonGroupCol)
	var results []PelicanTestResult
	err := DB.Model(&PelicanTestResult{}).
		Where("id IN (?)", latestIds).
		Find(&results).Error
	if err != nil {
		return nil, err
	}
	latest := make(map[PelicanProbeTarget]*PelicanTestResult, len(results))
	for i := range results {
		key := PelicanProbeTarget{Group: results[i].Group, Model: results[i].ModelName}
		latest[key] = &results[i]
	}
	return latest, nil
}

// GetPelicanTestResultById 读取单条记录，用于预览。
func GetPelicanTestResultById(id int64) (*PelicanTestResult, error) {
	var result PelicanTestResult
	err := DB.Where("id = ?", id).First(&result).Error
	if err != nil {
		return nil, err
	}
	return &result, nil
}

// GetPelicanTestHistory 返回一个（分组, 模型）组合的历史记录（不含 HTML 源码）。
func GetPelicanTestHistory(group, modelName string, limit int) ([]PelicanTestResult, error) {
	if limit <= 0 || limit > pelicanRetentionPerModel {
		limit = pelicanRetentionPerModel
	}
	var results []PelicanTestResult
	err := DB.Model(&PelicanTestResult{}).
		Select("id, model_name, "+commonGroupCol+", channel_id, status, verdict, score, latency_ms, dimensions, error, created_at").
		Where("model_name = ?", modelName).
		Where(commonGroupCol+" = ?", group).
		Order("id DESC").
		Limit(limit).
		Find(&results).Error
	return results, err
}

// IsPelicanResultMissing reports whether the load failed because the row is gone.
func IsPelicanResultMissing(err error) bool {
	return err == gorm.ErrRecordNotFound
}
