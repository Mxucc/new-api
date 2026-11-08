mod control;
mod session;

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path as AxumPath, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{delete, get, post, put},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Sha256;

use crate::{config::Config, proxy::AppState};

const ADMIN_ROOT: &str = "/ai-bridge";
const EXTERNAL_AUTH_COOKIE: &str = "new_api_external_auth";
const MAX_EXTERNAL_AUTH_AGE_SECONDS: i64 = 15 * 60;
const SNAPSHOT_ENTRY_LIMIT: usize = 500;
const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; base-uri 'none'; connect-src 'self'; img-src 'self' data:; script-src 'self'; style-src 'self'; form-action 'none'; frame-ancestors 'none'";
const INDEX_HTML: &str = include_str!("index.html");
const ADMIN_CSS: &str = include_str!("admin.css");
const ADMIN_JS: &str = include_str!("admin.js");
const FAVICON_SVG: &str = include_str!("favicon.svg");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdminAuthError {
    Unavailable,
    Unauthorized,
    Forbidden,
}

#[derive(Serialize)]
struct AdminIdentity {
    id: i64,
    username: String,
    role: i64,
    csrf_token: String,
}

#[derive(Serialize)]
struct AdminSnapshot {
    generated_at_ms: u64,
    uptime_secs: u64,
    concurrency: AdminConcurrency,
    retry_cooldowns: AdminRetryCooldowns,
    auth_cooldowns: crate::auth_failure::AdminAuthFailureSnapshot,
    blacklist: crate::auth::AdminBlacklistSnapshot,
    whitelist: crate::auth::AdminWhitelistSnapshot,
    observability: crate::observability::AdminObservabilitySnapshot,
}

#[derive(Serialize)]
struct AdminConcurrency {
    active: usize,
    tracked_keys: usize,
}

#[derive(Serialize)]
struct AdminRetryCooldowns {
    total: usize,
    truncated: bool,
    entries: Vec<crate::rate_limit::AdminRetryCooldown>,
}

#[derive(Deserialize)]
struct AdminCooldownDelete {
    id: String,
}

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
}

#[derive(Debug, Deserialize)]
struct AccessTokenClaims {
    exp: i64,
    iss: String,
    aud: serde_json::Value,
    token_use: String,
}

pub(crate) fn mount_routes(router: Router<Arc<AppState>>) -> Router<Arc<AppState>> {
    router
        .route(ADMIN_ROOT, get(index))
        .route("/ai-bridge/", get(index))
        .route("/ai-bridge/admin.css", get(stylesheet))
        .route("/ai-bridge/admin.js", get(script))
        .route("/ai-bridge/favicon.svg", get(favicon))
        .route("/ai-bridge/api/me", get(me))
        .route("/ai-bridge/auth/exchange", post(exchange_external_auth))
        .route("/ai-bridge/api/snapshot", get(snapshot))
        .route("/ai-bridge/api/control", get(control::get_control))
        .route(
            "/ai-bridge/api/config",
            put(control::put_config).layer(DefaultBodyLimit::max(64 * 1024)),
        )
        .route("/ai-bridge/api/reload", post(control::reload))
        .route(
            "/ai-bridge/api/cooldowns/:kind",
            delete(clear_cooldown).layer(DefaultBodyLimit::max(4 * 1024)),
        )
        .route(
            "/ai-bridge/api/lists/:kind",
            post(control::create_list_entry)
                .put(control::update_list_entry)
                .delete(control::delete_list_entry)
                .layer(DefaultBodyLimit::max(16 * 1024)),
        )
}

