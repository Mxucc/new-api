use std::{
    borrow::Borrow,
    collections::VecDeque,
    hash::Hash,
    net::IpAddr,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    body::Body,
    http::{HeaderMap, Response, StatusCode, header},
};
use bytes::Bytes;
use dashmap::DashMap;
use serde::Serialize;
use serde_json::{Map, Value, json};
use tracing::warn;

use crate::{
    auth::{ApiKeyInfo, admin_identity_fingerprint, admin_identity_id},
    config::{AuthFailurePolicy, Config},
};

pub(crate) const AUTH_FAILURE_CLASSIFICATION_BODY_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub struct AuthFailureTracker {
    key_entries: DashMap<String, FailureState>,
    ip_entries: DashMap<IpAddr, FailureState>,
    key_admission: Mutex<()>,
    ip_admission: Mutex<()>,
    last_cleanup_millis: AtomicU64,
    rejected_key_admissions: AtomicU64,
    rejected_ip_admissions: AtomicU64,
}

#[derive(Debug)]
struct FailureState {
    failures: VecDeque<u64>,
    cooldown_until_millis: u64,
    cached_error: Option<CachedError>,
}

#[derive(Debug, Clone)]
struct CachedError {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CooldownScope {
    ApiKey,
    ClientIp,
}

#[derive(Debug)]
pub struct CooldownHit {
    scope: CooldownScope,
    remaining: Duration,
    error: CachedError,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RecordOutcome {
    pub counted: bool,
    pub key_cooldown_opened: bool,
    pub ip_cooldown_opened: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct AdminAuthFailureSnapshot {
    pub total: usize,
    pub truncated: bool,
    pub cooldowns: Vec<AdminAuthCooldown>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct AdminAuthCooldown {
    pub id: String,
    pub scope: &'static str,
    pub identifier: String,
    pub remaining_secs: u64,
    pub status: u16,
    pub reason: &'static str,
}

impl AuthFailureTracker {
    pub fn new() -> Self {
        Self {
            key_entries: DashMap::new(),
            ip_entries: DashMap::new(),
            key_admission: Mutex::new(()),
            ip_admission: Mutex::new(()),
            last_cleanup_millis: AtomicU64::new(0),
            rejected_key_admissions: AtomicU64::new(0),
            rejected_ip_admissions: AtomicU64::new(0),
        }
    }

    pub fn check(
        &self,
        api_key: &ApiKeyInfo,
        client_ip: IpAddr,
        config: &Config,
    ) -> Option<CooldownHit> {
        if !config.auth_failure_cooldown_enabled
            || config.auth_failure_policy == AuthFailurePolicy::Off
            || !api_key.is_present()
        {
            return None;
        }

        let now = now_millis();
        self.maybe_cleanup(config, now);

        if let Some(hit) = check_map_entry(
            &self.key_entries,
            api_key.identity(),
            CooldownScope::ApiKey,
            now,
        ) {
            return Some(hit);
        }

        check_map_entry(&self.ip_entries, &client_ip, CooldownScope::ClientIp, now)
    }

    pub fn record_upstream_response(
        &self,
        api_key: &ApiKeyInfo,
        client_ip: IpAddr,
        upstream_status: u16,
        upstream_headers: &HeaderMap,
        upstream_body: &[u8],
        config: &Config,
    ) -> RecordOutcome {
        if !config.auth_failure_cooldown_enabled
            || config.auth_failure_policy == AuthFailurePolicy::Off
            || !api_key.is_present()
            || !upstream_response_counts_as_auth_failure_with_policy(
                upstream_status,
                upstream_body,
                config.auth_failure_policy,
            )
        {
            return RecordOutcome::default();
        }

        let now = now_millis();
        self.maybe_cleanup(config, now);
        let window_millis = duration_millis(config.auth_failure_window);
        let cooldown_millis = duration_millis(config.auth_failure_cooldown);
        let max_entries = config.auth_failure_max_entries;
        let key_error = CachedError::from_upstream(
            upstream_status,
            upstream_headers,
            upstream_body,
            config.auth_failure_error_body_max_bytes,
        );

        let ip_cooldown_opened = if config.auth_failure_ip_threshold == 0 {
            false
        } else {
            record_failure(
                &self.ip_entries,
                &self.ip_admission,
                client_ip,
                config.auth_failure_ip_threshold,
                max_entries.div_ceil(4).max(1),
                window_millis,
                cooldown_millis,
                now,
                CachedError::generic_forbidden(),
                &self.rejected_ip_admissions,
                "client_ip",
            )
        };

        let key_cooldown_opened = record_failure(
            &self.key_entries,
            &self.key_admission,
            api_key.identity().to_owned(),
            config.auth_failure_threshold,
            max_entries,
            window_millis,
            cooldown_millis,
            now,
            key_error,
            &self.rejected_key_admissions,
            "api_key",
        );

        RecordOutcome {
            counted: true,
            key_cooldown_opened,
            ip_cooldown_opened,
        }
    }

    pub(crate) fn admin_snapshot(&self, limit: usize) -> AdminAuthFailureSnapshot {
        self.admin_snapshot_at(limit, now_millis())
    }

    fn admin_snapshot_at(&self, limit: usize, now: u64) -> AdminAuthFailureSnapshot {
        let mut cooldowns = Vec::new();
        for entry in &self.key_entries {
            if entry.cooldown_until_millis > now {
                cooldowns.push(AdminAuthCooldown {
                    id: admin_identity_id(entry.key()),
                    scope: CooldownScope::ApiKey.as_str(),
                    identifier: admin_identity_fingerprint(entry.key()),
                    remaining_secs: remaining_secs(now, entry.cooldown_until_millis),
                    status: entry
                        .cached_error
                        .as_ref()
                        .map_or(StatusCode::FORBIDDEN.as_u16(), |error| {
                            error.status.as_u16()
                        }),
                    reason: "repeated_upstream_auth_failure",
                });
            }
        }
        for entry in &self.ip_entries {
            if entry.cooldown_until_millis > now {
                cooldowns.push(AdminAuthCooldown {
                    id: entry.key().to_string(),
                    scope: CooldownScope::ClientIp.as_str(),
                    identifier: entry.key().to_string(),
                    remaining_secs: remaining_secs(now, entry.cooldown_until_millis),
                    status: entry
                        .cached_error
                        .as_ref()
                        .map_or(StatusCode::FORBIDDEN.as_u16(), |error| {
                            error.status.as_u16()
                        }),
                    reason: "repeated_upstream_auth_failure",
                });
            }
        }

        cooldowns.sort_by(|left, right| {
            right
                .remaining_secs
                .cmp(&left.remaining_secs)
                .then_with(|| left.scope.cmp(right.scope))
                .then_with(|| left.identifier.cmp(&right.identifier))
        });
        let total = cooldowns.len();
        cooldowns.truncate(limit);
        AdminAuthFailureSnapshot {
            total,
            truncated: total > limit,
            cooldowns,
        }
    }

    pub(crate) fn clear_cooldown(&self, scope: &str, id: &str) -> bool {
        match scope {
            "api_key" => {
                for mut entry in self.key_entries.iter_mut() {
                    if admin_identity_id(entry.key()) == id {
                        return entry.clear_cooldown();
                    }
                }
                false
            }
            "client_ip" => id
                .parse::<IpAddr>()
                .ok()
                .and_then(|ip| self.ip_entries.get_mut(&ip))
                .is_some_and(|mut entry| entry.clear_cooldown()),
            _ => false,
        }
    }

    fn maybe_cleanup(&self, config: &Config, now: u64) {
        let interval = duration_millis(config.auth_failure_window).clamp(1, 60_000);
        let last = self.last_cleanup_millis.load(Ordering::Acquire);
        if now.saturating_sub(last) < interval {
            return;
        }
        if self
            .last_cleanup_millis
            .compare_exchange(last, now, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        cleanup_map(
            &self.key_entries,
            now,
            duration_millis(config.auth_failure_window),
        );
        cleanup_map(
            &self.ip_entries,
            now,
            duration_millis(config.auth_failure_window),
        );
    }

    #[cfg(test)]
    fn check_at(&self, api_key: &ApiKeyInfo, client_ip: IpAddr, now: u64) -> Option<CooldownHit> {
        check_map_entry(
            &self.key_entries,
            api_key.identity(),
            CooldownScope::ApiKey,
            now,
        )
        .or_else(|| check_map_entry(&self.ip_entries, &client_ip, CooldownScope::ClientIp, now))
    }

    #[cfg(test)]
    fn tracked_key_count(&self) -> usize {
        self.key_entries.len()
    }
}

impl Default for AuthFailureTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl CooldownHit {
    pub fn scope(&self) -> CooldownScope {
        self.scope
    }

    pub fn remaining(&self) -> Duration {
        self.remaining
    }

    pub fn status(&self) -> StatusCode {
        self.error.status
    }

    pub fn into_response(self) -> Response<Body> {
        let retry_after_secs = self.remaining.as_secs().max(1);
        let mut response = Response::new(Body::from(self.error.body));
        *response.status_mut() = self.error.status;
        *response.headers_mut() = self.error.headers;
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            "no-store".parse().expect("static header is valid"),
        );
        if let Ok(value) = retry_after_secs.to_string().parse() {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        response
    }
}

impl CooldownScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::ClientIp => "client_ip",
        }
    }
}

impl FailureState {
    fn new() -> Self {
        Self {
            failures: VecDeque::new(),
            cooldown_until_millis: 0,
            cached_error: None,
        }
    }

