package model

import (
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func resetPelicanResults(t *testing.T) {
	t.Helper()
	require.NoError(t, DB.Exec("DELETE FROM pelican_test_results").Error)
}

func TestSavePelicanTestResultKeepsTheNewestPerModel(t *testing.T) {
	resetPelicanResults(t)

	// 单模型保留 pelicanRetentionPerModel 条，写多一条把最旧的挤掉。
	for i := range pelicanRetentionPerModel + 5 {
		require.NoError(t, SavePelicanTestResult(&PelicanTestResult{
			ModelName: "gpt-4.1",
			Status:    "success",
			Verdict:   "healthy",
			Score:     i,
			CreatedAt: int64(1700000000 + i),
		}))
	}
	require.NoError(t, SavePelicanTestResult(&PelicanTestResult{
		ModelName: "claude-opus",
		Status:    "failed",
		Verdict:   "failed",
		Error:     "upstream 502",
		CreatedAt: 1700000900,
	}))

	var stored int64
	require.NoError(t, DB.Model(&PelicanTestResult{}).Where("model_name = ?", "gpt-4.1").Count(&stored).Error)
	assert.Equal(t, int64(pelicanRetentionPerModel), stored)

	latest, err := GetLatestPelicanResults()
	require.NoError(t, err)
	require.Len(t, latest, 2)
	// gpt-4.1 的最新一次是最后写入的那条，分数等于最后一次的序号。
	assert.Equal(t, pelicanRetentionPerModel+4, latest["gpt-4.1"].Score)
	assert.Equal(t, "failed", latest["claude-opus"].Verdict)
	assert.Equal(t, "upstream 502", latest["claude-opus"].Error)
}

func TestGetPelicanTestResultByIdReportsMissingRows(t *testing.T) {
	resetPelicanResults(t)

	saved := &PelicanTestResult{
		ModelName:  "gpt-4.1",
		Status:     "success",
		Verdict:    "degraded",
		Score:      65,
		Dimensions: "drawing,subject",
		Output:     "<svg></svg>",
	}
	require.NoError(t, SavePelicanTestResult(saved))
	require.NotZero(t, saved.Id)

	found, err := GetPelicanTestResultById(saved.Id)
	require.NoError(t, err)
	assert.Equal(t, "<svg></svg>", found.Output)

	_, err = GetPelicanTestResultById(saved.Id + 999)
	require.Error(t, err)
	assert.True(t, IsPelicanResultMissing(err))
}
