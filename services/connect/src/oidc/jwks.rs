use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct JwksResponse {
    pub keys: Vec<serde_json::Value>,
}

pub async fn jwks(State(app): State<AppState>) -> Json<JwksResponse> {
    let jwk = app.key_manager.export_jwk().expect("无法导出 JWK");

    Json(JwksResponse { keys: vec![jwk] })
}
