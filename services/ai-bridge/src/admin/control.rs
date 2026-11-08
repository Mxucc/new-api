use std::{
    collections::HashMap,
    fs,
    net::IpAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use axum::{
    Json,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::AdminListMutationResult,
    config::{ADMIN_EDITABLE_KEYS, Config, ImageResponseMode},
    private_file::atomic_write_private,
    proxy::AppState,
};

use super::{SNAPSHOT_ENTRY_LIMIT, authenticate, authorize_mutation, hardened};

const MAX_CONFIG_VALUE_BYTES: usize = 16 * 1024;
const MAX_API_KEY_BYTES: usize = 4 * 1024;

#[derive(Serialize)]
struct AdminControlSnapshot {
    generated_at_ms: u64,
    override_file: AdminOverrideFile,
    fields: Vec<AdminConfigField>,
    policies: AdminPolicyState,
    files: Vec<AdminFileDiagnostic>,
}

#[derive(Serialize)]
struct AdminOverrideFile {
    path: String,
    exists: bool,
    writable: bool,
    overridden_fields: usize,
    issue: Option<String>,
}

#[derive(Serialize)]
struct AdminConfigField {
    key: &'static str,
    label: &'static str,
    group: &'static str,
    kind: &'static str,
    value: String,
    overridden: bool,
    min: Option<u64>,
    max: Option<u64>,
    options: &'static [&'static str],
}

#[derive(Serialize)]
struct AdminPolicyState {
    cyber_policy_ban_enabled: bool,
    auth_failure_cooldown_enabled: bool,
    auth_failure_policy: &'static str,
    upstream_error_cooldown_enabled: bool,
    upstream_502_cooldown_enabled: bool,
    stream_event_stall_retry_enabled: bool,
}

#[derive(Serialize)]
struct AdminFileDiagnostic {
    kind: &'static str,
    status: &'static str,
    path: String,
    exists: bool,
    readable: bool,
    size_bytes: Option<u64>,
    modified_at_ms: Option<u64>,
    cached_entries: usize,
    issue: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct AdminConfigUpdate {
    values: HashMap<String, String>,
}

#[derive(Deserialize)]
pub(crate) struct AdminListCreate {
    value: String,
}

#[derive(Deserialize)]
pub(crate) struct AdminListUpdate {
    id: String,
    value: String,
}

#[derive(Deserialize)]
pub(crate) struct AdminListDelete {
    id: String,
}

pub(crate) async fn get_control(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let config = state.config_snapshot();
    if let Err(error) = authenticate(&headers, &config).await {
        return super::auth_error_response(error);
    }
    let state_for_task = state.clone();
    match tokio::task::spawn_blocking(move || control_snapshot(&state_for_task)).await {
        Ok(Ok(snapshot)) => hardened(Json(snapshot).into_response()),
        Ok(Err(error)) => control_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "admin_control_unavailable",
            &error.to_string(),
        ),
        Err(error) => control_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "admin_control_task_failed",
            &error.to_string(),
        ),
    }
}

pub(crate) async fn put_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(update): Json<AdminConfigUpdate>,
) -> Response {
    let config = state.config_snapshot();
    if let Err(error) = authorize_mutation(&headers, &config).await {
        return super::auth_error_response(error);
    }
    if let Err(message) = validate_config_update(&update.values) {
        return control_error(StatusCode::BAD_REQUEST, "invalid_admin_config", &message);
    }

    let state_for_task = state.clone();
    match tokio::task::spawn_blocking(move || apply_config_update(&state_for_task, update.values))
        .await
    {
        Ok(Ok(snapshot)) => {
            state
                .observability
                .record_admin("replace", "configuration", "success");
            hardened(Json(snapshot).into_response())
        }
        Ok(Err(error)) => {
            state
                .observability
                .record_admin("replace", "configuration", "failed");
            control_error(
                StatusCode::BAD_REQUEST,
                "config_update_failed",
                &error.to_string(),
            )
        }
        Err(error) => {
            state
                .observability
                .record_admin("replace", "configuration", "failed");
            control_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "config_update_task_failed",
                &error.to_string(),
            )
        }
    }
}

