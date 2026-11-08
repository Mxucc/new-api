use std::env;

#[derive(Clone)]
pub struct Config {
    pub session_secret: String,
    pub database_url: String,
    pub listen_addr: String,
    pub login_url: String,
    pub base_path: String,
    pub api_base_url: String,
    pub external_auth_cookie_secure: bool,
    pub issuer_url: String,
    pub private_key_path: String,
    pub id_token_signing_alg: String,
    pub id_token_expires_in: i64,
    pub refresh_token_expires_in: i64,
}

impl Config {
    pub fn from_env() -> Self {
        let raw_base = env::var("BASE_PATH").unwrap_or_default();
        let base_path = raw_base.trim_end_matches('/').to_string();

        let issuer_url = env::var("ISSUER_URL").expect("ISSUER_URL must be set for OIDC support");

        Self {
            session_secret: env::var("SESSION_SECRET").expect("SESSION_SECRET must be set"),
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite:connect.db?mode=rwc".into()),
            listen_addr: env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:4000".into()),
            login_url: env::var("LOGIN_URL").unwrap_or_else(|_| "/sign-in".into()),
            base_path,
            api_base_url: env::var("API_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:3000".into())
                .trim_end_matches('/')
                .to_string(),
            external_auth_cookie_secure: env::var("EXTERNAL_AUTH_COOKIE_SECURE")
                .map(|value| {
                    !matches!(
                        value.trim().to_ascii_lowercase().as_str(),
                        "0" | "false" | "no" | "off"
                    )
                })
                .unwrap_or(true),
            issuer_url,
            private_key_path: env::var("PRIVATE_KEY_PATH")
                .unwrap_or_else(|_| "keys/private.pem".into()),
            id_token_signing_alg: env::var("ID_TOKEN_SIGNING_ALG")
                .unwrap_or_else(|_| "RS256".into()),
            id_token_expires_in: env::var("ID_TOKEN_EXPIRES_IN")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(900),
            refresh_token_expires_in: env::var("REFRESH_TOKEN_EXPIRES_IN")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2592000),
        }
    }
}
