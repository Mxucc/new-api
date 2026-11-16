package main

import (
	"bytes"
	"html"
	"os"
	"strings"
)

// InjectEmbeddedFrameOrigins 把允许放开 same-origin 的嵌入来源交给前端。
// 站点常把静态资源放在子域上（例如 static.example.com），这类来源必须放开
// same-origin，否则 Vite 的 module 脚本在 opaque origin 里会被 CORS 拦掉，
// 嵌入页就只剩一片空白。来源由部署环境声明，仓库里不写死任何域名。
func InjectEmbeddedFrameOrigins() {
	raw := strings.TrimSpace(os.Getenv("EMBEDDED_FRAME_ORIGINS"))
	if raw == "" {
		return
	}
	tag := []byte(`<meta name="embedded-frame-origins" content="` + html.EscapeString(raw) + `">`)
	indexPage = injectBeforeHeadClose(indexPage, tag)
	classicIndexPage = injectBeforeHeadClose(classicIndexPage, tag)
}

func injectBeforeHeadClose(page []byte, tag []byte) []byte {
	if len(page) == 0 {
		return page
	}
	closing := []byte("</head>")
	if !bytes.Contains(page, closing) {
		return page
	}
	return bytes.Replace(page, closing, append(append([]byte{}, tag...), closing...), 1)
}