async fn clear_cooldown(
    State(state): State<Arc<AppState>>,
    AxumPath(kind): AxumPath<String>,
    headers: HeaderMap,
    Json(input): Json<AdminCooldownDelete>,
) -> Response {
    let config = state.config_snapshot();
    if let Err(error) = authorize_mutation(&headers, &config).await {
        return auth_error_response(error);
    }

    let valid_id = match kind.as_str() {
        "auth_api_key" | "retry" => {
            input.id.len() == 64 && input.id.bytes().all(|byte| byte.is_ascii_hexdigit())
        }
        "auth_client_ip" => input.id.parse::<std::net::IpAddr>().is_ok(),
        _ => {
            return hardened(
                (
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "error": {
                            "code": "unknown_cooldown_kind",
                            "message": "Unknown cooldown kind."
                        }
                    })),
                )
                    .into_response(),
            );
        }
    };
    if !valid_id {
        return hardened(
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {
                        "code": "invalid_cooldown_id",
                        "message": "Cooldown identifier is invalid."
                    }
                })),
            )
                .into_response(),
        );
    }

    let cleared = match kind.as_str() {
        "auth_api_key" => state
            .auth_failure_tracker
            .clear_cooldown("api_key", &input.id),
        "auth_client_ip" => state
            .auth_failure_tracker
            .clear_cooldown("client_ip", &input.id),
        "retry" => state.rate_limiter.clear_cooldown(&input.id),
        _ => false,
    };
    state.observability.record_admin(
        "clear_cooldown",
        &kind,
        if cleared { "success" } else { "not_found" },
    );
    if cleared {
        return hardened(Json(json!({ "ok": true, "result": "cleared" })).into_response());
    }
    hardened(
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": {
                    "code": "cooldown_not_found",
                    "message": "The cooldown is no longer active."
                }
            })),
        )
            .into_response(),
    )
}

async fn index(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let config = state.config_snapshot();
    match authenticate(&headers, &config).await {
        Ok(_) => hardened(Html(INDEX_HTML).into_response()),
        Err(AdminAuthError::Unauthorized) => redirect_or_error(&config),
        Err(error) => auth_error_response(error),
    }
}

async fn stylesheet(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    protected_asset(&state, &headers, "text/css; charset=utf-8", ADMIN_CSS).await
}

async fn script(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    protected_asset(&state, &headers, "text/javascript; charset=utf-8", ADMIN_JS).await
}

async fn favicon(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    protected_asset(&state, &headers, "image/svg+xml", FAVICON_SVG).await
}

async fn protected_asset(
    state: &AppState,
    headers: &HeaderMap,
    content_type: &'static str,
    content: &'static str,
) -> Response {
    let config = state.config_snapshot();
    if let Err(error) = authenticate(headers, &config).await {
        return auth_error_response(error);
    }
    let mut response = Response::new(Body::from(content));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    hardened(response)
}

async fn me(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let config = state.config_snapshot();
    match authenticate(&headers, &config).await {
        Ok(user) => hardened(
            Json(AdminIdentity {
                id: user.id,
                username: user.username,
                role: user.role,
                csrf_token: csrf_token_for_headers(&headers, &config).unwrap_or_default(),
            })
            .into_response(),
        ),
        Err(error) => auth_error_response(error),
    }
}

async fn snapshot(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let config = state.config_snapshot();
    if let Err(error) = authenticate(&headers, &config).await {
        return auth_error_response(error);
    }

    let rate = state.rate_limiter.admin_snapshot(SNAPSHOT_ENTRY_LIMIT);
    let payload = AdminSnapshot {
        generated_at_ms: unix_time_millis(),
        uptime_secs: state.uptime().as_secs(),
        concurrency: AdminConcurrency {
            active: rate.active,
            tracked_keys: rate.tracked_keys,
        },
        retry_cooldowns: AdminRetryCooldowns {
            total: rate.cooldown_total,
            truncated: rate.cooldowns_truncated,
            entries: rate.cooldowns,
        },
        auth_cooldowns: state
            .auth_failure_tracker
            .admin_snapshot(SNAPSHOT_ENTRY_LIMIT),
        blacklist: state.blacklist_cache.admin_snapshot(SNAPSHOT_ENTRY_LIMIT),
        whitelist: state.whitelist_cache.admin_snapshot(SNAPSHOT_ENTRY_LIMIT),
        observability: state.observability.snapshot(SNAPSHOT_ENTRY_LIMIT),
    };
    hardened(Json(payload).into_response())
}

