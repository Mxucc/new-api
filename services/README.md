# 内置服务

这里存放随网关一起发布的服务。它们原本为了不改动 new-api 而独立部署，现在源码、镜像和进程管理都并入本仓库，部署只需要一个容器。

| 目录 | 服务 | 网关转发的路径 | 默认监听 | 状态库 |
| --- | --- | --- | --- | --- |
| `ai-bridge/` | 模型协议前置代理、风控与重试 | `/ai-bridge/api/*`、`/ai-bridge/auth/exchange` | `127.0.0.1:PORT+1` | `/data/ai-bridge/*` 文本状态文件 |
| `invitations/` | 邀请返利、提现审批、小时级订单同步 | `/invitations/api/*` | `127.0.0.1:PORT+2` | 自己的 PostgreSQL |
| `connect/` | OAuth2 / OIDC 授权服务器 | `/connect/api/*`、`/connect/authorize`、`/connect/token`、`/connect/userinfo`、`/connect/.well-known/*`、`/connect/auth/exchange`、`/connect/clients` | `127.0.0.1:PORT+3` | `/data/connect.db`（SQLite） |

`/connect` 与 `/ai-bridge` 本身是**前端页面**（`web/src/features/connect`、`web/src/features/ai-bridge`），由网关的嵌入式前端渲染；服务自带的 HTML 控制台只在直接访问服务端口时可用。新增服务接口时，记得把路径加进 `router/service_proxy.go` 的 `connectServicePaths` / `aiBridgeServicePaths`。

## 网关如何发布它们

`router/service_proxy.go` 按下面的环境变量把服务自己的接口路径转发过去：变量为空时这些路径返回 `404 {"success":false,"message":"<service> service is not configured"}`，页面据此显示「本部署未启用」，其余路径（`/connect`、`/ai-bridge`、`/invitations`）始终是前端路由。

| 环境变量 | 作用 |
| --- | --- |
| `INVITATIONS_SERVICE_URL` | `/invitations/api/*` 的目标地址（转发时去掉 `/invitations` 前缀） |
| `CONNECT_SERVICE_URL` | Connect 自己的接口与 OAuth2 协议路径的目标地址（路径原样转发） |
| `AI_BRIDGE_SERVICE_URL` | `/ai-bridge/api/*` 与 `/ai-bridge/auth/exchange` 的目标地址（路径原样转发） |
| `AI_BRIDGE_RELAY_ENABLED` | `true` 时 `/v1`、`/v1beta`、`/mj`、`/suno`、`/kling/v1`、`/jimeng`、`/pg` 及 `/:mode/mj` 也经 ai-bridge 转发；要求 ai-bridge 服务地址已配置且其上游不是本网关（见下） |

- 镜像里缺少对应可执行文件时不动这些变量，路径仍由网关处理。
- `<SERVICE>_AUTOSTART=false` 时不启动该服务，也不改动对应地址（服务跑在容器外时用）。
- 必填配置缺失时不启动该服务，并清掉对应的 `*_SERVICE_URL`，避免转发到一个起不来的进程。
- 服务退出会按指数退避重启，连续失败到上限后停止重启并打日志；网关退出时一起收尾。

必填配置收敛到三项：`SESSION_SECRET`（与 new-api 同一个值）、`ISSUER_URL`（connect 用，客户端访问的源站地址）、`SQL_DSN`（PostgreSQL）。`docker/entrypoint.sh` 由它们推导其余配置：`NEWAPI_DATABASE_URL` 取 `SQL_DSN`，`INVITATIONS_DATABASE_URL` 未设置时也取 `SQL_DSN`（返利表建在 new-api 库里），监听端口取 `PORT+1/2/3`，回源与回调地址取 `127.0.0.1:${PORT}`。`SESSION_SECRET` 必须与 new-api 一致，服务靠它解析同域登录态。三个服务都只读 new-api 的 session cookie，不写 new-api 的库（`invitations` 只读订单表）。

### 容器内的变量隔离

这三个服务会读同名的 `DATABASE_URL`、`LISTEN_ADDR`、`BASE_PATH`，但含义不同（connect 的库是 SQLite、invitations 是 PostgreSQL；connect 的前缀是 `/connect`、invitations 是空）。`docker/entrypoint.sh` 因此把每个服务的配置写进它自己的进程，同时接受旧版单独部署时的名字作为回退，旧 `.env` 不需要改名即可迁移：

