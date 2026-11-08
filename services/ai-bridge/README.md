# ai-bridge

`ai-bridge` 是一个使用 Rust + Axum 编写的 AI API 前置中转代理。它部署在客户端与后端 `new-api` 服务之间，负责 API Key 追踪、重复鉴权/权限错误熔断、上游 `cyber_policy` 网络滥用封禁、上游 `429`/`502` 后按 key 透明冷却重试，以及图片生成接口的长连接保活。

目标是：**客户端仍按 OpenAI / Claude / Gemini / new-api 的模型协议规范调用，代理在中间完成风控与稳定性增强，并尽量不改变原接口行为。**

## 核心能力

- **模型协议透明代理**：支持 `new-api` 的 `/v1/*`、Gemini 原生 `/v1beta/*`，以及 `/mj/*`、`/suno/*`、`/kling/v1/*`、`/jimeng/*`、`/pg/*`、`/:mode/mj/*` 等协议入口，保留原 HTTP 方法、路径、查询参数、请求头和请求体。
- **客户端 IP 规范化透传**：先按可信直连来源解析客户 IP，再丢弃客户端提供的 `Forwarded` / `X-Forwarded-For` / `X-Real-IP`，向上游重建单值 `X-Forwarded-For` 与 `X-Real-IP`，避免公网客户端伪造 new-api 看到的来源地址。
- **API Key / Playground 调用追踪日志**：优先从 `Authorization: Bearer sk-xxx` 提取 key；没有 Bearer key（例如 `/pg/*` 登录态请求）时使用 `New-Api-User`、Cookie hash 或直连 IP 作为冷却身份，避免所有 Playground 请求落入同一个“missing key”冷却桶。日志仍只记录脱敏值和短 hash，不记录完整 key 或 Cookie。
- **可调鉴权失败熔断**：提供关闭、保守、平衡、严格四种判定模式；同一 Bearer key 达到窗口阈值后本地冷却，另有更高阈值的按客户 IP 防随机 key 轮换保护，管理员可查看原因并立即解除单条冷却。
- **上游 `cyber_policy` 检测后封禁**：上游 JSON 或 SSE 响应报告 `code: "cyber_policy"` / `error.code: "cyber_policy"` 时，代理做后置确认；确认后写入有界 `black_log`、把 Bearer key 写入 `black.txt`，并在同一客户端 IP 累计 3 次后写入 `black_ip`。取证队列有界，正文会截断，明文 key 默认不落盘。
- **上游可重试错误透明重试**：后端 `new-api` 或更上游返回普通 `429 Too Many Requests`、`502 Bad Gateway`，或 OpenAI 容量/过载错误 body 时，代理默认内部等待并重试；可识别的余额/配额错误即使使用 `429`/`502` 也不会重试，`503 Service Unavailable` 直接返回并写入 503 诊断日志。
- **按 API Key 冷却**：某个 key 触发上游可重试限流/过载后，代理只让该 key 在后续 retry delay 窗口内等待，不影响其他 key；白名单 key 遇到 `502` / 上游请求失败时不会写入跨请求 502 冷却队列。
- **流式请求心跳保活**：流式请求遇到上游可重试错误或同 key 冷却等待时，会发送 SSE 注释心跳 `: heartbeat\n\n` 稳住连接。
- **流式 SSE 事件停顿透明重试**：上游持续发送 raw 字节或只发 `response.created` 这类开始通知、但长期没有内容/工具参数/终止事件时，代理会断开本次上游流并透明重试，避免客户端一直停在”思考中”；`/pg/*` Playground 流式响应保持 raw SSE 透传，由 `new-api` 自身 ping 处理。
- **流式首字节优化（默认开启）**：收到上游响应头后立即发送 SSE 注释心跳给客户端，最小化首字节延迟（TTFB），提升用户感知性能。适用于所有返回 `text/event-stream` 的流式接口（`/v1/chat/completions`、`/v1/responses` 等）。可通过 `AI_BRIDGE_STREAM_EARLY_HEARTBEAT=false` 关闭。
- **图片接口保活**：`/v1/images/generations` 与 `/v1/images/edits` 默认返回 JSON，并在等待期间发送合法 JSON 空白心跳；显式请求 SSE 时返回 `text/event-stream`。
- **Responses 非流式兼容回退**：可选将非流式 `POST /v1/responses` 请求转换为上游 `/v1/chat/completions`，再把上游 Chat 响应转换回 Responses 形态，兼容尚未完整支持 Responses API 的上游。
- **结构化日志**：基于 `tracing` 输出 JSON 日志，可写 stdout 或文件，便于追责、检索和告警。
- **管理员运行控制台**：在 `/ai-bridge` 提供实时控制面，可查看并发、403/重试冷却、脱敏运行日志和累计指标，完整管理 Key/IP 黑名单、Key 白名单以及可热重载配置；复用 `new-api` 同域管理员会话，查询响应不向浏览器暴露原始 API Key。

## 请求处理流程

```text
客户端
  ↓
ai-bridge
  ├─ 从可信反代头解析客户 IP；若客户 IP 已在 black_ip：直接返回网络滥用封禁提示
  ├─ 若 Bearer key 已在 black.txt：直接返回网络滥用封禁提示
  ├─ 若 key / 客户 IP 命中重复鉴权失败冷却：在读取请求体前直接返回 401/403
  ├─ 读取普通请求体，必要时执行结构化输出规范化或响应包装
  ├─ 丢弃入站转发链，并用已解析客户 IP 重建 X-Forwarded-For / X-Real-IP
  ├─ 转发到后端 new-api
  ├─ 如果上游返回 cyber_policy：先做后置违禁确认；确认后写入 black_log / black.txt；同 IP 累计 3 次确认封禁后写入 black_ip，并返回网络滥用拦截提示
  ├─ 如果上游返回 429 / 502 / 容量错误：按 key 透明等待并重试；白名单 key 的 502 不写入跨请求冷却窗口；503 直接返回并记录诊断日志
  ├─ 如果是图片接口：按客户端能力返回 JSON 空白心跳或 SSE 心跳
  └─ 返回真实上游响应 / SSE 数据
  ↓
new-api
```

## 快速开始

### 1. 准备配置

```bash
cp .env.example .env
```

修改 `.env` 中的上游地址：

```bash
AI_BRIDGE_UPSTREAM_BASE_URL=http://127.0.0.1:3000
```

这里应填写后端 `new-api` 服务地址。代理会把客户端请求的 `/v1/...`、`/v1beta/...`、`/mj/...`、`/suno/...`、`/kling/v1/...`、`/jimeng/...`、`/pg/...` 以及 Midjourney mode 变体 `/:mode/mj/...` 路径拼接到该地址后转发。管理页面固定使用 `/ai-bridge`，受保护接口位于 `/ai-bridge/api/*`，不会占用模型协议路径。

启用管理控制台时，把 ai-bridge 与 `new-api` 挂在同一个浏览器域名下，并配置相同的会话密钥：

```bash
AI_BRIDGE_ADMIN_SESSION_SECRET=<与-new-api-相同的-SESSION_SECRET>
AI_BRIDGE_ADMIN_LOGIN_URL=/#/login
```

也可以只设置 `SESSION_SECRET` 作为兼容回退。未配置会话密钥时，`/ai-bridge` 和其 API 返回 `503`，不会暴露运行数据；正常模型代理路由继续工作。管理员页面要求有效的 `session` Cookie、正常用户状态以及 `role >= 10`。如果反向代理给 ai-bridge 使用独立子域，请确认 New API 的 Cookie Domain 覆盖该子域，否则浏览器不会携带会话 Cookie。