fn unix_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

async fn exchange_external_auth(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let config = state.config_snapshot();
    let Some(token) = bearer_token(&headers) else {
        return auth_error_response(AdminAuthError::Unauthorized);
    };
    let Some(max_age) = access_token_max_age(token) else {
        return auth_error_response(AdminAuthError::Unauthorized);
    };
    if let Err(error) = authenticate_new_api_token(token, &config).await {
        return auth_error_response(error);
    }

    let secure = if config.external_auth_cookie_secure {
        "; Secure"
    } else {
        ""
    };
    let cookie = format!(
        "{EXTERNAL_AUTH_COOKIE}={token}; Path=/ai-bridge; Max-Age={max_age}; HttpOnly; SameSite=Strict{secure}"
    );
    let Ok(value) = HeaderValue::from_str(&cookie) else {
        return auth_error_response(AdminAuthError::Unauthorized);
    };
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(header::SET_COOKIE, value);
    hardened(response)
}

async fn authenticate(
    headers: &HeaderMap,
    config: &Config,
) -> Result<session::AdminUser, AdminAuthError> {
    if let Some(token) = bearer_token(headers) {
        return authenticate_new_api_token(token, config).await;
    }
    if let Some(token) = external_auth_cookie(headers) {
        return authenticate_new_api_token(token, config).await;
    }
    authenticate_with_secret(headers, config.admin_session_secret.as_deref())
}

async fn authenticate_new_api_token(
    token: &str,
    config: &Config,
) -> Result<session::AdminUser, AdminAuthError> {
    let url = config
        .upstream_base_url
        .join("/api/user/self")
        .map_err(|_| AdminAuthError::Unavailable)?;
    let response = reqwest::Client::new()
        .get(url)
        .bearer_auth(token)
        .header("user-agent", "ai-bridge-admin-auth")
        .send()
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to reach new-api auth endpoint");
            AdminAuthError::Unavailable
        })?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(AdminAuthError::Unauthorized);
    }
    if response.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(AdminAuthError::Forbidden);
    }
    if !response.status().is_success() {
        tracing::error!(status = %response.status(), "new-api auth endpoint returned an error");
        return Err(AdminAuthError::Unavailable);
    }
    let body = response.json::<ApiSelfResponse>().await.map_err(|error| {
        tracing::error!(%error, "failed to parse new-api auth response");
        AdminAuthError::Unavailable
    })?;
    if !body.success {
        return Err(AdminAuthError::Unauthorized);
    }
    let user = body.data.ok_or(AdminAuthError::Unauthorized)?;
    require_admin(session::AdminUser {
        id: user.id,
        username: user.username,
        role: user.role,
    })
}

async fn authorize_mutation(
    headers: &HeaderMap,
    config: &Config,
) -> Result<session::AdminUser, AdminAuthError> {
    let user = authenticate(headers, config).await?;
    if headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value != "same-origin")
    {
        return Err(AdminAuthError::Forbidden);
    }
    let secret = config
        .admin_session_secret
        .as_deref()
        .ok_or(AdminAuthError::Unavailable)?;
    let cookie = auth_cookie(headers).ok_or(AdminAuthError::Unauthorized)?;
    let provided = headers
        .get("x-ai-bridge-csrf")
        .and_then(|value| value.to_str().ok())
        .ok_or(AdminAuthError::Forbidden)?;
    verify_csrf_token(cookie, secret, provided)
        .then_some(())
        .ok_or(AdminAuthError::Forbidden)?;
    Ok(user)
}

fn csrf_token_for_headers(headers: &HeaderMap, config: &Config) -> Option<String> {
    let secret = config.admin_session_secret.as_deref()?;
    let cookie = auth_cookie(headers)?;
    build_csrf_token(cookie, secret)
}

