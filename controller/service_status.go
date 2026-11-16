package controller

import (
	"net/http"
	"strconv"
	"time"

	"github.com/QuantumNous/new-api/common"
	"github.com/QuantumNous/new-api/model"
	"github.com/QuantumNous/new-api/service"
	"github.com/QuantumNous/new-api/setting/ratio_setting"
	"github.com/gin-gonic/gin"
	"github.com/samber/lo"
)

// serviceStatusWindows 是状态页支持的统计区间（小时）。
var serviceStatusWindows = []int{24, 72, 168}

// GetServiceStatus 返回被动式服务状态：只读真实请求的聚合结果，不发任何探测请求。
func GetServiceStatus(c *gin.Context) {
	hours := 24
	if raw := c.Query("hours"); raw != "" {
		parsed, err := strconv.Atoi(raw)
		if err != nil {
			common.ApiErrorMsg(c, "invalid hours")
			return
		}
		hours = parsed
	}
	if !lo.Contains(serviceStatusWindows, hours) {
		common.ApiErrorMsg(c, "invalid hours")
		return
	}

	now := time.Now()
	startTs := now.Unix() - int64(hours)*3600
	activeGroups := append(lo.Keys(ratio_setting.GetGroupRatioCopy()), "auto")
	buckets, err := model.GetPerfMetricsSummaryBucketsAll(startTs, now.Unix(), activeGroups)
	if err != nil {
		common.ApiError(c, err)
		return
	}

	samples := make([]service.ServiceStatusSample, 0, len(buckets))
	for _, bucket := range buckets {
		samples = append(samples, service.ServiceStatusSample{
			Group:     bucket.Group,
			Model:     bucket.ModelName,
			BucketTs:  bucket.BucketTs,
			Requests:  bucket.RequestCount,
			Successes: bucket.SuccessCount,
			LatencyMs: bucket.TotalLatencyMs,
		})
	}

	c.JSON(http.StatusOK, gin.H{
		"success": true,
		"data":    service.BuildServiceStatus(samples, hours, now),
	})
}
