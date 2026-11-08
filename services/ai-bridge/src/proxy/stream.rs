use std::{sync::Arc, time::Duration};

use async_stream::try_stream;
use axum::{
    body::Body,
    http::{HeaderMap, Response, StatusCode, header},
};
use bytes::Bytes;
use futures_util::{Stream, StreamExt, TryStreamExt};
use serde_json::json;
use tokio::time::MissedTickBehavior;
use tracing::{error, warn};

use crate::{
    auth_failure::AUTH_FAILURE_CLASSIFICATION_BODY_MAX_BYTES, config::Config, logger,
    rate_limit::RateLimitPermit,
};

use super::{
    AppState, BadGatewayRetryLog, CyberPolicyAction, ProxyRequest, block_cyber_policy_key,
    body_chunks_to_bytes, body_has_cyber_policy_error, execute_upstream, io_error,
    is_event_stream_headers, log_bad_gateway_final_request, log_bad_gateway_retry_request,
    log_proxy_request, record_retry_cooldown, record_upstream_auth_failure,
    request::cyber_policy_sse_error_event,
    request::{cyber_policy_json_error_response, filtered_response_headers, json_error_response},
    retry::{
        BufferedUpstreamResponse, RetryPlan, StreamUpstreamResponse, UpstreamRetryDecision,
        buffered_upstream_response_has_cyber_policy_error, classify_upstream_response_for_retry,
        final_upstream_error_should_passthrough, retry_plan_for_upstream_request_error,
    },
    sse::{
        CyberPolicyJsonInspector, CyberPolicySseInspector, SseEventBuffer, SseStreamStep,
        done_bytes, execute_upstream_retrying_stream_reconnect, heartbeat_bytes,
        log_sse_event_stall_retry, log_sse_event_stall_timeout, sse_event_bytes,
        sse_event_stall_error_event, sse_frame_has_cyber_policy_error,
        sse_frame_is_stream_progress, sse_response, stream_event_stall_max_attempts,
        upstream_error_event, upstream_response_error_event, upstream_response_into_byte_stream,
    },
    status_from_u16, stream_event_stall_timeout_for_request, synthetic_first_space_event,
};

pub(super) fn response_from_upstream(
    state: &AppState,
    config: &Config,
    ctx: ProxyRequest,
    upstream: BufferedUpstreamResponse,
    permit: RateLimitPermit,
    queued: bool,
) -> Response<Body> {
    record_upstream_auth_failure(
        state,
        &ctx,
        upstream.upstream_status,
        &upstream.headers,
        &upstream.body,
    );
    if buffered_upstream_response_has_cyber_policy_error(&upstream) {
        let action = block_cyber_policy_key(&ctx);
        match action {
            CyberPolicyAction::WhitelistLog | CyberPolicyAction::PassThrough => {
                log_proxy_request(
                    &ctx,
                    upstream.upstream_status,
                    queued,
                    Some(upstream.upstream_status),
                );
                drop(permit);
                let status = status_from_u16(upstream.upstream_status);
                let headers = filtered_response_headers(&upstream.headers);
                let mut response = Response::new(Body::from(upstream.body));
                *response.status_mut() = status;
                *response.headers_mut() = headers;
                return response;
            }
            CyberPolicyAction::Block => {
                log_proxy_request(
                    &ctx,
                    StatusCode::FORBIDDEN.as_u16(),
                    queued,
                    Some(upstream.upstream_status),
                );
                drop(permit);
                return cyber_policy_json_error_response();
            }
        }
    }

    let status = status_from_u16(upstream.upstream_status);
    let headers = filtered_response_headers(&upstream.headers);

    if status == StatusCode::BAD_GATEWAY {
        log_bad_gateway_final_request(
            config,
            &ctx,
            "upstream_http_502_final",
            Some(upstream.upstream_status),
            Some(&upstream.headers),
            Some(&upstream.body),
            None,
        );
    }

    // 为 503 错误添加详细日志，与 502 保持一致
    if status == StatusCode::SERVICE_UNAVAILABLE {
        log_bad_gateway_final_request(
            config,
            &ctx,
            "upstream_http_503_final",
            Some(upstream.upstream_status),
            Some(&upstream.headers),
            Some(&upstream.body),
            None,
        );
    }

    log_proxy_request(
        &ctx,
        status.as_u16(),
        queued,
        Some(upstream.upstream_status),
    );
    drop(permit);

    let mut response = Response::new(Body::from(upstream.body));
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
}