fn build_csrf_token(cookie: &str, secret: &str) -> Option<String> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).ok()?;
    mac.update(b"ai-bridge-admin-csrf\0");
    mac.update(cookie.as_bytes());
    Some(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

fn verify_csrf_token(cookie: &str, secret: &str, provided: &str) -> bool {
    let Ok(provided) = URL_SAFE_NO_PAD.decode(provided) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(b"ai-bridge-admin-csrf\0");
    mac.update(cookie.as_bytes());
    mac.verify_slice(&provided).is_ok()
}

fn authenticate_with_secret(
    headers: &HeaderMap,
    secret: Option<&str>,
) -> Result<session::AdminUser, AdminAuthError> {
    let secret = secret.ok_or(AdminAuthError::Unavailable)?;
    let cookie = session_cookie(headers).ok_or(AdminAuthError::Unauthorized)?;
    let user =
        session::decode(cookie, secret.as_bytes()).map_err(|_| AdminAuthError::Unauthorized)?;
    require_admin(user)
}

fn require_admin(user: session::AdminUser) -> Result<session::AdminUser, AdminAuthError> {
    user.is_admin()
        .then_some(user)
        .ok_or(AdminAuthError::Forbidden)
}

fn session_cookie(headers: &HeaderMap) -> Option<&str> {
    headers.get_all(header::COOKIE).iter().find_map(|value| {
        value.to_str().ok()?.split(';').find_map(|item| {
            let (name, value) = item.trim().split_once('=')?;
            (name.trim() == "session").then_some(value.trim())
        })
    })
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty()).then(|| token.trim())
}

fn external_auth_cookie(headers: &HeaderMap) -> Option<&str> {
    named_cookie(headers, EXTERNAL_AUTH_COOKIE)
}

fn auth_cookie(headers: &HeaderMap) -> Option<&str> {
    external_auth_cookie(headers).or_else(|| session_cookie(headers))
}

fn named_cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get_all(header::COOKIE).iter().find_map(|value| {
        value.to_str().ok()?.split(';').find_map(|item| {
            let (cookie_name, value) = item.trim().split_once('=')?;
            (cookie_name.trim() == name).then_some(value.trim())
        })
    })
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
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs()
        .min(i64::MAX as u64) as i64;
    let remaining = claims.exp.checked_sub(now)?;
    (remaining > 0).then(|| remaining.min(MAX_EXTERNAL_AUTH_AGE_SECONDS))
}

fn redirect_or_error(config: &Config) -> Response {
    let login_url = if config.admin_login_url.contains("redirect=") {
        config.admin_login_url.clone()
    } else {
        let separator = if config.admin_login_url.contains('?') {
            '&'
        } else {
            '?'
        };
        format!("{}{separator}redirect=%2Fai-bridge", config.admin_login_url)
    };
    if HeaderValue::from_str(&login_url).is_ok() {
        return hardened(Redirect::temporary(&login_url).into_response());
    }
    auth_error_response(AdminAuthError::Unauthorized)
}

fn auth_error_response(error: AdminAuthError) -> Response {
    let (status, code, message) = match error {
        AdminAuthError::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "admin_console_unavailable",
            "The administrator console is not configured.",
        ),
        AdminAuthError::Unauthorized => (
            StatusCode::UNAUTHORIZED,
            "admin_auth_required",
            "Administrator authentication is required.",
        ),
        AdminAuthError::Forbidden => (
            StatusCode::FORBIDDEN,
            "admin_access_forbidden",
            "This account does not have administrator access.",
        ),
    };
    hardened(
        (
            status,
            Json(json!({ "error": { "code": code, "message": message } })),
        )
            .into_response(),
    )
}

