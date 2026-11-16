package model

import (
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

// PelicanTestConfig 是后台的巡视设置。探测由系统自己跑，所有用户看同一份结果：
// 测哪些分组由管理员决定，用量记在保存这份配置的管理员名下。
type PelicanTestConfig struct {
	// Targets 是探测白名单，每个分组最多一个模型；空表示不探测任何组合。
	Targets []PelicanProbeTarget `json:"targets"`
	// IntervalMinutes 是自动巡视间隔，0 表示不自动巡视。
	IntervalMinutes int `json:"interval_minutes"`
	// OwnerUserId 是保存这份配置的管理员，探测的用量日志记在他名下。
	OwnerUserId int `json:"owner_user_id"`
}

// PelicanMaxIntervalMinutes 是巡视间隔上限（一天）：再长就没有巡视的意义了。
const PelicanMaxIntervalMinutes = 1440

// 智商时间条的时间窗与格数：格子按时间切片画，和状态页一样，
// 只测过一次的组合也能铺成一整条，而不是孤零零一个色块。
const (
	PelicanStripHours = 24
	PelicanStripCells = 12
)

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
	return cfg
}

// SavePelicanTestConfig 整体写入巡视设置：目标、间隔与记账身份一次落库，避免只写一半。
func SavePelicanTestConfig(cfg PelicanTestConfig) error {
	cfg.Targets = pelicanConfigTargets(cfg.Targets)
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

// GetPelicanResultsSince 返回 since 之后的记录（不含动画源码），最新的在前，
// 供页面把每个组合画成时间切片。
func GetPelicanResultsSince(since int64) ([]PelicanTestResult, error) {
	var results []PelicanTestResult
	err := DB.Model(&PelicanTestResult{}).
		Select("id, model_name, "+commonGroupCol+", channel_id, status, verdict, score, latency_ms, created_at").
		Where("created_at >= ?", since).
		Order("id DESC").
		Find(&results).Error
	return results, err
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