控制台的名单和配置写操作还要求会话派生的 CSRF 令牌与同源浏览器请求。完整 Key 只在新增/替换名单条目时通过当前 HTTPS 请求提交，服务端不会在响应或控制台日志中回显。配置保存到独立的私有覆盖文件，默认是可执行文件同目录的 `admin-config.env`；生产环境建议显式设置 `AI_BRIDGE_ADMIN_CONFIG_FILE` 到服务账号可写的持久化目录。覆盖文件只允许控制台列出的运行时配置，并优先于 `.env`，清除某项覆盖后会恢复 `.env`、进程环境或默认值。

策略名单的标准默认文件名分别是 `black.txt`、`black_ip` 和 `white.txt`。`while.txt` 不会被当作白名单；控制台会在 `white.txt` 缺失且同目录存在 `while.txt` 时显示明确诊断。外部编辑名单文件后可在控制台执行“重载配置和名单”，无需等待自动重载周期。

### 2. 启动代理

```bash
cargo run
```

默认监听：

```text
0.0.0.0:8080
```

客户端改为请求：

```text
http://127.0.0.1:8080/v1/...
http://127.0.0.1:8080/v1beta/...
http://127.0.0.1:8080/mj/...
http://127.0.0.1:8080/suno/...
http://127.0.0.1:8080/kling/v1/...
http://127.0.0.1:8080/jimeng/...
http://127.0.0.1:8080/pg/...
http://127.0.0.1:8080/{mode}/mj/...
```

例如：

```bash
curl http://127.0.0.1:8080/v1/chat/completions \
  -H 'Authorization: Bearer sk-example' \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role": "user", "content": "hello"}],
    "stream": false
  }'
```

## 配置项

| 环境变量 | 默认值 | 说明 |
| --- | --- | --- |
| `AI_BRIDGE_LISTEN_ADDR` | `0.0.0.0:8080` | 代理监听地址 |
| `AI_BRIDGE_UPSTREAM_BASE_URL` | `http://127.0.0.1:3000` | 后端 `new-api` 基础地址 |
| `AI_BRIDGE_ADMIN_SESSION_SECRET` | 未设置 | `/ai-bridge` 管理控制台的 New API 会话 HMAC 密钥；未设置时回退读取 `SESSION_SECRET`，两者都缺失时控制台安全关闭 |
| `AI_BRIDGE_ADMIN_LOGIN_URL` | `/#/login` | 管理页面缺少或无法验证会话时的登录跳转地址；推荐同域相对地址 |
| `AI_BRIDGE_ADMIN_CONFIG_FILE` | `<可执行文件目录>/admin-config.env` | 管理控制台运行时配置覆盖文件；使用私有权限和原子替换，只接受 allowlist 配置，建议放在服务账号可写的持久化目录 |
| `AI_BRIDGE_CONFIG_RELOAD_INTERVAL_SECS` | `300` | 运行时重新读取 `.env` 的间隔；`0` 表示关闭热加载。监听地址、日志输出与冷却清理 TTL 仍需重启才会生效 |
| `AI_BRIDGE_AUTO_RESTART_INTERVAL_SECS` | `0` | 周期性自重启间隔；`0` 表示关闭。`.env.example` 设置为 `86400`，到期后进程会优雅退出，需要外部 supervisor 重新拉起 |
| `AI_BRIDGE_RATE_LIMITER_IDLE_TTL_SECS` | `600` | API key 冷却条目空闲多久后可被回收 |
| `AI_BRIDGE_RATE_LIMITER_CLEANUP_INTERVAL_SECS` | `60` | 空闲 API key 冷却条目的清理检查间隔 |
| `AI_BRIDGE_RATE_LIMITER_MAX_ENTRIES` | `10000` | RateLimiter 最多保留的身份条目数；饱和后新身份使用无状态 permit，避免高基数内存增长 |
| `AI_BRIDGE_AUTH_FAILURE_COOLDOWN_ENABLED` | `true` | 是否启用重复上游鉴权/权限失败冷却 |
| `AI_BRIDGE_AUTH_FAILURE_POLICY` | `balanced` | 鉴权失败判定模式：`off` 关闭、`conservative` 仅明确 401、`balanced` 明确 401/403、`strict` 全部 New API 401/403；支持控制台热更新 |
| `AI_BRIDGE_AUTH_FAILURE_WINDOW_SECS` | `60` | 合格失败的滚动统计窗口 |
| `AI_BRIDGE_AUTH_FAILURE_THRESHOLD` | `5` | 同 key 在窗口内触发冷却的失败次数 |
| `AI_BRIDGE_AUTH_FAILURE_COOLDOWN_SECS` | `1800` | 重复鉴权/权限失败后的本地冷却时间 |
| `AI_BRIDGE_AUTH_FAILURE_IP_THRESHOLD` | `20` | 同客户 IP 的轮换 key 防护阈值；`0` 表示关闭 IP 维度 |
| `AI_BRIDGE_AUTH_FAILURE_MAX_ENTRIES` | `10000` | 鉴权失败 tracker 的 key 最大条目数；IP 条目上限为其四分之一 |
| `AI_BRIDGE_AUTH_FAILURE_ERROR_BODY_MAX_BYTES` | `8192` | 冷却时允许缓存并重放的上游错误体上限；超限时使用通用兼容错误 |
| `AI_BRIDGE_HEARTBEAT_INTERVAL_SECS` | `5` | SSE 心跳间隔，用于同 key 冷却等待、图片转换和上游重试等待 |
| `AI_BRIDGE_STREAM_EVENT_STALL_RETRY_ENABLED` | `true` | 是否启用首个有效流式进展前的 SSE 停顿透明重试；设为 `false` 可运行时关闭并回到原始 SSE 透传 |
| `AI_BRIDGE_STREAM_EVENT_STALL_TIMEOUT_SECS` | `60` | 上游 `text/event-stream` 在首个有效输出/工具参数/终止事件前停顿多久后断开并透明重试；`0` 表示关闭 |
| `AI_BRIDGE_STREAM_EVENT_STALL_MAX_RETRIES` | `2` | 首个有效流式进展前 SSE 停顿的最大透明重试次数；耗尽后返回 SSE error |
| `AI_BRIDGE_UPSTREAM_429_INITIAL_DELAY_SECS` | `2` | 上游 429 或容量/过载错误后的首次重试等待时间 |
| `AI_BRIDGE_UPSTREAM_429_MAX_DELAY_SECS` | `30` | 上游可重试错误最大退避等待时间；数字型 `Retry-After` 也会被该值封顶 |
| `AI_BRIDGE_UPSTREAM_429_MAX_RETRIES` | `3` | 上游可重试错误最大重试次数；显式设为 `0` 表示无限重试直到非可重试响应 |
| `AI_BRIDGE_UPSTREAM_ERROR_COOLDOWN_ENABLED` | `true` | 是否在上游 429 或容量/过载错误后对同一个 API key 启用跨请求冷却；没有上游错误时不会启动该限制 |
| `AI_BRIDGE_UPSTREAM_502_COOLDOWN_ENABLED` | `true` | 是否在上游 502 或上游请求失败后，对同一个 API key 启用透明冷却重试 |
| `AI_BRIDGE_UPSTREAM_502_COOLDOWN_ACCUMULATE_ENABLED` | `true` | 同 key 多条并发请求同时遇到 502 时，是否逐条追加 502 冷却时间 |
| `AI_BRIDGE_UPSTREAM_502_COOLDOWN_SECS` | `300` | 上游 502 或上游请求失败后的同 key 冷却等待时间 |
| `AI_BRIDGE_UPSTREAM_502_MAX_RETRIES` | `3` | 上游 502 或上游请求失败的最大透明重试次数；显式设为 `0` 表示无限重试 |
| `AI_BRIDGE_STREAM_EARLY_HEARTBEAT` | `true` | 是否为流式请求提前发送 SSE 注释心跳以降低 TTFB；开启后会提前提交 HTTP 200，适合只看 SSE 流事件的客户端 |
| `AI_BRIDGE_STREAM_SYNTHETIC_FIRST_SPACE` | `false` | 是否为符合条件的简单流式 `POST /v1/chat/completions` 请求提前发送首个合成空格 SSE data 事件；支持 `true/false`、`1/0`、`yes/no`、`on/off` |
| `AI_BRIDGE_REQUEST_TIMEOUT_SECS` | `900` | 单次上游请求总超时；设为 `0` 表示不设置总超时 |
| `AI_BRIDGE_CONNECT_TIMEOUT_SECS` | `30` | 连接上游的超时时间 |
| `AI_BRIDGE_MAX_BODY_BYTES` | `67108864` | 普通请求体最大缓冲大小；上传请求可跳过缓冲，但仍受独立上传上限保护 |
| `AI_BRIDGE_MAX_UPLOAD_BODY_BYTES` | `536870912` | 上传型请求最大字节数；已知长度会预先拒绝，未知/分块上传会在流式计数超限时中止并返回 `413` |
| `AI_BRIDGE_CYBER_POLICY_BAN_ENABLED` | `true` | 是否启用 cyber_policy 永久封禁和已封禁 key/IP 预拦截；设为 `false` 时透传上游 cyber_policy 响应且不写入黑名单 |
| `AI_BRIDGE_CYBER_POLICY_BLACKLIST_FILE` | `<可执行文件目录>/black.txt` | 上游返回 `cyber_policy` 后追加写入的永久 key 黑名单文件；后续命中的 Bearer key 会被直接 403 封禁 |
| `AI_BRIDGE_CYBER_POLICY_BLACK_LOG_FILE` | `<可执行文件目录>/black_log` | 上游返回 `cyber_policy` 后追加写入的有界 JSONL 取证日志；正文按配置截断，Bearer key 默认只写 masked/hash |
| `AI_BRIDGE_CYBER_POLICY_IP_BLACKLIST_FILE` | `<可执行文件目录>/black_ip` | 同一个客户端 IP 累计 3 次确认后的 `cyber_policy` black 记录后追加写入的永久 IP 黑名单文件；后续命中 IP 的请求会被直接 403 封禁 |
| `AI_BRIDGE_CYBER_POLICY_WHITELIST_FILE` | `<可执行文件目录>/white.txt` | cyber_policy 白名单 Bearer key 文件；每行一个完整 key，命中后只记录 white_log，不写永久黑名单；同一白名单 key 遇到 502 / 上游请求失败时不写入跨请求 502 冷却队列 |
| `AI_BRIDGE_CYBER_POLICY_WHITE_LOG_FILE` | `<可执行文件目录>/white_log.txt` | 白名单 key 触发 `cyber_policy` 时追加写入的有界 JSONL 取证日志；包含累计次数和截断快照，Bearer key 默认只写 masked/hash |
| `AI_BRIDGE_CYBER_POLICY_CONFIRMATION_RULES_FILE` | `cyber_policy_rules.json` | 后置违禁确认规则 JSON 文件；未设置时读取启动目录/项目根目录下的 `cyber_policy_rules.json`，设置后读取指定文件 |
| `AI_BRIDGE_LOG_LEVEL` | `info` | 日志级别；也可通过 `RUST_LOG` 覆盖 |
| `AI_BRIDGE_LOG_FORMAT` | `json` | 日志格式，支持 `json` 或 `pretty` |
| `AI_BRIDGE_LOG_FILE` | 未设置 | 日志文件路径；未设置时输出到 stdout |
| `AI_BRIDGE_FORENSIC_QUEUE_CAPACITY` | `256` | 取证后台队列容量；满时丢弃新记录而不阻塞请求 |
| `AI_BRIDGE_FORENSIC_MAX_BODY_BYTES` | `262144` | 每个请求体、转发体或上游体取证快照的最大字节数 |
| `AI_BRIDGE_FORENSIC_MAX_RECORDS_PER_MINUTE` | `120` | cyber black、cyber white、502/503 各目标每分钟最多接受的记录数；`0` 表示不写取证记录 |
| `AI_BRIDGE_FORENSIC_INCLUDE_API_KEY` | `false` | 是否在专用取证 JSONL 中写完整 Bearer key；默认只写 masked/hash，生产环境不建议开启 |
| `AI_BRIDGE_502_LOG_DIR` | `log/502` | 触发 502 相关返回或透明重试时写入诊断 JSON 文件的目录 |
| `AI_BRIDGE_503_LOG_DIR` | `log/503` | 最终返回上游 503 时写入诊断 JSON 文件的目录 |
| `AI_BRIDGE_INTERNAL_HEADER_PREFIX` | `x-ai-bridge-` | 转发上游前会移除此前缀的内部请求头 |
| `AI_BRIDGE_IMAGE_RESPONSE_MODE` | `auto` | 图片接口响应模式：`auto`、`json` 或 `sse` |
| `AI_BRIDGE_RESPONSES_NONSTREAM_CHAT_FALLBACK` | `false` | 是否把非流式 `POST /v1/responses` 转成上游 `/v1/chat/completions` 并把响应转回 Responses 形态；支持 `true/false`、`1/0`、`yes/no`、`on/off` |
| `AI_BRIDGE_DEBUG_OUTPUT` | `false` | 是否输出额外运行时调试事件；支持 `true/false`、`1/0`、`yes/no`、`on/off` |

