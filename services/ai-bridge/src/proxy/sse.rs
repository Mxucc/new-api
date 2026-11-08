use std::{pin::Pin, time::Duration};

use axum::{
    body::Body,
    http::{HeaderValue, Response, StatusCode, header},
};
use bytes::Bytes;
use futures_util::{Stream, TryStreamExt};
use serde_json::{Value, json};
use tracing::warn;

use crate::config::Config;

use super::{
    ProxyRequest, contains_substring_case_insensitive, execute_upstream,
    retry::{CyberPolicyCheck, cyber_policy_check_has_error},
};

pub(crate) fn heartbeat_bytes() -> Bytes {
    Bytes::from_static(b": heartbeat\n\n")
}

pub(crate) fn done_bytes() -> Bytes {
    Bytes::from_static(b"data: [DONE]\n\n")
}

pub(crate) fn sse_data_bytes(data: &[u8]) -> Bytes {
    // 优化：预估容量，避免多次重新分配
    // 每行需要 "data: " (6 字节) + 内容 + "\n" (1 字节)
    // 最后需要额外的 "\n" (1 字节)
    let text = String::from_utf8_lossy(data);
    let estimated_capacity = text.len() + 16;
    let mut output = String::with_capacity(estimated_capacity);

    if text.is_empty() {
        output.push_str("data: \n\n");
        return Bytes::from(output);
    }

    for line in text.lines() {
        output.push_str("data: ");
        output.push_str(line);
        output.push('\n');
    }
    output.push('\n');
    Bytes::from(output)
}

pub(crate) fn sse_event_bytes(event: &str, data: &[u8]) -> Bytes {
    let text = String::from_utf8_lossy(data);
    let mut output = String::with_capacity(text.len() + event.len() + 24);
    output.push_str("event: ");
    output.push_str(event);
    output.push('\n');

    if text.is_empty() {
        output.push_str("data: \n\n");
        return Bytes::from(output);
    }

    for line in text.lines() {
        output.push_str("data: ");
        output.push_str(line);
        output.push('\n');
    }
    output.push('\n');
    Bytes::from(output)
}