fn hardened(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    headers.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(CONTENT_SECURITY_POLICY),
    );
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );
    headers.insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_session_cookie_without_truncating_base64_padding() {
        let headers = HeaderMap::from_iter([(
            header::COOKIE,
            HeaderValue::from_static("theme=dark; session=abc==; other=value"),
        )]);
        assert_eq!(session_cookie(&headers), Some("abc=="));
    }

    #[test]
    fn hardened_responses_disable_caching_and_framing() {
        let response = auth_error_response(AdminAuthError::Unauthorized);
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()["x-frame-options"], "DENY");
        assert!(
            response.headers()["content-security-policy"]
                .to_str()
                .unwrap()
                .contains("frame-ancestors 'none'")
        );
    }

    #[test]
    fn authentication_fails_closed_without_secret_or_session() {
        assert_eq!(
            authenticate_with_secret(&HeaderMap::new(), None),
            Err(AdminAuthError::Unavailable)
        );
        assert_eq!(
            authenticate_with_secret(&HeaderMap::new(), Some("configured")),
            Err(AdminAuthError::Unauthorized)
        );
    }

    #[test]
    fn authenticated_non_admin_is_forbidden() {
        let user = session::AdminUser {
            id: 7,
            username: "member".to_owned(),
            role: 1,
        };
        assert_eq!(require_admin(user), Err(AdminAuthError::Forbidden));
    }

    #[test]
    fn embedded_assets_use_external_same_origin_files() {
        assert!(INDEX_HTML.contains("href=\"/ai-bridge/admin.css\""));
        assert!(INDEX_HTML.contains("src=\"/ai-bridge/admin.js\""));
        assert!(INDEX_HTML.contains("href=\"/ai-bridge/favicon.svg\""));
        assert!(!INDEX_HTML.contains("<style"));
        assert!(!INDEX_HTML.contains("<script>"));
        assert!(ADMIN_JS.contains("/ai-bridge/api/snapshot"));
        assert!(ADMIN_JS.contains("textContent"));
        assert!(!ADMIN_JS.contains("innerHTML"));
    }

    #[test]
    fn csrf_tokens_are_bound_to_session_cookie_and_secret() {
        let token = build_csrf_token("session-value", "shared-secret").unwrap();
        assert!(verify_csrf_token("session-value", "shared-secret", &token));
        assert!(!verify_csrf_token("other-session", "shared-secret", &token));
        assert!(!verify_csrf_token("session-value", "other-secret", &token));
        assert!(!verify_csrf_token(
            "session-value",
            "shared-secret",
            "not-base64"
        ));
    }

    fn token_with_payload(payload: serde_json::Value) -> String {
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
        format!("header.{payload}.signature")
    }

    #[test]
    fn accepts_and_caps_current_dashboard_access_tokens() {
        let exp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 3_600;
        let token = token_with_payload(serde_json::json!({
            "exp": exp,
            "iss": "new-api",
            "aud": ["new-api-dashboard", "other"],
            "token_use": "access"
        }));

        assert_eq!(
            access_token_max_age(&token),
            Some(MAX_EXTERNAL_AUTH_AGE_SECONDS)
        );
    }

    #[test]
    fn rejects_expired_or_wrong_issuer_dashboard_tokens() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        for payload in [
            serde_json::json!({
                "exp": now - 1,
                "iss": "new-api",
                "aud": "new-api-dashboard",
                "token_use": "access"
            }),
            serde_json::json!({
                "exp": now + 60,
                "iss": "other",
                "aud": "new-api-dashboard",
                "token_use": "access"
            }),
        ] {
            assert_eq!(access_token_max_age(&token_with_payload(payload)), None);
        }
    }

    #[test]
    fn prefers_external_auth_cookie_for_csrf_binding() {
        let headers = HeaderMap::from_iter([(
            header::COOKIE,
            HeaderValue::from_static("session=legacy; new_api_external_auth=dashboard.jwt"),
        )]);

        assert_eq!(external_auth_cookie(&headers), Some("dashboard.jwt"));
        assert_eq!(auth_cookie(&headers), Some("dashboard.jwt"));
    }
}
