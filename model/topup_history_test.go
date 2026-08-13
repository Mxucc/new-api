package model

import (
	"testing"

	"github.com/QuantumNous/new-api/common"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestGetUserTopUpsReturnsCompleteOwnHistory(t *testing.T) {
	truncateTables(t)

	const userID = 101
	const otherUserID = 202
	oldTime := common.GetTimestamp() - 31*24*60*60
	recentTime := common.GetTimestamp() - 60

	orders := []*TopUp{
		{UserId: userID, TradeNo: "old-user-order", Amount: 10, CreateTime: oldTime, Status: common.TopUpStatusSuccess},
		{UserId: userID, TradeNo: "recent-user-order", Amount: 20, CreateTime: recentTime, Status: common.TopUpStatusSuccess},
		{UserId: otherUserID, TradeNo: "other-user-order", Amount: 30, CreateTime: recentTime, Status: common.TopUpStatusSuccess},
	}
	for _, order := range orders {
		require.NoError(t, DB.Create(order).Error)
	}

	topups, total, err := GetUserTopUps(userID, &common.PageInfo{Page: 1, PageSize: 20})
	require.NoError(t, err)
	assert.Equal(t, int64(2), total)
	require.Len(t, topups, 2)
	assert.Equal(t, "recent-user-order", topups[0].TradeNo)
	assert.Equal(t, "old-user-order", topups[1].TradeNo)
	for _, topup := range topups {
		assert.Equal(t, userID, topup.UserId)
	}
}

func TestSearchUserTopUpsReturnsCompleteOwnHistory(t *testing.T) {
	truncateTables(t)

	const userID = 303
	const otherUserID = 404
	oldTime := common.GetTimestamp() - 31*24*60*60

	orders := []*TopUp{
		{UserId: userID, TradeNo: "legacy-search-order", Amount: 10, CreateTime: oldTime, Status: common.TopUpStatusSuccess},
		{UserId: userID, TradeNo: "recent-search-order", Amount: 20, CreateTime: common.GetTimestamp(), Status: common.TopUpStatusSuccess},
		{UserId: otherUserID, TradeNo: "private-other-order", Amount: 30, CreateTime: oldTime, Status: common.TopUpStatusSuccess},
	}
	for _, order := range orders {
		require.NoError(t, DB.Create(order).Error)
	}

	topups, total, err := SearchUserTopUps(userID, "legacy-search-order", &common.PageInfo{Page: 1, PageSize: 20})
	require.NoError(t, err)
	assert.Equal(t, int64(1), total)
	require.Len(t, topups, 1)
	assert.Equal(t, "legacy-search-order", topups[0].TradeNo)
	assert.Equal(t, userID, topups[0].UserId)

	topups, total, err = SearchUserTopUps(userID, "", &common.PageInfo{Page: 1, PageSize: 20})
	require.NoError(t, err)
	assert.Equal(t, int64(2), total)
	require.Len(t, topups, 2)
	assert.Equal(t, []string{"recent-search-order", "legacy-search-order"}, []string{topups[0].TradeNo, topups[1].TradeNo})

	// A keyword matching another user's order must not cross the user boundary.
	topups, total, err = SearchUserTopUps(userID, "private-other-order", &common.PageInfo{Page: 1, PageSize: 20})
	require.NoError(t, err)
	assert.Equal(t, int64(0), total)
	assert.Empty(t, topups)
}