| 变量 | connect | invitations | ai-bridge |
| --- | --- | --- | --- |
| 数据库 | `CONNECT_DATABASE_URL` →（仅 `sqlite:` 形式的）`DATABASE_URL` → `sqlite:connect.db?mode=rwc` | `INVITATIONS_DATABASE_URL` →（非 `sqlite:` 的）`DATABASE_URL` → `NEWAPI_DATABASE_URL` | — |
| 监听 | `CONNECT_LISTEN_ADDR`（默认 `PORT+3`，不继承全局 `LISTEN_ADDR`） | `INVITATIONS_LISTEN_ADDR`（默认 `PORT+2`，不继承全局 `LISTEN_ADDR`） | `AI_BRIDGE_LISTEN_ADDR`（默认 `PORT+1`） |
| 路径前缀 | `CONNECT_BASE_PATH`（默认 `/connect`） | `INVITATIONS_BASE_PATH`（默认空） | — |
| 回源 new-api | `CONNECT_API_BASE_URL` → `http://127.0.0.1:${PORT}` | `INVITATIONS_NEW_API_BASE_URL` → `NEW_API_BASE_URL` → `http://127.0.0.1:${PORT}` | `AI_BRIDGE_UPSTREAM_BASE_URL` → `http://127.0.0.1:${PORT}` |

`*_SERVICE_URL` 与启动探测地址都按上面的监听端口推导，改 `PORT` 或改监听地址都不需要再手写地址。PostgreSQL 形式的 `DATABASE_URL` 不会被交给 connect（它只用 SQLite），因此改端口或共用 `DATABASE_URL` 都不会让 connect 拿错库。

各服务自己的配置见同名 `.env.example`。整份配置可以只写一个 `/data/.env`（容器工作目录就是 `/data`）：网关、entrypoint 和三个服务都会读它，已存在于进程环境中的变量优先；ai-bridge 例外，它先读 `.env`，控制台写入的 `admin-config.env` 又优先于两者。

### 关于 /v1 经 ai-bridge 转发

网关把 `/v1` 等入口转给 ai-bridge 后，ai-bridge 会把请求转发给它的上游；如果这个上游就是本网关，同一条请求会不断回到自己（实测会把客户端端口耗尽并长时间挂住）。所以打开 `AI_BRIDGE_RELAY_ENABLED` 时必须让 `AI_BRIDGE_UPSTREAM_BASE_URL` 指向**另一个** new-api 实例：网关检测到上游是自身回环地址且端口相同时会忽略该开关，并在日志里说明原因。

想让同容器的这一套「网关 + ai-bridge」既做入口又保留 ai-bridge 的防护，需要网关额外提供一个不经 ai-bridge 转发的监听端口供 ai-bridge 回源；目前没有实现。

## 本地开发

不经过容器时，直接编译并跑服务，再让网关把路径转发过去：

```bash
make build-services                       # cargo build --release --locked，产物在 services/*/target/release

export AI_BRIDGE_SERVICE_URL=http://127.0.0.1:8080
export INVITATIONS_SERVICE_URL=http://127.0.0.1:6212
export CONNECT_SERVICE_URL=http://127.0.0.1:4000
go run main.go
```

服务自身要有独立的配置来源（同一份 `.env` 会同时被两个服务读取时要小心共用变量名，例如 `BASE_PATH`）：把 `.env` 复制到服务目录，或用 `env -C services/<name>` 之类的方式只给该服务加载。

## 前端页面

- `/connect`（全员可见）：我的 OAuth2 应用（client_id/client_secret、回调地址、创建/编辑/删除）与我的授权（scope、首次授权时间、撤销）。接口走 `/connect/api/*`；管理员在 `GET /connect/api/clients` 上会拿到全部应用并带 `owner_name`。
- `/ai-bridge`（管理员可见，`role>=10`）：运行控制台，含概览、冷却、名单、配置、日志五个面板。写操作除同源会话外还必须带 `/ai-bridge/api/me` 返回的 `csrf_token`（`X-AI-Bridge-CSRF` 头）。
- 授权同意页（`/connect/authorize`）仍是 connect 服务渲染的协议页面：它是第三方跳转过程中的一环，需要保留服务端渲染。

## 注意

- `invitations` 只支持 PostgreSQL，且必须能只读访问 new-api 的订单库；new-api 用 MySQL 时它无法同步。
- `connect/` 里的 `testutil/` 是独立的 Go 模块（用于核对 new-api 的 session gob 结构），不参与主模块构建。
- 服务数据文件放在 `/data` 下，随卷持久化；不要把状态文件写进镜像层。