    fn record(
        &mut self,
        threshold: usize,
        window_millis: u64,
        cooldown_millis: u64,
        now: u64,
        error: CachedError,
    ) -> bool {
        if self.cooldown_until_millis > now {
            return false;
        }
        if self.cooldown_until_millis != 0 {
            self.cooldown_until_millis = 0;
            self.cached_error = None;
        }

        let window_start = now.saturating_sub(window_millis);
        while self
            .failures
            .front()
            .is_some_and(|timestamp| *timestamp <= window_start)
        {
            self.failures.pop_front();
        }
        self.failures.push_back(now);

        if self.failures.len() < threshold {
            return false;
        }

        self.failures.clear();
        self.cooldown_until_millis = now.saturating_add(cooldown_millis);
        self.cached_error = Some(error);
        true
    }

    fn clear_cooldown(&mut self) -> bool {
        let was_active = self.cooldown_until_millis > now_millis();
        self.failures.clear();
        self.cooldown_until_millis = 0;
        self.cached_error = None;
        was_active
    }
}

impl CachedError {
    fn from_upstream(
        status: u16,
        upstream_headers: &HeaderMap,
        upstream_body: &[u8],
        max_body_bytes: usize,
    ) -> Self {
        if upstream_body.is_empty() || upstream_body.len() > max_body_bytes {
            return Self::generic_for_status(status);
        }

        let mut headers = HeaderMap::new();
        for name in [header::CONTENT_TYPE, header::WWW_AUTHENTICATE] {
            if let Some(value) = upstream_headers.get(&name)
                && value.as_bytes().len() <= 1024
            {
                headers.insert(name, value.clone());
            }
        }
        headers.entry(header::CONTENT_TYPE).or_insert(
            "application/json; charset=utf-8"
                .parse()
                .expect("static header is valid"),
        );

        Self {
            status: StatusCode::from_u16(status).unwrap_or(StatusCode::FORBIDDEN),
            headers,
            body: Bytes::copy_from_slice(upstream_body),
        }
    }

