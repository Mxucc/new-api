use std::{
    error::Error,
    fmt,
    net::IpAddr,
    time::{Duration, Instant},
};

use aho_corasick::AhoCorasick;
use axum::{
    body::Body,
    http::{HeaderMap, HeaderName, HeaderValue, Method, Response, StatusCode, Uri, header},
};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use serde_json::{Value, json};
use url::Url;

use crate::{
    auth::ApiKeyInfo,
    config::Config,
    logger::{self, PromptSummary},
};

use super::{
    CYBER_POLICY_BLOCK_MESSAGE, CYBER_POLICY_CODE, ProxyRequest, REQUEST_BODY_FORMAT_PATTERN,
    REQUEST_BODY_GPT_IMAGE_PATTERN, REQUEST_BODY_MARKER_MATCHER, REQUEST_BODY_MAX_TOKENS_PATTERN,
    REQUEST_BODY_RESPONSE_FORMAT_PATTERN, RequestBodyInspectionPlan,
    sse::sse_event_bytes,
    tool_filter::{ToolFilterIndex, request_body_may_need_tool_filter},
};

#[derive(Debug)]
pub(super) enum UploadBodyStreamError {
    Read(axum::Error),
    LimitExceeded { limit: usize },
}

impl fmt::Display for UploadBodyStreamError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => write!(formatter, "failed to read upload body: {error}"),
            Self::LimitExceeded { limit } => {
                write!(
                    formatter,
                    "upload body exceeds configured limit of {limit} bytes"
                )
            }
        }
    }
}

impl Error for UploadBodyStreamError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read(error) => Some(error),
            Self::LimitExceeded { .. } => None,
        }
    }
}

pub(super) fn limited_upload_body_stream(
    body: Body,
    max_bytes: usize,
) -> impl Stream<Item = Result<Bytes, UploadBodyStreamError>> + Send {
    async_stream::try_stream! {
        let mut stream = body.into_data_stream();
        let mut seen = 0_usize;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(UploadBodyStreamError::Read)?;
            seen = seen
                .checked_add(chunk.len())
                .ok_or(UploadBodyStreamError::LimitExceeded { limit: max_bytes })?;
            if seen > max_bytes {
                Err(UploadBodyStreamError::LimitExceeded { limit: max_bytes })?;
            }
            yield chunk;
        }
    }
}

pub(super) fn request_error_is_upload_body_limit(error: &reqwest::Error) -> bool {
    let mut current: Option<&(dyn Error + 'static)> = Some(error);
    while let Some(source) = current {
        if matches!(
            source.downcast_ref::<UploadBodyStreamError>(),
            Some(UploadBodyStreamError::LimitExceeded { .. })
        ) {
            return true;
        }
        current = source.source();
    }
    false
}

pub(super) fn should_stream_request_body_without_buffering(
    method: &Method,
    path: &str,
    headers: &HeaderMap,
    max_body_bytes: usize,
) -> bool {
    if !request_method_can_have_upload_body(method) {
        return false;
    }

    // Upload-like paths (e.g., /v1/files, /v1/images/edits) should always stream,
    // regardless of headers or content-length, to avoid OOM on large file uploads.
    if is_upload_like_path(path) {
        return true;
    }

    // For non-upload paths, only stream if the request looks like an upload
    // (by content-type) and either exceeds max_body_bytes or has no content-length.
    if !has_upload_like_content_type(headers) {
        return false;
    }

    request_content_length(headers)
        .is_some_and(|content_length| content_length > max_body_bytes as u64)
        || request_content_length(headers).is_none()
}

fn request_method_can_have_upload_body(method: &Method) -> bool {
    matches!(method, &Method::POST | &Method::PUT | &Method::PATCH)
}

/// Upload-like paths that should always stream request bodies without buffering,
/// regardless of headers or content-length, to avoid OOM on large file uploads.
const UPLOAD_PATHS: &[&str] = &[
    "/v1/files",
    "/v1/uploads",
    "/v1/images/edits",
    "/v1/images/variations",
    "/v1/audio/transcriptions",
    "/v1/audio/translations",
];

fn is_upload_like_path(path: &str) -> bool {
    let path = path.trim_end_matches('/');
    UPLOAD_PATHS
        .iter()
        .any(|&upload_path| path == upload_path || path.starts_with(&format!("{}/", upload_path)))
}

fn has_upload_like_content_type(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|content_type| {
            let content_type = content_type.to_ascii_lowercase();
            content_type.contains("multipart/form-data")
                || content_type.contains("application/octet-stream")
        })
        .unwrap_or(false)
}

