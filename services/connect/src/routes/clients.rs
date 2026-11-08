use axum::Json;
use axum::extract::{Path, State};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

use crate::AppState;
use crate::error::AppError;
use crate::session::SessionUser;

#[derive(Debug, Deserialize)]
pub struct CreateClientRequest {
    pub name: String,
    pub homepage_url: String,
    pub description: Option<String>,
    pub logo_url: Option<String>,
    pub redirect_uri: String,
}

#[derive(Debug, Serialize)]
pub struct CreateClientResponse {
    pub client_id: String,
    pub client_secret: String,
    pub name: String,
    pub homepage_url: String,
    pub description: String,
    pub logo_url: String,
    pub redirect_uri: String,
}

pub async fn create_client(
    State(app): State<AppState>,
    user: SessionUser,
    Json(req): Json<CreateClientRequest>,
) -> Result<Json<CreateClientResponse>, AppError> {
    // Validate required fields
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::BadRequest("应用名不能为空".into()));
    }
    let homepage_url = req.homepage_url.trim().to_string();
    if homepage_url.is_empty() {
        return Err(AppError::BadRequest("应用主页不能为空".into()));
    }
    let redirect_uri = req.redirect_uri.trim().to_string();
    if redirect_uri.is_empty() {
        return Err(AppError::BadRequest("回调地址不能为空".into()));
    }

    // Check name uniqueness
    let exists = sqlx::query("SELECT 1 FROM oauth_clients WHERE name = ?")
        .bind(&name)
        .fetch_optional(&app.pool)
        .await?;
    if exists.is_some() {
        return Err(AppError::BadRequest("应用名已被使用".into()));
    }

    let description = req.description.unwrap_or_default().trim().to_string();
    let logo_url = req.logo_url.unwrap_or_default().trim().to_string();

    let (client_id, client_secret, secret_hash, redirect_uris_json) = {
        let client_id = Uuid::new_v4().to_string();
        let mut rng = rand::rng();
        let secret_bytes: [u8; 32] = rng.random();
        let client_secret = format!("cs_{}", hex::encode(secret_bytes));
        let secret_hash = sha256_hash(&client_secret);
        let redirect_uris_json = serde_json::to_string(&[&redirect_uri])
            .map_err(|e| AppError::Internal(e.to_string()))?;
        (client_id, client_secret, secret_hash, redirect_uris_json)
    };

    sqlx::query(
        "INSERT INTO oauth_clients (id, secret_hash, secret_plain, name, redirect_uris, homepage_url, description, logo_url, owner_id, owner_name, trusted) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0)"
    )
    .bind(&client_id)
    .bind(&secret_hash)
    .bind(&client_secret)
    .bind(&name)
    .bind(&redirect_uris_json)
    .bind(&homepage_url)
    .bind(&description)
    .bind(&logo_url)
    .bind(user.id)
    .bind(&user.username)
    .execute(&app.pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError::BadRequest("应用名已被使用".into())
        } else {
            AppError::Database(e)
        }
    })?;

    Ok(Json(CreateClientResponse {
        client_id,
        client_secret,
        name,
        homepage_url,
        description,
        logo_url,
        redirect_uri,
    }))
}

pub fn sha256_hash(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

// --- API: 获取当前用户信息 ---

#[derive(Serialize)]
pub struct MeResponse {
    pub id: i64,
    pub username: String,
    pub role: i64,
    pub status: i64,
    pub group: String,
    pub is_admin: bool,
}

pub async fn me(user: SessionUser) -> Json<MeResponse> {
    Json(MeResponse {
        id: user.id,
        username: user.username.clone(),
        role: user.role,
        status: user.status,
        group: user.group.clone(),
        is_admin: user.is_admin(),
    })
}

// --- API: 列出应用 ---

#[derive(Serialize)]
pub struct ClientItem {
    pub id: String,
    pub name: String,
    pub homepage_url: String,
    pub description: String,
    pub logo_url: String,
    pub redirect_uri: String,
    pub client_secret: String,
    pub owner_id: i64,
    pub owner_name: String,
    pub created_at: String,
}

pub async fn list_clients(
    State(app): State<AppState>,
    user: SessionUser,
) -> Result<Json<Vec<ClientItem>>, AppError> {
    let rows = if user.is_admin() {
        sqlx::query("SELECT id, name, homepage_url, description, logo_url, redirect_uris, secret_plain, owner_id, owner_name, created_at FROM oauth_clients ORDER BY created_at DESC")
            .fetch_all(&app.pool)
            .await?
    } else {
        sqlx::query("SELECT id, name, homepage_url, description, logo_url, redirect_uris, secret_plain, owner_id, owner_name, created_at FROM oauth_clients WHERE owner_id = ? ORDER BY created_at DESC")
            .bind(user.id)
            .fetch_all(&app.pool)
            .await?
    };

    let items: Vec<ClientItem> = rows
        .iter()
        .map(|row| {
            let redirect_uris: String = row.get("redirect_uris");
            let uris: Vec<String> = serde_json::from_str(&redirect_uris).unwrap_or_default();
            ClientItem {
                id: row.get("id"),
                name: row.get("name"),
                homepage_url: row.get("homepage_url"),
                description: row.get("description"),
                logo_url: row.get("logo_url"),
                redirect_uri: uris.first().cloned().unwrap_or_default(),
                client_secret: row.get("secret_plain"),
                owner_id: row.get("owner_id"),
                owner_name: row.get("owner_name"),
                created_at: row.get("created_at"),
            }
        })
        .collect();

    Ok(Json(items))
}

// --- API: 修改应用 ---

#[derive(Deserialize)]
pub struct UpdateClientRequest {
    pub name: Option<String>,
    pub homepage_url: Option<String>,
    pub description: Option<String>,
    pub logo_url: Option<String>,
    pub redirect_uri: Option<String>,
}

pub async fn update_client(
    State(app): State<AppState>,
    user: SessionUser,
    Path(client_id): Path<String>,
    Json(req): Json<UpdateClientRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Check ownership
    let row = sqlx::query("SELECT owner_id FROM oauth_clients WHERE id = ?")
        .bind(&client_id)
        .fetch_optional(&app.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("应用不存在".into()))?;

    let owner_id: i64 = row.get("owner_id");
    if !user.is_admin() && owner_id != user.id {
        return Err(AppError::BadRequest("无权修改此应用".into()));
    }

    // Build update query dynamically
    let mut sets = Vec::new();
    let mut values: Vec<String> = Vec::new();

    if let Some(name) = &req.name {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::BadRequest("应用名不能为空".into()));
        }
        // Check uniqueness (exclude self)
        let dup = sqlx::query("SELECT 1 FROM oauth_clients WHERE name = ? AND id != ?")
            .bind(name)
            .bind(&client_id)
            .fetch_optional(&app.pool)
            .await?;
        if dup.is_some() {
            return Err(AppError::BadRequest("应用名已被使用".into()));
        }
        sets.push("name = ?");
        values.push(name.to_string());
    }
    if let Some(url) = &req.homepage_url {
        sets.push("homepage_url = ?");
        values.push(url.trim().to_string());
    }
    if let Some(desc) = &req.description {
        sets.push("description = ?");
        values.push(desc.trim().to_string());
    }
    if let Some(logo) = &req.logo_url {
        sets.push("logo_url = ?");
        values.push(logo.trim().to_string());
    }
    if let Some(uri) = &req.redirect_uri {
        let uri = uri.trim().to_string();
        let json = serde_json::to_string(&[&uri]).unwrap_or_default();
        sets.push("redirect_uris = ?");
        values.push(json);
    }

    if sets.is_empty() {
        return Err(AppError::BadRequest("没有需要修改的字段".into()));
    }

    let sql = format!("UPDATE oauth_clients SET {} WHERE id = ?", sets.join(", "));
    let mut query = sqlx::query(&sql);
    for v in &values {
        query = query.bind(v);
    }
    query = query.bind(&client_id);
    query.execute(&app.pool).await?;

    Ok(Json(serde_json::json!({"success": true})))
}