// Helper: Process SSE stream with cyber_policy detection and optional stall retry
pub(crate) async fn process_sse_stream_with_stall_retry(
    ctx: &ProxyRequest,
    upstream: reqwest::Response,
    timeout: Option<Duration>,
    max_attempts: usize,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> {
    try_stream! {
        if let Some(timeout) = timeout {
            let mut upstream = Some(upstream);
            let mut attempt = 1_usize;

            loop {
                let Some(current_upstream) = upstream.take() else {
                    yield sse_event_stall_error_event(timeout);
                    yield done_bytes();
                    break;
                };
                let mut upstream_stream = upstream_response_into_byte_stream(current_upstream);

                let mut event_buffer = SseEventBuffer::default();
                let mut progress_seen = false;
                let mut ended_before_progress = false;
                let mut stalled_buffered_bytes = None;
                let stall_sleep = tokio::time::sleep(timeout);
                tokio::pin!(stall_sleep);

                loop {
                    let step = tokio::select! {
                        chunk = upstream_stream.next() => SseStreamStep::from_chunk(chunk),
                        _ = &mut stall_sleep => SseStreamStep::Timeout,
                    };

                    match step {
                        SseStreamStep::Chunk(chunk) => {
                            let frames = event_buffer.push(&chunk);
                            let has_progress = frames
                                .iter()
                                .any(|frame| sse_frame_is_stream_progress(&frame.bytes));
                            let frames = frames
                                .into_iter()
                                .map(|frame| frame.bytes)
                                .collect::<Vec<_>>();

                            for frame in &frames {
                                yield frame.clone();
                            }

                            if frames
                                .iter()
                                .any(|frame| sse_frame_has_cyber_policy_error(frame))
                            {
                                let ctx_clone = ctx.clone();
                                tokio::spawn(async move {
                                    let action = block_cyber_policy_key(&ctx_clone);
                                    if matches!(action, CyberPolicyAction::Block) {
                                        warn!(
                                            event = "cyber_policy_detected_async",
                                            api_key_masked = ctx_clone.api_key.masked(),
                                            "cyber_policy detected in background, blacklisted for future requests"
                                        );
                                    }
                                });
                            }

                            if has_progress {
                                progress_seen = true;
                                break;
                            }
                        }
                        SseStreamStep::End => {
                            if let Some(remaining) = event_buffer.take_remaining() {
                                yield remaining.clone();

                                if sse_frame_has_cyber_policy_error(&remaining) {
                                    let ctx_clone = ctx.clone();
                                    tokio::spawn(async move {
                                        let action = block_cyber_policy_key(&ctx_clone);
                                        if matches!(action, CyberPolicyAction::Block) {
                                            warn!(
                                                event = "cyber_policy_detected_async",
                                                api_key_masked = ctx_clone.api_key.masked(),
                                                "cyber_policy detected in background, blacklisted for future requests"
                                            );
                                        }
                                    });
                                }
                            }
                            ended_before_progress = true;
                            break;
                        }
                        SseStreamStep::Error(error) => Err(io_error(error))?,
                        SseStreamStep::Timeout => {
                            stalled_buffered_bytes = Some(event_buffer.buffered_len());
                            break;
                        }
                    }
                }

                if progress_seen {
                        let mut inspector = CyberPolicySseInspector { event_buffer };
                        while let Some(chunk) = upstream_stream.next().await {
                            let chunk = chunk.map_err(io_error)?;
                            let inspection = inspector.push(&chunk);

                            // 先返回所有帧
                            for frame in &inspection.frames {
                                yield frame.clone();
                            }

                            // 后台异步检测
                            if inspection.detected {
                                let ctx_clone = ctx.clone();
                                tokio::spawn(async move {
                                    let action = block_cyber_policy_key(&ctx_clone);
                                    if matches!(action, CyberPolicyAction::Block) {
                                        warn!(
                                            event = "cyber_policy_detected_async",
                                            api_key_masked = ctx_clone.api_key.masked(),
                                            "cyber_policy detected in background, blacklisted for future requests"
                                        );
                                    }
                                });
                            }
                        }

                        // 最后检查剩余数据
                        let inspection = inspector.finish();
                        for frame in &inspection.frames {
                            yield frame.clone();
                        }
                        if inspection.detected {
                            let ctx_clone = ctx.clone();
                            tokio::spawn(async move {
                                let action = block_cyber_policy_key(&ctx_clone);
                                if matches!(action, CyberPolicyAction::Block) {
                                    warn!(
                                        event = "cyber_policy_detected_async",
                                        api_key_masked = ctx_clone.api_key.masked(),
                                        "cyber_policy detected in background, blacklisted for future requests"
                                    );
                                }
                            });
                        }
                        break;
                }

                if ended_before_progress {
                    break;
                }

                let Some(buffered_bytes) = stalled_buffered_bytes else {
                    continue;
                };

                let will_retry = attempt < max_attempts;
                log_sse_event_stall_timeout(ctx, timeout, buffered_bytes, attempt, will_retry);
                if !will_retry {
                    yield sse_event_stall_error_event(timeout);
                    yield done_bytes();
                    break;
                }
                attempt += 1;
                log_sse_event_stall_retry(ctx, attempt);
                let next_upstream = execute_upstream_retrying_stream_reconnect(ctx)
                    .await
                    .map_err(io_error)?;
                if !is_event_stream_headers(next_upstream.headers()) {
                    let upstream_status = next_upstream.status().as_u16();
                    let body = next_upstream.bytes().await.map_err(io_error)?;
                    if body_has_cyber_policy_error(&body) {
                        match block_cyber_policy_key(ctx) {
                            CyberPolicyAction::WhitelistLog | CyberPolicyAction::PassThrough => {
                                let payload = json!({
                                    "upstream_status": upstream_status,
                                    "body": String::from_utf8_lossy(&body),
                                }).to_string();
                                yield sse_event_bytes("upstream_response", payload.as_bytes());
                            }
                            CyberPolicyAction::Block => {
                                yield cyber_policy_sse_error_event();
                            }
                        }
                        yield done_bytes();
                        break;
                    }
                    let payload = json!({
                        "upstream_status": upstream_status,
                        "body": String::from_utf8_lossy(&body),
                    }).to_string();
                    yield sse_event_bytes("upstream_response", payload.as_bytes());
                    yield done_bytes();
                    break;
                }
                upstream = Some(next_upstream);
            }
        } else {
            let mut inspector = CyberPolicySseInspector::default();
            let mut upstream_stream = upstream.bytes_stream();
            while let Some(chunk) = upstream_stream.next().await {
                let chunk = chunk.map_err(io_error)?;
                let inspection = inspector.push(&chunk);

                // 先返回所有帧
                for frame in &inspection.frames {
                    yield frame.clone();
                }

                // 后台异步检测
                if inspection.detected {
                    let ctx_clone = ctx.clone();
                    tokio::spawn(async move {
                        let action = block_cyber_policy_key(&ctx_clone);
                        if matches!(action, CyberPolicyAction::Block) {
                            warn!(
                                event = "cyber_policy_detected_async",
                                api_key_masked = ctx_clone.api_key.masked(),
                                "cyber_policy detected in background, blacklisted for future requests"
                            );
                        }
                    });
                }
            }

            let inspection = inspector.finish();
            for frame in &inspection.frames {
                yield frame.clone();
            }
            if inspection.detected {
                let ctx_clone = ctx.clone();
                tokio::spawn(async move {
                    let action = block_cyber_policy_key(&ctx_clone);
                    if matches!(action, CyberPolicyAction::Block) {
                        warn!(
                            event = "cyber_policy_detected_async",
                            api_key_masked = ctx_clone.api_key.masked(),
                            "cyber_policy detected in background, blacklisted for future requests"
                        );
                    }
                });
            }
        }
    }
}

pub(crate) fn record_auth_failure_on_stream_completion<S>(
    state: Arc<AppState>,
    ctx: ProxyRequest,
    upstream_status: u16,
    upstream_headers: HeaderMap,
    stream: S,
) -> impl Stream<Item = Result<Bytes, std::io::Error>>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send,
{
    try_stream! {
        let should_capture = matches!(upstream_status, 401 | 403);
        let mut captured = Vec::new();
        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if should_capture
                && captured.len() < AUTH_FAILURE_CLASSIFICATION_BODY_MAX_BYTES
            {
                let remaining = AUTH_FAILURE_CLASSIFICATION_BODY_MAX_BYTES - captured.len();
                captured.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
            }
            yield chunk;
        }

        if should_capture {
            record_upstream_auth_failure(
                &state,
                &ctx,
                upstream_status,
                &upstream_headers,
                &captured,
            );
        }
    }
}