pub(crate) fn sse_response(
    stream: impl futures_util::TryStream<Ok = Bytes, Error = std::io::Error> + Send + 'static,
) -> Response<Body> {
    let mut response = Response::new(Body::from_stream(
        stream.map_err(|error: std::io::Error| error),
    ));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream; charset=utf-8"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

pub(crate) fn upstream_error_event(error: &str) -> Bytes {
    let payload = json!({
        "error": error,
        "code": "upstream_error",
    })
    .to_string();
    sse_event_bytes("error", payload.as_bytes())
}

pub(crate) fn upstream_response_error_payload(upstream_status: u16, body: &[u8]) -> Bytes {
    if matches!(serde_json::from_slice::<Value>(body), Ok(Value::Object(_))) {
        return Bytes::copy_from_slice(body);
    }

    let message = if body.is_empty() {
        format!("upstream returned HTTP {upstream_status}")
    } else {
        String::from_utf8_lossy(body).into_owned()
    };
    Bytes::from(
        json!({
            "error": {
                "message": message,
                "type": "upstream_error",
                "code": "upstream_error",
            },
            "upstream_status": upstream_status,
        })
        .to_string(),
    )
}

pub(crate) fn upstream_response_error_event(upstream_status: u16, body: &[u8]) -> Bytes {
    let payload = upstream_response_error_payload(upstream_status, body);
    sse_data_bytes(&payload)
}

pub(crate) fn stream_event_stall_max_attempts(config: &Config) -> usize {
    1 + config.stream_event_stall_max_retries
}

pub(super) fn sse_event_stall_error_event(timeout: Duration) -> Bytes {
    let payload = json!({
        "error": format!(
            "upstream event stream stalled without a complete SSE event for {} seconds",
            timeout.as_secs()
        ),
        "code": "stream_event_stall_timeout",
    })
    .to_string();
    sse_event_bytes("error", payload.as_bytes())
}

pub(super) fn log_sse_event_stall_timeout(
    ctx: &ProxyRequest,
    timeout: Duration,
    buffered_bytes: usize,
    attempt: usize,
    will_retry: bool,
) {
    warn!(
        event = "stream_event_stall_timeout",
        api_key_masked = ctx.api_key.masked(),
        api_key_hash = ctx.api_key.hash(),
        method = ctx.method_str(),
        path = %ctx.path,
        attempt,
        will_retry,
        timeout_ms = timeout.as_millis() as u64,
        buffered_bytes,
        "upstream SSE stream did not produce a complete event before timeout"
    );
}

pub(super) fn log_sse_event_stall_retry(ctx: &ProxyRequest, next_attempt: usize) {
    warn!(
        event = "stream_event_stall_retry",
        api_key_masked = ctx.api_key.masked(),
        api_key_hash = ctx.api_key.hash(),
        method = ctx.method_str(),
        path = %ctx.path,
        next_attempt,
        retry_reason = RETRY_REASON_STREAM_EVENT_STALL,
        "retrying upstream SSE stream after pre-progress stall"
    );
}

pub(super) const RETRY_REASON_STREAM_EVENT_STALL: &str = "stream_event_stall_timeout";

pub(super) type UpstreamByteStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static>>;

pub(super) enum SseStreamStep {
    Chunk(Bytes),
    End,
    Error(reqwest::Error),
    Timeout,
}

impl SseStreamStep {
    pub(super) fn from_chunk(chunk: Option<Result<Bytes, reqwest::Error>>) -> Self {
        match chunk {
            Some(Ok(chunk)) => Self::Chunk(chunk),
            Some(Err(error)) => Self::Error(error),
            None => Self::End,
        }
    }
}

#[derive(Default)]
pub(super) struct SseEventBuffer {
    pending: Vec<u8>,
}

pub(super) struct SseFrame {
    pub(super) bytes: Bytes,
}

impl SseEventBuffer {
    pub(super) fn push(&mut self, chunk: &[u8]) -> Vec<SseFrame> {
        self.pending.extend_from_slice(chunk);
        let mut frames = Vec::new();

        while let Some(end) = find_sse_event_end(&self.pending) {
            let event = self.pending.drain(..end).collect::<Vec<_>>();
            frames.push(SseFrame {
                bytes: Bytes::from(event),
            });
        }

        frames
    }

    pub(super) fn buffered_len(&self) -> usize {
        self.pending.len()
    }

    pub(super) fn take_remaining(&mut self) -> Option<Bytes> {
        if self.pending.is_empty() {
            return None;
        }

        Some(Bytes::from(std::mem::take(&mut self.pending)))
    }
}

#[derive(Default)]
pub(crate) struct CyberPolicySseInspector {
    pub(super) event_buffer: SseEventBuffer,
}

pub(crate) struct CyberPolicySseInspection {
    pub(crate) frames: Vec<Bytes>,
    pub(crate) detected: bool,
    #[allow(dead_code)]
    pub(crate) detected_frame: Option<Bytes>,
}

impl CyberPolicySseInspector {
    pub(crate) fn push(&mut self, chunk: &[u8]) -> CyberPolicySseInspection {
        let mut frames = Vec::new();
        for frame in self.event_buffer.push(chunk) {
            if sse_frame_has_cyber_policy_error(&frame.bytes) {
                return CyberPolicySseInspection {
                    frames,
                    detected: true,
                    detected_frame: Some(frame.bytes),
                };
            }
            frames.push(frame.bytes);
        }

        CyberPolicySseInspection {
            frames,
            detected: false,
            detected_frame: None,
        }
    }

    pub(crate) fn finish(&mut self) -> CyberPolicySseInspection {
        let Some(remaining) = self.event_buffer.take_remaining() else {
            return CyberPolicySseInspection {
                frames: Vec::new(),
                detected: false,
                detected_frame: None,
            };
        };

        if sse_frame_has_cyber_policy_error(&remaining) {
            CyberPolicySseInspection {
                frames: Vec::new(),
                detected: true,
                detected_frame: Some(remaining),
            }
        } else {
            CyberPolicySseInspection {
                frames: vec![remaining],
                detected: false,
                detected_frame: None,
            }
        }
    }
}

pub(crate) fn sse_body_has_cyber_policy_error(body: &[u8]) -> bool {
    let mut inspector = CyberPolicySseInspector::default();
    let inspection = inspector.push(body);
    inspection.detected || inspector.finish().detected
}

/// JSON 流式检测器，用于边读取边检测 cyber_policy 错误
///
/// 设计思路：
/// 1. 使用尾部缓冲区处理跨 chunk 边界的模式匹配
/// 2. 搜索 `"code":"cyber_policy"` 或 `"code": "cyber_policy"` 模式
/// 3. 每次处理 chunk 时，检测 buffer + chunk 的拼接数据
/// 4. 保留最后 BUFFER_SIZE 字节到 buffer，用于下次检测跨边界模式
///
/// 与 SSE 检测器的区别：
/// - SSE 检测器返回可安全输出的帧（因为 SSE 可以部分输出）
/// - JSON 检测器只返回是否检测到违规（因为 JSON 不能部分输出）
#[derive(Default)]
pub(crate) struct CyberPolicyJsonInspector {
    /// 尾部缓冲区，用于跨 chunk 边界检测
    /// 保留上一个 chunk 的尾部，与下一个 chunk 的开头拼接后检测
    buffer: Vec<u8>,
}

impl CyberPolicyJsonInspector {
    /// 缓冲区大小：30 字节足够覆盖最长的模式
    /// `"code": "cyber_policy"` = 24 字节
    const BUFFER_SIZE: usize = 30;

    /// 处理新的 chunk，返回是否检测到违规
    ///
    /// 检测逻辑：
    /// 1. 将 buffer + chunk 拼接后检测
    /// 2. 如果检测到违规，返回 true
    /// 3. 如果未检测到，保留 chunk 的尾部到 buffer，返回 false
    pub(crate) fn push(&mut self, chunk: &[u8]) -> bool {
        if chunk.is_empty() {
            return false;
        }

        // 将 buffer + chunk 拼接后检测
        // 需要临时拼接以检测跨边界的模式
        let detected = if self.buffer.is_empty() {
            json_chunk_has_cyber_policy_error(chunk)
        } else {
            let mut combined = Vec::with_capacity(self.buffer.len() + chunk.len());
            combined.extend_from_slice(&self.buffer);
            combined.extend_from_slice(chunk);
            json_chunk_has_cyber_policy_error(&combined)
        };

        if detected {
            // 检测到违规，清空 buffer
            self.buffer.clear();
            return true;
        }

        // 未检测到违规，更新 buffer：保留最后 BUFFER_SIZE 字节
        // 策略：从 (buffer + chunk) 的尾部保留 BUFFER_SIZE 字节
        if chunk.len() >= Self::BUFFER_SIZE {
            // chunk 足够大，只保留 chunk 的最后 BUFFER_SIZE 字节
            self.buffer.clear();
            self.buffer
                .extend_from_slice(&chunk[chunk.len() - Self::BUFFER_SIZE..]);
        } else {
            // chunk 较小，需要从 buffer + chunk 的尾部保留 BUFFER_SIZE 字节
            let total_len = self.buffer.len() + chunk.len();
            if total_len <= Self::BUFFER_SIZE {
                // buffer + chunk 总长度不超过 BUFFER_SIZE，全部保留
                self.buffer.extend_from_slice(chunk);
            } else {
                // buffer + chunk 总长度超过 BUFFER_SIZE，保留尾部 BUFFER_SIZE 字节
                let buffer_keep = Self::BUFFER_SIZE - chunk.len();
                let old_buffer = std::mem::take(&mut self.buffer);
                self.buffer
                    .extend_from_slice(&old_buffer[old_buffer.len() - buffer_keep..]);
                self.buffer.extend_from_slice(chunk);
            }
        }

        false
    }

    /// 处理 buffer 中的剩余数据，返回是否检测到违规
    pub(crate) fn finish(&self) -> bool {
        if self.buffer.is_empty() {
            return false;
        }

        json_chunk_has_cyber_policy_error(&self.buffer)
    }
}

/// 检测 JSON chunk 是否包含 cyber_policy 错误模式
///
/// 搜索模式：
/// - `"code":"cyber_policy"` (无空格)
/// - `"code": "cyber_policy"` (有空格)
fn json_chunk_has_cyber_policy_error(chunk: &[u8]) -> bool {
    contains_substring_case_insensitive(chunk, b"\"code\":\"cyber_policy\"")
        || contains_substring_case_insensitive(chunk, b"\"code\": \"cyber_policy\"")
}

pub(super) fn sse_frame_has_cyber_policy_error(frame: &[u8]) -> bool {
    // Fast path: short-circuit if frame doesn't contain "cyber_policy" substring
    // This avoids expensive JSON parsing for the vast majority of SSE frames
    // Use a simple byte search that works for both ASCII and UTF-8
    if !contains_substring_case_insensitive(frame, b"cyber_policy") {
        return false;
    }

    let mut data = String::new();
    let mut first = true;
    for field in sse_frame_fields(frame) {
        if field.name == "data" {
            if !first {
                data.push('\n');
            }
            data.push_str(field.value);
            first = false;
        }
    }

    if data.trim().is_empty() || data.trim() == "[DONE]" {
        return false;
    }

    // 使用轻量级结构体解析 SSE data 字段
    serde_json::from_str::<CyberPolicyCheck>(&data)
        .ok()
        .is_some_and(|check| cyber_policy_check_has_error(&check))
}

pub(super) fn sse_frame_is_stream_progress(frame: &[u8]) -> bool {
    let mut event_name = None;
    let mut data = String::new();
    let mut first = true;
    for field in sse_frame_fields(frame) {
        match field.name {
            "event" if event_name.is_none() => event_name = Some(field.value),
            "data" => {
                if !first {
                    data.push('\n');
                }
                data.push_str(field.value);
                first = false;
            }
            _ => {}
        }
    }

    if data.trim() == "[DONE]" {
        return true;
    }

    let Ok(value) = serde_json::from_str::<Value>(&data) else {
        return event_name.is_some_and(stream_event_name_is_progress);
    };

    json_value_is_stream_progress(&value) || event_name.is_some_and(stream_event_name_is_progress)
}

pub(super) struct SseField<'a> {
    pub(super) name: &'a str,
    pub(super) value: &'a str,
}

