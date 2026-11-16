package model

import (
	"strconv"
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

// PelicanProbeTarget 是一次鹈鹕探测的目标：同一个模型在不同分组可能走不同渠道，
// 智商与延迟必须分开记录，否则一个分组的故障会污染另一个分组的历史。
type PelicanProbeTarget struct {
	Group string `json:"group"`
	Model string `json:"model"`
}

// pelicanRetentionPerModel 限制单个模型保留的历史记录条数：趋势图靠这些元数据。
const pelicanRetentionPerModel = 200

// pelicanPreviewRetention 只为最近的若干条保留 HTML 源码，更早的记录只留元数据，
// 免得每个模型几十 KB 的动画源码把库撑大。
const pelicanPreviewRetention = 20

// 后台可配置项：参与巡视的（分组, 模型）组合（JSON 数组，留空表示自动覆盖全部可测组合）
// 与巡视间隔分钟数（0 表示不自动巡视）。
const (
	PelicanTestModelsOption   = "PelicanTestModels"
	PelicanTestIntervalOption = "PelicanTestIntervalMinutes"
)

func pelicanOption(key string) string {
	common.OptionMapRWMutex.RLock()
	defer common.OptionMapRWMutex.RUnlock()
	return common.OptionMap[key]
}

// pelicanTargetKey 去掉分组与模型名两侧的空白，避免配置里出现只差空格的重影条目。
func pelicanTargetKey(target PelicanProbeTarget) PelicanProbeTarget {
	return PelicanProbeTarget{
		Group: strings.TrimSpace(target.Group),
		Model: strings.TrimSpace(target.Model),
	}
}

// GetPelicanTestTargets 返回后台配置的巡视组合；未配置时返回空切片表示自动。
// 旧格式是只有模型名的字符串数组，读到它时把分组留空，表示「该模型在所有分组」。
func GetPelicanTestTargets() []PelicanProbeTarget {
	raw := strings.TrimSpace(pelicanOption(PelicanTestModelsOption))
	if raw == "" {
		return nil
	}
	var targets []PelicanProbeTarget
	if err := common.UnmarshalJsonStr(raw, &targets); err != nil {
		// 解码器可能先塞进若干零值元素再报错，所以旧格式单独用一个切片。
		var legacy []string
		if err := common.UnmarshalJsonStr(raw, &legacy); err != nil {
			return nil
		}
		legacyTargets := make([]PelicanProbeTarget, 0, len(legacy))
		for _, name := range legacy {
			target := pelicanTargetKey(PelicanProbeTarget{Model: name})
			if target.Model == "" {
				continue
			}
			legacyTargets = append(legacyTargets, target)
		}
		return legacyTargets
	}
	cleaned := make([]PelicanProbeTarget, 0, len(targets))
	seen := make(map[PelicanProbeTarget]struct{}, len(targets))
	for _, target := range targets {
		target = pelicanTargetKey(target)
		if target.Model == "" {
			continue
		}
		if _, ok := seen[target]; ok {
			continue
		}
		seen[target] = struct{}{}
		cleaned = append(cleaned, target)
	}
	return cleaned
}

// GetPelicanTestIntervalMinutes 返回自动巡视间隔，0 表示关闭。
func GetPelicanTestIntervalMinutes() int {
	minutes, err := strconv.Atoi(strings.TrimSpace(pelicanOption(PelicanTestIntervalOption)))
	if err != nil || minutes < 0 {
		return 0
	}
	return minutes
}

// SavePelicanTestConfig 保存后台配置。两个键一起写，避免只落一半。
func SavePelicanTestConfig(targets []PelicanProbeTarget, intervalMinutes int) error {
	if targets == nil {
		targets = []PelicanProbeTarget{}
	}
	encoded, err := common.Marshal(targets)
	if err != nil {
		return err
	}
	return UpdateOptionsBulk(map[string]string{
		PelicanTestModelsOption:   string(encoded),
		PelicanTestIntervalOption: strconv.Itoa(intervalMinutes),
	})
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