pub(crate) async fn reload(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let config = state.config_snapshot();
    if let Err(error) = authorize_mutation(&headers, &config).await {
        return super::auth_error_response(error);
    }
    let state_for_task = state.clone();
    match tokio::task::spawn_blocking(move || {
        let config = Config::from_env()?;
        state_for_task.reload_runtime(config)?;
        control_snapshot(&state_for_task)
    })
    .await
    {
        Ok(Ok(snapshot)) => {
            state.observability.record_reload(true, "manual");
            state
                .observability
                .record_admin("reload", "runtime", "success");
            hardened(Json(snapshot).into_response())
        }
        Ok(Err(error)) => {
            state.observability.record_reload(false, "manual");
            state
                .observability
                .record_admin("reload", "runtime", "failed");
            control_error(StatusCode::BAD_REQUEST, "reload_failed", &error.to_string())
        }
        Err(error) => {
            state.observability.record_reload(false, "manual");
            state
                .observability
                .record_admin("reload", "runtime", "failed");
            control_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "reload_task_failed",
                &error.to_string(),
            )
        }
    }
}

pub(crate) async fn create_list_entry(
    State(state): State<Arc<AppState>>,
    AxumPath(kind): AxumPath<String>,
    headers: HeaderMap,
    Json(input): Json<AdminListCreate>,
) -> Response {
    mutate_list(state, headers, kind, None, Some(input.value), "create").await
}

pub(crate) async fn update_list_entry(
    State(state): State<Arc<AppState>>,
    AxumPath(kind): AxumPath<String>,
    headers: HeaderMap,
    Json(input): Json<AdminListUpdate>,
) -> Response {
    mutate_list(
        state,
        headers,
        kind,
        Some(input.id),
        Some(input.value),
        "update",
    )
    .await
}

pub(crate) async fn delete_list_entry(
    State(state): State<Arc<AppState>>,
    AxumPath(kind): AxumPath<String>,
    headers: HeaderMap,
    Json(input): Json<AdminListDelete>,
) -> Response {
    mutate_list(state, headers, kind, Some(input.id), None, "delete").await
}

async fn mutate_list(
    state: Arc<AppState>,
    headers: HeaderMap,
    kind: String,
    id: Option<String>,
    value: Option<String>,
    action: &'static str,
) -> Response {
    let config = state.config_snapshot();
    if let Err(error) = authorize_mutation(&headers, &config).await {
        return super::auth_error_response(error);
    }
    if !matches!(
        kind.as_str(),
        "blacklist_key" | "blacklist_ip" | "whitelist_key"
    ) {
        return control_error(
            StatusCode::NOT_FOUND,
            "unknown_list",
            "Unknown policy list.",
        );
    }
    if let Some(value) = value.as_deref()
        && let Err(message) = validate_list_value(&kind, value)
    {
        return control_error(StatusCode::BAD_REQUEST, "invalid_list_value", &message);
    }
    if let Some(id) = id.as_deref()
        && let Err(message) = validate_list_id(&kind, id)
    {
        return control_error(StatusCode::BAD_REQUEST, "invalid_list_id", &message);
    }

    let state_for_task = state.clone();
    let kind_for_task = kind.clone();
    let result = tokio::task::spawn_blocking(move || {
        perform_list_mutation(&state_for_task, &kind_for_task, action, id, value)
    })
    .await;
    match result {
        Ok(Ok(result)) => match result {
            AdminListMutationResult::Created
            | AdminListMutationResult::Updated
            | AdminListMutationResult::Deleted => {
                let Some((status, outcome)) = mutation_response(result) else {
                    state.observability.record_admin(action, &kind, "failed");
                    return control_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "unexpected_list_mutation_result",
                        "The list mutation returned an unexpected result.",
                    );
                };
                state.observability.record_admin(action, &kind, "success");
                hardened((status, Json(json!({ "ok": true, "result": outcome }))).into_response())
            }
            AdminListMutationResult::AlreadyExists => {
                state.observability.record_admin(action, &kind, "conflict");
                control_error(
                    StatusCode::CONFLICT,
                    "list_entry_exists",
                    "The value already exists in this list.",
                )
            }
            AdminListMutationResult::NotFound => {
                state.observability.record_admin(action, &kind, "not_found");
                control_error(
                    StatusCode::NOT_FOUND,
                    "list_entry_not_found",
                    "The list entry no longer exists.",
                )
            }
        },
        Ok(Err(error)) => {
            state.observability.record_admin(action, &kind, "failed");
            control_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "list_mutation_failed",
                &error.to_string(),
            )
        }
        Err(error) => {
            state.observability.record_admin(action, &kind, "failed");
            control_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "list_mutation_task_failed",
                &error.to_string(),
            )
        }
    }
}

