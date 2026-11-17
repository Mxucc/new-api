package service

import (
	"sort"
	"time"
)

// 服务状态页是被动式的：它只读真实请求留下的聚合数据（perf_metrics），
// 不对模型额外发探测请求，所以状态反映的是「用户实际调用时发生了什么」。
// 服务状态只有两档：可用与不可用。可用率 >= 99% 记可用，其余记不可用；
// 区间内没有流量也算可用——没人用不等于挂了。
const (
	ServiceStatusAvailable   = "available"
	ServiceStatusUnavailable = "unavailable"
)

// serviceStatusCellCount 是状态条上的格子数量，窗口多长都保持同样的视觉密度。
const serviceStatusCellCount = 48

// ServiceStatusSample 是一个（分组, 模型）在一个时间桶里的被动采样结果。
type ServiceStatusSample struct {
	Group     string
	Model     string
	BucketTs  int64
	Requests  int64
	Successes int64
	LatencyMs int64
}

type ServiceStatusCell struct {
	Start       int64   `json:"start"`
	Requests    int64   `json:"requests"`
	SuccessRate float64 `json:"success_rate"`
	Status      string  `json:"status"`
}

type ServiceStatusModel struct {
	Group        string              `json:"group"`
	ModelName    string              `json:"model_name"`
	Requests     int64               `json:"requests"`
	SuccessRate  float64             `json:"success_rate"`
	AvgLatencyMs int64               `json:"avg_latency_ms"`
	Status       string              `json:"status"`
	Cells        []ServiceStatusCell `json:"cells"`
}

type ServiceStatusView struct {
	GeneratedAt int64                `json:"generated_at"`
	WindowHours int                  `json:"window_hours"`
	WindowStart int64                `json:"window_start"`
	WindowEnd   int64                `json:"window_end"`
	CellSeconds int64                `json:"cell_seconds"`
	DataThrough int64                `json:"data_through"`
	Requests    int64                `json:"requests"`
	SuccessRate float64              `json:"success_rate"`
	Status      string               `json:"status"`
	Cells       []ServiceStatusCell  `json:"cells"`
	Models      []ServiceStatusModel `json:"models"`
}

// ServiceStatusOfSuccessRate 把可用率换算成状态；没有流量时返回可用，
// 不把「没人用」当成「挂了」。
func ServiceStatusOfSuccessRate(successRate float64, requests int64) string {
	if requests <= 0 || successRate >= 99 {
		return ServiceStatusAvailable
	}
	return ServiceStatusUnavailable
}

// successRatePercent 返回两位小数的可用率百分比，和性能页的口径一致。
func successRatePercent(successes, requests int64) float64 {
	if requests <= 0 {
		return 0
	}
	rate := float64(successes) / float64(requests) * 100
	return float64(int64(rate*100+0.5)) / 100
}

// serviceStatusKey 让被动状态按（分组, 模型）聚合：同一个模型在不同分组
// 可能走不同渠道，混在一起看不出是哪个分组出问题。
type serviceStatusKey struct {
	group string
	model string
}

type serviceStatusCounters struct {
	requests  int64
	successes int64
	latencyMs int64
}

func (c *serviceStatusCounters) add(other serviceStatusCounters) {
	c.requests += other.requests
	c.successes += other.successes
	c.latencyMs += other.latencyMs
}

func (c serviceStatusCounters) cell(start int64) ServiceStatusCell {
	return ServiceStatusCell{
		Start:       start,
		Requests:    c.requests,
		SuccessRate: successRatePercent(c.successes, c.requests),
		Status:      ServiceStatusOfSuccessRate(successRatePercent(c.successes, c.requests), c.requests),
	}
}

// emptyServiceStatusCells 先铺满格子，没有流量的格子按可用处理，
// 这样状态条的时间轴始终连续，也不会把「没人用」画成故障。
func emptyServiceStatusCells(windowStart, cellSeconds int64) []ServiceStatusCell {
	cells := make([]ServiceStatusCell, serviceStatusCellCount)
	for i := range cells {
		cells[i] = ServiceStatusCell{
			Start:  windowStart + int64(i)*cellSeconds,
			Status: ServiceStatusAvailable,
		}
	}
	return cells
}

