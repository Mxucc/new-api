package perfmetrics

import (
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/QuantumNous/new-api/model"
	"github.com/glebarez/sqlite"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
	"gorm.io/gorm"
)

func TestQuerySummaryAllIncludesRequestCountAndTTFT(t *testing.T) {
	previousDB := model.DB
	t.Cleanup(func() {
		model.DB = previousDB
	})

	dsn := fmt.Sprintf("file:%s?mode=memory&cache=shared", strings.ReplaceAll(t.Name(), "/", "_"))
	db, err := gorm.Open(sqlite.Open(dsn), &gorm.Config{})
	require.NoError(t, err)
	model.DB = db
	require.NoError(t, db.AutoMigrate(&model.PerfMetric{}))

	now := time.Now().Unix()
	require.NoError(t, db.Create(&model.PerfMetric{
		ModelName:      "status-model",
		Group:          "default",
		BucketTs:       now,
		RequestCount:   3,
		SuccessCount:   2,
		TotalLatencyMs: 900,
		TtftSumMs:      300,
		TtftCount:      2,
		OutputTokens:   600,
		GenerationMs:   6000,
	}).Error)
	require.NoError(t, db.Create(&model.PerfMetric{
		ModelName:      "status-model",
		Group:          "default",
		BucketTs:       now - 60,
		RequestCount:   1,
		SuccessCount:   1,
		TotalLatencyMs: 500,
		TtftSumMs:      100,
		TtftCount:      1,
		OutputTokens:   200,
		GenerationMs:   2000,
	}).Error)
	require.NoError(t, db.Create(&model.PerfMetric{
		ModelName:      "status-model",
		Group:          "vip",
		BucketTs:       now,
		RequestCount:   2,
		SuccessCount:   2,
		TotalLatencyMs: 400,
		TtftSumMs:      100,
		TtftCount:      2,
		OutputTokens:   300,
		GenerationMs:   3000,
	}).Error)

	result, err := QuerySummaryAll(24, nil)
	require.NoError(t, err)
	require.Len(t, result.Models, 1)

	summary := result.Models[0]
	assert.Equal(t, "status-model", summary.ModelName)
	assert.Equal(t, int64(6), summary.RequestCount)
	assert.Equal(t, int64(100), summary.AvgTtftMs)
	assert.Equal(t, int64(300), summary.AvgLatencyMs)
	assert.Equal(t, 83.33, summary.SuccessRate)
	assert.Equal(t, 100.0, summary.AvgTps)
	require.Len(t, result.Trend, 2)
	assert.Equal(t, int64(100), result.Trend[0].AvgTtftMs)
	assert.Equal(t, 100.0, result.Trend[0].SuccessRate)
	assert.Equal(t, int64(1), result.Trend[0].RequestCount)
	assert.Equal(t, int64(100), result.Trend[1].AvgTtftMs)
	assert.Equal(t, 80.0, result.Trend[1].SuccessRate)
	assert.Equal(t, int64(5), result.Trend[1].RequestCount)
	require.Len(t, result.Groups, 2)
	assert.Equal(t, "default", result.Groups[0].Group)
	assert.Equal(t, int64(4), result.Groups[0].RequestCount)
	assert.Equal(t, int64(350), result.Groups[0].AvgLatencyMs)
	assert.Equal(t, int64(133), result.Groups[0].AvgTtftMs)
	assert.Equal(t, 75.0, result.Groups[0].SuccessRate)
	assert.Equal(t, 100.0, result.Groups[0].AvgTps)
	assert.Equal(t, "vip", result.Groups[1].Group)
	assert.Equal(t, int64(2), result.Groups[1].RequestCount)
	assert.Equal(t, int64(200), result.Groups[1].AvgLatencyMs)
	assert.Equal(t, int64(50), result.Groups[1].AvgTtftMs)
	assert.Equal(t, 100.0, result.Groups[1].SuccessRate)
	assert.Equal(t, 100.0, result.Groups[1].AvgTps)
}