pub(super) fn request_content_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
}

pub(super) fn request_content_length_exceeds_limit(
    headers: &HeaderMap,
    max_body_bytes: usize,
) -> Option<u64> {
    request_content_length(headers).filter(|content_length| *content_length > max_body_bytes as u64)
}

pub(super) fn is_stream_request(_headers: &HeaderMap, body: &Bytes) -> bool {
    // 优化：先用字节搜索快速过滤，避免对不包含 "stream" 或 "stream_options" 的请求进行 JSON 解析
    // 使用 memchr 的 memmem 进行高效字节搜索，避免 windows() 的边界问题和并发问题
    use memchr::memmem;

    let has_stream_marker = memmem::find(body, b"\"stream\"").is_some()
        || memmem::find(body, b"\"stream_options\"").is_some();

    if !has_stream_marker {
        return false;
    }

    // 完整逻辑：检查 stream 字段和 stream_options
    // 如果存在 stream_options，则认为是流式请求（即使 stream 字段不存在或为 false）
    serde_json::from_slice::<Value>(body)
        .ok()
        .map(|value| {
            // 检查 stream 字段是否为 true
            if value
                .get("stream")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return true;
            }
            // 如果 stream_options 存在，则认为是流式请求
            value.get("stream_options").is_some()
        })
        .unwrap_or(false)
}

pub(crate) fn stream_event_stall_timeout_for_request(ctx: &ProxyRequest) -> Option<Duration> {
    if is_playground_path(&ctx.path) {
        return None;
    }

    if ctx.is_image_generation {
        return None;
    }

    ctx.config.stream_event_stall_timeout
}

pub(super) fn is_playground_path(path: &str) -> bool {
    let path = path.trim_end_matches('/');
    path == "/pg" || path.starts_with("/pg/")
}

pub(super) fn effective_client_ip(headers: &HeaderMap, peer_ip: IpAddr) -> IpAddr {
    if is_trusted_forwarding_peer(peer_ip) {
        forwarded_client_ip(headers).unwrap_or(peer_ip)
    } else {
        peer_ip
    }
}

fn is_trusted_forwarding_peer(peer_ip: IpAddr) -> bool {
    match peer_ip {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_private(),
        IpAddr::V6(ip) => ip.is_loopback() || ip.is_unique_local(),
    }
}

fn forwarded_client_ip(headers: &HeaderMap) -> Option<IpAddr> {
    forwarded_for_client_ip(headers).or_else(|| header_ip(headers, "x-real-ip"))
}

fn forwarded_for_client_ip(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .find_map(parse_visible_ip)
}

fn header_ip(headers: &HeaderMap, name: &str) -> Option<IpAddr> {
    headers
        .get_all(name)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find_map(parse_visible_ip)
}

fn parse_visible_ip(value: &str) -> Option<IpAddr> {
    let value = value.trim().trim_matches('"');
    if value.is_empty() || value.eq_ignore_ascii_case("unknown") {
        return None;
    }
    if let Ok(ip) = value.parse::<IpAddr>() {
        return Some(ip);
    }

    if let Some(bracketed) = value.strip_prefix('[')
        && let Some((ip, _suffix)) = bracketed.split_once(']')
    {
        return ip.parse::<IpAddr>().ok();
    }

    let (host, port) = value.rsplit_once(':')?;
    if host.contains(':') || !port.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    host.parse::<IpAddr>().ok()
}

pub(super) fn header_contains(headers: &HeaderMap, name: HeaderName, needle: &str) -> bool {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_ascii_lowercase().contains(needle))
        .unwrap_or(false)
}

