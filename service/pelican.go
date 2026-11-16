package service

import (
	"fmt"
	"strings"

	"github.com/QuantumNous/new-api/common"
)

// 鹈鹕测智（Pelican IQ test）用同一个提示词向模型要一段「SVG 鹈鹕骑自行车 2D 动画」，
// 再按交付结果打分。题目本身不提供任何与打分相关的线索，所以分数衡量的是模型把
// 一条多约束指令落成可渲染图形代码的能力，而不是某个基准测试的记忆答案。
const PelicanPrompt = "创建一个 HTML，内容是 SVG 绘制一个鹈鹕骑自行车的 2D 动画，你不需要任何测试"

// 交付约定单独追加，避免改动上面这条公共题面。
const PelicanPromptContract = "只输出 HTML，不要使用 Markdown 代码块，不要解释。"

// 鹈鹕测智的判定档位。传输层失败用 failed，能拿到输出才进入三档评分。
const (
	PelicanVerdictHealthy  = "healthy"  // 满血：图形、主体、动画都到位
	PelicanVerdictDegraded = "degraded" // 降智：能画，但缺关键要素
	PelicanVerdictPoor     = "poor"     // 不达标：多数要素缺失
	PelicanVerdictFailed   = "failed"   // 调用或输出解析失败
	PelicanVerdictPending  = "pending"  // 还没有测过
)

// 各要素权重之和为 100，分数即模型智商（0-100）。
const (
	pelicanWeightDrawing   = 25 // 有没有 <svg> 图形
	pelicanWeightSubject   = 20 // 有没有鹈鹕
	pelicanWeightVehicle   = 20 // 有没有自行车
	pelicanWeightAnimation = 20 // 有没有动画
	pelicanWeightDelivery  = 15 // 交付体量是否像一段完整实现
)

var pelicanSubjectiveKeywords = []string{"pelican", "鹈鹕"}

var pelicanVehicleKeywords = []string{"bicycle", "bike", "自行车", "脚踏车"}

// 动画既可能来自 SMIL（<animate>），也可能来自 CSS 关键帧或 Web API。
var pelicanAnimationMarkers = []string{
	"<animate",
	"animatemotion",
	"animatetransform",
	"@keyframes",
	"requestanimationframe",
	"animation:",
	"animation-name",
	"transition:",
}

// PelicanScore 是鹈鹕测智的打分结果。
type PelicanScore struct {
	Score      int      `json:"score"`
	Verdict    string   `json:"verdict"`
	Dimensions []string `json:"dimensions"`
	HTML       string   `json:"-"`
}

func pelicanContainsAny(text string, markers []string) bool {
	for _, marker := range markers {
		if strings.Contains(text, marker) {
			return true
		}
	}
	return false
}

// ExtractPelicanHTML 从模型输出里取出 HTML/SVG 源码。
// 模型常把结果包在 ```html 围栏里，或在前后附带说明文字，这里都按「第一段可渲染代码」处理。
func ExtractPelicanHTML(output string) string {
	trimmed := strings.TrimSpace(output)
	if trimmed == "" {
		return ""
	}
	if start := strings.Index(trimmed, "```"); start >= 0 {
		rest := trimmed[start+3:]
		if newline := strings.IndexByte(rest, '\n'); newline >= 0 {
			rest = rest[newline+1:]
		}
		if end := strings.Index(rest, "```"); end >= 0 {
			rest = rest[:end]
		}
		if candidate := strings.TrimSpace(rest); candidate != "" {
			trimmed = candidate
		}
	}
	lower := strings.ToLower(trimmed)
	for _, opening := range []string{"<svg", "<!doctype", "<html", "<div"} {
		if index := strings.Index(lower, opening); index > 0 {
			trimmed = strings.TrimSpace(trimmed[index:])
			break
		}
	}
	return trimmed
}