## 网络滥用判定

代理不在转发前执行本地关键词、敏感词或正则规则过滤，也不会为了本地过滤改写 prompt 文本。网络滥用先由上游响应判定：当上游返回带有 `code: "cyber_policy"` 或 `error.code: "cyber_policy"` 的 JSON/SSE 错误时，代理才对当前请求内容执行一次后置违禁确认。确认命中后，代理会封禁当前 Bearer key 并向客户端返回统一的网络滥用拦截提示；未确认时只返回本次拦截响应，不写 `black_log`、`black.txt` 或 `black_ip`。这样可以避免在正常请求路径上扫描长文本，也降低误封永久黑名单的风险。

`AI_BRIDGE_CYBER_POLICY_BAN_ENABLED=false` 会关闭 cyber_policy 永久封禁系统：代理不再使用 `black.txt` / `black_ip` 对请求做预拦截，也不会因为新的上游 `cyber_policy` 响应写入 `black_log`、`black.txt` 或 `black_ip`；上游原始响应会按普通响应透传。

后置违禁确认规则默认读取启动目录/项目根目录下的 `cyber_policy_rules.json`。也可以通过 `AI_BRIDGE_CYBER_POLICY_CONFIRMATION_RULES_FILE` 指向其他 JSON 文件来自定义规则，并支持正则：

```json
{
  "keywords": ["phishing", "恶意软件", "漏洞利用"],
  "regex": ["(?i)credential\\s+stuffing", "(?i)reverse\\s+shell"]
}
```

`keywords` 会做大小写不敏感的子串匹配，`regex` 使用 Rust `regex` 语法直接匹配原始文本。JSON 字符串里的正则反斜杠需要转义，例如正则里的 `\s` 在 JSON 里要写成 `\\s`。规则文件为空数组时表示不匹配对应类型；默认文件缺失、JSON 格式错误、规则不是字符串、或正则无法编译时，启动会失败；热加载失败时会保留旧运行时配置。

大文件上传兼容：当请求看起来是上传请求（例如 `multipart/form-data`、`application/octet-stream`，或 `/v1/files`、`/v1/uploads`、`/v1/images/edits`、`/v1/audio/transcriptions` 等上传路径），代理不会先把整个请求体读入内存，而是直接把 body 流式转发到上游。流式直通不是无限制旁路：请求仍受 `AI_BRIDGE_MAX_UPLOAD_BODY_BYTES` 约束；已知 `Content-Length` 超限会在访问上游前返回 `413 payload_too_large`，未知长度或分块上传会在计数超限时中止上游请求并返回 `413`。该直通路径不会执行结构化输出规范化、Responses Chat fallback 或图片 JSON/SSE 包装。

