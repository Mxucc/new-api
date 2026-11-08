# Connect — new-api OAuth2 授权服务器

外嵌式 OAuth2 Authorization Server，让 new-api 用户通过标准 OAuth2 协议授权登录第三方网站。

**核心特性：零修改 new-api 代码**，通过同域反向代理 + Rust 直接解析 session cookie 实现。

## 架构

支持两种部署模式：

### 模式 A：同域路径（推荐）

通过 `BASE_PATH` 将 Connect 挂载到主站路径下（如 `dli.li/connect/`），天然共享 session cookie，无需任何 cookie 改写。

```
┌──────────────┐     ┌──────────────────────────────────┐     ┌──────────────┐
│  第三方应用   │────▶│  example.com/connect/authorize    │────▶│  第三方应用   │
│              │     │  (直接读取同域 session cookie)     │     │  (带 code)   │
└──────────────┘     └──────────────────────────────────┘     └──────────────┘
```

### 模式 B：子域名

部署在独立子域（如 `connect.example.com`），需要 nginx cookie 改写使 cookie 对子域可见。

```
┌──────────────┐     ┌─────────────────────────────────┐     ┌──────────────┐
│  第三方应用   │────▶│  connect.example.com/authorize   │────▶│  第三方应用   │
│              │     │  (解析 new-api session cookie)    │     │  (带 code)   │
└──────────────┘     └─────────────────────────────────┘     └──────────────┘
```

## 工作原理

1. **同域部署**（模式 A）：Connect 通过 nginx 反向代理挂载在主站路径下，浏览器自动携带同域 session cookie
2. **子域部署**（模式 B）：nginx 改写 cookie domain 为父域 `.example.com`，降级 SameSite 为 Lax
3. **session 解析**：直接解析 gorilla/securecookie 格式（HMAC-SHA256 验证 + gob 反序列化）
4. **OAuth2 流程**：标准 Authorization Code Grant

## 前置条件

- Rust 1.85+（edition 2024）
- 模式 A：new-api 和 connect 部署在同一域名下
- 模式 B：new-api 和 connect 部署在同一主域名的不同子域 + nginx 1.19.3+

## 快速开始

### 1. 配置环境变量

```bash
cp .env.example .env
```

编辑 `.env`：

```env
# 必须与 new-api 的 SESSION_SECRET 环境变量相同
SESSION_SECRET=your-new-api-session-secret

# SQLite 数据库路径
DATABASE_URL=sqlite:connect.db?mode=rwc

# 监听地址
LISTEN_ADDR=0.0.0.0:4000

# new-api 登录页地址（用户未登录时跳转到这里）
LOGIN_URL=https://example.com/sign-in

# 路径前缀（模式 A 必填，模式 B 留空）
# 设置后所有路由挂载在该前缀下，如 /connect/authorize
BASE_PATH=/connect
```

### 2. 编译运行

```bash
cargo build --release
./target/release/connect
```

### 3. 配置 nginx

#### 模式 A：同域路径（推荐）

在主站 nginx 配置中添加 location 块：

```nginx
# example.com 的 server 块中添加：
location /connect/ {
    proxy_pass http://127.0.0.1:4000/connect/;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
    proxy_set_header Cookie $http_cookie;
}
```

无需任何 cookie 改写，session cookie 天然可读。

#### 模式 B：子域名

在 new-api 的 nginx 配置中添加 cookie 改写（**不需要修改 new-api 代码**）：

```nginx
# api.example.com 的 location 块中添加：
location / {
    proxy_pass http://127.0.0.1:3000;
    # ... 其他 proxy_set_header ...

    # 改写 cookie domain 为父域
    proxy_cookie_domain api.example.com .example.com;
    # 降级 SameSite 为 Lax（允许第三方跳转时携带 cookie）
    proxy_cookie_flags session SameSite=Lax;
}
```

为 connect 添加 server block：

```nginx
server {
    listen 443 ssl;
    server_name connect.example.com;

    # ssl_certificate ...
    # ssl_certificate_key ...

    location / {
        proxy_pass http://127.0.0.1:4000;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header Cookie $http_cookie;
    }
}
```

