use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

pub async fn init_pool(database_url: &str) -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .expect("failed to connect to database");

    migrate(&pool).await;
    pool
}

async fn migrate(pool: &SqlitePool) {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS oauth_clients (
            id TEXT PRIMARY KEY,
            secret_hash TEXT NOT NULL,
            secret_plain TEXT NOT NULL DEFAULT '',
            name TEXT NOT NULL UNIQUE,
            redirect_uris TEXT NOT NULL,
            homepage_url TEXT NOT NULL DEFAULT '',
            description TEXT NOT NULL DEFAULT '',
            owner_id INTEGER NOT NULL DEFAULT 0,
            trusted INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )
        "#,
    )
    .execute(pool)
    .await
    .expect("failed to create oauth_clients table");

    // Migration: add columns if table already exists without them
    for col in [
        "homepage_url TEXT NOT NULL DEFAULT ''",
        "description TEXT NOT NULL DEFAULT ''",
        "owner_id INTEGER NOT NULL DEFAULT 0",
        "secret_plain TEXT NOT NULL DEFAULT ''",
        "owner_name TEXT NOT NULL DEFAULT ''",
        "public INTEGER NOT NULL DEFAULT 0",
        "logo_url TEXT NOT NULL DEFAULT ''",
    ] {
        let sql = format!("ALTER TABLE oauth_clients ADD COLUMN {}", col);
        let _ = sqlx::query(&sql).execute(pool).await;
    }

    // Migration: add unique index on name (ignore if exists)
    let _ =
        sqlx::query("CREATE UNIQUE INDEX IF NOT EXISTS idx_clients_name ON oauth_clients(name)")
            .execute(pool)
            .await;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS authorization_codes (
            code_hash TEXT PRIMARY KEY,
            client_id TEXT NOT NULL,
            user_id INTEGER NOT NULL,
            user_name TEXT NOT NULL,
            user_role INTEGER NOT NULL DEFAULT 0,
            user_status INTEGER NOT NULL DEFAULT 0,
            user_group TEXT NOT NULL DEFAULT '',
            scope TEXT NOT NULL DEFAULT '',
            redirect_uri TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            used INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )
        "#,
    )
    .execute(pool)
    .await
    .expect("failed to create authorization_codes table");

    // Migration: add user profile columns to authorization_codes
    for col in [
        "user_role INTEGER NOT NULL DEFAULT 0",
        "user_status INTEGER NOT NULL DEFAULT 0",
        "user_group TEXT NOT NULL DEFAULT ''",
        "user_email TEXT NOT NULL DEFAULT ''",
        "user_display_name TEXT NOT NULL DEFAULT ''",
        "nonce TEXT",
        "code_challenge TEXT",
        "code_challenge_method TEXT",
    ] {
        let sql = format!("ALTER TABLE authorization_codes ADD COLUMN {}", col);
        let _ = sqlx::query(&sql).execute(pool).await;
    }

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS access_tokens (
            token_hash TEXT PRIMARY KEY,
            client_id TEXT NOT NULL,
            user_id INTEGER NOT NULL,
            user_name TEXT NOT NULL,
            user_role INTEGER NOT NULL DEFAULT 0,
            user_status INTEGER NOT NULL DEFAULT 0,
            user_group TEXT NOT NULL DEFAULT '',
            user_email TEXT NOT NULL DEFAULT '',
            user_display_name TEXT NOT NULL DEFAULT '',
            scope TEXT NOT NULL DEFAULT '',
            expires_at TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )
        "#,
    )
    .execute(pool)
    .await
    .expect("failed to create access_tokens table");

    // Migration: add user profile columns to access_tokens
    for col in [
        "user_role INTEGER NOT NULL DEFAULT 0",
        "user_status INTEGER NOT NULL DEFAULT 0",
        "user_group TEXT NOT NULL DEFAULT ''",
        "user_email TEXT NOT NULL DEFAULT ''",
        "user_display_name TEXT NOT NULL DEFAULT ''",
    ] {
        let sql = format!("ALTER TABLE access_tokens ADD COLUMN {}", col);
        let _ = sqlx::query(&sql).execute(pool).await;
    }

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS user_grants (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id INTEGER NOT NULL,
            client_id TEXT NOT NULL,
            scope TEXT NOT NULL DEFAULT '',
            first_granted_at TEXT NOT NULL DEFAULT (datetime('now')),
            last_used_at TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(user_id, client_id)
        )
        "#,
    )
    .execute(pool)
    .await
    .expect("failed to create user_grants table");

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS grant_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id INTEGER NOT NULL,
            client_id TEXT NOT NULL,
            scope TEXT NOT NULL DEFAULT '',
            granted_at TEXT NOT NULL DEFAULT (datetime('now'))
        )
        "#,
    )
    .execute(pool)
    .await
    .expect("failed to create grant_history table");

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS refresh_tokens (
            token_hash TEXT PRIMARY KEY,
            client_id TEXT NOT NULL,
            user_id INTEGER NOT NULL,
            user_name TEXT NOT NULL,
            user_role INTEGER NOT NULL DEFAULT 0,
            user_status INTEGER NOT NULL DEFAULT 0,
            user_group TEXT NOT NULL DEFAULT '',
            user_email TEXT NOT NULL DEFAULT '',
            user_display_name TEXT NOT NULL DEFAULT '',
            scope TEXT NOT NULL DEFAULT '',
            expires_at TEXT NOT NULL,
            used INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )
        "#,
    )
    .execute(pool)
    .await
    .expect("failed to create refresh_tokens table");
}