## 上游 cyber_policy Key 永久封禁

如果上游返回的 JSON 错误体或 SSE `data:` 事件中包含：

```json
{
  "error": {
    "code": "cyber_policy"
  }
}
```

或顶层：

```json
{
  "code": "cyber_policy"
}
```

如果当前 Bearer key 已列入 `AI_BRIDGE_CYBER_POLICY_WHITELIST_FILE`，代理不会写入 `black_log`、`black.txt` 或 `black_ip`，而是将有界请求快照追加到 `AI_BRIDGE_CYBER_POLICY_WHITE_LOG_FILE`，普通 JSON 响应透传上游原始状态码和响应体，已经建立的 SSE 响应透传上游错误事件或发送 `event: upstream_response`。

如果 `AI_BRIDGE_CYBER_POLICY_BAN_ENABLED=false`，本节所有永久封禁动作都会跳过：已存在黑名单不拦截请求，新的 `cyber_policy` 只透传上游响应，不写黑名单或取证黑日志。

非白名单 key 会先用配置化规则确认当前请求内容是否确实命中违禁；确认失败时只返回本次拦截响应，不写永久黑名单。确认成功后执行以下动作：

1. 取当前请求的 `Authorization: Bearer <key>`。
2. 将有界取证记录追加写入 `AI_BRIDGE_CYBER_POLICY_BLACK_LOG_FILE`，默认是 ai-bridge 可执行文件所在目录的 `black_log`。请求体和转发体在入队前按 `AI_BRIDGE_FORENSIC_MAX_BODY_BYTES` 截断，Bearer key 默认只记录脱敏值和 hash；只有显式设置 `AI_BRIDGE_FORENSIC_INCLUDE_API_KEY=true` 才写入 `api_key_full`。队列满或超过分钟预算时会丢弃该取证记录，但当前进程的封禁计数仍继续更新。
3. 将 `<key>` 追加写入 `AI_BRIDGE_CYBER_POLICY_BLACKLIST_FILE`，默认是 ai-bridge 可执行文件所在目录的 `black.txt`。如果请求没有 Bearer key，则跳过 key 黑名单写入，但仍写入 `black_log`。
4. 如果同一个客户端 IP 在 `black_log` 中累计达到 3 条确认后的 `cyber_policy` black 记录，则将该 IP 追加写入 `AI_BRIDGE_CYBER_POLICY_IP_BLACKLIST_FILE`，默认是 ai-bridge 可执行文件所在目录的 `black_ip`。当 ai-bridge 的直连来源是本机或内网可信反代时，客户端 IP 优先从 `X-Forwarded-For` 的第一个有效 IP 解析，其次使用 `X-Real-IP`，最后才回退到直连来源。
5. 当前请求返回网络滥用封禁提示，不再继续 429/502 透明重试。
6. 后续使用同一个 Bearer key，或来自 `black_ip` 中同一个客户端 IP 的请求，会在读取请求体前直接返回 `403`，不会转发到上游。

`black_log` 每行是一条 JSON 记录，结构示例：

```json
{
  "event": "cyber_policy_black",
  "blocked_at_utc": "2026-05-16T12:34:56Z",
  "request": {
    "client_ip": "198.51.100.7",
    "method": "POST",
    "path": "/v1/responses",
    "api_key_full": null,
    "api_key_masked": "sk-proj...9xYz",
    "api_key_hash": "0123456789abcdef",
    "headers": {
      "authorization": [{ "redacted": true, "len": 16 }]
    },
    "body": {
      "len": 32,
      "utf8_lossy": "{\"model\":\"demo\",\"input\":\"hello\"}"
    }
  }
}
```

普通 JSON 响应会返回：

```http
HTTP/1.1 403 Forbidden
Content-Type: application/json; charset=utf-8
```

```json
{
  "error": "网络滥用拦截：上游返回 cyber_policy，请检查请求内容。",
  "code": "cyber_policy"
}
```

对于已经建立的 `text/event-stream` 响应，代理无法再改变 HTTP 状态码，因此会发送 SSE `event: error`，其 `data` 使用相同 JSON 结构，随后发送 `data: [DONE]` 并结束流。

## 重复鉴权/权限 403 冷却

默认启用 `AI_BRIDGE_AUTH_FAILURE_COOLDOWN_ENABLED=true` 和 `AI_BRIDGE_AUTH_FAILURE_POLICY=balanced`。同一 Bearer key 在滚动 60 秒窗口内累计 5 次合格上游拒绝后，会进入 30 分钟本地冷却；第 5 次请求仍收到真实上游错误，从下一次请求开始在读取 body、获取 permit、排队或访问上游之前直接返回缓存的有界 `401`/`403`。冷却响应附带 `Retry-After` 和 `Cache-Control: no-store`。

管理员可在 `/ai-bridge` 控制台即时切换判定策略：

- `off`：不累计鉴权失败，已有冷却仍可在控制台手工解除；
- `conservative`：只统计 New API 明确确认客户 token 无效的 `401`，忽略所有 `403`，误伤风险最低；
- `balanced`：在保守模式基础上，只统计带 `invalid_api_key`、`invalid_token` 等明确客户凭证错误码的 `403`，这是默认模式；
- `strict`：统计所有结构合法的 New API `401`/`403`，仅建议在攻击压力较高且业务权限拒绝很少的环境使用。

余额/额度、`cyber_policy`、模型/分组/IP 权限、用户封禁和上游渠道凭据故障在 `conservative` 与 `balanced` 下均不会被当作客户 key 无效。控制台会显示每条冷却的原因、范围、剩余时间，并允许管理员单条解除；解除操作只清理内存冷却状态，不会修改黑白名单。

状态只保存完整 SHA-256 身份摘要，不保存原始 Bearer token；key 与 IP tracker 都有条目上限和过期清理。默认 `AI_BRIDGE_AUTH_FAILURE_IP_THRESHOLD=20` 会用更高阈值对客户 IP 做随机 key 轮换保护；IP 冷却使用通用错误，避免把某个 key 的上游错误体泄露给同 IP 的其他 key。多副本部署的冷却状态不共享，每个实例独立统计。

## 上游错误后按 Key 冷却行为

代理不做常态入口并发排队。默认只有某个 API key 触发上游 `429`、`502` 或容量/过载错误后，才按该 key 记录一个冷却窗口；后续同 key 请求在窗口内透明等待，其他 key 不受影响。上游 `503` 不进入透明重试或跨请求冷却。

冷却时间来自上游数字型 `Retry-After` 响应头；如果没有该响应头，则使用 `AI_BRIDGE_UPSTREAM_429_INITIAL_DELAY_SECS` 和 `AI_BRIDGE_UPSTREAM_429_MAX_DELAY_SECS` 计算指数退避。

`502 Bad Gateway` 单独使用固定冷却时间，默认 `AI_BRIDGE_UPSTREAM_502_COOLDOWN_SECS=300`。开启时，当前请求不会直接把 `502` 回传给客户端，而是按同一个 API key 进入透明冷却等待，冷却结束后继续重试下一个上游请求；其他 key 不受影响。

默认 `AI_BRIDGE_UPSTREAM_502_MAX_RETRIES=3`。如果上游持续返回 `502` 或持续请求失败，代理最多透明重试 3 次，达到上限后返回最终 `502` 并写入诊断日志，避免同一指令无限重复发送。显式设置为 `0` 表示保留旧的无限重试行为。

