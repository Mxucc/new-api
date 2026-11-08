use chrono::Utc;
use jsonwebtoken::encode;
use serde::{Deserialize, Serialize};

use super::keys::KeyManager;

#[derive(Debug, Serialize, Deserialize)]
pub struct IdTokenClaims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub exp: i64,
    pub iat: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_verified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_username: Option<String>,
}

pub struct IdTokenParams<'a> {
    pub issuer: &'a str,
    pub client_id: &'a str,
    pub user_id: i64,
    pub username: &'a str,
    pub display_name: &'a str,
    pub email: &'a str,
    pub nonce: Option<&'a str>,
    pub expires_in: i64,
}

pub fn generate_id_token(
    key_manager: &KeyManager,
    params: IdTokenParams,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = Utc::now().timestamp();
    let exp = now + params.expires_in;

    let name = if params.display_name.is_empty() {
        None
    } else {
        Some(params.display_name.to_string())
    };

    let (email_claim, email_verified) = if params.email.is_empty() {
        (None, None)
    } else {
        (Some(params.email.to_string()), Some(true))
    };

    let claims = IdTokenClaims {
        iss: params.issuer.to_string(),
        sub: params.user_id.to_string(),
        aud: params.client_id.to_string(),
        exp,
        iat: now,
        nonce: params.nonce.map(|s| s.to_string()),
        email: email_claim,
        email_verified,
        name,
        preferred_username: Some(params.username.to_string()),
    };

    let header = key_manager.header();
    encode(&header, &claims, key_manager.encoding_key())
}
