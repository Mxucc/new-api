use axum::extract::State;
use axum::response::{Html, IntoResponse, Redirect, Response};

use crate::AppState;
use crate::error::AppError;
use crate::session::SessionUser;

pub async fn home(State(app): State<AppState>) -> Html<String> {
    Html(build_home(&app.config.base_path))
}

pub async fn new_client(
    State(app): State<AppState>,
    user: Result<SessionUser, AppError>,
) -> Response {
    match user {
        Ok(_) => Html(build_new_client(
            &app.config.base_path,
            &app.config.login_url,
        ))
        .into_response(),
        Err(_) => login_redirect(&app.config.login_url, &app.config.base_path, "/new"),
    }
}

pub async fn dashboard(
    State(app): State<AppState>,
    user: Result<SessionUser, AppError>,
) -> Response {
    match user {
        Ok(_) => Html(build_dashboard(
            &app.config.base_path,
            &app.config.login_url,
        ))
        .into_response(),
        Err(_) => login_redirect(&app.config.login_url, &app.config.base_path, "/dashboard"),
    }
}

pub async fn authorizations(
    State(app): State<AppState>,
    user: Result<SessionUser, AppError>,
) -> Response {
    match user {
        Ok(_) => Html(build_authorizations(
            &app.config.base_path,
            &app.config.login_url,
        ))
        .into_response(),
        Err(_) => login_redirect(
            &app.config.login_url,
            &app.config.base_path,
            "/authorizations",
        ),
    }
}

fn login_redirect(login_url: &str, base_path: &str, return_path: &str) -> Response {
    let return_url = format!("{}{}", base_path, return_path);
    let url = format!(
        "{}?redirect={}",
        login_url,
        urlencoding::encode(&return_url)
    );
    Redirect::temporary(&url).into_response()
}

fn base_script(base_path: &str) -> String {
    format!("<script>const BASE='{}';</script>", base_path)
}

