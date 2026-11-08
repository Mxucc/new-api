use std::time::Duration;

use axum::http::StatusCode;
use bytes::Bytes;
use serde_json::Value;
use tracing::warn;

use crate::config::Config;

use super::{ProxyRequest, contains_substring_case_insensitive, is_event_stream_headers};

pub(crate) struct BufferedUpstreamResponse {
    pub(crate) upstream_status: u16,
    pub(crate) headers: reqwest::header::HeaderMap,
    pub(crate) body: Bytes,
}

impl BufferedUpstreamResponse {
    pub(super) async fn from_response(upstream: reqwest::Response) -> reqwest::Result<Self> {
        let upstream_status = upstream.status().as_u16();
        let headers = upstream.headers().clone();
        let body = upstream.bytes().await?;

        Ok(Self {
            upstream_status,
            headers,
            body,
        })
    }
}

pub(crate) struct RetryPlan {
    pub(crate) delay: Duration,
    pub(crate) retry_after: Option<String>,
    pub(crate) reason: &'static str,
    pub(crate) upstream_status: u16,
}

pub(super) enum StreamUpstreamResponse {
    Response(reqwest::Response),
    Buffered(BufferedUpstreamResponse),
}

pub(crate) enum UpstreamRetryDecision {
    Forward(reqwest::Response),
    ForwardBuffered(BufferedUpstreamResponse),
    Retry {
        retry_plan: RetryPlan,
        upstream_headers: reqwest::header::HeaderMap,
        upstream_body: Option<Bytes>,
    },
}

pub(super) const RETRY_REASON_RATE_LIMITED: &str = "rate_limited";
pub(super) const RETRY_REASON_CAPACITY: &str = "capacity";
pub(super) const RETRY_REASON_BAD_GATEWAY: &str = "bad_gateway";

const NEW_API_INSUFFICIENT_USER_QUOTA_CODE: &str = "insufficient_user_quota";
const NEW_API_PRE_CONSUME_TOKEN_QUOTA_FAILED_CODE: &str = "pre_consume_token_quota_failed";
const OPENAI_INSUFFICIENT_QUOTA_CODE: &str = "insufficient_quota";

/// 轻量级结构体，仅用于检测 cyber_policy 错误
/// 使用 IgnoredAny 跳过不需要的字段，避免完整 JSON 树分配
#[derive(serde::Deserialize)]
pub(super) struct CyberPolicyCheck {
    #[serde(default)]
    pub(super) code: Option<String>,
    #[serde(default)]
    pub(super) error: Option<Box<CyberPolicyCheck>>,
}

pub(crate) fn body_has_cyber_policy_error(body: &[u8]) -> bool {
    // 优化：快速路径 - 如果响应体不包含 "cyber_policy" 字符串，直接返回 false
    // 这避免了对绝大多数正常响应进行昂贵的 JSON 解析
    if !contains_substring_case_insensitive(body, b"cyber_policy") {
        return false;
    }

    // 使用轻量级结构体解析，只检查 code 和 error 字段
    serde_json::from_slice::<CyberPolicyCheck>(body)
        .ok()
        .is_some_and(|check| cyber_policy_check_has_error(&check))
}

pub(super) fn cyber_policy_check_has_error(check: &CyberPolicyCheck) -> bool {
    check
        .code
        .as_deref()
        .is_some_and(|code| code.eq_ignore_ascii_case(super::CYBER_POLICY_CODE))
        || check
            .error
            .as_ref()
            .is_some_and(|nested| cyber_policy_check_has_error(nested))
}

pub(super) fn buffered_upstream_response_has_cyber_policy_error(
    upstream: &BufferedUpstreamResponse,
) -> bool {
    body_has_cyber_policy_error(&upstream.body)
        || (is_event_stream_headers(&upstream.headers)
            && super::sse_body_has_cyber_policy_error(&upstream.body))
}

pub(super) fn retry_plan_for_retryable_upstream_response(
    ctx: &ProxyRequest,
    upstream: &reqwest::Response,
    previous_retry_count: usize,
) -> Option<RetryPlan> {
    let reason = retry_reason_for_status(&ctx.config, upstream.status().as_u16())?;

    retry_plan_for_reason(
        ctx,
        upstream.headers(),
        upstream.status().as_u16(),
        previous_retry_count,
        reason,
    )
}