默认 `AI_BRIDGE_UPSTREAM_502_COOLDOWN_ACCUMULATE_ENABLED=true`。如果同一个 key 有多条并发请求同时遇到 `502`，每条请求都会把该 key 的冷却尾部再追加 `AI_BRIDGE_UPSTREAM_502_COOLDOWN_SECS`，例如 3 条并发 502 会累计约 15 分钟冷却。设为 `false` 后，502 冷却只保留当前最长窗口，不逐条累加。

如果 Bearer key 在 `AI_BRIDGE_CYBER_POLICY_WHITELIST_FILE` / `white.txt` 中，`502` 和上游请求失败仍会让当前请求按透明重试流程等待并重试，但不会为这个 key 记录跨请求 502 冷却，也不会把后续同 key 请求排在 502 冷却尾部。这个白名单绕过只作用于 `502` / 上游请求失败的跨请求 cooldown；`429` 和容量/过载错误仍按 `AI_BRIDGE_UPSTREAM_ERROR_COOLDOWN_ENABLED` 和 429 退避配置处理。

每次触发 502 透明重试、最终返回 502、或代理请求上游失败并返回 502 时，代理会在 `AI_BRIDGE_502_LOG_DIR` 写入一条诊断 JSONL 记录，用于追查真实触发条件。文件按 UTC 分钟命名，例如：

```text
log/502/20260509-1234Z.log
```

上游最终返回 `503` 时不再重试，会直接返回给客户端，并用同一诊断 JSONL 格式写入 `AI_BRIDGE_503_LOG_DIR`，默认路径是 `log/503`。

如果不希望上游错误影响同 key 后续请求，可以关闭跨请求冷却：

```bash
AI_BRIDGE_UPSTREAM_ERROR_COOLDOWN_ENABLED=false
```

关闭后，当前请求仍会按 `AI_BRIDGE_UPSTREAM_429_*` 策略透明重试；但不会给同一个 key 的后续请求设置等待窗口。

如果只想关闭 `502` 透明冷却重试：

```bash
AI_BRIDGE_UPSTREAM_502_COOLDOWN_ENABLED=false
```

关闭后，上游返回的 `502` 会按普通上游响应回传；上游请求失败导致的代理侧 `502` 也不会进入 5 分钟等待重试。

行为说明：

- 没有触发过上游可重试限流/过载的 key 不会被代理提前排队。
- 配置开启且某个 key 触发冷却后，只有同一个 key 的后续请求会等待。
- 白名单 key 触发 `502` 或上游请求失败时，当前请求仍会透明等待重试，但后续同 key 请求不会因为这次 502 进入本地冷却等待。
- 非流式请求会在代理内静默等待冷却窗口结束。
- 流式请求会先建立 SSE 响应，并在等待期间发送心跳：

```text
: heartbeat

```

- 图片请求默认保持 `application/json` 响应，并在等待期间发送 JSON 合法空白行：

```text


{"created":...,"data":[...]}
```

这样客户端感知为请求变慢，但不会因为上游限流或临时 502 直接收到错误。

如果客服反馈高并发状态下整体变慢，优先检查请求日志中的 `queued`、`queue_wait_ms` 和 `upstream_429_retry`：

- `upstream_429_retry` 增多，且同 key 后续请求 `queued=true` / `queue_wait_ms` 较高，说明这个 key 正在按上游限流、容量或 502 结果冷却。
- 如果 `retry_reason=bad_gateway` 但白名单 key 后续请求仍出现较长 `queue_wait_ms`，优先检查是否同时触发了 `429` 或容量/过载冷却；白名单只跳过 502 跨请求 cooldown。
- `queued=false` 但 `latency_ms` 较高，说明慢点主要在上游模型服务或网络。
- 如果多个 key 都慢，重点检查上游整体容量、网络和网关，而不是 ai-bridge 的全站限流；当前实现没有 429 后全站限流。

## 大量 API Key 处理能力

当接入方很多时，代理可能在短时间内看到成千上万个不同的 API key。当前实现针对这种高基数场景做了资源回收：

- 限速器使用 `DashMap` 分片并发结构，避免所有 key 竞争同一把全局锁。
- 每个 API key 独立维护运行时条目；条目只保存冷却截止时间、引用计数和最近使用时间，不保存全站限流状态。
- 每个条目维护引用计数，正在请求、正在等待同 key 冷却、正在流式返回或正在 429 重试的 key 不会被清理。
- 请求结束、连接断开或等待 future 被取消时，引用计数会通过 RAII 自动释放，避免引用泄漏。
- 空闲超过 `AI_BRIDGE_RATE_LIMITER_IDLE_TTL_SECS` 的 key 会在后续请求触发的清理周期中移除。
- 清理周期由 `AI_BRIDGE_RATE_LIMITER_CLEANUP_INTERVAL_SECS` 控制，避免每个请求都扫描全量 key。
- 条目总数由 `AI_BRIDGE_RATE_LIMITER_MAX_ENTRIES` 限制；饱和时新身份使用不保留本地冷却状态的 permit，并以指数频率记录告警，避免随机 key 喷洒造成无界内存增长。

默认配置适合大量临时 key：

```bash
AI_BRIDGE_RATE_LIMITER_IDLE_TTL_SECS=600
AI_BRIDGE_RATE_LIMITER_CLEANUP_INTERVAL_SECS=60
AI_BRIDGE_RATE_LIMITER_MAX_ENTRIES=10000
```

如果你的 key 数量非常大且很多 key 只请求一次，可以适当降低 TTL，例如：

```bash
AI_BRIDGE_RATE_LIMITER_IDLE_TTL_SECS=120
AI_BRIDGE_RATE_LIMITER_CLEANUP_INTERVAL_SECS=30
```

如果你的 key 数量不多但长连接很多，可以保持较长 TTL，减少重复创建冷却条目的开销。

## 上游可重试错误透明重试

如果后端 `new-api` 或更上游模型服务返回 HTTP `429`：

```http
HTTP/1.1 429 Too Many Requests
```

或者返回 OpenAI Responses 风格容量错误 body：

```json
{
  "error": {
    "code": "model_at_capacity",
    "message": "Selected model is at capacity. Please try a different model."
  }
}
```

代理默认不会把这些可重试错误传给客户端，而是：

1. 记录结构化日志事件 `upstream_429_retry`。
2. 读取数字型 `Retry-After` 响应头；如果没有，则使用指数退避。
3. 等待后再次请求上游。
4. 重复该过程，直到上游返回非可重试响应。
5. 再把最终真实响应传给客户端。

容量/过载 body 匹配会检查 OpenAI 标准错误结构里的 `error.message`、`error.code`，也兼容代理包装成字符串的 `error` 字段。当前匹配文本包括 `at capacity`、`server overloaded`、`overloaded`、`upstream failed`，并兼容下划线形式 `at_capacity`、`upstream_failed`。

默认重试策略：

```bash
AI_BRIDGE_UPSTREAM_429_INITIAL_DELAY_SECS=2
AI_BRIDGE_UPSTREAM_429_MAX_DELAY_SECS=30
AI_BRIDGE_UPSTREAM_429_MAX_RETRIES=3
```

其中 `AI_BRIDGE_UPSTREAM_429_MAX_RETRIES=3` 会最多透明重试 3 次；显式设置为 `0` 表示无限重试。变量名保留 `429` 是为了兼容已有部署配置；它现在同时约束 `429` 和容量/过载错误。若你希望最多重试 10 次，可以设置：

```bash
AI_BRIDGE_UPSTREAM_429_MAX_RETRIES=10
```

注意：