// ScorePelicanOutput 按要素给模型输出打分，返回 0-100 的智商分与档位。
func ScorePelicanOutput(output string) PelicanScore {
	html := ExtractPelicanHTML(output)
	score := PelicanScore{HTML: html, Dimensions: []string{}}
	if html == "" {
		score.Verdict = PelicanVerdictFailed
		return score
	}
	lower := strings.ToLower(html)

	if strings.Contains(lower, "<svg") && strings.Contains(lower, "</svg>") {
		score.Score += pelicanWeightDrawing
		score.Dimensions = append(score.Dimensions, "drawing")
	}
	if pelicanContainsAny(lower, pelicanSubjectiveKeywords) {
		score.Score += pelicanWeightSubject
		score.Dimensions = append(score.Dimensions, "subject")
	}
	if pelicanContainsAny(lower, pelicanVehicleKeywords) {
		score.Score += pelicanWeightVehicle
		score.Dimensions = append(score.Dimensions, "vehicle")
	}
	if pelicanContainsAny(lower, pelicanAnimationMarkers) {
		score.Score += pelicanWeightAnimation
		score.Dimensions = append(score.Dimensions, "animation")
	}
	switch length := len(html); {
	case length >= 2000:
		score.Score += pelicanWeightDelivery
		score.Dimensions = append(score.Dimensions, "delivery")
	case length >= 600:
		score.Score += pelicanWeightDelivery / 2
		score.Dimensions = append(score.Dimensions, "delivery_partial")
	}

	switch {
	case score.Score >= 75:
		score.Verdict = PelicanVerdictHealthy
	case score.Score >= 45:
		score.Verdict = PelicanVerdictDegraded
	default:
		score.Verdict = PelicanVerdictPoor
	}
	return score
}

// ChatCompletionOutput 取出站点对外返回（OpenAI 格式）的助手文本。
// 非流式读 choices[].message.content，流式把 choices[].delta.content 逐段拼起来；
// 两种形态都兼容 content 为字符串或分块数组，并把推理内容排除在外。
func ChatCompletionOutput(body []byte) (string, error) {
	trimmed := strings.TrimSpace(string(body))
	if trimmed == "" {
		return "", fmt.Errorf("pelican response is empty")
	}
	if strings.HasPrefix(trimmed, "data:") || strings.Contains(trimmed, "\ndata:") {
		return chatCompletionStreamOutput(trimmed)
	}

	var payload struct {
		Choices []struct {
			Message struct {
				Content any `json:"content"`
			} `json:"message"`
		} `json:"choices"`
		Error *struct {
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := common.Unmarshal(body, &payload); err != nil {
		return "", fmt.Errorf("parse pelican response: %w", err)
	}
	if payload.Error != nil && payload.Error.Message != "" {
		return "", fmt.Errorf("%s", payload.Error.Message)
	}
	if len(payload.Choices) == 0 {
		return "", fmt.Errorf("pelican response has no choices")
	}
	return pelicanContentText(payload.Choices[0].Message.Content)
}

// chatCompletionStreamOutput 把 SSE 里所有 delta.content 片段拼成完整输出。
func chatCompletionStreamOutput(body string) (string, error) {
	var builder strings.Builder
	events := 0
	for line := range strings.SplitSeq(body, "\n") {
		payload := strings.TrimSpace(strings.TrimPrefix(strings.TrimSpace(line), "data:"))
		if payload == "" || payload == "[DONE]" || !strings.HasPrefix(payload, "{") {
			continue
		}
		var chunk struct {
			Choices []struct {
				Delta struct {
					Content any `json:"content"`
				} `json:"delta"`
			} `json:"choices"`
			Error *struct {
				Message string `json:"message"`
			} `json:"error"`
		}
		if err := common.UnmarshalJsonStr(payload, &chunk); err != nil {
			continue
		}
		if chunk.Error != nil && chunk.Error.Message != "" {
			return "", fmt.Errorf("%s", chunk.Error.Message)
		}
		if len(chunk.Choices) == 0 {
			continue
		}
		text, err := pelicanContentText(chunk.Choices[0].Delta.Content)
		if err != nil {
			continue
		}
		events++
		builder.WriteString(text)
	}
	if events == 0 {
		return "", fmt.Errorf("pelican stream has no content")
	}
	return builder.String(), nil
}

// pelicanContentText 把 content 字段还原成文本：字符串原样返回，分块数组只取 text。
func pelicanContentText(content any) (string, error) {
	switch value := content.(type) {
	case string:
		return value, nil
	case []any:
		var builder strings.Builder
		for _, part := range value {
			chunk, ok := part.(map[string]any)
			if !ok {
				continue
			}
			if text, ok := chunk["text"].(string); ok {
				builder.WriteString(text)
			}
		}
		return builder.String(), nil
	case nil:
		return "", nil
	default:
		return "", fmt.Errorf("pelican response content is %T", content)
	}
}