pub(crate) async fn classify_upstream_response_for_retry(
    ctx: &ProxyRequest,
    upstream: reqwest::Response,
    previous_retry_count: usize,
) -> reqwest::Result<UpstreamRetryDecision> {
    if retry_reason_for_status(&ctx.config, upstream.status().as_u16()).is_some()
        && !is_event_stream_headers(upstream.headers())
    {
        let buffered = BufferedUpstreamResponse::from_response(upstream).await?;
        if let Some(retry_plan) =
            retry_plan_for_buffered_upstream_response(ctx, &buffered, previous_retry_count)
        {
            return Ok(UpstreamRetryDecision::Retry {
                retry_plan,
                upstream_headers: buffered.headers.clone(),
                upstream_body: Some(buffered.body.clone()),
            });
        }

        return Ok(UpstreamRetryDecision::ForwardBuffered(buffered));
    }

    if let Some(retry_plan) =
        retry_plan_for_retryable_upstream_response(ctx, &upstream, previous_retry_count)
    {
        return Ok(UpstreamRetryDecision::Retry {
            retry_plan,
            upstream_headers: upstream.headers().clone(),
            upstream_body: None,
        });
    }

    Ok(UpstreamRetryDecision::Forward(upstream))
}

pub(super) fn retry_plan_for_buffered_upstream_response(
    ctx: &ProxyRequest,
    upstream: &BufferedUpstreamResponse,
    previous_retry_count: usize,
) -> Option<RetryPlan> {
    if buffered_upstream_response_has_cyber_policy_error(upstream) {
        return None;
    }

    if buffered_upstream_response_has_non_retryable_error(upstream) {
        warn!(
            event = "upstream_retry_skipped_non_retryable_error",
            api_key_masked = ctx.api_key.masked(),
            api_key_hash = ctx.api_key.hash(),
            method = ctx.method_str(),
            path = %ctx.path,
            upstream_status = upstream.upstream_status,
            previous_retry_count,
            "upstream response looks non-retryable; returning it without retry"
        );
        return None;
    }

    let reason = retry_reason_for_status(&ctx.config, upstream.upstream_status)
        .or_else(|| retry_reason_for_body(upstream.upstream_status, &upstream.body))?;

    retry_plan_for_reason(
        ctx,
        &upstream.headers,
        upstream.upstream_status,
        previous_retry_count,
        reason,
    )
}

pub(super) fn retry_plan_for_reason(
    ctx: &ProxyRequest,
    headers: &reqwest::header::HeaderMap,
    upstream_status: u16,
    previous_retry_count: usize,
    reason: &'static str,
) -> Option<RetryPlan> {
    let max_retries = retry_max_retries_for_reason(&ctx.config, reason);
    if max_retries.is_some_and(|max_retries| previous_retry_count >= max_retries) {
        warn!(
            event = "upstream_retry_exhausted",
            api_key_masked = ctx.api_key.masked(),
            api_key_hash = ctx.api_key.hash(),
            method = ctx.method_str(),
            path = %ctx.path,
            upstream_status,
            retry_reason = reason,
            max_retries,
            previous_retry_count,
            "upstream retry limit exhausted; returning upstream response"
        );
        return None;
    }

    Some(
        retry_plan_for_reason_delay(headers, previous_retry_count, &ctx.config, reason)
            .with_reason(reason, upstream_status),
    )
}

pub(crate) fn retry_plan_for_upstream_request_error(
    ctx: &ProxyRequest,
    previous_retry_count: usize,
) -> Option<RetryPlan> {
    if !ctx.config.upstream_502_cooldown_enabled {
        return None;
    }

    retry_plan_for_reason(
        ctx,
        &reqwest::header::HeaderMap::new(),
        StatusCode::BAD_GATEWAY.as_u16(),
        previous_retry_count,
        RETRY_REASON_BAD_GATEWAY,
    )
}

fn retry_plan_for_reason_delay(
    headers: &reqwest::header::HeaderMap,
    previous_retry_count: usize,
    config: &Config,
    reason: &'static str,
) -> RetryPlan {
    if reason == RETRY_REASON_BAD_GATEWAY {
        return retry_plan_for_bad_gateway(config);
    }

    retry_plan_from_headers(headers, previous_retry_count, config)
}

pub(super) fn retry_plan_for_bad_gateway(config: &Config) -> RetryPlan {
    RetryPlan {
        delay: config.upstream_502_cooldown_delay,
        retry_after: None,
        reason: RETRY_REASON_BAD_GATEWAY,
        upstream_status: StatusCode::BAD_GATEWAY.as_u16(),
    }
}

fn retry_max_retries_for_reason(config: &Config, reason: &str) -> Option<usize> {
    if reason == RETRY_REASON_BAD_GATEWAY {
        config.upstream_502_max_retries
    } else {
        config.upstream_429_max_retries
    }
}