pub(super) struct SseFrameFields<'a> {
    frame: &'a [u8],
    start: usize,
}

impl<'a> Iterator for SseFrameFields<'a> {
    type Item = SseField<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.start < self.frame.len() {
            let end = memchr::memchr(b'\n', &self.frame[self.start..])
                .map(|pos| self.start + pos)
                .unwrap_or(self.frame.len());

            let line = &self.frame[self.start..end];
            self.start = end.saturating_add(1);

            let line = if line.last() == Some(&b'\r') {
                &line[..line.len() - 1]
            } else {
                line
            };

            if line.is_empty() || line[0] == b':' {
                continue;
            }

            let Some(colon_pos) = memchr::memchr(b':', line) else {
                continue;
            };
            let name = &line[..colon_pos];
            let value = &line[colon_pos + 1..];
            let value = if value.first() == Some(&b' ') {
                &value[1..]
            } else {
                value
            };

            let (Ok(name), Ok(value)) = (std::str::from_utf8(name), std::str::from_utf8(value))
            else {
                continue;
            };

            return Some(SseField { name, value });
        }

        None
    }
}

pub(super) fn sse_frame_fields(frame: &[u8]) -> SseFrameFields<'_> {
    SseFrameFields { frame, start: 0 }
}

fn stream_event_name_is_progress(event_name: &str) -> bool {
    matches!(
        event_name,
        "response.output_text.delta"
            | "response.function_call_arguments.delta"
            | "response.output_item.done"
            | "response.content_part.done"
            | "response.output_text.done"
            | "response.completed"
            | "response.failed"
            | "response.incomplete"
            | "response.cancelled"
            | "response.error"
            | "error"
    )
}

