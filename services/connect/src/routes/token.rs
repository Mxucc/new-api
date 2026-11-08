use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use base64::Engine;
use chrono::Utc;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::AppState;
use crate::error::AppError;

use super::authorize::hash_token;

#[derive(Debug, Deserialize)]
pub struct TokenRequest {
    pub grant_type: String,
    pub code: Option<String>,
    pub redirect_uri: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub code_verifier: Option<String>,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
}

pub async fn token(
    State(app): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<TokenResponse>, AppError> {
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let mut req: TokenRequest = if content_type.contains("application/x-www-form-urlencoded") {
        serde_urlencoded::from_bytes(&body)
            .map_err(|e| AppError::BadRequest(format!("invalid form body: {}", e)))?
    } else {
        serde_json::from_slice(&body)
            .map_err(|e| AppError::BadRequest(format!("invalid json body: {}", e)))?
    };

    // 尝试从 Authorization header 解析客户端凭证 (Basic Auth)
    if (req.client_id.is_none() || req.client_secret.is_none())
        && let Some(auth_header) = headers.get("authorization").and_then(|v| v.to_str().ok())
        && let Some(basic) = auth_header.strip_prefix("Basic ")
        && let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(basic)
        && let Ok(credentials) = String::from_utf8(decoded)
        && let Some((id, secret)) = credentials.split_once(':')
    {
        req.client_id = Some(id.to_string());
        req.client_secret = Some(secret.to_string());
    }

    // 处理不同的 grant_type
    match req.grant_type.as_str() {
        "authorization_code" => handle_authorization_code_grant(app, req).await,
        "refresh_token" => handle_refresh_token_grant(app, req).await,
        _ => Err(AppError::BadRequest("unsupported grant_type".into())),
    }
}

async fn handle_authorization_code_grant(
    app: AppState,
    req: TokenRequest,
) -> Result<Json<TokenResponse>, AppError> {
    let code = req
        .code
        .ok_or_else(|| AppError::BadRequest("code required".into()))?;
    let redirect_uri = req
        .redirect_uri
        .ok_or_else(|| AppError::BadRequest("redirect_uri required".into()))?;
    let client_id = req
        .client_id
        .ok_or_else(|| AppError::BadRequest("client_id required".into()))?;

    // Validate client
    let client_row = sqlx::query("SELECT id, secret_hash, public FROM oauth_clients WHERE id = ?")
        .bind(&client_id)
        .fetch_optional(&app.pool)
        .await?
        .ok_or_else(|| AppError::BadRequest("invalid client_id".into()))?;

    let is_public: i64 = client_row.try_get("public").unwrap_or(0);

    // 验证客户端凭证
    if is_public == 0 {
        // 机密客户端，需要验证 secret
        let client_secret = req
            .client_secret
            .ok_or_else(|| AppError::BadRequest("client_secret required".into()))?;
        let secret_hash: String = client_row.get("secret_hash");
        let provided_hash = super::clients::sha256_hash(&client_secret);
        if secret_hash != provided_hash {
            return Err(AppError::BadRequest("invalid client_secret".into()));
        }
    }
    // 公开客户端不需要验证 secret

    // Validate authorization code
    let code_hash = hash_token(&code);
    let code_row = sqlx::query(
        "SELECT client_id, user_id, user_name, user_role, user_status, user_group, user_email, user_display_name, scope, redirect_uri, expires_at, nonce, code_challenge, code_challenge_method FROM authorization_codes WHERE code_hash = ? AND used = 0"
    )
    .bind(&code_hash)
    .fetch_optional(&app.pool)
    .await?
    .ok_or_else(|| AppError::BadRequest("invalid or expired code".into()))?;

    let code_client_id: String = code_row.get("client_id");
    let code_redirect_uri: String = code_row.get("redirect_uri");
    let code_user_id: i64 = code_row.get("user_id");
    let code_user_name: String = code_row.get("user_name");
    let code_user_role: i64 = code_row.get("user_role");
    let code_user_status: i64 = code_row.get("user_status");
    let code_user_group: String = code_row.get("user_group");
    let code_user_email: String = code_row.get("user_email");
    let code_user_display_name: String = code_row.get("user_display_name");
    let code_scope: String = code_row.get("scope");
    let code_expires_at: String = code_row.get("expires_at");
    let code_nonce: Option<String> = code_row.get("nonce");
    let code_challenge: Option<String> = code_row.get("code_challenge");
    let code_challenge_method: Option<String> = code_row.get("code_challenge_method");

    if code_client_id != client_id {
        return Err(AppError::BadRequest(
            "code was not issued to this client".into(),
        ));
    }
    if code_redirect_uri != redirect_uri {
        return Err(AppError::BadRequest("redirect_uri mismatch".into()));
    }

    let expires_at = chrono::NaiveDateTime::parse_from_str(&code_expires_at, "%Y-%m-%d %H:%M:%S")
        .map_err(|_| AppError::Internal("invalid expires_at format".into()))?;
    if expires_at < Utc::now().naive_utc() {
        return Err(AppError::BadRequest("code expired".into()));
    }

    // 验证 PKCE
    if let Some(challenge) = code_challenge {
        let verifier = req
            .code_verifier
            .ok_or_else(|| AppError::BadRequest("code_verifier required".into()))?;
        let method = code_challenge_method.as_deref().unwrap_or("plain");

        let computed_challenge = match method {
            "S256" => {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(verifier.as_bytes());
                let hash = hasher.finalize();
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash)
            }
            "plain" => verifier.clone(),
            _ => {
                return Err(AppError::BadRequest(
                    "unsupported code_challenge_method".into(),
                ));
            }
        };

        if computed_challenge != challenge {
            return Err(AppError::BadRequest("code_verifier mismatch".into()));
        }
    }

    // Mark code as used
    sqlx::query("UPDATE authorization_codes SET used = 1 WHERE code_hash = ?")
        .bind(&code_hash)
        .execute(&app.pool)
        .await?;

    // Generate access token
    let access_token = generate_access_token();
    let token_hash = hash_token(&access_token);
    let expires_in: i64 = 3600;
    let token_expires_at = (Utc::now() + chrono::Duration::seconds(expires_in))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    sqlx::query(
        "INSERT INTO access_tokens (token_hash, client_id, user_id, user_name, user_role, user_status, user_group, user_email, user_display_name, scope, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&token_hash)
    .bind(&client_id)
    .bind(code_user_id)
    .bind(&code_user_name)
    .bind(code_user_role)
    .bind(code_user_status)
    .bind(&code_user_group)
    .bind(&code_user_email)
    .bind(&code_user_display_name)
    .bind(&code_scope)
    .bind(&token_expires_at)
    .execute(&app.pool)
    .await?;

    // 生成 refresh token
    let refresh_token = generate_refresh_token();
    let refresh_token_hash = hash_token(&refresh_token);
    let refresh_expires_in = app.config.refresh_token_expires_in;
    let refresh_expires_at = (Utc::now() + chrono::Duration::seconds(refresh_expires_in))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    sqlx::query(
        "INSERT INTO refresh_tokens (token_hash, client_id, user_id, user_name, user_role, user_status, user_group, user_email, user_display_name, scope, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&refresh_token_hash)
    .bind(&client_id)
    .bind(code_user_id)
    .bind(&code_user_name)
    .bind(code_user_role)
    .bind(code_user_status)
    .bind(&code_user_group)
    .bind(&code_user_email)
    .bind(&code_user_display_name)
    .bind(&code_scope)
    .bind(&refresh_expires_at)
    .execute(&app.pool)
    .await?;

    // 生成 ID Token（如果 scope 包含 openid）
    let id_token = if code_scope.split_whitespace().any(|s| s == "openid") {
        let token = crate::oidc::generate_id_token(
            &app.key_manager,
            crate::oidc::id_token::IdTokenParams {
                issuer: &app.config.issuer_url,
                client_id: &client_id,
                user_id: code_user_id,
                username: &code_user_name,
                display_name: &code_user_display_name,
                email: &code_user_email,
                nonce: code_nonce.as_deref(),
                expires_in: app.config.id_token_expires_in,
            },
        )
        .map_err(|e| AppError::Internal(format!("failed to generate id_token: {}", e)))?;
        Some(token)
    } else {
        None
    };

    Ok(Json(TokenResponse {
        access_token,
        token_type: "Bearer".into(),
        expires_in,
        scope: code_scope,
        id_token,
        refresh_token: Some(refresh_token),
    }))
}

async fn handle_refresh_token_grant(
    app: AppState,
    req: TokenRequest,
) -> Result<Json<TokenResponse>, AppError> {
    let refresh_token = req
        .refresh_token
        .ok_or_else(|| AppError::BadRequest("refresh_token required".into()))?;
    let client_id = req
        .client_id
        .ok_or_else(|| AppError::BadRequest("client_id required".into()))?;

    // 验证客户端
    let client_row = sqlx::query("SELECT id, secret_hash, public FROM oauth_clients WHERE id = ?")
        .bind(&client_id)
        .fetch_optional(&app.pool)
        .await?
        .ok_or_else(|| AppError::BadRequest("invalid client_id".into()))?;

    let is_public: i64 = client_row.try_get("public").unwrap_or(0);

    // 验证客户端凭证
    if is_public == 0 {
        let client_secret = req
            .client_secret
            .ok_or_else(|| AppError::BadRequest("client_secret required".into()))?;
        let secret_hash: String = client_row.get("secret_hash");
        let provided_hash = super::clients::sha256_hash(&client_secret);
        if secret_hash != provided_hash {
            return Err(AppError::BadRequest("invalid client_secret".into()));
        }
    }

    // 验证 refresh token
    let refresh_token_hash = hash_token(&refresh_token);
    let token_row = sqlx::query(
        "SELECT client_id, user_id, user_name, user_role, user_status, user_group, user_email, user_display_name, scope, expires_at, used FROM refresh_tokens WHERE token_hash = ?"
    )
    .bind(&refresh_token_hash)
    .fetch_optional(&app.pool)
    .await?
    .ok_or_else(|| AppError::BadRequest("invalid refresh token".into()))?;

    let token_client_id: String = token_row.get("client_id");
    let token_user_id: i64 = token_row.get("user_id");
    let token_user_name: String = token_row.get("user_name");
    let token_user_role: i64 = token_row.get("user_role");
    let token_user_status: i64 = token_row.get("user_status");
    let token_user_group: String = token_row.get("user_group");
    let token_user_email: String = token_row.get("user_email");
    let token_user_display_name: String = token_row.get("user_display_name");
    let token_scope: String = token_row.get("scope");
    let token_expires_at: String = token_row.get("expires_at");
    let token_used: i64 = token_row.get("used");

    // 检查 client_id 匹配
    if token_client_id != client_id {
        return Err(AppError::BadRequest(
            "refresh token was not issued to this client".into(),
        ));
    }

    // 检查是否已使用
    if token_used != 0 {
        return Err(AppError::BadRequest("refresh token already used".into()));
    }

    // 检查是否过期
    let expires_at = chrono::NaiveDateTime::parse_from_str(&token_expires_at, "%Y-%m-%d %H:%M:%S")
        .map_err(|_| AppError::Internal("invalid expires_at format".into()))?;
    if expires_at < Utc::now().naive_utc() {
        return Err(AppError::BadRequest("refresh token expired".into()));
    }

    // 标记旧的 refresh token 为已使用
    sqlx::query("UPDATE refresh_tokens SET used = 1 WHERE token_hash = ?")
        .bind(&refresh_token_hash)
        .execute(&app.pool)
        .await?;

    // 生成新的 access token
    let access_token = generate_access_token();
    let access_token_hash = hash_token(&access_token);
    let expires_in: i64 = 3600;
    let access_expires_at = (Utc::now() + chrono::Duration::seconds(expires_in))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    sqlx::query(
        "INSERT INTO access_tokens (token_hash, client_id, user_id, user_name, user_role, user_status, user_group, user_email, user_display_name, scope, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&access_token_hash)
    .bind(&client_id)
    .bind(token_user_id)
    .bind(&token_user_name)
    .bind(token_user_role)
    .bind(token_user_status)
    .bind(&token_user_group)
    .bind(&token_user_email)
    .bind(&token_user_display_name)
    .bind(&token_scope)
    .bind(&access_expires_at)
    .execute(&app.pool)
    .await?;

    // 生成新的 refresh token (token rotation)
    let new_refresh_token = generate_refresh_token();
    let new_refresh_token_hash = hash_token(&new_refresh_token);
    let refresh_expires_in = app.config.refresh_token_expires_in;
    let refresh_expires_at = (Utc::now() + chrono::Duration::seconds(refresh_expires_in))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    sqlx::query(
        "INSERT INTO refresh_tokens (token_hash, client_id, user_id, user_name, user_role, user_status, user_group, user_email, user_display_name, scope, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&new_refresh_token_hash)
    .bind(&client_id)
    .bind(token_user_id)
    .bind(&token_user_name)
    .bind(token_user_role)
    .bind(token_user_status)
    .bind(&token_user_group)
    .bind(&token_user_email)
    .bind(&token_user_display_name)
    .bind(&token_scope)
    .bind(&refresh_expires_at)
    .execute(&app.pool)
    .await?;

    Ok(Json(TokenResponse {
        access_token,
        token_type: "Bearer".into(),
        expires_in,
        scope: token_scope,
        id_token: None,
        refresh_token: Some(new_refresh_token),
    }))
}

fn generate_access_token() -> String {
    let mut rng = rand::rng();
    let bytes: [u8; 32] = rng.random();
    format!("cat_{}", hex::encode(bytes))
}

fn generate_refresh_token() -> String {
    let mut rng = rand::rng();
    let bytes: [u8; 32] = rng.random();
    format!("crt_{}", hex::encode(bytes))
}