    fn generic_forbidden() -> Self {
        Self::generic_for_status(StatusCode::FORBIDDEN.as_u16())
    }

    fn generic_for_status(status: u16) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            "application/json; charset=utf-8"
                .parse()
                .expect("static header is valid"),
        );
        Self {
            status: StatusCode::from_u16(status).unwrap_or(StatusCode::FORBIDDEN),
            headers,
            body: Bytes::from(
                json!({
                    "error": {
                        "message": "Too many repeated upstream authentication or authorization failures; this identity is temporarily cooling down.",
                        "type": "ai_bridge_error",
                        "code": "auth_failure_cooldown"
                    }
                })
                .to_string(),
            ),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn record_failure<K>(
    map: &DashMap<K, FailureState>,
    admission: &Mutex<()>,
    key: K,
    threshold: usize,
    max_entries: usize,
    window_millis: u64,
    cooldown_millis: u64,
    now: u64,
    error: CachedError,
    rejected_admissions: &AtomicU64,
    scope: &'static str,
) -> bool
where
    K: Eq + Hash + Clone,
{
    if let Some(mut state) = map.get_mut(&key) {
        return state.record(threshold, window_millis, cooldown_millis, now, error);
    }

    let _guard = admission
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(mut state) = map.get_mut(&key) {
        return state.record(threshold, window_millis, cooldown_millis, now, error);
    }

    if map.len() >= max_entries {
        cleanup_map(map, now, window_millis);
    }
    if map.len() >= max_entries {
        let rejected = rejected_admissions.fetch_add(1, Ordering::Relaxed) + 1;
        if rejected.is_power_of_two() {
            warn!(
                event = "auth_failure_tracker_saturated",
                scope,
                max_entries,
                rejected_admissions = rejected,
                "auth-failure tracker is saturated; refusing a new state entry"
            );
        }
        return false;
    }

    map.insert(key.clone(), FailureState::new());
    map.get_mut(&key).is_some_and(|mut state| {
        state.record(threshold, window_millis, cooldown_millis, now, error)
    })
}

fn check_map_entry<K, Q>(
    map: &DashMap<K, FailureState>,
    key: &Q,
    scope: CooldownScope,
    now: u64,
) -> Option<CooldownHit>
where
    K: Eq + Hash + Borrow<Q>,
    Q: Eq + Hash + ?Sized,
{
    let state = map.get(key)?;
    if state.cooldown_until_millis <= now {
        return None;
    }
    let error = state
        .cached_error
        .clone()
        .unwrap_or_else(CachedError::generic_forbidden);
    Some(CooldownHit {
        scope,
        remaining: Duration::from_millis(state.cooldown_until_millis - now),
        error,
    })
}

fn cleanup_map<K>(map: &DashMap<K, FailureState>, now: u64, window_millis: u64)
where
    K: Eq + Hash,
{
    let window_start = now.saturating_sub(window_millis);
    map.retain(|_, state| {
        if state.cooldown_until_millis > now {
            return true;
        }
        while state
            .failures
            .front()
            .is_some_and(|timestamp| *timestamp <= window_start)
        {
            state.failures.pop_front();
        }
        !state.failures.is_empty()
    });
}

#[cfg(test)]
pub fn upstream_response_counts_as_auth_failure(status: u16, body: &[u8]) -> bool {
    upstream_response_counts_as_auth_failure_with_policy(status, body, AuthFailurePolicy::Balanced)
}

fn upstream_response_counts_as_auth_failure_with_policy(
    status: u16,
    body: &[u8],
    policy: AuthFailurePolicy,
) -> bool {
    if policy == AuthFailurePolicy::Off {
        return false;
    }
    if policy == AuthFailurePolicy::Strict {
        if !matches!(status, 401 | 403) || body.len() > AUTH_FAILURE_CLASSIFICATION_BODY_MAX_BYTES {
            return false;
        }
        let Ok(value) = serde_json::from_slice::<Value>(body) else {
            return false;
        };
        return new_api_error_object(&value).is_some();
    }

    // New API returns its own invalid-token message as 401 with an empty code.
    // A 403 covers many valid-token restrictions, so message-only evidence is unsafe there.
    let allow_message_only = match status {
        401 => true,
        403 if policy == AuthFailurePolicy::Balanced => false,
        _ => return false,
    };

    body_is_explicit_customer_credential_error(body, allow_message_only)
}

fn body_is_explicit_customer_credential_error(body: &[u8], allow_message_only: bool) -> bool {
    if body.len() > AUTH_FAILURE_CLASSIFICATION_BODY_MAX_BYTES {
        return false;
    }
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    let Some(error) = new_api_error_object(&value) else {
        return false;
    };

    error
        .get("code")
        .and_then(Value::as_str)
        .is_some_and(is_credential_failure_code)
        || allow_message_only
            && error
                .get("message")
                .and_then(Value::as_str)
                .is_some_and(is_credential_failure_message)
}

fn new_api_error_object(value: &Value) -> Option<&Map<String, Value>> {
    let root = value.as_object()?;
    let error = root.get("error").and_then(Value::as_object).unwrap_or(root);
    error
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|error_type| error_type.eq_ignore_ascii_case("new_api_error"))
        .then_some(error)
}

fn is_credential_failure_code(code: &str) -> bool {
    matches!(
        code.trim().to_ascii_lowercase().as_str(),
        "invalid_api_key"
            | "invalid_token"
            | "token_invalid"
            | "authentication_error"
            | "authentication_failed"
            | "expired_token"
            | "token_expired"
    )
}

fn is_credential_failure_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("invalid token")
        || message.contains("token invalid")
        || message.contains("invalid api key")
        || message.contains("api key invalid")
        || message.contains("incorrect api key")
        || message.contains("authentication failed")
        || message.contains("token expired")
        || message.contains("expired token")
        || message.contains("无效的令牌")
        || message.contains("無效的令牌")
        || message.contains("api key 无效")
        || message.contains("api key 無效")
        || message.contains("鉴权失败")
        || message.contains("鑑權失敗")
        || message.contains("认证失败")
        || message.contains("認證失敗")
        || message.contains("令牌已过期")
        || message.contains("令牌已過期")
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn remaining_secs(now: u64, until: u64) -> u64 {
    until.saturating_sub(now).saturating_add(999) / 1_000
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    use axum::http::{HeaderValue, header};

    use super::*;

    fn api_key(value: &'static str) -> ApiKeyInfo {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static(value));
        ApiKeyInfo::from_headers(&headers)
    }

    fn record_at(
        tracker: &AuthFailureTracker,
        key: &ApiKeyInfo,
        ip: IpAddr,
        now: u64,
        threshold: usize,
        ip_threshold: usize,
        max_entries: usize,
    ) -> RecordOutcome {
        let error = CachedError::from_upstream(
            403,
            &HeaderMap::new(),
            br#"{"error":{"type":"new_api_error","message":"forbidden"}}"#,
            8192,
        );
        let ip_opened = if ip_threshold == 0 {
            false
        } else {
            record_failure(
                &tracker.ip_entries,
                &tracker.ip_admission,
                ip,
                ip_threshold,
                max_entries,
                60_000,
                1_800_000,
                now,
                CachedError::generic_forbidden(),
                &tracker.rejected_ip_admissions,
                "client_ip",
            )
        };
        let key_opened = record_failure(
            &tracker.key_entries,
            &tracker.key_admission,
            key.identity().to_owned(),
            threshold,
            max_entries,
            60_000,
            1_800_000,
            now,
            error,
            &tracker.rejected_key_admissions,
            "api_key",
        );
        RecordOutcome {
            counted: true,
            key_cooldown_opened: key_opened,
            ip_cooldown_opened: ip_opened,
        }
    }

    #[test]
    fn auth_failure_fifth_failure_opens_key_cooldown() {
        let tracker = AuthFailureTracker::new();
        let key = api_key("Bearer sk-auth-failure");
        let ip = "198.51.100.10".parse().unwrap();

        for offset in 0..4 {
            assert!(!record_at(&tracker, &key, ip, 1_000 + offset, 5, 0, 100).key_cooldown_opened);
        }
        assert!(record_at(&tracker, &key, ip, 1_004, 5, 0, 100).key_cooldown_opened);

        let hit = tracker.check_at(&key, ip, 1_005).unwrap();
        assert_eq!(hit.scope(), CooldownScope::ApiKey);
        assert_eq!(hit.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn auth_failure_window_expires_old_failures() {
        let tracker = AuthFailureTracker::new();
        let key = api_key("Bearer sk-window");
        let ip = "198.51.100.11".parse().unwrap();

        for offset in 0..4 {
            record_at(&tracker, &key, ip, 1_000 + offset, 5, 0, 100);
        }
        assert!(!record_at(&tracker, &key, ip, 61_001, 5, 0, 100).key_cooldown_opened);
        assert!(tracker.check_at(&key, ip, 61_002).is_none());
    }

    #[test]
    fn auth_failure_cooldown_expires() {
        let tracker = AuthFailureTracker::new();
        let key = api_key("Bearer sk-expiry");
        let ip = "198.51.100.12".parse().unwrap();
        for offset in 0..5 {
            record_at(&tracker, &key, ip, 1_000 + offset, 5, 0, 100);
        }

        assert!(tracker.check_at(&key, ip, 1_800_999).is_some());
        assert!(tracker.check_at(&key, ip, 1_801_004).is_none());
    }

    #[test]
    fn auth_failure_tracker_bounds_random_key_cardinality() {
        let tracker = AuthFailureTracker::new();
        let ip = "198.51.100.13".parse().unwrap();
        for index in 0..20 {
            let header = format!("Bearer sk-random-{index}");
            let mut headers = HeaderMap::new();
            headers.insert(header::AUTHORIZATION, header.parse().unwrap());
            let key = ApiKeyInfo::from_headers(&headers);
            record_at(&tracker, &key, ip, 1_000, 5, 0, 4);
        }
        assert_eq!(tracker.tracked_key_count(), 4);
    }

    #[test]
    fn auth_failure_ip_threshold_blocks_rotating_keys() {
        let tracker = AuthFailureTracker::new();
        let ip = "198.51.100.14".parse().unwrap();
        for index in 0..3 {
            let header = format!("Bearer sk-rotate-{index}");
            let mut headers = HeaderMap::new();
            headers.insert(header::AUTHORIZATION, header.parse().unwrap());
            let key = ApiKeyInfo::from_headers(&headers);
            record_at(&tracker, &key, ip, 1_000 + index, 5, 3, 100);
        }
        let unseen_key = api_key("Bearer sk-unseen");
        let hit = tracker.check_at(&unseen_key, ip, 1_004).unwrap();
        assert_eq!(hit.scope(), CooldownScope::ClientIp);
    }

    #[test]
    fn auth_failure_concurrent_fifth_failure_opens_one_coherent_cooldown() {
        let tracker = Arc::new(AuthFailureTracker::new());
        let key = Arc::new(api_key("Bearer sk-concurrent"));
        let ip: IpAddr = "198.51.100.15".parse().unwrap();
        let barrier = Arc::new(Barrier::new(6));

        let opened = std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for _ in 0..5 {
                let tracker = tracker.clone();
                let key = key.clone();
                let barrier = barrier.clone();
                handles.push(scope.spawn(move || {
                    barrier.wait();
                    record_at(&tracker, &key, ip, 1_000, 5, 0, 100).key_cooldown_opened
                }));
            }
            barrier.wait();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .filter(|opened| *opened)
                .count()
        });

        assert_eq!(opened, 1);
        assert!(tracker.check_at(&key, ip, 1_001).is_some());
    }

    #[test]
    fn auth_failure_different_key_is_not_cooled() {
        let tracker = AuthFailureTracker::new();
        let blocked_key = api_key("Bearer sk-blocked");
        let other_key = api_key("Bearer sk-other");
        let ip: IpAddr = "198.51.100.16".parse().unwrap();
        for offset in 0..5 {
            record_at(&tracker, &blocked_key, ip, 1_000 + offset, 5, 0, 100);
        }

        assert!(tracker.check_at(&blocked_key, ip, 1_006).is_some());
        assert!(tracker.check_at(&other_key, ip, 1_006).is_none());
    }

    #[test]
    fn auth_failure_classification_excludes_noncredential_403_errors() {
        for body in [
            br#"{"error":{"type":"new_api_error","code":"insufficient_user_quota"}}"#.as_slice(),
            br#"{"error":{"code":"cyber_policy"}}"#.as_slice(),
            br#"{"error":{"code":"channel:invalid_key"}}"#.as_slice(),
            br#"{"error":{"type":"new_api_error","code":"token_no_model_access","message":"This token has no access to model gpt-x"}}"#.as_slice(),
            br#"{"error":{"type":"new_api_error","code":"access_denied","message":"Unauthorized"}}"#.as_slice(),
            br#"{"error":{"type":"new_api_error","code":"access_denied","message":"model denied","details":{"code":"invalid_token"}}}"#.as_slice(),
            r#"{"error":{"type":"new_api_error","code":"","message":"您的 IP 不在令牌允许访问的列表中"}}"#.as_bytes(),
            r#"{"error":{"type":"new_api_error","code":"","message":"无权访问 default 分组"}}"#.as_bytes(),
            r#"{"error":{"type":"new_api_error","code":"","message":"API Key 无效或无权限"}}"#.as_bytes(),
        ] {
            assert!(!upstream_response_counts_as_auth_failure(403, body));
        }
    }

    #[test]
    fn auth_failure_classification_counts_explicit_customer_invalid_token() {
        let customer_en =
            br#"{"error":{"type":"new_api_error","code":"","message":"Invalid token"}}"#;
        let customer_zh =
            r#"{"error":{"type":"new_api_error","code":"","message":"无效的令牌"}}"#.as_bytes();
        let compatible_403 = br#"{"error":{"type":"new_api_error","code":"invalid_token","message":"credential rejected"}}"#;
        let message_only_403 =
            br#"{"error":{"type":"new_api_error","code":"","message":"Invalid token"}}"#;
        let channel = br#"{"error":{"type":"invalid_request_error","code":"invalid_api_key","message":"Incorrect API key provided"}}"#;

        assert!(upstream_response_counts_as_auth_failure(401, customer_en));
        assert!(upstream_response_counts_as_auth_failure(401, customer_zh));
        assert!(upstream_response_counts_as_auth_failure(
            403,
            compatible_403
        ));
        assert!(!upstream_response_counts_as_auth_failure(
            403,
            message_only_403
        ));
        assert!(!upstream_response_counts_as_auth_failure(401, channel));
        assert!(!upstream_response_counts_as_auth_failure(403, channel));
        assert!(!upstream_response_counts_as_auth_failure(429, customer_en));
    }

    #[test]
    fn auth_failure_policy_modes_trade_false_positives_for_coverage() {
        let explicit = br#"{"error":{"type":"new_api_error","code":"invalid_token"}}"#;
        let permission = br#"{"error":{"type":"new_api_error","code":"token_no_model_access"}}"#;

        assert!(!upstream_response_counts_as_auth_failure_with_policy(
            401,
            explicit,
            AuthFailurePolicy::Off,
        ));
        assert!(upstream_response_counts_as_auth_failure_with_policy(
            401,
            explicit,
            AuthFailurePolicy::Conservative,
        ));
        assert!(!upstream_response_counts_as_auth_failure_with_policy(
            403,
            explicit,
            AuthFailurePolicy::Conservative,
        ));
        assert!(upstream_response_counts_as_auth_failure_with_policy(
            403,
            explicit,
            AuthFailurePolicy::Balanced,
        ));
        assert!(!upstream_response_counts_as_auth_failure_with_policy(
            403,
            permission,
            AuthFailurePolicy::Balanced,
        ));
        assert!(upstream_response_counts_as_auth_failure_with_policy(
            403,
            permission,
            AuthFailurePolicy::Strict,
        ));
    }

    #[test]
    fn auth_failure_classification_rejects_unstructured_or_oversized_bodies() {
        let oversized = vec![b'x'; AUTH_FAILURE_CLASSIFICATION_BODY_MAX_BYTES + 1];

        for body in [
            b"".as_slice(),
            b"forbidden".as_slice(),
            br#"{"error":{"message":"Invalid token"}}"#.as_slice(),
            b"data: {\"error\":{\"type\":\"new_api_error\",\"message\":\"Invalid token\"}}\n\n"
                .as_slice(),
            oversized.as_slice(),
        ] {
            assert!(!upstream_response_counts_as_auth_failure(403, body));
        }
    }

    #[test]
    fn auth_failure_cached_body_is_replaced_when_oversized() {
        let cached = CachedError::from_upstream(403, &HeaderMap::new(), &[b'x'; 32], 8);
        assert!(String::from_utf8_lossy(&cached.body).contains("auth_failure_cooldown"));
    }

    #[test]
    fn auth_failure_cached_error_keeps_only_safe_response_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            "application/problem+json".parse().unwrap(),
        );
        headers.insert(
            header::WWW_AUTHENTICATE,
            "Bearer realm=\"new-api\"".parse().unwrap(),
        );
        headers.insert(header::SET_COOKIE, "session=secret".parse().unwrap());
        headers.insert("x-upstream-debug", "private".parse().unwrap());