- 可识别的余额/额度错误会在容量关键字之前分类，即使上游用 `429`、`502` 或消息中包含 `upstream failed`，也只请求上游一次并尽快返回真实错误。
- 上游返回 `503` 时，代理会直接返回该响应并写入 `AI_BRIDGE_503_LOG_DIR`，不会把它当作容量错误继续重试。
- 非流式请求在重试期间不会发送任何内容，以保持原 JSON/HTTP 行为兼容。
- 流式请求如果首次上游响应正常，会原样透传上游 SSE。
- 流式上游响应会先等待有效流式进展；如果上游只持续发送 raw 字节，或只发 `response.created` 这类任务开始通知但没有内容/工具参数/终止事件，`AI_BRIDGE_STREAM_EVENT_STALL_TIMEOUT_SECS` 到期后会断开本次上游流并透明重试。
- 流式请求如果等待同 key 冷却或遇到可按状态码识别的 `429` / `502`，会进入 SSE 心跳保活模式，直到拿到正常上游响应。
- 图片接口默认使用 JSON 兼容包装并发送空白行心跳；显式请求 SSE 时使用 SSE 心跳。两种模式都先检查错误正文，余额/额度型 `429` 不会透明重试。

## 流式 SSE 事件停顿透明重试

默认配置：

```bash
AI_BRIDGE_STREAM_EVENT_STALL_RETRY_ENABLED=true
AI_BRIDGE_STREAM_EVENT_STALL_TIMEOUT_SECS=60
AI_BRIDGE_STREAM_EVENT_STALL_MAX_RETRIES=2
```

这个超时只用于上游已经返回 `Content-Type: text/event-stream` 的流式响应。它衡量的是“有效流式进展”，不是 TCP raw chunk 进度，也不是单纯的 `response.created`：

- `response.created` 只表示上游创建了响应任务，不代表有可展示内容。
- `response.output_text.delta`、`response.function_call_arguments.delta`、`response.completed`、`response.failed` 等才算有效进展。
- Chat Completions 的非空 `choices[].delta.content`、工具调用 delta 或 `finish_reason` 也算有效进展。

完整 SSE 帧会立即转发以降低首字节延迟，但在首个有效进展出现前，停顿计时仍保持激活。若上游只持续发送不完整 raw 字节，或只发开始通知后长时间没有有效进展，代理会断开本次上游流并重新发起同一个请求。重试仍未拿到有效进展且达到 `AI_BRIDGE_STREAM_EVENT_STALL_MAX_RETRIES` 后，才向下游发送：

```text
event: error
data: {"error":"upstream event stream stalled without a complete SSE event for 60 seconds","code":"stream_event_stall_timeout"}

data: [DONE]

```

把 `AI_BRIDGE_STREAM_EVENT_STALL_RETRY_ENABLED=false` 或 `AI_BRIDGE_STREAM_EVENT_STALL_TIMEOUT_SECS=0` 可关闭此保护。关闭后代理会回到纯 raw chunk 透传行为，适合排查上游协议问题，但客户端可能继续长时间显示“思考中”。该开关会随 `.env` 热加载生效，无需重启进程。

## 配置热加载

代理启动后默认每 300 秒重新读取当前工作目录下的 `.env`：

```bash
AI_BRIDGE_CONFIG_RELOAD_INTERVAL_SECS=300
```

这意味着你可以直接修改 `.env`，在下一个热加载周期后让运行时配置生效，例如临时关闭流式 SSE 停顿透明重试：

```bash
AI_BRIDGE_STREAM_EVENT_STALL_RETRY_ENABLED=false
```

也可以把热加载间隔设小以便快速验证：

```bash
AI_BRIDGE_CONFIG_RELOAD_INTERVAL_SECS=10
```

设为 `0` 会关闭后续热加载。热加载成功会记录 `event="config_reloaded"`；解析失败会记录 `event="config_reload_failed"`，并继续使用上一份有效配置。

热加载适用于上游地址、请求超时、心跳、鉴权失败阈值、重试/冷却开关、图片响应模式、取证正文/分钟预算和调试输出等运行时行为。监听地址、日志输出方式、RateLimiter 的 TTL/清理间隔/最大条目数、取证队列容量等启动期资源或共享状态需要重启进程后生效。

## 周期性自重启

如需让 ai-bridge 每 24 小时重启一次，可以在 `.env` 中配置：

```bash
AI_BRIDGE_AUTO_RESTART_INTERVAL_SECS=86400
```

该配置为启动期生命周期配置：进程启动后开始计时，到期后触发 graceful shutdown，等待已接入的请求自然结束，然后以正常状态退出。ai-bridge 不会在进程内重新执行自身，必须由 systemd、Docker、supervisord 等外部进程管理器重新拉起。

如果使用 systemd，重启策略需要能覆盖正常退出，例如：

```ini
Restart=always
```

如果只配置 `Restart=on-failure`，到期后的正常退出不会触发重启。把 `AI_BRIDGE_AUTO_RESTART_INTERVAL_SECS=0` 可关闭周期性自重启；运行中修改该值不会改动当前计时器，需要等下一次进程启动后生效。

## 流式 Chat 首字节优化（可选）

默认配置：

```bash
AI_BRIDGE_STREAM_SYNTHETIC_FIRST_SPACE=false
```

保持关闭时，代理对普通流式 Chat Completions 的行为不变：

- 如果首次上游响应正常，会在首个有效流式进展出现后透传上游 SSE；进展前卡住则先透明重试。
- 如果请求正在等待同 key 冷却，或首次上游返回 `429` 等可重试状态，代理会先建立 SSE 响应并发送 `: heartbeat` 注释帧保活，再等待真实上游流式响应。

某些前端或 SDK 不把 SSE 注释帧计入“首字内容”，只在收到第一个 `data:` 事件后才停止首字节计时。若需要兼容这类客户端，可以开启：

```bash
AI_BRIDGE_STREAM_SYNTHETIC_FIRST_SPACE=true
```

开启后，符合条件的简单流式 Chat 请求会立即进入代理自有 SSE 包装路径。代理会先向客户端发送一次 OpenAI Chat Completions 风格的 chunk，然后继续连接上游、等待上游响应头、处理透明重试，并在后续原样转发真实上游 SSE：

```text
data: {"id":"chatcmpl-ai-bridge-synthetic-first-space","object":"chat.completion.chunk","created":...,"model":"...","choices":[{"index":0,"delta":{"role":"assistant","content":" "},"finish_reason":null}]}

```

这个事件的 `delta.content` 是单个空格，用于让客户端尽快收到一个可解析的 SSE data 帧和首个可见内容。它只发送给客户端，不会写入上游请求体，也不会写入任何服务端会话历史；后续真实上游 SSE 会继续原样透传。

为了避免改变结构化输出或工具调用语义，此开关只作用于满足全部条件的请求：

- `POST /v1/chat/completions`
- 请求体为 JSON，且 `stream: true`
- 包含 `messages` 数组
- 未使用 `response_format.type=json_object` 或 `json_schema`
- 未使用 `tools`、`functions`、`tool_choice` 或 `function_call`
- 未请求 `n > 1`
- 若设置 `modalities`，必须只包含文本模态

不满足条件的流式请求仍只使用原来的 SSE 心跳/透明重试行为。此功能默认关闭，是因为合成空格会成为客户端可见输出的一部分；如果调用方严格比较输出文本、使用 JSON mode、结构化输出或工具调用，请保持关闭。

如果目标只是降低 HTTP TTFB，而客户端会忽略 SSE 注释帧，可以开启早发心跳：

```bash
AI_BRIDGE_STREAM_EARLY_HEARTBEAT=true
```

开启后，流式请求会先收到一次 `: heartbeat` 注释帧，然后代理再等待上游响应头、处理透明重试并转发真实上游 SSE。这个模式不会改写模型内容，也适用于 JSON mode 和工具调用等不适合合成空格的流式请求。代价是代理会提前提交 `HTTP 200` 和 `text/event-stream`；如果上游随后返回非 SSE 错误，代理只能在已经打开的 SSE 流里发送 `event: upstream_response` 或 `event: error`，不能再把原始 HTTP 状态码返回给客户端。