pub(super) fn build_upstream_url(base_url: &Url, uri: &Uri) -> Url {
    let base = base_url.as_str().trim_end_matches('/');
    let path_and_query = uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or_else(|| uri.path());
    Url::parse(&format!("{base}{path_and_query}"))
        .expect("base URL and request URI should form a valid upstream URL")
}

fn should_skip_request_header(
    name: &HeaderName,
    internal_prefix: &str,
    connection_headers: &[HeaderName],
) -> bool {
    let name = name.as_str();
    is_hop_by_hop_header(name)
        || connection_headers
            .iter()
            .any(|connection_header| connection_header.as_str().eq_ignore_ascii_case(name))
        || name.eq_ignore_ascii_case("host")
        || name.eq_ignore_ascii_case("content-length")
        || name.eq_ignore_ascii_case("forwarded")
        || name.eq_ignore_ascii_case("x-forwarded-for")
        || name.eq_ignore_ascii_case("x-real-ip")
        || name
            .to_ascii_lowercase()
            .starts_with(&internal_prefix.to_ascii_lowercase())
}

pub(super) fn upstream_request_headers(
    headers: &HeaderMap,
    internal_header_prefix: &str,
    client_ip: IpAddr,
) -> HeaderMap {
    let mut forwarded = HeaderMap::new();
    let connection_headers = connection_header_names(headers);
    for (name, value) in headers.iter() {
        if should_skip_request_header(name, internal_header_prefix, &connection_headers) {
            continue;
        }
        forwarded.append(name, value.clone());
    }
    set_client_ip_forwarding_headers(&mut forwarded, client_ip);
    forwarded
}

fn set_client_ip_forwarding_headers(headers: &mut HeaderMap, client_ip: IpAddr) {
    let client_ip = client_ip.to_string();

    headers.insert(
        "x-forwarded-for",
        HeaderValue::from_str(&client_ip).expect("IP address strings are valid headers"),
    );
    headers.insert(
        "x-real-ip",
        HeaderValue::from_str(&client_ip).expect("IP address strings are valid headers"),
    );
}

pub(super) fn filtered_response_headers(headers: &reqwest::header::HeaderMap) -> HeaderMap {
    let mut filtered = HeaderMap::new();
    let connection_headers = connection_header_names(headers);
    for (name, value) in headers.iter() {
        if is_hop_by_hop_header(name.as_str())
            || connection_headers.iter().any(|connection_header| {
                connection_header
                    .as_str()
                    .eq_ignore_ascii_case(name.as_str())
            })
        {
            continue;
        }
        filtered.append(name, value.clone());
    }
    filtered
}

fn connection_header_names(headers: &HeaderMap) -> Vec<HeaderName> {
    headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|value| HeaderName::from_bytes(value.trim().as_bytes()).ok())
        .collect()
}

fn is_hop_by_hop_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

pub(super) fn json_error_response(status: StatusCode, error: &str, code: &str) -> Response<Body> {
    let body = json!({
        "error": error,
        "code": code,
    })
    .to_string();

    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response
}

pub(super) fn payload_too_large_response(
    api_key: &ApiKeyInfo,
    method: &Method,
    path: &str,
    started_at: Instant,
) -> Response<Body> {
    let summary = PromptSummary::default();
    logger::log_request(logger::RequestLog {
        api_key,
        method: method.as_str(),
        path,
        status: StatusCode::PAYLOAD_TOO_LARGE.as_u16(),
        latency: started_at.elapsed(),
        queue_wait: Duration::ZERO,
        prompt_summary: &summary,
        queued: false,
        upstream_status: None,
    });

    json_error_response(
        StatusCode::PAYLOAD_TOO_LARGE,
        "request body too large",
        "payload_too_large",
    )
}

pub(super) fn cyber_policy_json_error_response() -> Response<Body> {
    json_error_response(
        StatusCode::FORBIDDEN,
        CYBER_POLICY_BLOCK_MESSAGE,
        CYBER_POLICY_CODE,
    )
}

pub(crate) fn cyber_policy_json_error_payload() -> Bytes {
    Bytes::from(
        json!({
            "error": CYBER_POLICY_BLOCK_MESSAGE,
            "code": CYBER_POLICY_CODE,
        })
        .to_string(),
    )
}

