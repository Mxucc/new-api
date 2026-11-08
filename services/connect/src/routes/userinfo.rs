use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use chrono::Utc;
use serde::Serialize;
use sqlx::Row;

use crate::AppState;
use crate::error::AppError;

use super::authorize::hash_token;

#[derive(Debug, Serialize)]
pub struct UserInfoResponse {
    pub sub: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_verified: Option<bool>,
    pub role: i64,
    pub status: i64,
    pub group: String,
}

pub async fn userinfo(
    State(app): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<UserInfoResponse>, AppError> {
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or(AppError::Unauthorized)?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or(AppError::Unauthorized)?;

    let token_hash = hash_token(token);

    let row = sqlx::query(
        "SELECT user_id, user_name, user_role, user_status, user_group, user_email, user_display_name, scope, expires_at FROM access_tokens WHERE token_hash = ?"
    )
    .bind(&token_hash)
    .fetch_optional(&app.pool)
    .await?
    .ok_or(AppError::Unauthorized)?;

    let expires_at: String = row.get("expires_at");
    let parsed = chrono::NaiveDateTime::parse_from_str(&expires_at, "%Y-%m-%d %H:%M:%S")
        .map_err(|_| AppError::Internal("invalid expires_at".into()))?;
    if parsed < Utc::now().naive_utc() {
        return Err(AppError::Unauthorized);
    }

    let display_name: String = row.get("user_display_name");
    let username: String = row.get("user_name");
    let email: String = row.get("user_email");
    let scope: String = row.get("scope");

    let scopes: Vec<&str> = scope.split_whitespace().collect();
    let has_profile = scopes.contains(&"profile");
    let has_email = scopes.contains(&"email");

    let name = if has_profile {
        if display_name.is_empty() {
            None
        } else {
            Some(display_name)
        }
    } else {
        None
    };

    let preferred_username = if has_profile { Some(username) } else { None };

    let (email_claim, email_verified) = if has_email && !email.is_empty() {
        (Some(email), Some(true))
    } else {
        (None, None)
    };

    Ok(Json(UserInfoResponse {
        sub: row.get::<i64, _>("user_id").to_string(),
        name,
        preferred_username,
        email: email_claim,
        email_verified,
        role: row.get("user_role"),
        status: row.get("user_status"),
        group: row.get("user_group"),
    }))
}