> 注意：模式 B 要求 Go 后端设置 cookie 时带 `Domain` 属性，否则 `proxy_cookie_domain` 无法改写。

完整配置参考 `nginx/connect.conf`。

### 4. 注册 OAuth 客户端

```bash
curl -X POST http://localhost:4000/clients \
  -H "Content-Type: application/json" \
  -d '{
    "name": "我的应用",
    "redirect_uris": ["https://myapp.com/callback"],
    "trusted": true
  }'
```

返回：

```json
{
  "client_id": "550e8400-e29b-41d4-a716-446655440000",
  "client_secret": "cs_a1b2c3d4...",
  "name": "我的应用",
  "redirect_uris": ["https://myapp.com/callback"],
  "trusted": true
}
```

> `trusted: true` 的客户端会自动授权（跳过用户确认页面）。

### 5. 第三方应用接入

标准 OAuth2 Authorization Code 流程：

**Step 1 — 跳转授权**

```
GET https://connect.example.com/authorize
  ?response_type=code
  &client_id=YOUR_CLIENT_ID
  &redirect_uri=https://myapp.com/callback
  &scope=profile
  &state=random_state_string
```

用户如果已在 new-api 登录，会直接跳回 `redirect_uri?code=xxx&state=xxx`。
未登录则先跳转到 new-api 登录页，登录后自动回来。

**Step 2 — 换取 Token**

```bash
curl -X POST https://connect.example.com/token \
  -H "Content-Type: application/json" \
  -d '{
    "grant_type": "authorization_code",
    "code": "从回调获取的 code",
    "redirect_uri": "https://myapp.com/callback",
    "client_id": "YOUR_CLIENT_ID",
    "client_secret": "YOUR_CLIENT_SECRET"
  }'
```

返回：

```json
{
  "access_token": "cat_a1b2c3...",
  "token_type": "Bearer",
  "expires_in": 3600,
  "scope": "profile"
}
```

**Step 3 — 获取用户信息**

```bash
curl https://connect.example.com/userinfo \
  -H "Authorization: Bearer cat_a1b2c3..."
```

返回：

```json
{
  "sub": "1",
  "name": "admin",
  "preferred_username": "admin"
}
```

## API 端点

所有路径在设置 `BASE_PATH` 后会加上对应前缀（如 `BASE_PATH=/connect` 时为 `/connect/authorize`）。

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/authorize` | OAuth2 授权端点 |
| POST | `/token` | Token 端点（code 换 access_token） |
| GET | `/userinfo` | 用户信息端点（Bearer token 认证） |
| POST | `/clients` | 注册新的 OAuth 客户端 |
| GET | `/api/me` | 当前用户信息 |
| GET | `/api/clients` | 列出我的应用 |
| PUT | `/api/clients/{id}` | 修改应用 |
| DELETE | `/api/clients/{id}` | 删除应用 |

## 安全说明

- Session cookie 通过 HMAC-SHA256 验证完整性，未通过验证的 cookie 会被拒绝
- Authorization code 一次性使用，10 分钟过期
- Access token 1 小时过期
- 所有 token/code 在数据库中以 SHA-256 哈希存储
- Client secret 同样以 SHA-256 哈希存储

## 技术细节

### Session 解码

connect 直接解析 new-api 的 gorilla/securecookie v1.1.1 格式：

1. Base64 URL 解码 cookie 值
2. 分割为 `timestamp|value|mac` 三部分
3. HMAC-SHA256 验证（密钥 = SESSION_SECRET）
4. 检查时间戳未过期（30 天）
5. Base64 URL 解码 value 部分
6. Go gob 格式反序列化，提取 `id`、`username`、`role`、`status`、`group`

### 为什么不用 auth_request

- new-api 的 `authHelper` 中间件要求 `New-Api-User` header（需预知 user ID），无法作为 auth_request 目标
- 直接解码无网络开销，更快更可靠

## 开发

```bash
# 运行测试
cargo test

# 开发模式运行
RUST_LOG=debug cargo run

# 生成测试 cookie（用于验证解码器）
cd testutil && go run main.go "your-session-secret"
```