fn nav_html(base_path: &str, active: &str) -> String {
    let items = [
        ("", "Connect", "home"),
        ("/new", "注册应用", "new"),
        ("/dashboard", "管理应用", "dashboard"),
        ("/authorizations", "授权记录", "authorizations"),
    ];
    let links: Vec<String> = items
        .iter()
        .map(|(path, label, id)| {
            let href = format!("{}{}", base_path, if path.is_empty() { "/" } else { *path });
            if *id == active {
                format!(
                    "<a href=\"{}\" class=\"nav-link active\">{}</a>",
                    href, label
                )
            } else {
                format!("<a href=\"{}\" class=\"nav-link\">{}</a>", href, label)
            }
        })
        .collect();
    format!(r#"<nav class="site-nav">{}</nav>"#, links.join(""))
}

const NAV_STYLE: &str = r#"
.site-nav { max-width: 800px; margin: 0 auto 24px; display: flex; align-items: center; gap: 4px; padding: 8px 12px; background: #fff; border-radius: 10px; box-shadow: 0 1px 4px rgba(0,0,0,0.06); }
.nav-link { padding: 6px 14px; border-radius: 6px; font-size: 14px; color: #555; text-decoration: none; transition: background 0.15s; }
.nav-link:hover { background: #f3f4f6; }
.nav-link.active { background: #4f46e5; color: #fff; }
"#;

fn build_home(base_path: &str) -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Connect 授权中心</title>
{base_script}
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; background: #f5f7fa; min-height: 100vh; padding: 20px; }}
{nav_style}
.container {{ width: 100%; max-width: 600px; text-align: center; margin: 0 auto; }}
.logo {{ font-size: 36px; font-weight: 700; color: #4f46e5; margin-bottom: 8px; }}
.subtitle {{ color: #666; font-size: 16px; margin-bottom: 48px; }}
.cards {{ display: grid; grid-template-columns: 1fr 1fr; gap: 16px; margin-bottom: 48px; }}
.card {{ background: #fff; border-radius: 12px; padding: 32px 20px; box-shadow: 0 2px 12px rgba(0,0,0,0.06); text-decoration: none; transition: transform 0.15s, box-shadow 0.15s; }}
.card:hover {{ transform: translateY(-2px); box-shadow: 0 6px 20px rgba(0,0,0,0.1); }}
.card-icon {{ font-size: 32px; margin-bottom: 12px; }}
.card-title {{ font-size: 16px; font-weight: 600; color: #1a1a1a; margin-bottom: 6px; }}
.card-desc {{ font-size: 13px; color: #888; }}
.flow {{ background: #fff; border-radius: 12px; padding: 28px; box-shadow: 0 2px 12px rgba(0,0,0,0.06); text-align: left; }}
.flow h3 {{ font-size: 15px; color: #333; margin-bottom: 16px; }}
.flow ol {{ padding-left: 20px; font-size: 14px; color: #555; line-height: 2; }}
.flow code {{ background: #f3f4f6; padding: 2px 6px; border-radius: 4px; font-size: 13px; }}
</style>
</head>
<body>
{nav}
<div class="container">
  <div class="logo">Connect</div>
  <p class="subtitle">基于 OAuth2 的统一身份认证服务</p>
  <div class="cards">
    <a class="card" href="{base}/new">
      <div class="card-icon">+</div>
      <div class="card-title">注册应用</div>
      <div class="card-desc">接入 OAuth2，获取 Client 凭证</div>
    </a>
    <a class="card" href="{base}/dashboard">
      <div class="card-icon">⚙</div>
      <div class="card-title">管理应用</div>
      <div class="card-desc">查看、编辑、删除已注册的应用</div>
    </a>
  </div>
  <div class="flow">
    <h3>OAuth2 接入流程</h3>
    <ol>
      <li>注册应用，获取 <code>client_id</code> 和 <code>client_secret</code></li>
      <li>引导用户跳转授权<br><code>{base}/authorize?response_type=code&amp;client_id=ID&amp;redirect_uri=URL&amp;scope=profile&amp;state=STATE</code></li>
      <li>用户授权后，回调地址收到 <code>?code=xxx&amp;state=STATE</code></li>
      <li>用 code 换取 token<br><code>POST {base}/token</code><pre style="background:#f9fafb;padding:8px;border-radius:4px;font-size:12px;margin-top:4px">{{
  "grant_type": "authorization_code",
  "code": "从回调获取的 code",
  "redirect_uri": "你的回调地址",
  "client_id": "YOUR_CLIENT_ID",
  "client_secret": "YOUR_CLIENT_SECRET"
}}</pre>响应：<pre style="background:#f9fafb;padding:8px;border-radius:4px;font-size:12px;margin-top:4px">{{
  "access_token": "cat_xxx",
  "token_type": "Bearer",
  "expires_in": 3600,
  "scope": "profile"
}}</pre></li>
      <li>用 access_token 获取用户信息<br><code>GET {base}/userinfo</code>（Header: <code>Authorization: Bearer cat_xxx</code>）<pre style="background:#f9fafb;padding:8px;border-radius:4px;font-size:12px;margin-top:4px">{{
  "sub": "用户ID",
  "name": "显示名称",
  "preferred_username": "用户名",
  "email": "邮箱",
  "role": 100,
  "status": 1,
  "group": "default"
}}</pre></li>
    </ol>
  </div>
</div>
</body>
</html>"##,
        base_script = base_script(base_path),
        nav_style = NAV_STYLE,
        nav = nav_html(base_path, "home"),
        base = base_path
    )
}

fn build_new_client(base_path: &str, login_url: &str) -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Connect - OAuth 应用接入</title>
{base_script}
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; background: #f5f7fa; min-height: 100vh; padding: 20px; }}
{nav_style}
.container {{ width: 100%; max-width: 520px; margin: 0 auto; }}
.card {{ background: #fff; border-radius: 12px; padding: 40px; box-shadow: 0 2px 12px rgba(0,0,0,0.08); }}
h1 {{ font-size: 24px; font-weight: 600; margin-bottom: 8px; color: #1a1a1a; }}
.subtitle {{ color: #666; font-size: 14px; margin-bottom: 32px; }}
.form-group {{ margin-bottom: 20px; }}
label {{ display: block; font-size: 14px; font-weight: 500; color: #333; margin-bottom: 6px; }}
label .required {{ color: #e53e3e; }}
input, textarea {{ width: 100%; padding: 10px 14px; border: 1px solid #d9d9d9; border-radius: 8px; font-size: 14px; transition: border-color 0.2s; outline: none; }}
input:focus, textarea:focus {{ border-color: #4f46e5; box-shadow: 0 0 0 3px rgba(79,70,229,0.1); }}
textarea {{ resize: vertical; min-height: 80px; }}
.hint {{ font-size: 12px; color: #999; margin-top: 4px; }}
.btn {{ width: 100%; padding: 12px; background: #4f46e5; color: #fff; border: none; border-radius: 8px; font-size: 15px; font-weight: 500; cursor: pointer; transition: background 0.2s; }}
.btn:hover {{ background: #4338ca; }}
.btn:disabled {{ background: #a5a5a5; cursor: not-allowed; }}
.error {{ background: #fef2f2; border: 1px solid #fecaca; color: #dc2626; padding: 10px 14px; border-radius: 8px; font-size: 13px; margin-bottom: 16px; display: none; }}
.result {{ display: none; }}
.result h2 {{ font-size: 18px; margin-bottom: 16px; color: #059669; }}
.credential {{ background: #f9fafb; border: 1px solid #e5e7eb; border-radius: 8px; padding: 14px; margin-bottom: 12px; }}
.credential-label {{ font-size: 12px; color: #666; margin-bottom: 4px; }}
.credential-value {{ font-family: "SF Mono", Monaco, monospace; font-size: 13px; word-break: break-all; color: #1a1a1a; display: flex; align-items: center; gap: 8px; }}
.credential-value code {{ flex: 1; }}
.copy-btn {{ background: none; border: 1px solid #d9d9d9; border-radius: 4px; padding: 4px 8px; font-size: 12px; cursor: pointer; color: #666; white-space: nowrap; }}
.copy-btn:hover {{ background: #f3f4f6; }}
.info-row {{ display: flex; gap: 8px; font-size: 13px; color: #666; margin-bottom: 6px; }}
.info-row span:first-child {{ font-weight: 500; min-width: 70px; }}
.warn {{ background: #fffbeb; border: 1px solid #fde68a; color: #92400e; padding: 12px 14px; border-radius: 8px; font-size: 13px; margin-top: 16px; }}
.login-prompt {{ text-align: center; padding: 60px 20px; }}
.login-prompt p {{ color: #666; margin-bottom: 16px; }}
.login-prompt a {{ color: #4f46e5; text-decoration: none; font-weight: 500; }}
</style>
</head>
<body>
{nav}
<div class="container">
<div class="card" id="main-card">
  <h1>OAuth 应用接入</h1>
  <p class="subtitle">注册你的应用，获取 OAuth2 接入凭证</p>

  <div class="error" id="error-msg"></div>

  <form id="app-form">
    <div class="form-group">
      <label>应用名 <span class="required">*</span></label>
      <input type="text" name="name" required placeholder="你的应用名称（全局唯一）" maxlength="64">
    </div>
    <div class="form-group">
      <label>应用主页 <span class="required">*</span></label>
      <input type="url" name="homepage_url" required placeholder="https://example.com">
      <div class="hint">用于展示，如应用官网地址</div>
    </div>
    <div class="form-group">
      <label>应用图标</label>
      <input type="url" name="logo_url" placeholder="https://example.com/logo.png">
      <div class="hint">应用图标 URL（选填，未填写时显示首字母头像）</div>
    </div>
    <div class="form-group">
      <label>应用描述</label>
      <textarea name="description" placeholder="简要描述你的应用（选填）" maxlength="256"></textarea>
    </div>
    <div class="form-group">
      <label>回调地址 <span class="required">*</span></label>
      <input type="url" name="redirect_uri" required placeholder="https://example.com/callback">
      <div class="hint">OAuth 授权成功后跳转的地址</div>
    </div>
    <button type="submit" class="btn" id="submit-btn">提交申请</button>
  </form>

  <div class="result" id="result">
    <h2>接入成功</h2>
    <div class="credential">
      <div class="credential-label">Client ID</div>
      <div class="credential-value"><code id="res-client-id"></code><button class="copy-btn" onclick="copyText('res-client-id')">复制</button></div>
    </div>
    <div class="credential">
      <div class="credential-label">Client Secret</div>
      <div class="credential-value"><code id="res-client-secret"></code><button class="copy-btn" onclick="copyText('res-client-secret')">复制</button></div>
    </div>
    <div class="info-row"><span>应用名</span><span id="res-name"></span></div>
    <div class="info-row"><span>主页</span><span id="res-homepage"></span></div>
    <div class="info-row"><span>回调地址</span><span id="res-redirect"></span></div>
    <div class="warn">请妥善保存 Client Secret，此信息仅显示一次，关闭后无法再次查看。</div>
  </div>
</div>

<div class="card login-prompt" id="login-card" style="display:none">
  <p>你需要先登录才能注册 OAuth 应用</p>
  <a id="login-link" href="#">前往登录 →</a>
</div>
</div>

<script>
const form = document.getElementById('app-form');
const errorEl = document.getElementById('error-msg');
const resultEl = document.getElementById('result');
const submitBtn = document.getElementById('submit-btn');
const mainCard = document.getElementById('main-card');
const loginCard = document.getElementById('login-card');

function showError(msg) {{
  errorEl.textContent = msg;
  errorEl.style.display = 'block';
}}
function hideError() {{ errorEl.style.display = 'none'; }}

function showLogin() {{
  mainCard.style.display = 'none';
  loginCard.style.display = 'block';
  const returnUrl = encodeURIComponent(location.pathname + location.search);
  document.getElementById('login-link').href = '{login_url}?redirect=' + returnUrl;
}}

function copyText(id) {{
  const text = document.getElementById(id).textContent;
  navigator.clipboard.writeText(text).then(() => {{
    const btn = document.getElementById(id).parentElement.querySelector('.copy-btn');
    btn.textContent = '已复制';
    setTimeout(() => btn.textContent = '复制', 1500);
  }});
}}

form.addEventListener('submit', async (e) => {{
  e.preventDefault();
  hideError();
  submitBtn.disabled = true;
  submitBtn.textContent = '提交中...';

  const data = {{
    name: form.name.value.trim(),
    homepage_url: form.homepage_url.value.trim(),
    logo_url: form.logo_url.value.trim() || null,
    description: form.description.value.trim() || null,
    redirect_uri: form.redirect_uri.value.trim(),
  }};

  try {{
    const res = await fetch(BASE + '/clients', {{
      method: 'POST',
      headers: {{ 'Content-Type': 'application/json' }},
      credentials: 'include',
      body: JSON.stringify(data),
    }});

    if (res.status === 401) {{
      showLogin();
      return;
    }}

    const body = await res.json();
    if (!res.ok) {{
      showError(body.error || '提交失败，请重试');
      return;
    }}

    form.style.display = 'none';
    document.querySelector('.subtitle').style.display = 'none';
    document.getElementById('res-client-id').textContent = body.client_id;
    document.getElementById('res-client-secret').textContent = body.client_secret;
    document.getElementById('res-name').textContent = body.name;
    document.getElementById('res-homepage').textContent = body.homepage_url;
    document.getElementById('res-redirect').textContent = body.redirect_uri;
    resultEl.style.display = 'block';
  }} catch (err) {{
    showError('网络错误，请检查连接后重试');
  }} finally {{
    submitBtn.disabled = false;
    submitBtn.textContent = '提交申请';
  }}
}});
</script>
</body>
</html>"##,
        base_script = base_script(base_path),
        nav_style = NAV_STYLE,
        nav = nav_html(base_path, "new"),
        login_url = login_url
    )
}

fn build_dashboard(base_path: &str, login_url: &str) -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>应用管理 - Connect</title>
{base_script}
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; background: #f5f7fa; min-height: 100vh; padding: 20px; }}
{nav_style}
.container {{ max-width: 800px; margin: 0 auto; }}
.empty {{ text-align: center; padding: 60px 20px; color: #999; }}
.empty a {{ color: #4f46e5; text-decoration: none; }}
.app-card {{ background: #fff; border-radius: 10px; padding: 20px; margin-bottom: 12px; box-shadow: 0 1px 4px rgba(0,0,0,0.06); display: flex; gap: 16px; align-items: flex-start; }}
.app-logo {{ width: 48px; height: 48px; border-radius: 8px; flex-shrink: 0; }}
.app-logo-placeholder {{ width: 48px; height: 48px; border-radius: 8px; background: #4f46e5; color: #fff; display: flex; align-items: center; justify-content: center; font-size: 20px; font-weight: 600; flex-shrink: 0; }}
.app-content {{ flex: 1; min-width: 0; }}
.app-card-header {{ display: flex; align-items: center; justify-content: space-between; margin-bottom: 10px; }}
.app-name {{ font-size: 16px; font-weight: 600; color: #1a1a1a; }}
.app-meta {{ font-size: 13px; color: #999; }}
.app-info {{ font-size: 13px; color: #555; margin-bottom: 4px; }}
.app-info span {{ color: #999; display: inline-block; min-width: 70px; }}
.app-actions {{ display: flex; gap: 8px; margin-top: 12px; }}
.btn-sm {{ padding: 6px 14px; border-radius: 6px; font-size: 13px; border: none; cursor: pointer; }}
.btn-edit {{ background: #f3f4f6; color: #333; }}
.btn-edit:hover {{ background: #e5e7eb; }}
.btn-del {{ background: #fef2f2; color: #dc2626; }}
.btn-del:hover {{ background: #fee2e2; }}
.modal-overlay {{ display: none; position: fixed; inset: 0; background: rgba(0,0,0,0.4); z-index: 100; align-items: center; justify-content: center; }}
.modal-overlay.active {{ display: flex; }}
.modal {{ background: #fff; border-radius: 12px; padding: 28px; width: 90%; max-width: 460px; }}
.modal h3 {{ margin-bottom: 20px; font-size: 18px; }}
.modal .form-group {{ margin-bottom: 16px; }}
.modal label {{ display: block; font-size: 13px; font-weight: 500; color: #333; margin-bottom: 4px; }}
.modal input, .modal textarea {{ width: 100%; padding: 8px 12px; border: 1px solid #d9d9d9; border-radius: 6px; font-size: 14px; outline: none; }}
.modal input:focus, .modal textarea:focus {{ border-color: #4f46e5; }}
.modal textarea {{ resize: vertical; min-height: 60px; }}
.modal-actions {{ display: flex; gap: 8px; justify-content: flex-end; margin-top: 20px; }}
.btn {{ padding: 8px 18px; border-radius: 6px; font-size: 14px; border: none; cursor: pointer; }}
.btn-primary {{ background: #4f46e5; color: #fff; }}
.btn-primary:hover {{ background: #4338ca; }}
.btn-cancel {{ background: #f3f4f6; color: #333; }}
.btn-cancel:hover {{ background: #e5e7eb; }}
.login-prompt {{ text-align: center; padding: 80px 20px; }}
.login-prompt a {{ color: #4f46e5; text-decoration: none; font-weight: 500; }}
.toast {{ position: fixed; top: 20px; right: 20px; background: #065f46; color: #fff; padding: 10px 18px; border-radius: 8px; font-size: 14px; z-index: 200; display: none; }}
</style>
</head>
<body>
{nav}
<div class="container">
  <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:16px">
    <h1 style="font-size:22px;color:#1a1a1a">应用管理</h1>
    <span id="user-info" style="font-size:14px;color:#666"></span>
  </div>
</div>
<div class="container" id="app-list"></div>
<div class="login-prompt" id="login-prompt" style="display:none">
  <p>请先登录</p>
  <a id="login-link" href="#">前往登录 →</a>
</div>
<div class="toast" id="toast"></div>

<div class="modal-overlay" id="edit-modal">
  <div class="modal">
    <h3>编辑应用</h3>
    <input type="hidden" id="edit-id">
    <div class="form-group"><label>应用名</label><input id="edit-name"></div>
    <div class="form-group"><label>应用主页</label><input id="edit-homepage"></div>
    <div class="form-group"><label>应用图标</label><input id="edit-logo" placeholder="https://example.com/logo.png"></div>
    <div class="form-group"><label>应用描述</label><textarea id="edit-desc"></textarea></div>
    <div class="form-group"><label>回调地址</label><input id="edit-redirect"></div>
    <div class="modal-actions">
      <button class="btn btn-cancel" onclick="closeModal()">取消</button>
      <button class="btn btn-primary" onclick="saveEdit()">保存</button>
    </div>
  </div>
</div>

<script>
let currentUser = null;

async function init() {{
  try {{
    const res = await fetch(BASE + '/api/me', {{ credentials: 'include' }});
    if (res.status === 401) {{ showLogin(); return; }}
    currentUser = await res.json();
    document.getElementById('user-info').innerHTML =
      currentUser.username + (currentUser.is_admin ? ' <span class="badge">管理员</span>' : '');
    loadClients();
  }} catch {{ showLogin(); }}
}}

function showLogin() {{
  document.getElementById('app-list').style.display = 'none';
  const el = document.getElementById('login-prompt');
  el.style.display = 'block';
  const returnUrl = encodeURIComponent(location.pathname + location.search);
  document.getElementById('login-link').href = '{login_url}?redirect=' + returnUrl;
}}

async function loadClients() {{
  const res = await fetch(BASE + '/api/clients', {{ credentials: 'include' }});
  const clients = await res.json();
  const container = document.getElementById('app-list');
  if (clients.length === 0) {{
    container.innerHTML = '<div class="empty">暂无应用，<a href="' + BASE + '/new">去注册一个</a></div>';
    return;
  }}
  container.innerHTML = clients.map(c => `
    <div class="app-card">
      ${{c.logo_url ?
        `<img src="${{esc(c.logo_url)}}" class="app-logo" onerror="this.style.display='none';this.nextElementSibling.style.display='flex'" /><div class="app-logo-placeholder" style="display:none">${{c.name.charAt(0).toUpperCase()}}</div>` :
        `<div class="app-logo-placeholder">${{c.name.charAt(0).toUpperCase()}}</div>`
      }}
      <div class="app-content">
      <div class="app-card-header">
        <span class="app-name">${{esc(c.name)}} ${{currentUser.is_admin ? '<span style="background:#dbeafe;color:#1d4ed8;font-size:11px;padding:2px 6px;border-radius:4px;margin-left:6px">' + (c.owner_name || '#' + c.owner_id) + '</span>' : ''}}</span>
        <span class="app-meta">${{c.created_at}}</span>
      </div>
      <div class="app-info"><span>Client ID</span> <code>${{c.id}}</code></div>
      <div class="app-info"><span>Secret</span> <code>${{c.client_secret || '(旧应用，请重新注册)'}}</code></div>
      <div class="app-info"><span>主页</span> ${{esc(c.homepage_url)}}</div>
      <div class="app-info"><span>回调地址</span> ${{esc(c.redirect_uri)}}</div>
      ${{c.description ? '<div class="app-info"><span>描述</span> ' + esc(c.description) + '</div>' : ''}}
      <div class="app-actions">
        <button class="btn-sm btn-edit" onclick='openEdit(${{JSON.stringify(c)}})'>编辑</button>
        <button class="btn-sm btn-del" onclick="deleteApp('${{c.id}}','${{esc(c.name)}}')">删除</button>
      </div>
      </div>
    </div>
  `).join('');
}}

function esc(s) {{ const d = document.createElement('div'); d.textContent = s || ''; return d.innerHTML; }}

function openEdit(c) {{
  document.getElementById('edit-id').value = c.id;
  document.getElementById('edit-name').value = c.name;
  document.getElementById('edit-homepage').value = c.homepage_url;
  document.getElementById('edit-logo').value = c.logo_url || '';
  document.getElementById('edit-desc').value = c.description || '';
  document.getElementById('edit-redirect').value = c.redirect_uri;
  document.getElementById('edit-modal').classList.add('active');
}}

function closeModal() {{ document.getElementById('edit-modal').classList.remove('active'); }}

async function saveEdit() {{
  const id = document.getElementById('edit-id').value;
  const data = {{
    name: document.getElementById('edit-name').value,
    homepage_url: document.getElementById('edit-homepage').value,
    logo_url: document.getElementById('edit-logo').value,
    description: document.getElementById('edit-desc').value,
    redirect_uri: document.getElementById('edit-redirect').value,
  }};
  const res = await fetch(BASE + '/api/clients/' + id, {{
    method: 'PUT', credentials: 'include',
    headers: {{ 'Content-Type': 'application/json' }},
    body: JSON.stringify(data),
  }});
  if (!res.ok) {{ const e = await res.json(); toast(e.error || '保存失败'); return; }}
  closeModal();
  toast('已保存');
  loadClients();
}}

async function deleteApp(id, name) {{
  if (!confirm('确定删除应用「' + name + '」？此操作不可撤销。')) return;
  const res = await fetch(BASE + '/api/clients/' + id, {{ method: 'DELETE', credentials: 'include' }});
  if (!res.ok) {{ const e = await res.json(); toast(e.error || '删除失败'); return; }}
  toast('已删除');
  loadClients();
}}

function toast(msg) {{
  const el = document.getElementById('toast');
  el.textContent = msg; el.style.display = 'block';
  setTimeout(() => el.style.display = 'none', 2000);
}}

init();
</script>
</body>
</html>"##,
        base_script = base_script(base_path),
        nav_style = NAV_STYLE,
        nav = nav_html(base_path, "dashboard"),
        login_url = login_url
    )
}

fn build_authorizations(base_path: &str, login_url: &str) -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>授权记录 - Connect</title>
{base_script}
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; background: #f5f7fa; min-height: 100vh; padding: 20px; }}
{nav_style}
.container {{ max-width: 800px; margin: 0 auto; }}
.empty {{ text-align: center; padding: 60px 20px; color: #999; }}
.grant-card {{ background: #fff; border-radius: 10px; padding: 20px; margin-bottom: 12px; box-shadow: 0 1px 4px rgba(0,0,0,0.06); display: flex; gap: 16px; align-items: flex-start; }}
.grant-logo {{ width: 48px; height: 48px; border-radius: 8px; flex-shrink: 0; }}
.grant-logo-placeholder {{ width: 48px; height: 48px; border-radius: 8px; background: #4f46e5; color: #fff; display: flex; align-items: center; justify-content: center; font-size: 20px; font-weight: 600; flex-shrink: 0; }}
.grant-content {{ flex: 1; min-width: 0; }}
.grant-header {{ display: flex; align-items: center; justify-content: space-between; margin-bottom: 8px; }}
.grant-name {{ font-size: 16px; font-weight: 600; color: #1a1a1a; }}
.grant-desc {{ font-size: 13px; color: #666; margin-bottom: 8px; }}
.grant-meta {{ font-size: 12px; color: #999; }}
.grant-meta span {{ margin-right: 16px; }}
.btn-revoke {{ padding: 6px 14px; border-radius: 6px; font-size: 13px; border: none; cursor: pointer; background: #fef2f2; color: #dc2626; }}
.btn-revoke:hover {{ background: #fee2e2; }}
.login-prompt {{ text-align: center; padding: 80px 20px; }}
.login-prompt a {{ color: #4f46e5; text-decoration: none; font-weight: 500; }}
.toast {{ position: fixed; top: 20px; right: 20px; background: #065f46; color: #fff; padding: 10px 18px; border-radius: 8px; font-size: 14px; z-index: 200; display: none; }}
</style>
</head>
<body>
{nav}
<div class="container">
  <h1 style="font-size:22px;color:#1a1a1a;margin-bottom:16px">授权记录</h1>
  <div id="grant-list"></div>
</div>
<div class="login-prompt" id="login-prompt" style="display:none">
  <p>请先登录</p>
  <a id="login-link" href="#">前往登录 →</a>
</div>
<div class="toast" id="toast"></div>

<script>
async function init() {{
  try {{
    const res = await fetch(BASE + '/api/authorizations', {{ credentials: 'include' }});
    if (res.status === 401) {{ showLogin(); return; }}
    const grants = await res.json();
    renderGrants(grants);
  }} catch {{ showLogin(); }}
}}

function showLogin() {{
  document.querySelector('.container').style.display = 'none';
  const el = document.getElementById('login-prompt');
  el.style.display = 'block';
  const returnUrl = encodeURIComponent(location.pathname + location.search);
  document.getElementById('login-link').href = '{login_url}?redirect=' + returnUrl;
}}

function renderGrants(grants) {{
  const container = document.getElementById('grant-list');
  if (grants.length === 0) {{
    container.innerHTML = '<div class="empty">暂无授权记录</div>';
    return;
  }}
  container.innerHTML = grants.map(g => {{
    const logo = g.logo_url ?
      `<img src="${{esc(g.logo_url)}}" class="grant-logo" onerror="this.style.display='none';this.nextElementSibling.style.display='flex'" /><div class="grant-logo-placeholder" style="display:none">${{g.name.charAt(0).toUpperCase()}}</div>` :
      `<div class="grant-logo-placeholder">${{g.name.charAt(0).toUpperCase()}}</div>`;
    return `
      <div class="grant-card">
        ${{logo}}
        <div class="grant-content">
          <div class="grant-header">
            <span class="grant-name">${{esc(g.name)}}</span>
            <button class="btn-revoke" onclick="revoke('${{g.client_id}}','${{esc(g.name)}}')">撤销授权</button>
          </div>
          <div class="grant-desc">${{esc(g.description)}}</div>
          <div class="grant-meta">
            <span>首次授权：${{g.first_granted_at}}</span>
            <span>最后使用：${{g.last_used_at}}</span>
          </div>
        </div>
      </div>
    `;
  }}).join('');
}}

function esc(s) {{ const d = document.createElement('div'); d.textContent = s || ''; return d.innerHTML; }}

async function revoke(clientId, name) {{
  if (!confirm('确定撤销对「' + name + '」的授权？撤销后该应用将无法访问你的信息。')) return;
  const res = await fetch(BASE + '/api/authorizations/' + clientId, {{ method: 'DELETE', credentials: 'include' }});
  if (!res.ok) {{ const e = await res.json(); toast(e.error || '撤销失败'); return; }}
  toast('已撤销授权');
  init();
}}

function toast(msg) {{
  const el = document.getElementById('toast');
  el.textContent = msg; el.style.display = 'block';
  setTimeout(() => el.style.display = 'none', 2000);
}}

init();
</script>
</body>
</html>"##,
        base_script = base_script(base_path),
        nav_style = NAV_STYLE,
        nav = nav_html(base_path, "authorizations"),
        login_url = login_url
    )
}
