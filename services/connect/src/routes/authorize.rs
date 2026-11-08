use axum::extract::{Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use chrono::Utc;
use rand::Rng;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::Row;

use crate::AppState;
use crate::error::AppError;
use crate::session::SessionUser;

#[derive(Debug, Deserialize)]
pub struct AuthorizeParams {
    pub response_type: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub scope: Option<String>,
    pub state: Option<String>,
    pub nonce: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
}

pub async fn authorize(
    State(app): State<AppState>,
    user: Result<SessionUser, AppError>,
    Query(params): Query<AuthorizeParams>,
) -> Result<Response, AppError> {
    let user = match user {
        Ok(u) => u,
        Err(_) => {
            let return_url = format!(
                "{}/authorize?response_type={}&client_id={}&redirect_uri={}&scope={}&state={}",
                app.config.base_path,
                params.response_type,
                params.client_id,
                urlencoding::encode(&params.redirect_uri),
                params.scope.as_deref().unwrap_or(""),
                params.state.as_deref().unwrap_or(""),
            );
            let login_redirect = format!(
                "{}?redirect={}",
                app.config.login_url,
                urlencoding::encode(&return_url),
            );
            return Ok(Redirect::temporary(&login_redirect).into_response());
        }
    };

    if params.response_type != "code" {
        return Err(AppError::BadRequest("unsupported response_type".into()));
    }

    // Validate client
    let client_row = sqlx::query(
        "SELECT id, secret_hash, name, description, homepage_url, logo_url, redirect_uris, trusted, created_at FROM oauth_clients WHERE id = ?"
    )
    .bind(&params.client_id)
    .fetch_optional(&app.pool)
    .await?
    .ok_or_else(|| AppError::BadRequest("invalid client_id".into()))?;

    let redirect_uris: String = client_row.get("redirect_uris");
    let uri_list: Vec<String> = serde_json::from_str(&redirect_uris).unwrap_or_default();
    if !uri_list.iter().any(|u| u == &params.redirect_uri) {
        return Err(AppError::BadRequest("invalid redirect_uri".into()));
    }

    let trusted: i64 = client_row.get("trusted");
    let client_id: String = client_row.get("id");
    let scope = params.scope.as_deref().unwrap_or("profile");

    // Check if user has already granted access with the same scope
    if trusted == 0 {
        let grant =
            sqlx::query("SELECT scope FROM user_grants WHERE user_id = ? AND client_id = ?")
                .bind(user.id)
                .bind(&client_id)
                .fetch_optional(&app.pool)
                .await?;

        if let Some(grant_row) = grant {
            let granted_scope: String = grant_row.get("scope");
            // If scope changed, show consent page
            if granted_scope != scope {
                return show_consent_page(&app, &client_row, &params, &user).await;
            }
            // Otherwise, update last_used_at and proceed
            sqlx::query("UPDATE user_grants SET last_used_at = datetime('now') WHERE user_id = ? AND client_id = ?")
                .bind(user.id)
                .bind(&client_id)
                .execute(&app.pool)
                .await?;
        } else {
            // First time authorization, show consent page
            return show_consent_page(&app, &client_row, &params, &user).await;
        }
    }

    let email = user.email.clone().unwrap_or_default();
    let display_name = user.display_name.clone().unwrap_or_default();

    // Generate authorization code
    let code = generate_code();
    let code_hash = hash_token(&code);
    let scope = params.scope.unwrap_or_default();
    let expires_at = (Utc::now() + chrono::Duration::minutes(10))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let client_id: String = client_row.get("id");

    sqlx::query(
        "INSERT INTO authorization_codes (code_hash, client_id, user_id, user_name, user_role, user_status, user_group, user_email, user_display_name, scope, redirect_uri, expires_at, nonce, code_challenge, code_challenge_method) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&code_hash)
    .bind(&client_id)
    .bind(user.id)
    .bind(&user.username)
    .bind(user.role)
    .bind(user.status)
    .bind(&user.group)
    .bind(&email)
    .bind(&display_name)
    .bind(&scope)
    .bind(&params.redirect_uri)
    .bind(&expires_at)
    .bind(&params.nonce)
    .bind(&params.code_challenge)
    .bind(&params.code_challenge_method)
    .execute(&app.pool)
    .await?;

    let mut redirect_url = params.redirect_uri.clone();
    redirect_url.push_str(if redirect_url.contains('?') { "&" } else { "?" });
    redirect_url.push_str(&format!("code={}", code));
    if let Some(state) = params.state {
        redirect_url.push_str(&format!("&state={}", state));
    }

    Ok(Redirect::temporary(&redirect_url).into_response())
}

fn generate_code() -> String {
    let mut rng = rand::rng();
    let bytes: [u8; 32] = rng.random();
    hex::encode(bytes)
}

pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

async fn show_consent_page(
    app: &AppState,
    client_row: &sqlx::sqlite::SqliteRow,
    params: &AuthorizeParams,
    user: &SessionUser,
) -> Result<Response, AppError> {
    use axum::response::Html;
    use sqlx::Row;

    let client_name: String = client_row.get("name");
    let client_desc: String = client_row.get("description");
    let client_homepage: String = client_row.get("homepage_url");
    let client_logo: String = client_row.get("logo_url");
    let scope = params.scope.as_deref().unwrap_or("profile");

    // Check if scope changed
    let grant = sqlx::query("SELECT scope FROM user_grants WHERE user_id = ? AND client_id = ?")
        .bind(user.id)
        .bind(&params.client_id)
        .fetch_optional(&app.pool)
        .await?;

    let scope_changed = grant.is_some();
    let scope_changed_text = if scope_changed {
        "<p style='color:#d97706;font-size:14px;margin-bottom:16px'>⚠️ 此应用请求了新的权限</p>"
    } else {
        ""
    };

    let logo_html = if client_logo.is_empty() {
        let initial = client_name
            .chars()
            .next()
            .unwrap_or('A')
            .to_uppercase()
            .to_string();
        format!(
            r#"<div style="width:64px;height:64px;border-radius:12px;background:#4f46e5;color:#fff;display:flex;align-items:center;justify-content:center;font-size:28px;font-weight:600;margin:0 auto 16px">{}</div>"#,
            initial
        )
    } else {
        format!(
            r#"<img src="{}" alt="logo" style="width:64px;height:64px;border-radius:12px;margin:0 auto 16px;display:block" onerror="this.style.display='none';this.nextElementSibling.style.display='flex'" /><div style="width:64px;height:64px;border-radius:12px;background:#4f46e5;color:#fff;display:none;align-items:center;justify-content:center;font-size:28px;font-weight:600;margin:0 auto 16px">{}</div>"#,
            html_escape(&client_logo),
            client_name.chars().next().unwrap_or('A').to_uppercase()
        )
    };

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>授权确认</title>
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; background: #f5f7fa; min-height: 100vh; display: flex; align-items: center; justify-content: center; padding: 20px; }}
.card {{ background: #fff; border-radius: 16px; padding: 40px; max-width: 440px; width: 100%; box-shadow: 0 4px 20px rgba(0,0,0,0.08); text-align: center; }}
h1 {{ font-size: 20px; font-weight: 600; color: #1a1a1a; margin-bottom: 8px; }}
.app-name {{ font-size: 18px; font-weight: 600; color: #4f46e5; margin-bottom: 8px; }}
.app-desc {{ font-size: 14px; color: #666; margin-bottom: 16px; }}
.app-homepage {{ font-size: 13px; color: #4f46e5; text-decoration: none; margin-bottom: 24px; display: inline-block; }}
.scope-box {{ background: #f9fafb; border: 1px solid #e5e7eb; border-radius: 8px; padding: 16px; margin-bottom: 24px; text-align: left; }}
.scope-title {{ font-size: 13px; font-weight: 600; color: #333; margin-bottom: 8px; }}
.scope-item {{ font-size: 14px; color: #555; padding: 6px 0; display: flex; align-items: center; gap: 8px; }}
.scope-item::before {{ content: "✓"; color: #10b981; font-weight: 600; }}
.buttons {{ display: flex; gap: 12px; }}
.btn {{ flex: 1; padding: 12px; border-radius: 8px; font-size: 15px; font-weight: 500; border: none; cursor: pointer; transition: all 0.2s; }}
.btn-primary {{ background: #4f46e5; color: #fff; }}
.btn-primary:hover {{ background: #4338ca; }}
.btn-secondary {{ background: #f3f4f6; color: #333; }}
.btn-secondary:hover {{ background: #e5e7eb; }}
</style>
</head>
<body>
<div class="card">
  {}
  <h1>授权确认</h1>
  <div class="app-name">{}</div>
  <div class="app-desc">{}</div>
  <a href="{}" target="_blank" class="app-homepage">访问应用主页 →</a>
  {}
  <div class="scope-box">
    <div class="scope-title">此应用将获得以下权限：</div>
    <div class="scope-item">读取你的基本信息（用户名、邮箱、显示名称）</div>
  </div>
  <form method="POST" action="{}/authorize/consent">
    <input type="hidden" name="client_id" value="{}">
    <input type="hidden" name="redirect_uri" value="{}">
    <input type="hidden" name="scope" value="{}">
    <input type="hidden" name="state" value="{}">
    <input type="hidden" name="nonce" value="{}">
    <input type="hidden" name="code_challenge" value="{}">
    <input type="hidden" name="code_challenge_method" value="{}">
    <div class="buttons">
      <button type="submit" name="action" value="deny" class="btn btn-secondary">拒绝</button>
      <button type="submit" name="action" value="allow" class="btn btn-primary">授权</button>
    </div>
  </form>
</div>
</body>
</html>"#,
        logo_html,
        html_escape(&client_name),
        html_escape(&client_desc),
        html_escape(&client_homepage),
        scope_changed_text,
        app.config.base_path,
        html_escape(&params.client_id),
        html_escape(&params.redirect_uri),
        html_escape(scope),
        html_escape(params.state.as_deref().unwrap_or("")),
        html_escape(params.nonce.as_deref().unwrap_or("")),
        html_escape(params.code_challenge.as_deref().unwrap_or("")),
        html_escape(params.code_challenge_method.as_deref().unwrap_or("")),
    );

    Ok(Html(html).into_response())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

#[derive(Debug, Deserialize)]
pub struct ConsentForm {
    pub client_id: String,
    pub redirect_uri: String,
    pub scope: String,
    pub state: Option<String>,
    pub nonce: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub action: String,
}

pub async fn authorize_consent(
    State(app): State<AppState>,
    user: SessionUser,
    axum::Form(form): axum::Form<ConsentForm>,
) -> Result<Response, AppError> {
    // Handle deny
    if form.action == "deny" {
        let mut redirect_url = form.redirect_uri.clone();
        redirect_url.push_str(if redirect_url.contains('?') { "&" } else { "?" });
        redirect_url
            .push_str("error=access_denied&error_description=User%20denied%20the%20request");
        if let Some(state) = form.state {
            redirect_url.push_str(&format!("&state={}", urlencoding::encode(&state)));
        }
        // Use 303 See Other to ensure browser uses GET method after POST
        use axum::http::StatusCode;
        return Ok((StatusCode::SEE_OTHER, [("Location", redirect_url.as_str())]).into_response());
    }

    // Validate client
    let client_row = sqlx::query("SELECT id FROM oauth_clients WHERE id = ?")
        .bind(&form.client_id)
        .fetch_optional(&app.pool)
        .await?
        .ok_or_else(|| AppError::BadRequest("invalid client_id".into()))?;

    let client_id: String = client_row.get("id");

    let email = user.email.clone().unwrap_or_default();
    let display_name = user.display_name.clone().unwrap_or_default();

    // Update or create user_grants
    let existing = sqlx::query("SELECT id FROM user_grants WHERE user_id = ? AND client_id = ?")
        .bind(user.id)
        .bind(&client_id)
        .fetch_optional(&app.pool)
        .await?;

    if existing.is_some() {
        sqlx::query("UPDATE user_grants SET scope = ?, last_used_at = datetime('now') WHERE user_id = ? AND client_id = ?")
            .bind(&form.scope)
            .bind(user.id)
            .bind(&client_id)
            .execute(&app.pool)
            .await?;
    } else {
        sqlx::query("INSERT INTO user_grants (user_id, client_id, scope) VALUES (?, ?, ?)")
            .bind(user.id)
            .bind(&client_id)
            .bind(&form.scope)
            .execute(&app.pool)
            .await?;
    }

    // Record in grant_history
    sqlx::query("INSERT INTO grant_history (user_id, client_id, scope) VALUES (?, ?, ?)")
        .bind(user.id)
        .bind(&client_id)
        .bind(&form.scope)
        .execute(&app.pool)
        .await?;

    // Generate authorization code
    let code = generate_code();
    let code_hash = hash_token(&code);
    let expires_at = (Utc::now() + chrono::Duration::minutes(10))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    sqlx::query(
        "INSERT INTO authorization_codes (code_hash, client_id, user_id, user_name, user_role, user_status, user_group, user_email, user_display_name, scope, redirect_uri, expires_at, nonce, code_challenge, code_challenge_method) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&code_hash)
    .bind(&client_id)
    .bind(user.id)
    .bind(&user.username)
    .bind(user.role)
    .bind(user.status)
    .bind(&user.group)
    .bind(&email)
    .bind(&display_name)
    .bind(&form.scope)
    .bind(&form.redirect_uri)
    .bind(&expires_at)
    .bind(&form.nonce)
    .bind(&form.code_challenge)
    .bind(&form.code_challenge_method)
    .execute(&app.pool)
    .await?;

    let mut redirect_url = form.redirect_uri.clone();
    redirect_url.push_str(if redirect_url.contains('?') { "&" } else { "?" });
    redirect_url.push_str(&format!("code={}", code));
    if let Some(state) = form.state {
        redirect_url.push_str(&format!("&state={}", state));
    }

    // Use 303 See Other to ensure browser uses GET method after POST
    use axum::http::StatusCode;
    Ok((StatusCode::SEE_OTHER, [("Location", redirect_url.as_str())]).into_response())
}