pub(crate) fn cyber_policy_sse_error_event() -> Bytes {
    sse_event_bytes("error", cyber_policy_json_error_payload().as_ref())
}

pub(super) fn is_image_model_path(path: &str) -> bool {
    matches!(path, "/v1/images/generations" | "/v1/images/edits")
}

fn request_body_marker_matcher() -> &'static AhoCorasick {
    REQUEST_BODY_MARKER_MATCHER.get_or_init(|| {
        AhoCorasick::new([
            "gpt-image",
            r#""response_format""#,
            r#""format""#,
            r#""max_tokens""#,
        ])
        .expect("failed to build request body marker matcher")
    })
}

pub(super) fn request_body_inspection_plan(
    method: &Method,
    path: &str,
    body: &Bytes,
    tool_filter_index: &ToolFilterIndex,
) -> RequestBodyInspectionPlan {
    let mut plan = RequestBodyInspectionPlan {
        should_filter_tools: request_body_may_need_tool_filter(body, tool_filter_index),
        ..RequestBodyInspectionPlan::default()
    };

    if body.is_empty() {
        return plan;
    }

    let may_need_image_model_check = !is_image_model_path(path);
    let may_need_chat_structured_output = method == Method::POST && path == "/v1/chat/completions";
    let may_need_responses_update = method == Method::POST && path == "/v1/responses";

    if !may_need_image_model_check && !may_need_chat_structured_output && !may_need_responses_update
    {
        return plan;
    }

    for mat in request_body_marker_matcher().find_iter(body) {
        match mat.pattern().as_usize() {
            REQUEST_BODY_GPT_IMAGE_PATTERN if may_need_image_model_check => {
                plan.should_check_image_model = true;
            }
            REQUEST_BODY_RESPONSE_FORMAT_PATTERN
                if may_need_chat_structured_output || may_need_responses_update =>
            {
                plan.should_normalize_structured_output = true;
            }
            REQUEST_BODY_FORMAT_PATTERN if may_need_responses_update => {
                plan.should_normalize_structured_output = true;
            }
            REQUEST_BODY_MAX_TOKENS_PATTERN if may_need_responses_update => {
                plan.should_normalize_responses_compat = true;
            }
            _ => {}
        }

        let fixed_marker_checks_complete = (!may_need_image_model_check
            || plan.should_check_image_model)
            && (!(may_need_chat_structured_output || may_need_responses_update)
                || plan.should_normalize_structured_output)
            && (!may_need_responses_update || plan.should_normalize_responses_compat);

        if fixed_marker_checks_complete {
            break;
        }
    }

    plan
}

/// 检查 gpt-image 模型是否调用了错误的接口。
///
/// gpt-image* 模型只能调用 /v1/images/generations 和 /v1/images/edits。
pub(super) fn should_block_image_model_request(model: &str, path: &str) -> bool {
    if !model.starts_with("gpt-image") {
        return false;
    }

    !is_image_model_path(path)
}

/// 生成图像模型被阻止时的错误响应。
///
/// 当 gpt-image* 模型调用了非图像接口时，返回 503 错误响应。
pub(super) fn image_model_blocked_response(
    config: &Config,
    api_key: &ApiKeyInfo,
    method: &Method,
    path: &str,
    model: &str,
    started_at: Instant,
) -> Response<Body> {
    logger::log_debug_operation(
        config,
        api_key,
        method.as_str(),
        path,
        "image_model_blocked",
        format!("model={} path={}", model, path),
    );

    let summary = PromptSummary::default();
    logger::log_request(logger::RequestLog {
        api_key,
        method: method.as_str(),
        path,
        status: StatusCode::SERVICE_UNAVAILABLE.as_u16(),
        latency: started_at.elapsed(),
        queue_wait: Duration::ZERO,
        prompt_summary: &summary,
        queued: false,
        upstream_status: None,
    });

    let error_body = json!({
        "error": {
            "message": format!("model {} is only supported on /v1/images/generations and /v1/images/edits", model),
            "type": "invalid_request_error",
            "code": "model_not_supported"
        }
    })
    .to_string();

    let mut response = Response::new(Body::from(error_body));
    *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response
}
