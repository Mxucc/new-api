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

	result, err := QuerySummaryAll(24, nil)
	require.NoError(t, err)
	require.Len(t, result.Models, 1)

	summary := result.Models[0]
	assert.Equal(t, "status-model", summary.ModelName)
	assert.Equal(t, int64(4), summary.RequestCount)
	assert.Equal(t, int64(133), summary.AvgTtftMs)
	assert.Equal(t, int64(350), summary.AvgLatencyMs)
	assert.Equal(t, 75.0, summary.SuccessRate)
	assert.Equal(t, 100.0, summary.AvgTps)
}
