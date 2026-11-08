use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use serde::Deserialize;

use crate::error::AppError;
use crate::session::securecookie::SecureCookieDecoder;
use crate::session::SessionUser;
use crate::AppState;

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
}

impl From<ApiSelfUser> for SessionUser {
    fn from(user: ApiSelfUser) -> Self {
        Self {
            id: user.id,
            username: user.username,
            role: user.role,
            status: user.status,
            group: user.group,
        }
    }
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

        decode_legacy_session(parts, state)
    }
}

async fn introspect_access_token(state: &AppState, token: &str) -> Result<SessionUser, AppError> {
    let url = format!("{}/api/user/self", state.config.new_api_base_url);
    let response = reqwest::Client::new()
        .get(url)
        .bearer_auth(token)
        .header("user-agent", "invitations-auth")
        .send()
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to reach new-api auth endpoint");
            AppError::Internal("new-api authentication service is unavailable".to_string())
        })?;

    if response.status() == reqwest::StatusCode::UNAUTHORIZED
        || response.status() == reqwest::StatusCode::FORBIDDEN
    {
        return Err(AppError::Unauthorized);
    }
    if !response.status().is_success() {
        tracing::error!(status = %response.status(), "new-api auth endpoint returned an error");
        return Err(AppError::Internal(
            "new-api authentication service returned an error".to_string(),
        ));
    }

    let body = response.json::<ApiSelfResponse>().await.map_err(|error| {
        tracing::error!(%error, "failed to parse new-api auth response");
        AppError::Internal("invalid new-api authentication response".to_string())
    })?;
    if !body.success {
        return Err(AppError::Unauthorized);
    }
    body.data.map(Into::into).ok_or(AppError::Unauthorized)
}

fn bearer_token(headers: &axum::http::HeaderMap) -> Option<&str> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let (scheme, token) = value.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty()).then(|| token.trim())
}

fn decode_legacy_session(parts: &Parts, state: &AppState) -> Result<SessionUser, AppError> {
    let cookie_header = parts
        .headers
        .get(axum::http::header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .ok_or(AppError::Unauthorized)?;
    let session_value = cookie_value(cookie_header, "session").ok_or(AppError::Unauthorized)?;
    let decoder = SecureCookieDecoder::new(state.config.session_secret.as_bytes(), "session");
    decoder.decode(session_value).map_err(|error| {
        tracing::warn!(%error, "failed to decode legacy new-api session cookie");
        AppError::Unauthorized
    })
}

fn cookie_value<'a>(cookie_header: &'a str, name: &str) -> Option<&'a str> {
    cookie_header.split(';').map(str::trim).find_map(|cookie| {
        let (cookie_name, value) = cookie.split_once('=')?;
        (cookie_name.trim() == name).then(|| value.trim())
    })
}

// 管理员权限检查
pub struct AdminUser(pub SessionUser);

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = SessionUser::from_request_parts(parts, state).await?;

        if !user.is_admin() {
            return Err(AppError::Forbidden);
        }

        Ok(AdminUser(user))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{header, HeaderMap, HeaderValue};

    #[test]
    fn extracts_bearer_case_insensitively() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("bEaReR token"),
        );
        assert_eq!(bearer_token(&headers), Some("token"));
    }

    #[test]
    fn preserves_base64_padding_in_legacy_cookie() {
        assert_eq!(
            cookie_value("theme=dark; session=abc==; x=1", "session"),
            Some("abc==")
        );
    }
}
