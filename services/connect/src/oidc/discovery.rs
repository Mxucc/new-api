use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct DiscoveryResponse {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub userinfo_endpoint: String,
    pub jwks_uri: String,
    pub response_types_supported: Vec<String>,
    pub subject_types_supported: Vec<String>,
    pub id_token_signing_alg_values_supported: Vec<String>,
    pub scopes_supported: Vec<String>,
    pub token_endpoint_auth_methods_supported: Vec<String>,
    pub claims_supported: Vec<String>,
    pub code_challenge_methods_supported: Vec<String>,
}

pub async fn discovery(State(app): State<AppState>) -> Json<DiscoveryResponse> {
    let issuer = &app.config.issuer_url;
    let base = &app.config.base_path;

    Json(DiscoveryResponse {
        issuer: issuer.clone(),
        authorization_endpoint: format!("{}{}/authorize", issuer, base),
        token_endpoint: format!("{}{}/token", issuer, base),
        userinfo_endpoint: format!("{}{}/userinfo", issuer, base),
        jwks_uri: format!("{}{}/.well-known/jwks.json", issuer, base),
        response_types_supported: vec!["code".to_string()],
        subject_types_supported: vec!["public".to_string()],
        id_token_signing_alg_values_supported: vec![
            "RS256".to_string(),
            "ES256".to_string(),
            "PS256".to_string(),
        ],
        scopes_supported: vec![
            "openid".to_string(),
            "profile".to_string(),
            "email".to_string(),
        ],
        token_endpoint_auth_methods_supported: vec![
            "client_secret_post".to_string(),
            "client_secret_basic".to_string(),
            "none".to_string(),
        ],
        claims_supported: vec![
            "sub".to_string(),
            "iss".to_string(),
            "aud".to_string(),
            "exp".to_string(),
            "iat".to_string(),
            "name".to_string(),
            "preferred_username".to_string(),
            "email".to_string(),
            "email_verified".to_string(),
        ],
        code_challenge_methods_supported: vec!["S256".to_string(), "plain".to_string()],
    })
}
