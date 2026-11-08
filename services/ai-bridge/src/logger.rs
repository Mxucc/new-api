use std::{
    error::Error,
    io,
    net::IpAddr,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use axum::http::{HeaderMap, Uri};
use serde_json::{Map, Value, json};
use tracing::{info, warn};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    auth::ApiKeyInfo,
    config::{Config, LogFormat},
    observability,
};

#[derive(Debug, Clone, Default)]
pub struct PromptSummary {
    pub text_count: usize,
    pub char_count: usize,
    pub summary: String,
}

pub enum LogGuard {
    NonBlocking(WorkerGuard),
}

impl LogGuard {
    pub fn keepalive_marker(&self) {
        let Self::NonBlocking(_guard) = self;
    }
}

pub fn init_tracing(config: &Config) -> Result<LogGuard> {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&config.log_level))
        .context("failed to build tracing env filter")?;

    let (writer, guard) = if let Some(path) = &config.log_file {
        file_writer(path)?
    } else {
        tracing_appender::non_blocking(io::stdout())
    };

    match config.log_format {
        LogFormat::Json => tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().json().with_writer(writer))
            .init(),
        LogFormat::Pretty => tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().with_writer(writer))
            .init(),
    }

    Ok(LogGuard::NonBlocking(guard))
}

pub struct RequestLog<'a> {
    pub api_key: &'a ApiKeyInfo,
    pub method: &'a str,
    pub path: &'a str,
    pub status: u16,
    pub latency: Duration,
    pub queue_wait: Duration,
    pub prompt_summary: &'a PromptSummary,
    pub queued: bool,
    pub upstream_status: Option<u16>,
}

pub fn log_request(entry: RequestLog<'_>) {
    if let Some(observability) = observability::global() {
        observability.record_request(
            entry.method,
            entry.path,
            entry.status,
            entry.upstream_status,
            entry.latency.as_millis().min(u128::from(u64::MAX)) as u64,
            entry.queue_wait.as_millis().min(u128::from(u64::MAX)) as u64,
            entry.api_key.hash(),
        );
    }
    info!(
        event = "request",
        api_key_masked = entry.api_key.masked(),
        api_key_hash = entry.api_key.hash(),
        api_key_present = entry.api_key.is_present(),
        method = entry.method,
        path = entry.path,
        status = entry.status,
        upstream_status = entry.upstream_status,
        latency_ms = entry.latency.as_millis() as u64,
        queue_wait_ms = entry.queue_wait.as_millis() as u64,
        queued = entry.queued,
        prompt_text_count = entry.prompt_summary.text_count,
        prompt_char_count = entry.prompt_summary.char_count,
        prompt_summary = %entry.prompt_summary.summary,
    );
}

pub fn log_debug_operation(
    config: &Config,
    api_key: &ApiKeyInfo,
    method: &str,
    path: &str,
    operation: &str,
    detail: impl AsRef<str>,
) {
    if !config.debug_output {
        return;
    }

    info!(
        event = "debug_operation",
        api_key_masked = api_key.masked(),
        api_key_hash = api_key.hash(),
        api_key_present = api_key.is_present(),
        method,
        path,
        operation,
        detail = detail.as_ref(),
    );
}

pub fn error_chain(error: &(dyn Error + 'static)) -> String {
    let mut parts = vec![error.to_string()];
    let mut source = error.source();

    while let Some(error) = source {
        parts.push(error.to_string());
        source = error.source();
    }

    parts.join("; caused by: ")
}

pub struct UpstreamRetryLog<'a> {
    pub api_key: &'a ApiKeyInfo,
    pub method: &'a str,
    pub path: &'a str,
    pub attempt: usize,
    pub upstream_status: u16,
    pub retry_reason: &'a str,
    pub delay: Duration,
    pub retry_after: Option<String>,
}

pub fn log_upstream_retry(entry: UpstreamRetryLog<'_>) {
    if let Some(observability) = observability::global() {
        observability.record_retry(
            entry.path,
            entry.upstream_status,
            entry.attempt,
            entry.retry_reason,
            entry.delay.as_millis().min(u128::from(u64::MAX)) as u64,
            entry.api_key.hash(),
        );
    }
    warn!(
        event = "upstream_429_retry",
        api_key_masked = entry.api_key.masked(),
        api_key_hash = entry.api_key.hash(),
        api_key_present = entry.api_key.is_present(),
        method = entry.method,
        path = entry.path,
        attempt = entry.attempt,
        upstream_status = entry.upstream_status,
        retry_reason = entry.retry_reason,
        delay_ms = entry.delay.as_millis() as u64,
        retry_after = entry.retry_after,
        "upstream returned retryable response; retrying transparently"
    );
}

/// Borrowed view of a cyber-policy black-log record. The actual write is
/// performed asynchronously by [`crate::forensic_writer::ForensicWriter`].
pub struct CyberPolicyBlackRequestLog<'a> {
    pub config: &'a Config,
    pub api_key: &'a ApiKeyInfo,
    pub method: &'a str,
    pub uri: &'a Uri,
    pub path: &'a str,
    pub headers: &'a HeaderMap,
    pub original_body: &'a [u8],
    pub body: &'a [u8],
    pub client_ip: IpAddr,
    pub prompt_summary: &'a PromptSummary,
    pub queue_wait: Duration,
}