        let cached = CachedError::from_upstream(401, &headers, br#"{"error":"invalid"}"#, 8192);

        assert_eq!(
            cached.headers.get(header::CONTENT_TYPE).unwrap(),
            "application/problem+json"
        );
        assert_eq!(
            cached.headers.get(header::WWW_AUTHENTICATE).unwrap(),
            "Bearer realm=\"new-api\""
        );
        assert!(!cached.headers.contains_key(header::SET_COOKIE));
        assert!(!cached.headers.contains_key("x-upstream-debug"));
    }

    #[test]
    fn admin_snapshot_exposes_active_cooldowns_without_key_or_cached_body() {
        let tracker = AuthFailureTracker::new();
        let key = api_key("Bearer sk-admin-snapshot-secret");
        let ip: IpAddr = "198.51.100.77".parse().unwrap();
        for offset in 0..5 {
            record_at(&tracker, &key, ip, 1_000 + offset, 5, 5, 100);
        }

        let snapshot = tracker.admin_snapshot_at(10, 1_005);
        assert_eq!(snapshot.total, 2);
        assert!(
            snapshot
                .cooldowns
                .iter()
                .any(|entry| entry.scope == "api_key")
        );
        assert!(
            snapshot
                .cooldowns
                .iter()
                .any(|entry| entry.identifier == ip.to_string())
        );
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(!json.contains("sk-admin-snapshot-secret"));
        assert!(!json.contains("forbidden"));
    }

    #[test]
    fn administrator_can_clear_key_and_ip_cooldowns_by_opaque_id() {
        let tracker = AuthFailureTracker::new();
        let key = api_key("Bearer sk-admin-clear-secret");
        let ip: IpAddr = "2001:db8::42".parse().unwrap();
        let now = now_millis();
        for offset in 0..5 {
            record_at(&tracker, &key, ip, now + offset, 5, 5, 100);
        }

        let snapshot = tracker.admin_snapshot(10);
        let key_entry = snapshot
            .cooldowns
            .iter()
            .find(|entry| entry.scope == "api_key")
            .unwrap();
        let ip_entry = snapshot
            .cooldowns
            .iter()
            .find(|entry| entry.scope == "client_ip")
            .unwrap();
        assert_eq!(key_entry.reason, "repeated_upstream_auth_failure");
        assert!(tracker.clear_cooldown("api_key", &key_entry.id));
        assert!(tracker.clear_cooldown("client_ip", &ip_entry.id));
        assert_eq!(tracker.admin_snapshot(10).total, 0);
        assert!(!tracker.clear_cooldown("api_key", &key_entry.id));
    }
}
