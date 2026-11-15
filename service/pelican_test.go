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

	t.Run("empty choices reports a failure", func(t *testing.T) {
		_, err := ChatCompletionOutput([]byte(`{"choices":[]}`))
		require.Error(t, err)
	})
}