fn json_value_is_stream_progress(value: &Value) -> bool {
    value
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(stream_event_name_is_progress)
        || chat_completion_chunk_has_progress(value)
}

fn chat_completion_chunk_has_progress(value: &Value) -> bool {
    value
        .get("choices")
        .and_then(Value::as_array)
        .is_some_and(|choices| choices.iter().any(chat_choice_has_progress))
}

fn chat_choice_has_progress(choice: &Value) -> bool {
    if !choice
        .get("finish_reason")
        .unwrap_or(&Value::Null)
        .is_null()
    {
        return true;
    }

    let Some(delta) = choice.get("delta").and_then(Value::as_object) else {
        return false;
    };

    // 优化：将包含 role 的 delta 也视为进度，减少首字节延迟
    // 这样初始的 {"delta": {"role": "assistant"}} 帧也会立即转发
    delta.contains_key("role")
        || delta
            .get("content")
            .and_then(Value::as_str)
            .is_some_and(|content| !content.is_empty())
        || delta
            .get("tool_calls")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
        || delta.get("function_call").is_some()
}

pub(super) fn upstream_response_into_byte_stream(
    upstream: reqwest::Response,
) -> UpstreamByteStream {
    Box::pin(upstream.bytes_stream())
}

pub(super) async fn execute_upstream_retrying_stream_reconnect(
    ctx: &ProxyRequest,
) -> reqwest::Result<reqwest::Response> {
    execute_upstream(ctx).await
}

fn find_sse_event_end(bytes: &[u8]) -> Option<usize> {
    let mut pos = 0;
    while let Some(offset) = memchr::memchr2(b'\n', b'\r', &bytes[pos..]) {
        let index = pos + offset;
        let Some((first_len, first_kind)) = sse_line_ending_at(bytes, index) else {
            pos = index + 1;
            continue;
        };
        let second = index + first_len;
        let Some((second_len, second_kind)) = sse_line_ending_at(bytes, second) else {
            pos = second;
            continue;
        };
        if first_kind == second_kind {
            return Some(second + second_len);
        }
        pos = second;
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SseLineEnding {
    Lf,
    CrLf,
    Cr,
}

fn sse_line_ending_at(bytes: &[u8], index: usize) -> Option<(usize, SseLineEnding)> {
    match bytes.get(index).copied() {
        Some(b'\n') => Some((1, SseLineEnding::Lf)),
        Some(b'\r') if bytes.get(index + 1).copied() == Some(b'\n') => {
            Some((2, SseLineEnding::CrLf))
        }
        Some(b'\r') => Some((1, SseLineEnding::Cr)),
        _ => None,
    }
}