// BuildServiceStatus 把被动采样聚合成状态页数据。
// 时间桶按固定格子宽度落位；窗口内没有流量的格子按可用处理。
func BuildServiceStatus(samples []ServiceStatusSample, hours int, now time.Time) ServiceStatusView {
	if hours <= 0 {
		hours = 24
	}
	windowSeconds := int64(hours) * 3600
	windowEnd := now.Unix()
	windowStart := windowEnd - windowSeconds
	cellSeconds := windowSeconds / serviceStatusCellCount
	if cellSeconds <= 0 {
		cellSeconds = 1
	}

	cells := emptyServiceStatusCells(windowStart, cellSeconds)
	totals := make([]serviceStatusCounters, serviceStatusCellCount)
	modelTotals := map[serviceStatusKey]*serviceStatusCounters{}
	modelCells := map[serviceStatusKey][]serviceStatusCounters{}
	var overall serviceStatusCounters
	dataThrough := int64(0)

	for _, sample := range samples {
		if sample.BucketTs < windowStart || sample.BucketTs > windowEnd {
			continue
		}
		if sample.Requests <= 0 {
			continue
		}
		index := int((sample.BucketTs - windowStart) / cellSeconds)
		if index < 0 || index >= serviceStatusCellCount {
			index = serviceStatusCellCount - 1
		}
		counters := serviceStatusCounters{
			requests:  sample.Requests,
			successes: sample.Successes,
			latencyMs: sample.LatencyMs,
		}
		totals[index].add(counters)
		overall.add(counters)
		if sample.BucketTs > dataThrough {
			dataThrough = sample.BucketTs
		}

		key := serviceStatusKey{group: sample.Group, model: sample.Model}
		total, ok := modelTotals[key]
		if !ok {
			total = &serviceStatusCounters{}
			modelTotals[key] = total
			modelCells[key] = make([]serviceStatusCounters, serviceStatusCellCount)
		}
		total.add(counters)
		modelCells[key][index].add(counters)
	}

	for i, counters := range totals {
		cells[i] = counters.cell(windowStart + int64(i)*cellSeconds)
	}

	overallRate := successRatePercent(overall.successes, overall.requests)
	models := make([]ServiceStatusModel, 0, len(modelTotals))
	for key, total := range modelTotals {
		rate := successRatePercent(total.successes, total.requests)
		cells := emptyServiceStatusCells(windowStart, cellSeconds)
		for i, counters := range modelCells[key] {
			cells[i] = counters.cell(windowStart + int64(i)*cellSeconds)
		}
		models = append(models, ServiceStatusModel{
			Group:        key.group,
			ModelName:    key.model,
			Requests:     total.requests,
			SuccessRate:  rate,
			AvgLatencyMs: averageLatency(total),
			Status:       ServiceStatusOfSuccessRate(rate, total.requests),
			Cells:        cells,
		})
	}
	// 流量最大的（分组, 模型）排前面：状态页先回答「主力分组现在怎么样」。
	sort.Slice(models, func(i, j int) bool {
		if models[i].Requests != models[j].Requests {
			return models[i].Requests > models[j].Requests
		}
		if models[i].Group != models[j].Group {
			return models[i].Group < models[j].Group
		}
		return models[i].ModelName < models[j].ModelName
	})

	return ServiceStatusView{
		GeneratedAt: windowEnd,
		WindowHours: hours,
		WindowStart: windowStart,
		WindowEnd:   windowEnd,
		CellSeconds: cellSeconds,
		DataThrough: dataThrough,
		Requests:    overall.requests,
		SuccessRate: overallRate,
		Status:      ServiceStatusOfSuccessRate(overallRate, overall.requests),
		Cells:       cells,
		Models:      models,
	}
}

func averageLatency(counters *serviceStatusCounters) int64 {
	if counters == nil || counters.requests <= 0 {
		return 0
	}
	return counters.latencyMs / counters.requests
}
