package model

import (
	"time"

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

// pelicanRetentionPerModel 限制单个模型保留的历史记录条数，避免 HTML 源码无限增长。
const pelicanRetentionPerModel = 20

// SavePelicanTestResult 写入一条记录并裁剪该模型的历史记录。
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
	return DB.Exec(
		"DELETE FROM pelican_test_results WHERE model_name = ? AND id NOT IN ("+
			"SELECT id FROM (SELECT id FROM pelican_test_results WHERE model_name = ? ORDER BY id DESC LIMIT ?) AS keep_rows)",
		result.ModelName, result.ModelName, pelicanRetentionPerModel,
	).Error
}

// GetLatestPelicanResults 返回每个模型最近一次记录，模型名作为键。
// 用 MAX(id) 取样，MySQL、PostgreSQL 与 SQLite 都支持。
func GetLatestPelicanResults() (map[string]*PelicanTestResult, error) {
	var results []PelicanTestResult
	err := DB.Model(&PelicanTestResult{}).
		Where("id IN (SELECT MAX(id) FROM pelican_test_results GROUP BY model_name)").
		Find(&results).Error
	if err != nil {
		return nil, err
	}
	latest := make(map[string]*PelicanTestResult, len(results))
	for i := range results {
		latest[results[i].ModelName] = &results[i]
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

// GetPelicanTestHistory 返回一个模型的历史记录（不含 HTML 源码）。
func GetPelicanTestHistory(modelName string, limit int) ([]PelicanTestResult, error) {
	if limit <= 0 || limit > pelicanRetentionPerModel {
		limit = pelicanRetentionPerModel
	}
	var results []PelicanTestResult
	err := DB.Model(&PelicanTestResult{}).
		Select("id, model_name, group, channel_id, status, verdict, score, latency_ms, dimensions, error, created_at").
		Where("model_name = ?", modelName).
		Order("id DESC").
		Limit(limit).
		Find(&results).Error
	return results, err
}

// IsPelicanResultMissing reports whether the load failed because the row is gone.
func IsPelicanResultMissing(err error) bool {
	return err == gorm.ErrRecordNotFound
}
