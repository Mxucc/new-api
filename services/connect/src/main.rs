mod config;
mod db;
mod error;
mod middleware;
mod oidc;
mod routes;
mod session;

use axum::{
    Router,
    routing::{delete, get, post, put},
};
use sqlx::sqlite::SqlitePool;
use tower_http::cors::CorsLayer;
use tracing_subscriber::EnvFilter;

use config::Config;
use oidc::KeyManager;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub pool: SqlitePool,
    pub key_manager: KeyManager,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config = Config::from_env();
    let pool = db::init_pool(&config.database_url).await;
    let listen_addr = config.listen_addr.clone();
    let base_path = config.base_path.clone();

    let algorithm =
        oidc::SigningAlgorithm::from_str(&config.id_token_signing_alg).expect("无效的签名算法");

    let key_manager = KeyManager::load_or_generate(&config.private_key_path, algorithm)
        .expect("无法加载或生成密钥");

    let state = AppState {
        config,
        pool,
        key_manager,
    };

    let b = &base_path;
    let mut app = Router::new()
        .route(&format!("{b}/"), get(routes::page::home))
        .route(&format!("{b}/new"), get(routes::page::new_client))
        .route(&format!("{b}/dashboard"), get(routes::page::dashboard))
        .route(
            &format!("{b}/authorizations"),
            get(routes::page::authorizations),
        )
        .route(&format!("{b}/authorize"), get(routes::authorize::authorize))
        .route(
            &format!("{b}/authorize/consent"),
            post(routes::authorize::authorize_consent),
        )
        .route(&format!("{b}/token"), post(routes::token::token))
        .route(&format!("{b}/userinfo"), get(routes::userinfo::userinfo))
        .route(
            &format!("{b}/auth/exchange"),
            post(middleware::exchange_external_auth),
        )
        .route(
            &format!("{b}/clients"),
            post(routes::clients::create_client),
        )
        .route(&format!("{b}/api/me"), get(routes::clients::me))
        .route(
            &format!("{b}/api/clients"),
            get(routes::clients::list_clients),
        )
        .route(
            &format!("{b}/api/clients/{{id}}"),
            put(routes::clients::update_client),
        )
        .route(
            &format!("{b}/api/clients/{{id}}"),
            delete(routes::clients::delete_client),
        )
        .route(
            &format!("{b}/api/authorizations"),
            get(routes::clients::list_authorizations),
        )
        .route(
            &format!("{b}/api/authorizations/{{client_id}}"),
            delete(routes::clients::revoke_authorization),
        )
        .route(
            &format!("{b}/.well-known/openid-configuration"),
            get(oidc::discovery::discovery),
        )
        .route(&format!("{b}/.well-known/jwks.json"), get(oidc::jwks::jwks));

    if !base_path.is_empty() {
        app = app.route(b, get(routes::page::home));
    }

    let app = app.layer(CorsLayer::permissive()).with_state(state);

    let listener = tokio::net::TcpListener::bind(&listen_addr)
        .await
        .expect("failed to bind");

    tracing::info!(
        "connect oauth server listening on {} (base_path: {:?})",
        listen_addr,
        base_path
    );
    axum::serve(listener, app).await.unwrap();
}