## Responses 非流式 Chat 回退

默认 `AI_BRIDGE_RESPONSES_NONSTREAM_CHAT_FALLBACK=false`，代理会透明转发 `POST /v1/responses` 到上游同一路径。

如果后端 `new-api` 或更上游对 **非流式** Responses API 支持不完整，可以启用兼容回退：

```bash
AI_BRIDGE_RESPONSES_NONSTREAM_CHAT_FALLBACK=true
```

启用后，仅以下请求会被改写：

- HTTP 方法必须是 `POST`
- 路径必须精确等于 `/v1/responses`
- 请求体必须是 JSON
- 请求体 `stream` 不能是 `true`

不会影响：

- `stream: true` 的 `/v1/responses` 流式请求
- 使用内置 `image_generation` tool 的非流式 `/v1/responses` 请求
- `/v1/responses/compact`
- `/v1/responses/{response_id}` 等子路径
- `/v1/chat/completions`、图片接口和其他协议根

转换行为：

- 请求路径从 `/v1/responses` 改为上游 `/v1/chat/completions`，查询参数会保留。
- `instructions` 会转换为首条 `system` 消息。
- 字符串 `input` 会转换为一条 `user` 消息；Responses `message`、`function_call`、`function_call_output` 等输入项会尽量转换为 Chat 的 `messages`、`tool_calls` 和 `tool` 消息。
- Responses 扁平 `tools: [{"type":"function","name":...}]` 会转换为 Chat 的 `tools: [{"type":"function","function":{...}}]`。
- `max_output_tokens` 会转换为 `max_tokens`。
- 上游 Chat 响应中的 `choices[].message.content` 会转换为 Responses `output[].content[].type="output_text"` 和 `output_text`。
- 上游 Chat 响应中的 `tool_calls` 会转换为 Responses `function_call` 输出项。

这是兼容性兜底，不是完整 Responses API 模拟层。复杂 Responses-only 能力（例如完整 reasoning item 生命周期、内置工具的专用输出结构、后台响应管理等）无法由 Chat Completions 完全表达；需要完整语义时应保持该开关关闭并修复上游 Responses 支持。使用内置 `image_generation` tool 的 Responses 请求会保留 `/v1/responses` 上游路径，因为非流式图片结果需要从 `output[].type="image_generation_call"` 的 `result` 字段读取，不能由 Chat Completions fallback 表达。

## 图片接口响应模式

以下接口会被特殊处理：

- `/v1/images/generations`
- `/v1/images/edits`

默认 `AI_BRIDGE_IMAGE_RESPONSE_MODE=auto`：

- 普通客户端返回 `application/json; charset=utf-8`。
- 等待同 key 冷却、等待上游响应、上游重试或读取上游 body 时，代理发送 `\n` 作为心跳。
- `\n` 是合法 JSON 前导空白，因此 `response.json()`、`JSON.parse()` 和普通 OpenAI SDK 不会因为心跳解析失败。
- 客户端显式发送 `Accept: text/event-stream` 或 `X-AI-Bridge-Image-SSE: 1` 时，代理返回 SSE。

普通 JSON 模式示例：

```text


{"created":...,"data":[...]}
```

显式 SSE 模式示例：

```text
: heartbeat

: heartbeat

data: {"created":...,"data":[...]}

data: [DONE]

```

这样可以防止图片生成耗时过长时被 Cloudflare、Nginx 或客户端读超时中断。

如果需要强制全局模式，可以设置：

```bash
AI_BRIDGE_IMAGE_RESPONSE_MODE=json  # 始终返回 JSON 空白心跳
AI_BRIDGE_IMAGE_RESPONSE_MODE=sse   # 始终返回 SSE
AI_BRIDGE_IMAGE_RESPONSE_MODE=auto  # 默认：按请求头自动选择
```

注意：JSON 空白心跳会提前固定 HTTP 响应状态码为 `200`。如果心跳发送后上游才返回非 2xx，代理会把上游状态码和 body 包装进 JSON 响应体，而不是再修改 HTTP 状态码。

## 日志

默认输出 JSON 结构化日志。正常请求会记录：

- API key 脱敏值
- API key hash
- HTTP 方法
- 路径
- 状态码
- 上游状态码
- 延迟毫秒数
- 是否因同 key 冷却等待

上游透明重试会记录：

- 重试次数
- 等待毫秒数
- `Retry-After` 值
- API key hash 和路径

如果开启：

```bash
AI_BRIDGE_DEBUG_OUTPUT=true
```

代理会额外输出 `event="debug_operation"` 日志，用于观察运行时操作，例如：

- 收到请求和请求体大小
- 开始转发上游请求
- 图片接口选择了 `json` 还是 `sse` 响应模式
- 图片长耗时等待期间发送了哪一类心跳
- 图片上游响应转换结果和 body 字节数
- 可选流式首字节优化是否发送了合成空格

调试输出仍只记录 API key 脱敏值/hash、路径、操作阶段和字节数，不记录完整 API key 或完整请求/响应体。

常规结构化日志不会写入完整 API key 和完整 prompt。为了排查真实 502/503 根因，代理会在 `AI_BRIDGE_502_LOG_DIR` / `AI_BRIDGE_503_LOG_DIR` 写入诊断 JSONL。所有凭证头都会脱敏，请求体、转发体和上游体会在入队前按 `AI_BRIDGE_FORENSIC_MAX_BODY_BYTES` 截断；`request.api_key_full` 默认是 `null`，同时记录 masked/hash。只有显式启用 `AI_BRIDGE_FORENSIC_INCLUDE_API_KEY=true` 才写真实 key。队列和分钟预算会限制攻击流量造成的内存/磁盘写入速率，但文件仍可能包含敏感请求文本，应限制访问并配置外部轮转。

## 兼容性说明

- 当前代理挂载 `/v1`、`/v1beta`、`/mj`、`/suno`、`/kling/v1`、`/jimeng`、`/pg`、`/:mode/mj` 及其子路径，面向 `new-api` 的模型协议入口；`/api/*` 管理接口不代理。
- 普通响应会尽量保留上游状态码、响应头和响应体。
- Hop-by-hop 头及 `Connection` 点名的扩展头都会被移除，例如 `connection`、`transfer-encoding`、`upgrade` 等。
- 请求头中的 `host`、`content-length`、入站转发链和内部追踪头会在转发上游前移除；客户 IP 头由 ai-bridge 重新生成。
- 流式响应会逐块转发，避免缓冲上游 SSE。

## 开发与验证

格式化代码：

```bash
cargo fmt
```

编译检查：

```bash
cargo check
```

运行测试：

```bash
cargo test
```

当前测试覆盖：

- 协议路由根覆盖 `/v1`、`/v1beta`、`/mj`、`/suno`、`/kling/v1`、`/jimeng`、`/pg`、`/:mode/mj` 且排除 `/api`
- 客户端 IP 转发头规范化、伪造链丢弃与 Connection 扩展头过滤
- Bearer API key 提取
- 重复 403 冷却的阈值、排除项、并发、高基数、IP 喷洒和 SSE 完成记录
- 已知长度及分块上传的独立字节上限
- 有界取证队列、正文截断、分钟预算、默认 key 脱敏和私有文件权限
- 上游 `cyber_policy` JSON/SSE 判定与 Bearer key 黑名单
- 上游 URL 拼接
- cyber_policy 后置确认关键词、正则规则和非法正则校验
- 429 `Retry-After` 封顶
- 429 指数退避封顶


## Linux 编译与运行

本项目可以直接在 macOS 上开发，也可以通过 Docker 编译 Linux 发布包。由于依赖使用 `rustls` 而不是 OpenSSL，Linux 构建不需要额外准备 OpenSSL 开发库。