/// Borrowed view of a 502/503 forensic-log record. The actual write is performed
/// asynchronously by [`crate::forensic_writer::ForensicWriter`].
pub struct BadGatewayRequestLog<'a> {
    pub config: &'a Config,
    pub api_key: &'a ApiKeyInfo,
    pub method: &'a str,
    pub uri: &'a Uri,
    pub path: &'a str,
    pub headers: &'a HeaderMap,
    pub original_body: &'a [u8],
    pub body: &'a [u8],
    pub client_ip: IpAddr,
    pub prompt_summary: &'a PromptSummary,
    pub queue_wait: Duration,
    pub trigger: BadGatewayTriggerLog<'a>,
}

pub struct BadGatewayTriggerLog<'a> {
    pub condition: &'static str,
    pub upstream_status: Option<u16>,
    pub upstream_headers: Option<&'a HeaderMap>,
    pub upstream_body: Option<&'a [u8]>,
    pub upstream_error: Option<&'a str>,
    pub retry_reason: Option<&'static str>,
    pub retry_attempt: Option<usize>,
    pub retry_delay: Option<Duration>,
    pub retry_after: Option<&'a str>,
}

pub(crate) fn cyber_policy_black_request_json(
    entry: &CyberPolicyBlackRequestLog<'_>,
    now_seconds: u64,
    now_millis: u128,
) -> Value {
    json!({
        "event": "cyber_policy_black",
        "blocked_at_unix_ms": now_millis,
        "blocked_at_utc": utc_second_string(now_seconds),
        "recorded_at_utc_minute": utc_minute_string(now_seconds),
        "warning": "This forensic record may contain bounded request bodies and prompt text. Full bearer keys are included only when AI_BRIDGE_FORENSIC_INCLUDE_API_KEY is enabled. Treat this file as sensitive material.",
        "ban": {
            "reason": "upstream_cyber_policy",
            "key_blacklist_file": entry.config.cyber_policy_blacklist_file.display().to_string(),
            "black_log_file": entry.config.cyber_policy_black_log_file.display().to_string(),
            "ip_blacklist_file": entry.config.cyber_policy_ip_blacklist_file.display().to_string(),
        },
        "request": {
            "client_ip": entry.client_ip.to_string(),
            "method": entry.method,
            "uri": entry.uri.to_string(),
            "path": entry.path,
            "query": entry.uri.query(),
            "api_key_full": entry.api_key.raw_secret_for_forensic_log(),
            "api_key_masked": (!entry.config.forensic_include_api_key).then(|| entry.api_key.masked()),
            "api_key_hash": (!entry.config.forensic_include_api_key).then(|| entry.api_key.hash()),
            "headers": headers_json(entry.headers),
            "original_body": bytes_json(entry.original_body),
            "body": bytes_json(entry.body),
            "body_was_modified_before_upstream": entry.original_body != entry.body,
            "body_was_buffered": !entry.body.is_empty() || !entry.original_body.is_empty(),
            "snapshot_limit_bytes": entry.config.forensic_max_body_bytes,
            "prompt_text_count": entry.prompt_summary.text_count,
            "prompt_char_count": entry.prompt_summary.char_count,
            "prompt_summary": entry.prompt_summary.summary,
            "queue_wait_ms": entry.queue_wait.as_millis() as u64,
        }
    })
}

pub struct CyberPolicyWhiteRequestLog<'a> {
    pub config: &'a Config,
    pub api_key: &'a ApiKeyInfo,
    pub method: &'a str,
    pub uri: &'a Uri,
    pub path: &'a str,
    pub headers: &'a HeaderMap,
    pub original_body: &'a [u8],
    pub body: &'a [u8],
    pub client_ip: IpAddr,
    pub prompt_summary: &'a PromptSummary,
    pub queue_wait: Duration,
    pub trigger_count: usize,
}

pub(crate) fn cyber_policy_white_request_json(
    entry: &CyberPolicyWhiteRequestLog<'_>,
    now_seconds: u64,
    now_millis: u128,
) -> Value {
    json!({
        "event": "cyber_policy_white",
        "triggered_at_unix_ms": now_millis,
        "triggered_at_utc": utc_second_string(now_seconds),
        "trigger_count": entry.trigger_count,
        "request": {
            "client_ip": entry.client_ip.to_string(),
            "method": entry.method,
            "uri": entry.uri.to_string(),
            "path": entry.path,
            "query": entry.uri.query(),
            "api_key_full": entry.api_key.raw_secret_for_forensic_log(),
            "api_key_masked": (!entry.config.forensic_include_api_key).then(|| entry.api_key.masked()),
            "api_key_hash": (!entry.config.forensic_include_api_key).then(|| entry.api_key.hash()),
            "headers": headers_json(entry.headers),
            "original_body": bytes_json(entry.original_body),
            "body": bytes_json(entry.body),
            "body_was_modified_before_upstream": entry.original_body != entry.body,
            "snapshot_limit_bytes": entry.config.forensic_max_body_bytes,
            "prompt_text_count": entry.prompt_summary.text_count,
            "prompt_char_count": entry.prompt_summary.char_count,
            "prompt_summary": entry.prompt_summary.summary,
            "queue_wait_ms": entry.queue_wait.as_millis() as u64,
        }
    })
}

