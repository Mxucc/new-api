package service

import (
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestScorePelicanOutput(t *testing.T) {
	tests := []struct {
		name        string
		output      string
		wantScore   int
		wantVerdict string
		wantHTML    string
	}{
		{
			name: "complete animation earns every dimension",
			output: "<!DOCTYPE html><html><body><svg viewBox=\"0 0 100 100\">" +
				"<g id=\"pelican\"></g><g id=\"bicycle\"></g>" +
				"<animateTransform attributeName=\"transform\" dur=\"2s\" repeatCount=\"indefinite\"/>" +
				"</svg></body></html>" + strings.Repeat("<!-- detail -->", 200),
			wantScore:   100,
			wantVerdict: PelicanVerdictHealthy,
			wantHTML:    "<svg",
		},
		{
			name:        "missing animation and short delivery stay degraded",
			output:      "<svg><g class=\"pelican\"></g><g class=\"bicycle\"></g></svg>",
			wantScore:   65,
			wantVerdict: PelicanVerdictDegraded,
			wantHTML:    "<svg>",
		},
		{
			name:        "prose without markup scores zero",
			output:      "抱歉，我无法绘制矢量动画。",
			wantScore:   0,
			wantVerdict: PelicanVerdictPoor,
			wantHTML:    "抱歉，我无法绘制矢量动画。",
		},
		{
			name:        "empty output fails",
			output:      "   ",
			wantScore:   0,
			wantVerdict: PelicanVerdictFailed,
			wantHTML:    "",
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			score := ScorePelicanOutput(tt.output)
			assert.Equal(t, tt.wantScore, score.Score, "dimensions %v", score.Dimensions)
			assert.Equal(t, tt.wantVerdict, score.Verdict)
			assert.True(t, strings.HasPrefix(score.HTML, tt.wantHTML), "html %q should start with %q", score.HTML, tt.wantHTML)
		})
	}
}

func TestExtractPelicanHTMLUnwrapsFencedCode(t *testing.T) {
	output := "好的，以下是实现：\n\n```html\n<svg><animate attributeName=\"x\"/></svg>\n```\n\n希望有帮助。"
	assert.Equal(t, "<svg><animate attributeName=\"x\"/></svg>", ExtractPelicanHTML(output))
}

func TestChatCompletionOutput(t *testing.T) {
	t.Run("string content", func(t *testing.T) {
		got, err := ChatCompletionOutput([]byte(`{"choices":[{"message":{"role":"assistant","content":"<svg/>"}}]}`))
		require.NoError(t, err)
		assert.Equal(t, "<svg/>", got)
	})

	t.Run("part array content joins text parts", func(t *testing.T) {
		got, err := ChatCompletionOutput([]byte(`{"choices":[{"message":{"content":[{"type":"text","text":"<svg>"},{"type":"text","text":"</svg>"}]}}]}`))
		require.NoError(t, err)
		assert.Equal(t, "<svg></svg>", got)
	})

	t.Run("error payload surfaces the upstream message", func(t *testing.T) {
		_, err := ChatCompletionOutput([]byte(`{"error":{"message":"no available channel"}}`))
		require.Error(t, err)
		assert.Contains(t, err.Error(), "no available channel")
	})

	t.Run("stream chunks are joined into the full answer", func(t *testing.T) {
		body := strings.Join([]string{
			`data: {"choices":[{"delta":{"role":"assistant"}}]}`,
			`data: {"choices":[{"delta":{"content":"<svg>"}}]}`,
			`data: {"choices":[{"delta":{"content":"<animate/>"}}]}`,
			`data: {"choices":[{"delta":{"content":"</svg>"}}]}`,
			`data: [DONE]`,
		}, "\n\n")
		got, err := ChatCompletionOutput([]byte(body))
		require.NoError(t, err)
		assert.Equal(t, "<svg><animate/></svg>", got)
	})

	t.Run("stream with an error event reports it", func(t *testing.T) {
		body := `data: {"error":{"message":"Upstream service temporarily unavailable"}}`
		_, err := ChatCompletionOutput([]byte(body))
		require.Error(t, err)
		assert.Contains(t, err.Error(), "Upstream service temporarily unavailable")
	})

	t.Run("stream without any content event is a failure", func(t *testing.T) {
		_, err := ChatCompletionOutput([]byte("data: [DONE]"))
		require.Error(t, err)
	})
}

// 正文空着时要说清为什么：推理模型把预算全花在思考上，只有用量能证明这件事。
func TestPelicanAnswerNoteReportsWhyNothingWasRenderable(t *testing.T) {
	stream := "data: {\"choices\":[{\"delta\":{\"content\":\"\"},\"finish_reason\":\"length\"}]}\n" +
		"data: {\"choices\":[],\"usage\":{\"completion_tokens\":17566}}"
	note := PelicanAnswerNote([]byte(stream))
	assert.Contains(t, note, "finish_reason=length")
	assert.Contains(t, note, "completion_tokens=17566")

	// 什么都没有的响应也不能编一个原因出来。
	assert.Equal(t, "the answer carried no HTML", PelicanAnswerNote(nil))
}