fn perform_list_mutation(
    state: &AppState,
    kind: &str,
    action: &str,
    id: Option<String>,
    value: Option<String>,
) -> Result<AdminListMutationResult> {
    let id = id.as_deref().unwrap_or_default();
    let value = value.as_deref().map(str::trim).unwrap_or_default();
    match (kind, action) {
        ("blacklist_key", "create") => Ok(state.blacklist_cache.create_api_key(value)?),
        ("blacklist_key", "update") => Ok(state.blacklist_cache.update_api_key(id, value)?),
        ("blacklist_key", "delete") => Ok(state.blacklist_cache.delete_api_key(id)?),
        ("whitelist_key", "create") => Ok(state.whitelist_cache.create_api_key(value)?),
        ("whitelist_key", "update") => Ok(state.whitelist_cache.update_api_key(id, value)?),
        ("whitelist_key", "delete") => Ok(state.whitelist_cache.delete_api_key(id)?),
        ("blacklist_ip", "create") => Ok(state.blacklist_cache.create_client_ip(value.parse()?)?),
        ("blacklist_ip", "update") => Ok(state
            .blacklist_cache
            .update_client_ip(id.parse()?, value.parse()?)?),
        ("blacklist_ip", "delete") => Ok(state.blacklist_cache.delete_client_ip(id.parse()?)?),
        _ => anyhow::bail!("unsupported list mutation"),
    }
}

fn mutation_response(result: AdminListMutationResult) -> Option<(StatusCode, &'static str)> {
    match result {
        AdminListMutationResult::Created => Some((StatusCode::CREATED, "created")),
        AdminListMutationResult::Updated => Some((StatusCode::OK, "updated")),
        AdminListMutationResult::Deleted => Some((StatusCode::OK, "deleted")),
        AdminListMutationResult::AlreadyExists | AdminListMutationResult::NotFound => None,
    }
}

fn apply_config_update(
    state: &AppState,
    values: HashMap<String, String>,
) -> Result<AdminControlSnapshot> {
    let candidate = Config::from_admin_overrides(values.clone())?;
    let path = Config::admin_config_file_path()?;
    let previous = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", path.display()));
        }
    };
    let encoded = encode_admin_overrides(&values)?;
    atomic_write_private(&path, encoded.as_bytes())
        .with_context(|| format!("failed to write {}", path.display()))?;

    if let Err(error) = state.reload_runtime(candidate) {
        restore_override_file(&path, previous.as_deref())?;
        return Err(error).context("runtime rejected configuration; previous file restored");
    }
    control_snapshot(state)
}