pub(super) async fn streamed_response_from_upstream(
    state: Arc<AppState>,
    ctx: ProxyRequest,
    upstream: reqwest::Response,
    permit: RateLimitPermit,
    queued: bool,
) -> Response<Body> {
    let status = status_from_u16(upstream.status().as_u16());
    let upstream_headers = upstream.headers().clone();
    let is_event_stream = is_event_stream_headers(upstream.headers());
    let mut headers = filtered_response_headers(upstream.headers());
    let stream_event_stall_timeout = stream_event_stall_timeout_for_request(&ctx);
    let stream_event_stall_max_attempts = stream_event_stall_max_attempts(&ctx.config);
    if is_event_stream {
        headers.remove(header::CONTENT_LENGTH);
    }

    if !is_event_stream {
        let upstream_status = status.as_u16();
        let mut upstream_stream = upstream.bytes_stream();
        let mut body_chunks = Vec::new();

        // 快速读取所有数据，不做同步检测
        while let Some(chunk) = upstream_stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    drop(permit);
                    error!(error = %error, path = %ctx.path, "stream upstream response body read failed");
                    log_bad_gateway_final_request(
                        &ctx.config,
                        &ctx,
                        "stream_upstream_body_error_final",
                        Some(StatusCode::BAD_GATEWAY.as_u16()),
                        None,
                        None,
                        Some(&logger::error_chain(&error)),
                    );
                    log_proxy_request(&ctx, StatusCode::BAD_GATEWAY.as_u16(), queued, None);
                    return json_error_response(
                        StatusCode::BAD_GATEWAY,
                        "upstream request failed",
                        "bad_gateway",
                    );
                }
            };

            if !chunk.is_empty() {
                body_chunks.push(chunk);
            }
        }

        let body = body_chunks_to_bytes(body_chunks);

        record_upstream_auth_failure(&state, &ctx, upstream_status, &headers, &body);

        // 后台异步检测 cyber_policy
        let ctx_clone = ctx.clone();
        let body_clone = body.clone();
        tokio::spawn(async move {
            if body_has_cyber_policy_error(&body_clone) {
                let action = block_cyber_policy_key(&ctx_clone);
                if matches!(action, CyberPolicyAction::Block) {
                    warn!(
                        event = "cyber_policy_detected_async_json",
                        api_key_masked = ctx_clone.api_key.masked(),
                        "cyber_policy detected in background (non-stream), blacklisted for future requests"
                    );
                }
            }
        });

        if status == StatusCode::BAD_GATEWAY {
            log_bad_gateway_final_request(
                &ctx.config,
                &ctx,
                "stream_upstream_http_502_final",
                Some(upstream_status),
                Some(&headers),
                Some(&body),
                None,
            );
        }

        // 为 503 错误添加详细日志，与 502 保持一致
        if status == StatusCode::SERVICE_UNAVAILABLE {
            log_bad_gateway_final_request(
                &ctx.config,
                &ctx,
                "stream_upstream_http_503_final",
                Some(upstream_status),
                Some(&headers),
                Some(&body),
                None,
            );
        }

        log_proxy_request(&ctx, status.as_u16(), queued, Some(upstream_status));
        drop(permit);

        let mut response = Response::new(Body::from(body));
        *response.status_mut() = status;
        *response.headers_mut() = headers;
        return response;
    }

    if status == StatusCode::BAD_GATEWAY {
        log_bad_gateway_final_request(
            &ctx.config,
            &ctx,
            "stream_upstream_http_502_final",
            Some(upstream.status().as_u16()),
            Some(upstream.headers()),
            None,
            None,
        );
    }

    // 为 503 错误添加详细日志，与 502 保持一致
    if status == StatusCode::SERVICE_UNAVAILABLE {
        log_bad_gateway_final_request(
            &ctx.config,
            &ctx,
            "stream_upstream_http_503_final",
            Some(upstream.status().as_u16()),
            Some(upstream.headers()),
            None,
            None,
        );
    }

    log_proxy_request(&ctx, status.as_u16(), queued, Some(status.as_u16()));

    let stream = try_stream! {
        let _permit = permit;
        if is_event_stream {
            // 立即发送初始心跳，降低首字节延迟
            if ctx.config.stream_early_heartbeat {
                logger::log_debug_operation(
                    &ctx.config,
                    &ctx.api_key,
                    ctx.method_str(),
                    &ctx.path,
                    "stream_early_heartbeat",
                    "emitted=true",
                );
                yield heartbeat_bytes();
            }

            let sse_stream = process_sse_stream_with_stall_retry(
                &ctx,
                upstream,
                stream_event_stall_timeout,
                stream_event_stall_max_attempts,
            ).await;
            let sse_stream = record_auth_failure_on_stream_completion(
                state,
                ctx.clone(),
                status.as_u16(),
                upstream_headers,
                sse_stream,
            );
            tokio::pin!(sse_stream);
            while let Some(chunk) = sse_stream.next().await {
                yield chunk?;
            }
        }
    };

    let mut response = Response::new(Body::from_stream(
        stream.map_err(|error: std::io::Error| error),
    ));
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
}