### 推荐产物

推荐优先发布 musl 静态版：

```text
dist/ai-bridge-linux-amd64-musl
```

原因：musl 版是 `static-pie linked`，对目标服务器的 glibc 版本依赖最少，更适合分发到不同 Linux 机器。

如果明确目标服务器是较新的 Debian/Ubuntu，也可以使用 glibc 动态链接版：

```text
dist/ai-bridge-linux-amd64
```

当前 glibc 版要求最高到 `GLIBC_2.34`，适合 Debian 12、Ubuntu 22.04+ 等环境。

### Docker 编译 musl 静态版

在项目根目录执行：

```bash
docker run --rm \
  --platform linux/amd64 \
  -v "$PWD":/home/rust/src \
  -w /home/rust/src \
  -e CARGO_TARGET_DIR=/home/rust/src/target/docker-linux-amd64-musl \
  messense/rust-musl-cross:x86_64-musl \
  cargo build --release
```

构建产物路径：

```text
target/docker-linux-amd64-musl/x86_64-unknown-linux-musl/release/ai-bridge
```

复制并打包到 `dist/`：

```bash
mkdir -p dist
cp target/docker-linux-amd64-musl/x86_64-unknown-linux-musl/release/ai-bridge dist/ai-bridge-linux-amd64-musl
chmod +x dist/ai-bridge-linux-amd64-musl
tar -czf dist/ai-bridge-linux-amd64-musl.tar.gz -C dist ai-bridge-linux-amd64-musl
```

可选：在 Linux amd64 容器里 strip 减小体积：

```bash
docker run --rm \
  --platform linux/amd64 \
  -v "$PWD":/home/rust/src \
  -w /home/rust/src \
  messense/rust-musl-cross:x86_64-musl \
  strip dist/ai-bridge-linux-amd64-musl
```

校验文件类型：

```bash
file dist/ai-bridge-linux-amd64-musl
```

正常应看到类似：

```text
ELF 64-bit LSB pie executable, x86-64, static-pie linked
```

### Docker 编译 glibc 动态版

如果目标环境是 Debian/Ubuntu，并且希望使用官方 Rust Debian 镜像构建：

```bash
docker run --rm \
  --platform linux/amd64 \
  -v "$PWD":/workspace \
  -w /workspace \
  -e CARGO_TARGET_DIR=/workspace/target/docker-linux-amd64 \
  rust:1-bookworm \
  cargo build --release
```

构建产物路径：

```text
target/docker-linux-amd64/release/ai-bridge
```

复制并打包到 `dist/`：

```bash
mkdir -p dist
cp target/docker-linux-amd64/release/ai-bridge dist/ai-bridge-linux-amd64
chmod +x dist/ai-bridge-linux-amd64
tar -czf dist/ai-bridge-linux-amd64.tar.gz -C dist ai-bridge-linux-amd64
```

检查动态库依赖：

```bash
docker run --rm \
  --platform linux/amd64 \
  -v "$PWD":/workspace \
  -w /workspace \
  rust:1-bookworm \
  ldd dist/ai-bridge-linux-amd64
```

### 服务器运行

上传推荐的 musl 包到服务器后：

```bash
tar -xzf ai-bridge-linux-amd64-musl.tar.gz
chmod +x ai-bridge-linux-amd64-musl
```

准备 `.env`，示例：

```bash
AI_BRIDGE_LISTEN_ADDR=0.0.0.0:1801
AI_BRIDGE_UPSTREAM_BASE_URL=https://api.example.com
AI_BRIDGE_CONFIG_RELOAD_INTERVAL_SECS=300
AI_BRIDGE_STREAM_EVENT_STALL_RETRY_ENABLED=true
AI_BRIDGE_IMAGE_RESPONSE_MODE=auto
AI_BRIDGE_RESPONSES_NONSTREAM_CHAT_FALLBACK=false
AI_BRIDGE_DEBUG_OUTPUT=false
AI_BRIDGE_LOG_FORMAT=json
```

启动：

```bash
./ai-bridge-linux-amd64-musl
```

也可以不使用 `.env`，直接通过环境变量启动：

```bash
AI_BRIDGE_LISTEN_ADDR=0.0.0.0:1801 \
AI_BRIDGE_UPSTREAM_BASE_URL=https://api.example.com \
AI_BRIDGE_IMAGE_RESPONSE_MODE=auto \
./ai-bridge-linux-amd64-musl
```

注意：如果同时设置了进程环境变量和 `.env`，进程环境变量优先；热加载时 `.env` 不能覆盖已经存在的进程环境变量。systemd `EnvironmentFile=/opt/ai-bridge/.env` 会把该文件注入为进程环境变量，因此后续修改该文件通常仍需重启服务。如果希望使用 ai-bridge 内置热加载，请让进程在 `WorkingDirectory` 下读取普通 `.env` 文件，而不要把同一文件作为 systemd `EnvironmentFile` 注入。

验证服务：

```bash
curl -i http://127.0.0.1:1801/v1/models \
  -H 'Authorization: Bearer sk-example'
```

### 后台运行示例

临时后台运行可以用 `nohup`：

```bash
nohup ./ai-bridge-linux-amd64-musl > ai-bridge.log 2>&1 &
```

生产环境建议使用 systemd。示例 `/etc/systemd/system/ai-bridge.service`：

```ini
[Unit]
Description=ai-bridge proxy
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
WorkingDirectory=/opt/ai-bridge
ExecStart=/opt/ai-bridge/ai-bridge-linux-amd64-musl
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
```

启用并查看状态：

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now ai-bridge
sudo systemctl status ai-bridge
journalctl -u ai-bridge -f
```

## 部署建议

- 将 `AI_BRIDGE_UPSTREAM_BASE_URL` 指向内网 `new-api` 地址，避免后端直接暴露公网。
- 如果同一个域名同时承载 `new-api` Web/UI 和模型协议接口，可参考 `deploy/nginx-ai-bridge.conf`：模型协议入口转发到 `ai-bridge`，其余流量（包括 `/api/*` 后台接口）仍转发到 `new-api`。
- `ai-bridge` 会丢弃入站 `Forwarded` / `X-Forwarded-For` / `X-Real-IP`，再把已解析的客户 IP 作为单一值写入上游 `X-Forwarded-For` 和 `X-Real-IP`。如果前面还有 Nginx/CDN，请确保它连接 ai-bridge 的来源是本机或私网可信反代，并正确传入客户 IP；公网直连来源提供的转发头会被忽略。
- `black_log` / `black_ip` 的本地封禁 IP 会在直连来源是本机或内网可信反代时，优先使用 `X-Forwarded-For` / `X-Real-IP` 里的客户 IP，避免把 `127.0.0.1` 这类反代地址写入 IP 黑名单。
- 生产环境推荐 `AI_BRIDGE_LOG_FORMAT=json`，便于日志系统采集。
- 取证文件和黑名单在 Unix 上创建为 `0600`，新建取证目录为 `0700`。仍应为 `black_log`、`white_log`、`log/502`、`log/503` 配置外部日志轮转/保留策略；内置分钟预算限制攻击速率，但不替代长期磁盘配额。
- 若 `queue_wait_ms` 长期偏高且伴随 `upstream_429_retry`，说明对应 key 正在频繁按上游限流、容量或 502 结果冷却；应增加上游 key 池容量、降低单 key 压力或检查上游稳定性。可信白名单 key 可用于避免临时 `502` 把后续同 key 请求排入本地 502 cooldown，但它不会绕过 `429` / 容量错误冷却。
- 若网关超时时间较短，可保持 `AI_BRIDGE_HEARTBEAT_INTERVAL_SECS=5` 或更低。
- 若不希望无限等待上游 429，可设置有限的 `AI_BRIDGE_UPSTREAM_429_MAX_RETRIES`。