fn restore_override_file(path: &Path, previous: Option<&[u8]>) -> Result<()> {
    if let Some(previous) = previous {
        atomic_write_private(path, previous)?;
    } else {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn encode_admin_overrides(values: &HashMap<String, String>) -> Result<String> {
    let mut keys = values.keys().collect::<Vec<_>>();
    keys.sort();
    let mut output =
        String::from("# Managed by /ai-bridge. Edit through the administrator console.\n");
    for key in keys {
        let encoded = serde_json::to_string(&values[key])?;
        output.push_str(key);
        output.push('=');
        output.push_str(&encoded);
        output.push('\n');
    }
    Ok(output)
}

fn control_snapshot(state: &AppState) -> Result<AdminControlSnapshot> {
    let config = state.config_snapshot();
    let override_path = Config::admin_config_file_path()?;
    let (overrides, override_issue) = match Config::load_admin_overrides() {
        Ok((_, overrides)) => (overrides, None),
        Err(error) => (
            HashMap::new(),
            Some(format!(
                "管理员覆盖文件无法解析：{error}。可在本页重新保存配置以修复。"
            )),
        ),
    };
    let blacklist = state.blacklist_cache.admin_snapshot(SNAPSHOT_ENTRY_LIMIT);
    let whitelist = state.whitelist_cache.admin_snapshot(SNAPSHOT_ENTRY_LIMIT);
    let override_metadata = fs::metadata(&override_path).ok();
    let parent_writable = override_path
        .parent()
        .and_then(|parent| fs::metadata(parent).ok())
        .is_some_and(|metadata| !metadata.permissions().readonly());

    Ok(AdminControlSnapshot {
        generated_at_ms: unix_time_millis(),
        override_file: AdminOverrideFile {
            path: override_path.display().to_string(),
            exists: override_metadata.is_some(),
            writable: override_metadata
                .as_ref()
                .is_some_and(|metadata| !metadata.permissions().readonly())
                || parent_writable,
            overridden_fields: overrides.len(),
            issue: override_issue,
        },
        fields: admin_config_fields(&config, &overrides),
        policies: AdminPolicyState {
            cyber_policy_ban_enabled: config.cyber_policy_ban_enabled,
            auth_failure_cooldown_enabled: config.auth_failure_cooldown_enabled,
            auth_failure_policy: config.auth_failure_policy.as_str(),
            upstream_error_cooldown_enabled: config.upstream_error_cooldown_enabled,
            upstream_502_cooldown_enabled: config.upstream_502_cooldown_enabled,
            stream_event_stall_retry_enabled: config.stream_event_stall_retry_enabled,
        },
        files: vec![
            inspect_file(
                "blacklist_key",
                &config.cyber_policy_blacklist_file,
                blacklist.total_api_keys,
                None,
            ),
            inspect_file(
                "blacklist_ip",
                &config.cyber_policy_ip_blacklist_file,
                blacklist.total_client_ips,
                None,
            ),
            inspect_file(
                "whitelist_key",
                &config.cyber_policy_whitelist_file,
                whitelist.total_api_keys,
                legacy_while_candidate(&config.cyber_policy_whitelist_file),
            ),
            inspect_file(
                "confirmation_rules",
                config
                    .cyber_policy_confirmation_rules_file
                    .as_deref()
                    .unwrap_or_else(|| Path::new("cyber_policy_rules.json")),
                config.cyber_policy_confirmation_rules.keywords.len()
                    + config.cyber_policy_confirmation_rules.regex.len(),
                None,
            ),
        ],
    })
}

fn inspect_file(
    kind: &'static str,
    path: &Path,
    cached_entries: usize,
    legacy_candidate: Option<PathBuf>,
) -> AdminFileDiagnostic {
    let metadata = fs::metadata(path).ok();
    let readable = fs::File::open(path).is_ok();
    let issue = if metadata.is_none() {
        legacy_candidate
            .filter(|candidate| candidate.is_file())
            .map(|candidate| {
                format!(
                    "检测到旧的错误文件名 {}，但 ai-bridge 实际读取的是 {}。请改名后重载。",
                    candidate.display(),
                    path.display()
                )
            })
    } else if !readable {
        Some("配置文件已存在，但 ai-bridge 进程没有读取权限。".to_owned())
    } else {
        None
    };
    let status = if issue.is_some() {
        "error"
    } else if metadata.is_none() {
        "missing"
    } else {
        "ok"
    };
    AdminFileDiagnostic {
        kind,
        status,
        path: path.display().to_string(),
        exists: metadata.is_some(),
        readable,
        size_bytes: metadata.as_ref().map(fs::Metadata::len),
        modified_at_ms: metadata
            .and_then(|metadata| metadata.modified().ok())
            .map(system_time_millis),
        cached_entries,
        issue,
    }
}

fn legacy_while_candidate(path: &Path) -> Option<PathBuf> {
    (path.file_name().and_then(|name| name.to_str()) == Some("white.txt"))
        .then(|| path.with_file_name("while.txt"))
}

fn validate_config_update(values: &HashMap<String, String>) -> std::result::Result<(), String> {
    for (key, value) in values {
        if !ADMIN_EDITABLE_KEYS.contains(&key.as_str()) {
            return Err(format!("Unsupported configuration key: {key}"));
        }
        if value.len() > MAX_CONFIG_VALUE_BYTES {
            return Err(format!("Configuration value is too large: {key}"));
        }
        let Some(meta) = FIELD_META.iter().find(|meta| meta.key == key) else {
            return Err(format!("Configuration metadata is missing: {key}"));
        };
        match meta.kind {
            "number" => {
                let parsed = value
                    .parse::<u64>()
                    .map_err(|_| format!("Configuration value must be an integer: {key}"))?;
                if meta.min.is_some_and(|minimum| parsed < minimum)
                    || meta.max.is_some_and(|maximum| parsed > maximum)
                {
                    return Err(format!(
                        "Configuration value is outside the allowed range: {key}"
                    ));
                }
            }
            "boolean" if !matches!(value.as_str(), "true" | "false") => {
                return Err(format!("Configuration value must be true or false: {key}"));
            }
            "select" if !meta.options.contains(&value.as_str()) => {
                return Err(format!(
                    "Configuration value is not an allowed option: {key}"
                ));
            }
            "json" => {
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(value)
                    .map_err(|_| format!("Configuration value must be a JSON object: {key}"))?;
            }
            "text" if value.trim().is_empty() => {
                return Err(format!("Configuration value cannot be empty: {key}"));
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_list_value(kind: &str, value: &str) -> std::result::Result<(), String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("Value cannot be empty.".to_owned());
    }
    if value.len() > MAX_API_KEY_BYTES || value.contains(['\r', '\n']) {
        return Err("Value is too large or contains a line break.".to_owned());
    }
    if kind == "blacklist_ip" {
        value
            .parse::<IpAddr>()
            .map_err(|_| "Client IP must be a valid IPv4 or IPv6 address.".to_owned())?;
    }
    Ok(())
}

fn validate_list_id(kind: &str, id: &str) -> std::result::Result<(), String> {
    if kind == "blacklist_ip" {
        id.parse::<IpAddr>()
            .map_err(|_| "Current client IP is invalid.".to_owned())?;
    } else if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("API Key entry identifier is invalid.".to_owned());
    }
    Ok(())
}

fn admin_config_fields(
    config: &Config,
    overrides: &HashMap<String, String>,
) -> Vec<AdminConfigField> {
    FIELD_META
        .iter()
        .map(|meta| AdminConfigField {
            key: meta.key,
            label: meta.label,
            group: meta.group,
            kind: meta.kind,
            value: config_value(config, meta.key),
            overridden: overrides.contains_key(meta.key),
            min: meta.min,
            max: meta.max,
            options: meta.options,
        })
        .collect()
}

struct FieldMeta {
    key: &'static str,
    label: &'static str,
    group: &'static str,
    kind: &'static str,
    min: Option<u64>,
    max: Option<u64>,
    options: &'static [&'static str],
}

const BOOL_OPTIONS: &[&str] = &["true", "false"];
const IMAGE_OPTIONS: &[&str] = &["auto", "json", "sse"];
const AUTH_FAILURE_POLICY_OPTIONS: &[&str] = &["off", "conservative", "balanced", "strict"];
const FIELD_META: &[FieldMeta] = &[
    field(
        "AI_BRIDGE_UPSTREAM_BASE_URL",
        "New API 上游地址",
        "上游",
        "text",
        None,
        None,
        &[],
    ),
    field(
        "AI_BRIDGE_REQUEST_TIMEOUT_SECS",
        "请求总超时（秒）",
        "上游",
        "number",
        Some(0),
        Some(86_400),
        &[],
    ),
    field(
        "AI_BRIDGE_CONNECT_TIMEOUT_SECS",
        "连接超时（秒）",
        "上游",
        "number",
        Some(1),
        Some(3_600),
        &[],
    ),
    field(
        "AI_BRIDGE_HEARTBEAT_INTERVAL_SECS",
        "心跳间隔（秒）",
        "流式",
        "number",
        Some(1),
        Some(300),
        &[],
    ),
    field(
        "AI_BRIDGE_STREAM_EVENT_STALL_RETRY_ENABLED",
        "流停顿重试",
        "流式",
        "boolean",
        None,
        None,
        BOOL_OPTIONS,
    ),
    field(
        "AI_BRIDGE_STREAM_EVENT_STALL_TIMEOUT_SECS",
        "流停顿超时（秒）",
        "流式",
        "number",
        Some(0),
        Some(3_600),
        &[],
    ),
    field(
        "AI_BRIDGE_STREAM_EVENT_STALL_MAX_RETRIES",
        "流停顿最大重试",
        "流式",
        "number",
        Some(0),
        Some(100),
        &[],
    ),
    field(
        "AI_BRIDGE_STREAM_EARLY_HEARTBEAT",
        "首字节心跳",
        "流式",
        "boolean",
        None,
        None,
        BOOL_OPTIONS,
    ),
    field(
        "AI_BRIDGE_STREAM_SYNTHETIC_FIRST_SPACE",
        "合成首空格",
        "流式",
        "boolean",
        None,
        None,
        BOOL_OPTIONS,
    ),
    field(
        "AI_BRIDGE_UPSTREAM_429_INITIAL_DELAY_SECS",
        "429 初始延迟（秒）",
        "重试",
        "number",
        Some(0),
        Some(3_600),
        &[],
    ),
    field(
        "AI_BRIDGE_UPSTREAM_429_MAX_DELAY_SECS",
        "429 最大延迟（秒）",
        "重试",
        "number",
        Some(0),
        Some(86_400),
        &[],
    ),
    field(
        "AI_BRIDGE_UPSTREAM_429_MAX_RETRIES",
        "429 最大重试（0 为无限）",
        "重试",
        "number",
        Some(0),
        Some(10_000),
        &[],
    ),
    field(
        "AI_BRIDGE_UPSTREAM_ERROR_COOLDOWN_ENABLED",
        "429 跨请求冷却",
        "重试",
        "boolean",
        None,
        None,
        BOOL_OPTIONS,
    ),
    field(
        "AI_BRIDGE_UPSTREAM_502_COOLDOWN_ENABLED",
        "502 冷却重试",
        "重试",
        "boolean",
        None,
        None,
        BOOL_OPTIONS,
    ),
    field(
        "AI_BRIDGE_UPSTREAM_502_COOLDOWN_ACCUMULATE_ENABLED",
        "502 累加冷却",
        "重试",
        "boolean",
        None,
        None,
        BOOL_OPTIONS,
    ),
    field(
        "AI_BRIDGE_UPSTREAM_502_COOLDOWN_SECS",
        "502 冷却（秒）",
        "重试",
        "number",
        Some(0),
        Some(86_400),
        &[],
    ),
    field(
        "AI_BRIDGE_UPSTREAM_502_MAX_RETRIES",
        "502 最大重试（0 为无限）",
        "重试",
        "number",
        Some(0),
        Some(10_000),
        &[],
    ),
    field(
        "AI_BRIDGE_AUTH_FAILURE_COOLDOWN_ENABLED",
        "鉴权失败冷却",
        "鉴权",
        "boolean",
        None,
        None,
        BOOL_OPTIONS,
    ),
    field(
        "AI_BRIDGE_AUTH_FAILURE_POLICY",
        "鉴权冷却策略",
        "鉴权",
        "select",
        None,
        None,
        AUTH_FAILURE_POLICY_OPTIONS,
    ),
    field(
        "AI_BRIDGE_AUTH_FAILURE_WINDOW_SECS",
        "鉴权统计窗口（秒）",
        "鉴权",
        "number",
        Some(1),
        Some(3_600),
        &[],
    ),
    field(
        "AI_BRIDGE_AUTH_FAILURE_THRESHOLD",
        "Key 失败阈值",
        "鉴权",
        "number",
        Some(1),
        Some(1_000),
        &[],
    ),
    field(
        "AI_BRIDGE_AUTH_FAILURE_COOLDOWN_SECS",
        "Key 冷却（秒）",
        "鉴权",
        "number",
        Some(1),
        Some(86_400),
        &[],
    ),
    field(
        "AI_BRIDGE_AUTH_FAILURE_IP_THRESHOLD",
        "IP 失败阈值",
        "鉴权",
        "number",
        Some(0),
        Some(10_000),
        &[],
    ),
    field(
        "AI_BRIDGE_AUTH_FAILURE_ERROR_BODY_MAX_BYTES",
        "鉴权错误体上限",
        "鉴权",
        "number",
        Some(1),
        Some(1_048_576),
        &[],
    ),
    field(
        "AI_BRIDGE_MAX_BODY_BYTES",
        "普通请求体上限",
        "请求",
        "number",
        Some(1),
        Some(1_073_741_824),
        &[],
    ),
    field(
        "AI_BRIDGE_MAX_UPLOAD_BODY_BYTES",
        "上传请求体上限",
        "请求",
        "number",
        Some(1),
        Some(4_294_967_295),
        &[],
    ),
    field(
        "AI_BRIDGE_IMAGE_RESPONSE_MODE",
        "图片响应模式",
        "兼容",
        "select",
        None,
        None,
        IMAGE_OPTIONS,
    ),
    field(
        "AI_BRIDGE_RESPONSES_NONSTREAM_CHAT_FALLBACK",
        "Responses 非流回退",
        "兼容",
        "boolean",
        None,
        None,
        BOOL_OPTIONS,
    ),
    field(
        "AI_BRIDGE_DEBUG_OUTPUT",
        "额外调试日志",
        "观测",
        "boolean",
        None,
        None,
        BOOL_OPTIONS,
    ),
    field(
        "AI_BRIDGE_CYBER_POLICY_BAN_ENABLED",
        "Cyber Policy 封禁",
        "策略",
        "boolean",
        None,
        None,
        BOOL_OPTIONS,
    ),
    field(
        "AI_BRIDGE_CYBER_POLICY_BLACKLIST_FILE",
        "Key 黑名单文件",
        "策略",
        "text",
        None,
        None,
        &[],
    ),
    field(
        "AI_BRIDGE_CYBER_POLICY_IP_BLACKLIST_FILE",
        "IP 黑名单文件",
        "策略",
        "text",
        None,
        None,
        &[],
    ),
    field(
        "AI_BRIDGE_CYBER_POLICY_WHITELIST_FILE",
        "Key 白名单文件",
        "策略",
        "text",
        None,
        None,
        &[],
    ),
    field(
        "AI_BRIDGE_CYBER_POLICY_CONFIRMATION_RULES_FILE",
        "确认规则文件",
        "策略",
        "text",
        None,
        None,
        &[],
    ),
    field(
        "AI_BRIDGE_FILTER_TOOLS_BY_MODEL",
        "模型工具过滤 JSON",
        "工具",
        "json",
        None,
        None,
        &[],
    ),
    field(
        "AI_BRIDGE_TOOL_FILTER_SCAN_LIMIT",
        "工具快速扫描字节",
        "工具",
        "number",
        Some(1),
        Some(16_777_216),
        &[],
    ),
];

const fn field(
    key: &'static str,
    label: &'static str,
    group: &'static str,
    kind: &'static str,
    min: Option<u64>,
    max: Option<u64>,
    options: &'static [&'static str],
) -> FieldMeta {
    FieldMeta {
        key,
        label,
        group,
        kind,
        min,
        max,
        options,
    }
}

fn config_value(config: &Config, key: &str) -> String {
    match key {
        "AI_BRIDGE_UPSTREAM_BASE_URL" => config.upstream_base_url.to_string(),
        "AI_BRIDGE_REQUEST_TIMEOUT_SECS" => config
            .request_timeout
            .map_or(0, |value| value.as_secs())
            .to_string(),
        "AI_BRIDGE_CONNECT_TIMEOUT_SECS" => config.connect_timeout.as_secs().to_string(),
        "AI_BRIDGE_HEARTBEAT_INTERVAL_SECS" => config.heartbeat_interval.as_secs().to_string(),
        "AI_BRIDGE_STREAM_EVENT_STALL_RETRY_ENABLED" => {
            config.stream_event_stall_retry_enabled.to_string()
        }
        "AI_BRIDGE_STREAM_EVENT_STALL_TIMEOUT_SECS" => config
            .stream_event_stall_timeout
            .map_or(0, |value| value.as_secs())
            .to_string(),
        "AI_BRIDGE_STREAM_EVENT_STALL_MAX_RETRIES" => {
            config.stream_event_stall_max_retries.to_string()
        }
        "AI_BRIDGE_STREAM_EARLY_HEARTBEAT" => config.stream_early_heartbeat.to_string(),
        "AI_BRIDGE_STREAM_SYNTHETIC_FIRST_SPACE" => config.stream_synthetic_first_space.to_string(),
        "AI_BRIDGE_UPSTREAM_429_INITIAL_DELAY_SECS" => {
            config.upstream_429_initial_delay.as_secs().to_string()
        }
        "AI_BRIDGE_UPSTREAM_429_MAX_DELAY_SECS" => {
            config.upstream_429_max_delay.as_secs().to_string()
        }
        "AI_BRIDGE_UPSTREAM_429_MAX_RETRIES" => {
            config.upstream_429_max_retries.unwrap_or(0).to_string()
        }
        "AI_BRIDGE_UPSTREAM_ERROR_COOLDOWN_ENABLED" => {
            config.upstream_error_cooldown_enabled.to_string()
        }
        "AI_BRIDGE_UPSTREAM_502_COOLDOWN_ENABLED" => {
            config.upstream_502_cooldown_enabled.to_string()
        }
        "AI_BRIDGE_UPSTREAM_502_COOLDOWN_ACCUMULATE_ENABLED" => {
            config.upstream_502_cooldown_accumulate_enabled.to_string()
        }
        "AI_BRIDGE_UPSTREAM_502_COOLDOWN_SECS" => {
            config.upstream_502_cooldown_delay.as_secs().to_string()
        }
        "AI_BRIDGE_UPSTREAM_502_MAX_RETRIES" => {
            config.upstream_502_max_retries.unwrap_or(0).to_string()
        }
        "AI_BRIDGE_AUTH_FAILURE_COOLDOWN_ENABLED" => {
            config.auth_failure_cooldown_enabled.to_string()
        }
        "AI_BRIDGE_AUTH_FAILURE_POLICY" => config.auth_failure_policy.as_str().to_owned(),
        "AI_BRIDGE_AUTH_FAILURE_WINDOW_SECS" => config.auth_failure_window.as_secs().to_string(),
        "AI_BRIDGE_AUTH_FAILURE_THRESHOLD" => config.auth_failure_threshold.to_string(),
        "AI_BRIDGE_AUTH_FAILURE_COOLDOWN_SECS" => {
            config.auth_failure_cooldown.as_secs().to_string()
        }
        "AI_BRIDGE_AUTH_FAILURE_IP_THRESHOLD" => config.auth_failure_ip_threshold.to_string(),
        "AI_BRIDGE_AUTH_FAILURE_ERROR_BODY_MAX_BYTES" => {
            config.auth_failure_error_body_max_bytes.to_string()
        }
        "AI_BRIDGE_MAX_BODY_BYTES" => config.max_body_bytes.to_string(),
        "AI_BRIDGE_MAX_UPLOAD_BODY_BYTES" => config.max_upload_body_bytes.to_string(),
        "AI_BRIDGE_IMAGE_RESPONSE_MODE" => match config.image_response_mode {
            ImageResponseMode::Auto => "auto",
            ImageResponseMode::Json => "json",
            ImageResponseMode::Sse => "sse",
        }
        .to_owned(),
        "AI_BRIDGE_RESPONSES_NONSTREAM_CHAT_FALLBACK" => {
            config.responses_nonstream_chat_fallback.to_string()
        }
        "AI_BRIDGE_DEBUG_OUTPUT" => config.debug_output.to_string(),
        "AI_BRIDGE_CYBER_POLICY_BAN_ENABLED" => config.cyber_policy_ban_enabled.to_string(),
        "AI_BRIDGE_CYBER_POLICY_BLACKLIST_FILE" => {
            config.cyber_policy_blacklist_file.display().to_string()
        }
        "AI_BRIDGE_CYBER_POLICY_IP_BLACKLIST_FILE" => {
            config.cyber_policy_ip_blacklist_file.display().to_string()
        }
        "AI_BRIDGE_CYBER_POLICY_WHITELIST_FILE" => {
            config.cyber_policy_whitelist_file.display().to_string()
        }
        "AI_BRIDGE_CYBER_POLICY_CONFIRMATION_RULES_FILE" => config
            .cyber_policy_confirmation_rules_file
            .as_deref()
            .map(Path::display)
            .map(|value| value.to_string())
            .unwrap_or_default(),
        "AI_BRIDGE_FILTER_TOOLS_BY_MODEL" => {
            serde_json::to_string(&config.filter_tools_by_model).unwrap_or_else(|_| "{}".to_owned())
        }
        "AI_BRIDGE_TOOL_FILTER_SCAN_LIMIT" => config.tool_filter_scan_limit.to_string(),
        _ => String::new(),
    }
}

fn unix_time_millis() -> u64 {
    system_time_millis(SystemTime::now())
}

fn system_time_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn control_error(status: StatusCode, code: &str, message: &str) -> Response {
    hardened(
        (
            status,
            Json(json!({ "error": { "code": code, "message": message } })),
        )
            .into_response(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_override_encoding_round_trips_dotenv_values() {
        let values = HashMap::from([
            ("AI_BRIDGE_DEBUG_OUTPUT".to_owned(), "true".to_owned()),
            (
                "AI_BRIDGE_FILTER_TOOLS_BY_MODEL".to_owned(),
                r#"{"gpt":["image_generation"]}"#.to_owned(),
            ),
        ]);
        let encoded = encode_admin_overrides(&values).unwrap();
        let parsed = dotenvy::from_read_iter(encoded.as_bytes())
            .map(|item| item.unwrap())
            .collect::<HashMap<_, _>>();
        assert_eq!(parsed, values);
        assert!(!encoded.contains("SESSION_SECRET"));
    }

    #[test]
    fn mutation_validation_rejects_secrets_line_breaks_and_invalid_ips() {
        assert!(
            validate_config_update(&HashMap::from([(
                "AI_BRIDGE_ADMIN_SESSION_SECRET".to_owned(),
                "secret".to_owned(),
            )]))
            .is_err()
        );
        assert!(validate_list_value("whitelist_key", "sk-a\nsk-b").is_err());
        assert!(validate_list_value("blacklist_ip", "not-an-ip").is_err());
        assert!(validate_list_id("blacklist_key", "short").is_err());
    }

    #[test]
    fn missing_white_file_reports_legacy_while_filename() {
        let directory =
            std::env::temp_dir().join(format!("ai-bridge-admin-diagnostic-{}", std::process::id()));
        let white = directory.join("white.txt");
        let legacy = directory.join("while.txt");
        let _ = fs::create_dir_all(&directory);
        let _ = fs::remove_file(&white);
        fs::write(&legacy, "sk-example\n").unwrap();

        let diagnostic = inspect_file("whitelist_key", &white, 0, legacy_while_candidate(&white));
        assert!(!diagnostic.exists);
        assert_eq!(diagnostic.status, "error");
        assert!(diagnostic.issue.unwrap().contains("while.txt"));

        let _ = fs::remove_file(legacy);
        let _ = fs::remove_dir(directory);
    }

    #[test]
    fn missing_empty_policy_file_is_reported_as_not_created() {
        let path = std::env::temp_dir().join(format!(
            "ai-bridge-missing-policy-{}-{}",
            std::process::id(),
            unix_time_millis()
        ));
        let diagnostic = inspect_file("blacklist_key", &path, 0, None);
        assert!(!diagnostic.exists);
        assert_eq!(diagnostic.status, "missing");
        assert!(diagnostic.issue.is_none());
    }
}