pub(crate) fn bad_gateway_request_json(
    entry: &BadGatewayRequestLog<'_>,
    now_seconds: u64,
    now_millis: u128,
) -> Value {
    json!({
        "event": "bad_gateway_request_forensic",
        "recorded_at_unix_ms": now_millis,
        "recorded_at_utc_minute": utc_minute_string(now_seconds),
        "warning": "This forensic record may contain bounded request and upstream bodies. Full bearer keys are included only when AI_BRIDGE_FORENSIC_INCLUDE_API_KEY is enabled. Treat this file as sensitive material.",
        "trigger": {
            "condition": entry.trigger.condition,
            "upstream_status": entry.trigger.upstream_status,
            "upstream_headers": entry.trigger.upstream_headers.map(headers_json),
            "upstream_body": entry.trigger.upstream_body.map(bytes_json),
            "upstream_error": entry.trigger.upstream_error,
            "retry_reason": entry.trigger.retry_reason,
            "retry_attempt": entry.trigger.retry_attempt,
            "retry_delay_ms": entry.trigger.retry_delay.map(|delay| delay.as_millis() as u64),
            "retry_after": entry.trigger.retry_after,
        },
        "request": {
            "client_ip": entry.client_ip.to_string(),
            "method": entry.method,
            "uri": entry.uri.to_string(),
            "path": entry.path,
            "query": entry.uri.query(),
            "api_key_full": entry.api_key.raw_secret_for_forensic_log(),
            "api_key_masked": (!entry.config.forensic_include_api_key).then(|| entry.api_key.masked()),
            "api_key_hash": (!entry.config.forensic_include_api_key).then(|| entry.api_key.hash()),
            "headers": headers_json(entry.headers),
            "original_body": bytes_json(entry.original_body),
            "body": bytes_json(entry.body),
            "body_was_modified_before_upstream": entry.original_body != entry.body,
            "snapshot_limit_bytes": entry.config.forensic_max_body_bytes,
            "prompt_text_count": entry.prompt_summary.text_count,
            "prompt_char_count": entry.prompt_summary.char_count,
            "prompt_summary": entry.prompt_summary.summary,
            "queue_wait_ms": entry.queue_wait.as_millis() as u64,
        }
    })
}

pub(crate) fn bad_gateway_request_log_filename(unix_seconds: u64) -> String {
    format!("{}.log", utc_minute_string(unix_seconds))
}

pub(crate) fn unix_timestamp_parts(time: SystemTime) -> (u64, u128) {
    let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    (duration.as_secs(), duration.as_millis())
}

fn headers_json(headers: &HeaderMap) -> Value {
    let mut object = Map::new();
    for (name, value) in headers.iter() {
        object
            .entry(name.as_str().to_owned())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .expect("inserted header entry is always an array")
            .push(header_value_json(name.as_str(), value.as_bytes()));
    }
    Value::Object(object)
}

fn header_value_json(name: &str, bytes: &[u8]) -> Value {
    if is_sensitive_header(name) {
        return json!({
            "len": bytes.len(),
            "redacted": true,
        });
    }

    bytes_json(bytes)
}

fn is_sensitive_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization" | "proxy-authorization" | "cookie" | "set-cookie"
    )
}

fn bytes_json(bytes: &[u8]) -> Value {
    json!({
        "len": bytes.len(),
        "utf8_lossy": String::from_utf8_lossy(bytes),
    })
}

fn utc_minute_string(unix_seconds: u64) -> String {
    let minute_seconds = unix_seconds - (unix_seconds % 60);
    let (year, month, day, hour, minute) = unix_seconds_to_utc_minute(minute_seconds);
    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}Z")
}

fn utc_second_string(unix_seconds: u64) -> String {
    let (year, month, day, hour, minute) = unix_seconds_to_utc_minute(unix_seconds);
    let second = (unix_seconds % 60) as u32;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn unix_seconds_to_utc_minute(unix_seconds: u64) -> (i32, u32, u32, u32, u32) {
    let days = (unix_seconds / 86_400) as i64;
    let seconds_of_day = unix_seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = (seconds_of_day / 3_600) as u32;
    let minute = ((seconds_of_day % 3_600) / 60) as u32;
    (year, month, day, hour, minute)
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };

    (year as i32, month as u32, day as u32)
}

fn file_writer(path: &Path) -> Result<(tracing_appender::non_blocking::NonBlocking, WorkerGuard)> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let filename = path
        .file_name()
        .context("AI_BRIDGE_LOG_FILE must include a file name")?;
    let file_appender = tracing_appender::rolling::never(directory, filename);
    Ok(tracing_appender::non_blocking(file_appender))
}
