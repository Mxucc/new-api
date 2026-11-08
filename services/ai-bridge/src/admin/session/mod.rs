mod gob;

use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine, engine::general_purpose::URL_SAFE};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use self::gob::GobValue;

const SESSION_MAX_AGE_SECS: i64 = 30 * 24 * 60 * 60;
const MAX_COOKIE_BYTES: usize = 16 * 1024;
const MAX_FUTURE_SKEW_SECS: i64 = 60;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AdminUser {
    pub id: i64,
    pub username: String,
    pub role: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionError {
    Invalid,
    Expired,
}

pub(super) fn decode(cookie_value: &str, secret: &[u8]) -> Result<AdminUser, SessionError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SessionError::Invalid)?
        .as_secs()
        .min(i64::MAX as u64) as i64;
    decode_at(cookie_value, secret, now)
}

fn decode_at(cookie_value: &str, secret: &[u8], now: i64) -> Result<AdminUser, SessionError> {
    if cookie_value.len() > MAX_COOKIE_BYTES || secret.is_empty() {
        return Err(SessionError::Invalid);
    }

    let decoded = URL_SAFE
        .decode(cookie_value)
        .map_err(|_| SessionError::Invalid)?;
    let mut parts = decoded.splitn(3, |byte| *byte == b'|');
    let timestamp_bytes = parts.next().ok_or(SessionError::Invalid)?;
    let value = parts.next().ok_or(SessionError::Invalid)?;
    let supplied_mac = parts.next().ok_or(SessionError::Invalid)?;

    let mut message = Vec::with_capacity(8 + timestamp_bytes.len() + value.len());
    message.extend_from_slice(b"session|");
    message.extend_from_slice(timestamp_bytes);
    message.push(b'|');
    message.extend_from_slice(value);

    let mut mac = HmacSha256::new_from_slice(secret).map_err(|_| SessionError::Invalid)?;
    mac.update(&message);
    mac.verify_slice(supplied_mac)
        .map_err(|_| SessionError::Invalid)?;

    let timestamp = std::str::from_utf8(timestamp_bytes)
        .map_err(|_| SessionError::Invalid)?
        .parse::<i64>()
        .map_err(|_| SessionError::Invalid)?;
    if timestamp < now.saturating_sub(SESSION_MAX_AGE_SECS) {
        return Err(SessionError::Expired);
    }
    if timestamp > now.saturating_add(MAX_FUTURE_SKEW_SECS) {
        return Err(SessionError::Invalid);
    }

    let gob_bytes = URL_SAFE.decode(value).map_err(|_| SessionError::Invalid)?;
    if gob_bytes.len() > MAX_COOKIE_BYTES {
        return Err(SessionError::Invalid);
    }
    let map = gob::decode_session_map(&gob_bytes).map_err(|_| SessionError::Invalid)?;
    AdminUser::from_map(&map).ok_or(SessionError::Invalid)
}

impl AdminUser {
    pub fn is_admin(&self) -> bool {
        self.role >= 10
    }

    fn from_map(map: &HashMap<String, GobValue>) -> Option<Self> {
        let GobValue::Int(id) = map.get("id")? else {
            return None;
        };
        let GobValue::String(username) = map.get("username")? else {
            return None;
        };
        let GobValue::Int(role) = map.get("role")? else {
            return None;
        };
        let GobValue::Int(status) = map.get("status")? else {
            return None;
        };
        let GobValue::String(group) = map.get("group")? else {
            return None;
        };

        if *id <= 0
            || *status != 1
            || username.is_empty()
            || username.len() > 256
            || group.len() > 256
        {
            return None;
        }

        Some(Self {
            id: *id,
            username: username.clone(),
            role: *role,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"test-session-secret-123";
    const ADMIN_COOKIE: &str = "MTc3OTcyNDM3NnxEWDhFQVFMX2dBQUJFQUVRQUFEX2tQLUFBQVVHYzNSeWFXNW5EQVFBQW1sa0EybHVkQVFDQUFJR2MzUnlhVzVuREFvQUNIVnpaWEp1WVcxbEJuTjBjbWx1Wnd3SEFBVmhaRzFwYmdaemRISnBibWNNQmdBRWNtOXNaUU5wYm5RRUJBRC1COUFHYzNSeWFXNW5EQWdBQm5OMFlYUjFjd05wYm5RRUFnQUNCbk4wY21sdVp3d0hBQVZuY205MWNBWnpkSEpwYm1jTUNRQUhaR1ZtWVhWc2RBPT18JrOMCNzFyThCpUE0KIGpsvgE4HDxKjtQuWdXg_EN3jg=";

    #[test]
    fn decodes_known_new_api_admin_cookie() {
        let user = decode_at(ADMIN_COOKIE, SECRET, 1_779_724_376).unwrap();
        assert_eq!(user.id, 1);
        assert_eq!(user.username, "admin");
        assert_eq!(user.role, 1000);
        assert!(user.is_admin());
    }

    #[test]
    fn rejects_wrong_secret_expired_and_future_cookies() {
        assert_eq!(
            decode_at(ADMIN_COOKIE, b"wrong", 1_779_724_376),
            Err(SessionError::Invalid)
        );
        assert_eq!(
            decode_at(ADMIN_COOKIE, SECRET, 1_900_000_000),
            Err(SessionError::Expired)
        );
        assert_eq!(
            decode_at(ADMIN_COOKIE, SECRET, 1_700_000_000),
            Err(SessionError::Invalid)
        );
    }

    #[test]
    fn rejects_oversized_cookie_before_decoding() {
        let oversized = "a".repeat(MAX_COOKIE_BYTES + 1);
        assert_eq!(decode_at(&oversized, SECRET, 0), Err(SessionError::Invalid));
    }
}