// --- API: 删除应用 ---

pub async fn delete_client(
    State(app): State<AppState>,
    user: SessionUser,
    Path(client_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let row = sqlx::query("SELECT owner_id FROM oauth_clients WHERE id = ?")
        .bind(&client_id)
        .fetch_optional(&app.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("应用不存在".into()))?;

    let owner_id: i64 = row.get("owner_id");
    if !user.is_admin() && owner_id != user.id {
        return Err(AppError::BadRequest("无权删除此应用".into()));
    }

    sqlx::query("DELETE FROM oauth_clients WHERE id = ?")
        .bind(&client_id)
        .execute(&app.pool)
        .await?;

    Ok(Json(serde_json::json!({"success": true})))
}

// --- API: 获取授权记录 ---

#[derive(Serialize)]
pub struct GrantItem {
    pub client_id: String,
    pub name: String,
    pub description: String,
    pub homepage_url: String,
    pub logo_url: String,
    pub scope: String,
    pub first_granted_at: String,
    pub last_used_at: String,
}

pub async fn list_authorizations(
    State(app): State<AppState>,
    user: SessionUser,
) -> Result<Json<Vec<GrantItem>>, AppError> {
    let rows = sqlx::query(
        r#"
        SELECT
            g.client_id,
            g.scope,
            g.first_granted_at,
            g.last_used_at,
            c.name,
            c.description,
            c.homepage_url,
            c.logo_url
        FROM user_grants g
        JOIN oauth_clients c ON g.client_id = c.id
        WHERE g.user_id = ?
        ORDER BY g.last_used_at DESC
        "#,
    )
    .bind(user.id)
    .fetch_all(&app.pool)
    .await?;

    let items: Vec<GrantItem> = rows
        .iter()
        .map(|row| GrantItem {
            client_id: row.get("client_id"),
            name: row.get("name"),
            description: row.get("description"),
            homepage_url: row.get("homepage_url"),
            logo_url: row.get("logo_url"),
            scope: row.get("scope"),
            first_granted_at: row.get("first_granted_at"),
            last_used_at: row.get("last_used_at"),
        })
        .collect();

    Ok(Json(items))
}

// --- API: 撤销授权 ---

pub async fn revoke_authorization(
    State(app): State<AppState>,
    user: SessionUser,
    Path(client_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Check if grant exists and belongs to user
    let grant = sqlx::query("SELECT id FROM user_grants WHERE user_id = ? AND client_id = ?")
        .bind(user.id)
        .bind(&client_id)
        .fetch_optional(&app.pool)
        .await?;

    if grant.is_none() {
        return Err(AppError::NotFound("授权记录不存在".into()));
    }

    // Delete grant
    sqlx::query("DELETE FROM user_grants WHERE user_id = ? AND client_id = ?")
        .bind(user.id)
        .bind(&client_id)
        .execute(&app.pool)
        .await?;

    // Delete all access tokens for this user and client
    sqlx::query("DELETE FROM access_tokens WHERE user_id = ? AND client_id = ?")
        .bind(user.id)
        .bind(&client_id)
        .execute(&app.pool)
        .await?;

    // Delete all refresh tokens for this user and client
    sqlx::query("DELETE FROM refresh_tokens WHERE user_id = ? AND client_id = ?")
        .bind(user.id)
        .bind(&client_id)
        .execute(&app.pool)
        .await?;

    Ok(Json(serde_json::json!({"success": true})))
}