pub(super) fn retry_plan_from_headers(
    headers: &reqwest::header::HeaderMap,
    previous_retry_count: usize,
    config: &Config,
) -> RetryPlan {
    let retry_after = headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    let header_delay = retry_after
        .as_deref()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs);

    let delay = header_delay.unwrap_or_else(|| exponential_429_delay(previous_retry_count, config));

    RetryPlan {
        delay: delay.min(config.upstream_429_max_delay),
        retry_after,
        reason: RETRY_REASON_RATE_LIMITED,
        upstream_status: reqwest::StatusCode::TOO_MANY_REQUESTS.as_u16(),
    }
}

impl RetryPlan {
    fn with_reason(mut self, reason: &'static str, upstream_status: u16) -> Self {
        self.reason = reason;
        self.upstream_status = upstream_status;
        self
    }
}

pub(super) fn retry_reason_for_status(config: &Config, status: u16) -> Option<&'static str> {
    match status {
        429 => Some(RETRY_REASON_RATE_LIMITED),
        502 if config.upstream_502_cooldown_enabled => Some(RETRY_REASON_BAD_GATEWAY),
        _ => None,
    }
}

/// 字节级快速路径：检查 body 是否可能包含 quota 错误关键词。
/// 如果不包含任何关键词，跳过昂贵的 JSON 解析。
fn body_may_have_quota_error(body: &[u8]) -> bool {
    contains_substring_case_insensitive(body, b"insufficient")
        || contains_substring_case_insensitive(body, b"balance")
        || contains_substring_case_insensitive(body, b"quota")
        || contains_substring_case_insensitive(body, b"credit")
        || contains_substring_case_insensitive(body, b"billing")
        || body
            .windows(6)
            .any(|w| w == "额度".as_bytes() || w == "余额".as_bytes())
        || body.windows(9).any(|w| w == "预扣费".as_bytes())
        || body.windows(6).any(|w| w == "订阅".as_bytes())
}

pub(super) fn retry_reason_for_body(upstream_status: u16, body: &[u8]) -> Option<&'static str> {
    if upstream_status == StatusCode::SERVICE_UNAVAILABLE.as_u16() {
        return None;
    }

    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        if retryable_error_value(&value) {
            return Some(RETRY_REASON_CAPACITY);
        }
        return None;
    }

    if !(200..300).contains(&upstream_status) {
        let body = String::from_utf8_lossy(body);
        if has_non_retryable_quota_error_text(&body) {
            return None;
        }
        if has_retryable_capacity_text(&body) {
            return Some(RETRY_REASON_CAPACITY);
        }
    }

    None
}

pub(super) fn buffered_upstream_response_has_non_retryable_error(
    upstream: &BufferedUpstreamResponse,
) -> bool {
    if upstream.upstream_status < 400 {
        return false;
    }

    // 快速路径：如果 body 不包含任何 quota 关键词，跳过 quota 相关的 JSON 深度遍历
    let may_have_quota = body_may_have_quota_error(&upstream.body);

    match serde_json::from_slice::<Value>(&upstream.body) {
        Ok(value) => {
            (may_have_quota && json_value_has_non_retryable_quota_error(&value))
                || (upstream.upstream_status == StatusCode::SERVICE_UNAVAILABLE.as_u16()
                    || (400..500).contains(&upstream.upstream_status))
                    && json_value_has_non_retryable_request_error(&value)
        }
        Err(_) => {
            if !may_have_quota {
                return false;
            }
            let body = String::from_utf8_lossy(&upstream.body);
            has_non_retryable_quota_error_text(&body)
        }
    }
}

pub(crate) fn final_upstream_error_should_passthrough(status: u16, body: &[u8]) -> bool {
    if status < 400 {
        return false;
    }
    if status != StatusCode::TOO_MANY_REQUESTS.as_u16() {
        return true;
    }

    let may_have_quota = body_may_have_quota_error(body);
    match serde_json::from_slice::<Value>(body) {
        Ok(value) => {
            json_value_has_new_api_error_type(&value)
                || (may_have_quota && json_value_has_non_retryable_quota_error(&value))
        }
        Err(_) => {
            may_have_quota && has_non_retryable_quota_error_text(&String::from_utf8_lossy(body))
        }
    }
}

fn json_value_has_new_api_error_type(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(json_value_has_new_api_error_type),
        Value::Object(object) => {
            object
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|error_type| error_type.eq_ignore_ascii_case("new_api_error"))
                || object
                    .get("error")
                    .is_some_and(json_value_has_new_api_error_type)
        }
        _ => false,
    }
}