pub(super) fn stream_response_with_optional_permit(
    state: Arc<AppState>,
    ctx: ProxyRequest,
    initial_permit: Option<RateLimitPermit>,
    initial_retry: Option<(usize, RetryPlan)>,
) -> Response<Body> {
    let ctx = state.bind_runtime_snapshot(ctx);
    let initial_frame = initial_stream_frame(&ctx.config, &ctx);

    stream_response_with_initial_frame(state, ctx, initial_permit, initial_retry, initial_frame)
}

pub(super) enum InitialStreamFrame {
    SyntheticFirstSpace(Bytes),
    EarlyHeartbeat,
}

pub(super) fn initial_stream_frame(
    config: &Config,
    ctx: &ProxyRequest,
) -> Option<InitialStreamFrame> {
    if let Some(chunk) = synthetic_first_space_event(config, ctx) {
        Some(InitialStreamFrame::SyntheticFirstSpace(chunk))
    } else if config.stream_early_heartbeat {
        Some(InitialStreamFrame::EarlyHeartbeat)
    } else {
        None
    }
}

pub(super) fn stream_response_with_initial_frame(
    state: Arc<AppState>,
    ctx: ProxyRequest,
    initial_permit: Option<RateLimitPermit>,
    initial_retry: Option<(usize, RetryPlan)>,
    initial_frame: Option<InitialStreamFrame>,
) -> Response<Body> {
    let stream = try_stream! {
        let mut ctx = ctx;
        if let Some(initial_frame) = initial_frame {
            match initial_frame {
                InitialStreamFrame::SyntheticFirstSpace(chunk) => {
                    logger::log_debug_operation(
                        &ctx.config,
                        &ctx.api_key,
                        ctx.method_str(),
                        &ctx.path,
                        "stream_synthetic_first_space",
                        "emitted=true",
                    );
                    yield chunk;
                }
                InitialStreamFrame::EarlyHeartbeat => {
                    logger::log_debug_operation(
                        &ctx.config,
                        &ctx.api_key,
                        ctx.method_str(),
                        &ctx.path,
                        "stream_early_heartbeat",
                        "emitted=true",
                    );
                    yield heartbeat_bytes();
                }
            }
        }

        let mut interval = tokio::time::interval(ctx.config.heartbeat_interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        let queued = initial_permit.is_none();
        let permit = match initial_permit {
            Some(permit) => permit,
            None => {
                ctx.start_queue_wait();
                let key = ctx.api_key.identity().to_owned();
                let acquire = state.rate_limiter.acquire(&key);
                tokio::pin!(acquire);

                loop {
                    tokio::select! {
                        permit = &mut acquire => break permit,
                        _ = interval.tick() => yield heartbeat_bytes(),
                    }
                }
            }
        };
        ctx.finish_queue_wait();
        let _permit = permit;

        let mut retry_count = 0_usize;
        if let Some((attempt, retry_plan)) = initial_retry {
            retry_count = attempt;
            logger::log_upstream_retry(logger::UpstreamRetryLog {
                api_key: &ctx.api_key,
                method: ctx.method_str(),
                path: &ctx.path,
                attempt: retry_count,
                upstream_status: retry_plan.upstream_status,
                retry_reason: retry_plan.reason,
                delay: retry_plan.delay,
                retry_after: retry_plan.retry_after,
            });
            let retry_wait = state
                .rate_limiter
                .wait_for_retry_delay(ctx.api_key.identity(), retry_plan.delay);
            tokio::pin!(retry_wait);
            loop {
                tokio::select! {
                    () = &mut retry_wait => break,
                    _ = interval.tick() => yield heartbeat_bytes(),
                }
            }
        }

        let upstream = loop {
            let upstream_future = execute_upstream(&ctx);
            tokio::pin!(upstream_future);
            let upstream = loop {
                tokio::select! {
                    result = &mut upstream_future => break result,
                    _ = interval.tick() => yield heartbeat_bytes(),
                }
            };

            match upstream {
                Ok(upstream) => {
                    let retry_decision =
                        match classify_upstream_response_for_retry(&ctx, upstream, retry_count).await
                        {
                            Ok(decision) => decision,
                            Err(error) => break Err(error),
                        };

                    match retry_decision {
                        UpstreamRetryDecision::Forward(upstream) => {
                            break Ok(StreamUpstreamResponse::Response(upstream));
                        }
                        UpstreamRetryDecision::ForwardBuffered(upstream) => {
                            break Ok(StreamUpstreamResponse::Buffered(upstream));
                        }
                        UpstreamRetryDecision::Retry {
                            retry_plan,
                            upstream_headers,
                            upstream_body,
                        } => {
                            record_retry_cooldown(&state, &ctx, &retry_plan);
                            retry_count += 1;
                            log_bad_gateway_retry_request(
                                &ctx.config,
                                &ctx,
                                &retry_plan,
                                retry_count,
                                BadGatewayRetryLog {
                                    condition: "stream_upstream_http_502_retry",
                                    upstream_headers: Some(&upstream_headers),
                                    upstream_body: upstream_body.as_deref(),
                                    upstream_error: None,
                                },
                            );
                            logger::log_upstream_retry(logger::UpstreamRetryLog {
                                api_key: &ctx.api_key,
                                method: ctx.method_str(),
                                path: &ctx.path,
                                attempt: retry_count,
                                upstream_status: retry_plan.upstream_status,
                                retry_reason: retry_plan.reason,
                                delay: retry_plan.delay,
                                retry_after: retry_plan.retry_after,
                            });
                            let retry_wait = state
                                .rate_limiter
                                .wait_for_retry_delay(ctx.api_key.identity(), retry_plan.delay);
                            tokio::pin!(retry_wait);
                            loop {
                                tokio::select! {
                                    () = &mut retry_wait => break,
                                    _ = interval.tick() => yield heartbeat_bytes(),
                                }
                            }
                        }
                    }
                }
                Err(error) => {
                    let Some(retry_plan) =
                        retry_plan_for_upstream_request_error(&ctx, retry_count)
                    else {
                        break Err(error);
                    };

                    record_retry_cooldown(&state, &ctx, &retry_plan);
                    retry_count += 1;
                    log_bad_gateway_retry_request(
                        &ctx.config,
                        &ctx,
                        &retry_plan,
                        retry_count,
                        BadGatewayRetryLog {
                            condition: "stream_upstream_request_error_retry",
                            upstream_headers: None,
                            upstream_body: None,
                            upstream_error: Some(&logger::error_chain(&error)),
                        },
                    );
                    logger::log_upstream_retry(logger::UpstreamRetryLog {
                        api_key: &ctx.api_key,
                        method: ctx.method_str(),
                        path: &ctx.path,
                        attempt: retry_count,
                        upstream_status: retry_plan.upstream_status,
                        retry_reason: retry_plan.reason,
                        delay: retry_plan.delay,
                        retry_after: retry_plan.retry_after,
                    });
                    let retry_wait = state
                        .rate_limiter
                        .wait_for_retry_delay(ctx.api_key.identity(), retry_plan.delay);
                    tokio::pin!(retry_wait);
                    loop {
                        tokio::select! {
                            () = &mut retry_wait => break,
                            _ = interval.tick() => yield heartbeat_bytes(),
                        }
                    }
                }
            }
        };

        match upstream {
            Ok(StreamUpstreamResponse::Response(upstream)) => {
                let upstream_status = upstream.status().as_u16();
                let upstream_headers = upstream.headers().clone();
                log_proxy_request(&ctx, StatusCode::OK.as_u16(), queued, Some(upstream_status));

                if is_event_stream_headers(upstream.headers()) {
                    let timeout = stream_event_stall_timeout_for_request(&ctx);
                    let max_attempts = stream_event_stall_max_attempts(&ctx.config);
                    let sse_stream = process_sse_stream_with_stall_retry(
                        &ctx,
                        upstream,
                        timeout,
                        max_attempts,
                    ).await;
                    let sse_stream = record_auth_failure_on_stream_completion(
                        state.clone(),
                        ctx.clone(),
                        upstream_status,
                        upstream_headers,
                        sse_stream,
                    );
                    tokio::pin!(sse_stream);
                    while let Some(chunk) = sse_stream.next().await {
                        yield chunk?;
                    }
                } else {
                    let mut inspector = CyberPolicyJsonInspector::default();
                    let mut body_chunks = Vec::new();
                    let mut body_error = None;
                    let mut cyber_policy_action = None;
                    let mut upstream_stream = upstream.bytes_stream();
                    loop {
                        tokio::select! {
                            chunk = upstream_stream.next() => {
                                match chunk {
                                    Some(Ok(chunk)) => {
                                        let detected = cyber_policy_action.is_none()
                                            && inspector.push(&chunk);
                                        if !chunk.is_empty() {
                                            body_chunks.push(chunk);
                                        }
                                        if detected {
                                            let action = block_cyber_policy_key(&ctx);
                                            let should_stop = matches!(action, CyberPolicyAction::Block);
                                            cyber_policy_action = Some(action);
                                            if should_stop {
                                                break;
                                            }
                                        }
                                    }
                                    Some(Err(error)) => {
                                        body_error = Some(error.to_string());
                                        break;
                                    }
                                    None => {
                                        break;
                                    }
                                }
                            }
                            _ = interval.tick() => yield heartbeat_bytes(),
                        }
                    }

                    if let Some(error) = body_error {
                        yield upstream_error_event(&error);
                        yield done_bytes();
                    } else {
                        let body = body_chunks_to_bytes(body_chunks);
                        record_upstream_auth_failure(
                            &state,
                            &ctx,
                            upstream_status,
                            &upstream_headers,
                            &body,
                        );
                        let cyber_policy_action = cyber_policy_action.or_else(|| {
                            (inspector.finish() || body_has_cyber_policy_error(&body))
                                .then(|| block_cyber_policy_key(&ctx))
                        });
                        if let Some(action) = cyber_policy_action {
                            match action {
                                CyberPolicyAction::WhitelistLog | CyberPolicyAction::PassThrough => {
                                    let payload = json!({
                                        "upstream_status": upstream_status,
                                        "body": String::from_utf8_lossy(&body),
                                    }).to_string();
                                    yield sse_event_bytes("upstream_response", payload.as_bytes());
                                }
                                CyberPolicyAction::Block => {
                                    yield cyber_policy_sse_error_event();
                                }
                            }
                            yield done_bytes();
                        } else if final_upstream_error_should_passthrough(
                            upstream_status,
                            &body,
                        ) {
                            yield upstream_response_error_event(upstream_status, &body);
                            yield done_bytes();
                        } else {
                            let payload = json!({
                                "upstream_status": upstream_status,
                                "body": String::from_utf8_lossy(&body),
                            }).to_string();
                            yield sse_event_bytes("upstream_response", payload.as_bytes());
                            yield done_bytes();
                        }
                    }
                }
            }
            Ok(StreamUpstreamResponse::Buffered(upstream)) => {
                let upstream_status = upstream.upstream_status;
                record_upstream_auth_failure(
                    &state,
                    &ctx,
                    upstream_status,
                    &upstream.headers,
                    &upstream.body,
                );
                log_proxy_request(&ctx, StatusCode::OK.as_u16(), queued, Some(upstream_status));
                if body_has_cyber_policy_error(&upstream.body) {
                    match block_cyber_policy_key(&ctx) {
                        CyberPolicyAction::WhitelistLog | CyberPolicyAction::PassThrough => {
                            let payload = json!({
                                "upstream_status": upstream_status,
                                "body": String::from_utf8_lossy(&upstream.body),
                            }).to_string();
                            yield sse_event_bytes("upstream_response", payload.as_bytes());
                        }
                        CyberPolicyAction::Block => {
                            yield cyber_policy_sse_error_event();
                        }
                    }
                } else if final_upstream_error_should_passthrough(
                    upstream_status,
                    &upstream.body,
                ) {
                    yield upstream_response_error_event(upstream_status, &upstream.body);
                } else {
                    let payload = json!({
                        "upstream_status": upstream_status,
                        "body": String::from_utf8_lossy(&upstream.body),
                    }).to_string();
                    yield sse_event_bytes("upstream_response", payload.as_bytes());
                }
                yield done_bytes();
            }
            Err(error) => {
                error!(error = %error, path = %ctx.path, "queued streaming upstream request failed");
                log_bad_gateway_final_request(
                    &ctx.config,
                    &ctx,
                    "queued_stream_upstream_request_error_final",
                    Some(StatusCode::BAD_GATEWAY.as_u16()),
                    None,
                    None,
                    Some(&logger::error_chain(&error)),
                );
                log_proxy_request(&ctx, StatusCode::BAD_GATEWAY.as_u16(), queued, None);
                yield upstream_error_event(&error.to_string());
                yield done_bytes();
            }
        }
    };

    sse_response(stream)
}
