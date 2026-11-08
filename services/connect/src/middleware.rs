use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{FromRequestParts, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts};
use axum::response::{IntoResponse, Response};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::session::SessionUser;
use crate::session::securecookie::SecureCookieDecoder;

const EXTERNAL_AUTH_COOKIE: &str = "new_api_external_auth";
const MAX_EXTERNAL_AUTH_AGE_SECONDS: i64 = 15 * 60;

#[derive(Debug, Deserialize)]
struct ApiSelfResponse {
    success: bool,
    data: Option<ApiSelfUser>,
}

#[derive(Debug, Deserialize)]
struct ApiSelfUser {
    id: i64,
    username: String,
    role: i64,
    status: i64,
    group: String,
    email: Option<String>,
    display_name: Option<String>,
}

impl From<ApiSelfUser> for SessionUser {
    fn from(user: ApiSelfUser) -> Self {
        Self {
            id: user.id,
            username: user.username,
            role: user.role,
            status: user.status,
            group: user.group,
            email: user.email,
            display_name: user.display_name,
        }
    }
}

#[derive(Debug, Deserialize)]
struct AccessTokenClaims {
    exp: i64,
    iss: String,
    aud: serde_json::Value,
    token_use: String,
}

impl FromRequestParts<AppState> for SessionUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if let Some(token) = bearer_token(&parts.headers) {
            return introspect_access_token(state, token).await;
        }
        if let Some(token) = cookie_from_headers(&parts.headers, EXTERNAL_AUTH_COOKIE) {
            return introspect_access_token(state, token).await;
        }
        decode_legacy_session(parts, state)
    }
}

pub async fn exchange_external_auth(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let token = bearer_token(&headers).ok_or(AppError::Unauthorized)?;
    let max_age = access_token_max_age(token).ok_or(AppError::Unauthorized)?;
    introspect_access_token(&state, token).await?;

    let path = if state.config.base_path.is_empty() {
        "/"
    } else {
        state.config.base_path.as_str()
    };
    let secure = if state.config.external_auth_cookie_secure {
        "; Secure"
    } else {
        ""
    };
    let cookie = format!(
        "{EXTERNAL_AUTH_COOKIE}={token}; Path={path}; Max-Age={max_age}; HttpOnly; SameSite=Strict{secure}"
    );
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie)
            .map_err(|_| AppError::Internal("failed to create auth cookie".into()))?,
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    Ok(response)
}

async fn introspect_access_token(state: &AppState, token: &str) -> Result<SessionUser, AppError> {
    let url = format!("{}/api/user/self", state.config.api_base_url);
    let response = reqwest::Client::new()
        .get(url)
        .bearer_auth(token)
        .header("user-agent", "connect-oauth-server")
        .send()
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to reach new-api auth endpoint");
            AppError::Internal("new-api authentication service is unavailable".into())
        })?;

    if response.status() == reqwest::StatusCode::UNAUTHORIZED
        || response.status() == reqwest::StatusCode::FORBIDDEN
    {
        return Err(AppError::Unauthorized);
    }
    if !response.status().is_success() {
        tracing::error!(status = %response.status(), "new-api auth endpoint returned an error");
        return Err(AppError::Internal(
            "new-api authentication service returned an error".into(),
        ));
    }

    let body = response.json::<ApiSelfResponse>().await.map_err(|error| {
        tracing::error!(%error, "failed to parse new-api auth response");
        AppError::Internal("invalid new-api authentication response".into())
    })?;
    if !body.success {
        return Err(AppError::Unauthorized);
    }
    body.data.map(Into::into).ok_or(AppError::Unauthorized)
}

fn access_token_max_age(token: &str) -> Option<i64> {
    let mut segments = token.split('.');
    let _header = segments.next()?;
    let payload = segments.next()?;
    let _signature = segments.next()?;
    if segments.next().is_some() {
        return None;
    }
    let claims: AccessTokenClaims =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
    let valid_audience = claims.aud.as_str() == Some("new-api-dashboard")
        || claims.aud.as_array().is_some_and(|values| {
            values
                .iter()
                .any(|value| value.as_str() == Some("new-api-dashboard"))
        });
    if claims.iss != "new-api" || claims.token_use != "access" || !valid_audience {
        return None;
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    let remaining = claims.exp.checked_sub(now)?;
    (remaining > 0).then(|| remaining.min(MAX_EXTERNAL_AUTH_AGE_SECONDS))
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty()).then(|| token.trim())
}

fn cookie_from_headers<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    cookie_value(cookie_header, name)
}

fn cookie_value<'a>(cookie_header: &'a str, name: &str) -> Option<&'a str> {
    cookie_header.split(';').map(str::trim).find_map(|cookie| {
        let (cookie_name, value) = cookie.split_once('=')?;
        (cookie_name.trim() == name).then(|| value.trim())
    })
}

fn decode_legacy_session(parts: &Parts, state: &AppState) -> Result<SessionUser, AppError> {
    let session_value =
        cookie_from_headers(&parts.headers, "session").ok_or(AppError::Unauthorized)?;
    let decoder = SecureCookieDecoder::new(state.config.session_secret.as_bytes(), "session");
    decoder.decode(session_value).map_err(|error| {
        tracing::warn!(%error, "failed to decode legacy new-api session cookie");
        AppError::Unauthorized
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token_with_payload(payload: serde_json::Value) -> String {
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
        format!("header.{payload}.signature")
    }

    #[test]
    fn accepts_current_dashboard_access_token_shape() {
        let exp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 120;
        let token = token_with_payload(serde_json::json!({
            "exp": exp,
            "iss": "new-api",
            "aud": "new-api-dashboard",
            "token_use": "access"
        }));
        assert!(matches!(access_token_max_age(&token), Some(1..=120)));
    }

    #[test]
    fn rejects_non_dashboard_tokens() {
        let token = token_with_payload(serde_json::json!({
            "exp": i64::MAX,
            "iss": "other",
            "aud": "new-api-dashboard",
            "token_use": "access"
        }));
        assert_eq!(access_token_max_age(&token), None);
    }

    #[test]
    fn preserves_cookie_padding() {
        assert_eq!(
            cookie_value("session=abc==; other=1", "session"),
            Some("abc==")
        );
    }
}