fn json_value_has_non_retryable_quota_error(value: &Value) -> bool {
    match value {
        Value::String(text) => has_non_retryable_quota_error_text(text),
        Value::Array(items) => items.iter().any(json_value_has_non_retryable_quota_error),
        Value::Object(object) => {
            object
                .get("error")
                .is_some_and(json_value_has_non_retryable_quota_error)
                || ["message", "code", "type"]
                    .iter()
                    .filter_map(|field| object.get(*field).and_then(Value::as_str))
                    .any(has_non_retryable_quota_error_text)
        }
        _ => false,
    }
}

fn json_value_has_non_retryable_request_error(value: &Value) -> bool {
    match value {
        Value::String(text) => has_non_retryable_request_error_text(text),
        Value::Array(items) => items.iter().any(json_value_has_non_retryable_request_error),
        Value::Object(object) => {
            object
                .get("error")
                .is_some_and(json_value_has_non_retryable_request_error)
                || ["message", "code", "type", "param"]
                    .iter()
                    .filter_map(|field| object.get(*field).and_then(Value::as_str))
                    .any(has_non_retryable_request_error_text)
        }
        _ => false,
    }
}

fn has_non_retryable_request_error_text(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains("invalid_request_error")
        || text.contains("invalid_request")
        || text.contains("bad_request")
        || text.contains("validation_error")
        || text.contains("unsupported_parameter")
        || text.contains("unknown_parameter")
        || text.contains("missing_required_parameter")
        || text.contains("invalid schema")
        || text.contains("invalid json schema")
        || text.contains("invalid response_format")
        || text.contains("invalid text.format")
        || text.contains("unsupported parameter")
        || text.contains("unknown parameter")
        || (text.contains("invalid") && text_mentions_schema_or_format(&text))
        || (text.contains("missing")
            && text.contains("name")
            && text_mentions_schema_or_format(&text))
}

fn text_mentions_schema_or_format(text: &str) -> bool {
    text.contains("response_format")
        || text.contains("text.format")
        || text.contains("json_schema")
        || text.contains("schema")
        || text.contains("format")
}

pub(super) fn has_non_retryable_quota_error_text(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains(NEW_API_INSUFFICIENT_USER_QUOTA_CODE)
        || text.contains(NEW_API_PRE_CONSUME_TOKEN_QUOTA_FAILED_CODE)
        || text.contains(OPENAI_INSUFFICIENT_QUOTA_CODE)
        || text.contains("insufficient balance")
        || text.contains("balance insufficient")
        || text.contains("not enough balance")
        || text.contains("not enough credit")
        || text.contains("credit balance is too low")
        || text.contains("quota is not enough")
        || text.contains("not enough quota")
        || text.contains("billing hard limit")
        || text.contains("用户额度不足")
        || text.contains("预扣费额度失败")
        || text.contains("订阅额度不足")
        || text.contains("余额不足")
        || text.contains("额度不足")
}

fn retryable_error_value(value: &Value) -> bool {
    if let Some(text) = value.as_str() {
        if has_non_retryable_quota_error_text(text) {
            return false;
        }
        return has_retryable_capacity_text(text);
    }

    let Some(object) = value.as_object() else {
        return false;
    };

    if json_value_has_non_retryable_quota_error(value) {
        return false;
    }

    if object.get("error").is_some_and(retryable_error_value) {
        return true;
    }

    ["message", "code", "type"]
        .iter()
        .filter_map(|field| object.get(*field).and_then(Value::as_str))
        .any(has_retryable_capacity_text)
}

pub(super) fn has_retryable_capacity_text(text: &str) -> bool {
    // 优化：使用单次遍历检查多个模式，避免 6 次独立的字符串搜索
    // 转换为小写后进行匹配，比多次 case-insensitive 搜索更快
    let text_lower = text.to_ascii_lowercase();
    text_lower.contains("at capacity")
        || text_lower.contains("at_capacity")
        || text_lower.contains("server overloaded")
        || text_lower.contains("overloaded")
        || text_lower.contains("upstream failed")
        || text_lower.contains("upstream_failed")
}

fn exponential_429_delay(previous_retry_count: usize, config: &Config) -> Duration {
    let multiplier = 1_u32
        .checked_shl(previous_retry_count.min(6) as u32)
        .unwrap_or(64);
    config
        .upstream_429_initial_delay
        .saturating_mul(multiplier)
        .min(config.upstream_429_max_delay)
}
