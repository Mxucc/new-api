use std::sync::Arc;

use async_stream::try_stream;
use axum::{
    body::Body,
    http::{HeaderMap, HeaderValue, Response, StatusCode, header},
};
use bytes::Bytes;
use futures_util::{StreamExt, TryStreamExt};
use serde_json::json;
use tokio::time::MissedTickBehavior;
use tracing::error;

use crate::{
    config::ImageResponseMode,
    logger,
    proxy::{self, AppState, ProxyRequest},
    rate_limit::RateLimitPermit,
};

pub fn is_image_endpoint(path: &str) -> bool {
    path.starts_with("/v1/images/generations") || path.starts_with("/v1/images/edits")
}

pub fn handle_image_request(state: Arc<AppState>, ctx: ProxyRequest) -> Response<Body> {
    let ctx = state.bind_runtime_snapshot(ctx);
    let initial_permit = if ctx.whitelist_cache.api_key_is_whitelisted(&ctx.api_key) {
        Some(state.rate_limiter.force_acquire(ctx.api_key.identity()))
    } else {
        state.rate_limiter.try_acquire(ctx.api_key.identity())
    };
    match selected_response_mode(&ctx) {
        SelectedImageResponseMode::Json => image_json_response(state, ctx, initial_permit),
        SelectedImageResponseMode::Sse => image_sse_response(state, ctx, initial_permit),
    }
}

pub fn handle_image_json_request(state: Arc<AppState>, ctx: ProxyRequest) -> Response<Body> {
    let ctx = state.bind_runtime_snapshot(ctx);
    let initial_permit = if ctx.whitelist_cache.api_key_is_whitelisted(&ctx.api_key) {
        Some(state.rate_limiter.force_acquire(ctx.api_key.identity()))
    } else {
        state.rate_limiter.try_acquire(ctx.api_key.identity())
    };
    image_json_response(state, ctx, initial_permit)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectedImageResponseMode {
    Json,
    Sse,
}

enum ImageUpstreamResponse {
    Response(reqwest::Response),
    Buffered(proxy::BufferedUpstreamResponse),
}

impl ImageUpstreamResponse {
    fn status(&self) -> u16 {
        match self {
            Self::Response(response) => response.status().as_u16(),
            Self::Buffered(response) => response.upstream_status,
        }
    }

    fn headers(&self) -> &HeaderMap {
        match self {
            Self::Response(response) => response.headers(),
            Self::Buffered(response) => &response.headers,
        }
    }
}

fn selected_response_mode(ctx: &ProxyRequest) -> SelectedImageResponseMode {
    match ctx.config.image_response_mode {
        ImageResponseMode::Json => SelectedImageResponseMode::Json,
        ImageResponseMode::Sse => SelectedImageResponseMode::Sse,
        ImageResponseMode::Auto => {
            if wants_image_sse(&ctx.headers) {
                SelectedImageResponseMode::Sse
            } else {
                SelectedImageResponseMode::Json
            }
        }
    }
}

fn wants_image_sse(headers: &HeaderMap) -> bool {
    header_contains(headers, header::ACCEPT, "text/event-stream")
        || headers
            .get("x-ai-bridge-image-sse")
            .and_then(|value| value.to_str().ok())
            .map(|value| {
                matches!(
                    value.to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false)
}

fn header_contains(headers: &HeaderMap, name: header::HeaderName, needle: &str) -> bool {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_ascii_lowercase().contains(needle))
        .unwrap_or(false)
}

fn image_json_response(
    state: Arc<AppState>,
    ctx: ProxyRequest,
    initial_permit: Option<RateLimitPermit>,
) -> Response<Body> {
    let stream = try_stream! {
        let mut ctx = ctx;
        let queued = initial_permit.is_none();
        logger::log_debug_operation(
            &ctx.config,
            &ctx.api_key,
            ctx.method.as_str(),
            &ctx.path,
            "image_response_mode",
            format!("mode=json queued={queued}"),
        );
        let mut interval = tokio::time::interval(ctx.config.heartbeat_interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

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
                        _ = interval.tick() => {
                            log_image_debug_heartbeat(&ctx, "json_queue_wait");
                            yield json_heartbeat_bytes();
                        }
                    }
                }
            }
        };
        ctx.finish_queue_wait();
        let _permit = permit;

        let mut retry_count = 0_usize;
        let upstream = loop {
            let upstream_future = proxy::execute_upstream(&ctx);
            tokio::pin!(upstream_future);
            let upstream = loop {
                tokio::select! {
                    result = &mut upstream_future => break result,
                    _ = interval.tick() => {
                        log_image_debug_heartbeat(&ctx, "json_upstream_wait");
                        yield json_heartbeat_bytes();
                    }
                }
            };

            match upstream {
                Ok(upstream) => {
                    let retry_decision = match proxy::classify_upstream_response_for_retry(
                        &ctx,
                        upstream,
                        retry_count,
                    )
                    .await
                    {
                        Ok(decision) => decision,
                        Err(error) => break Err(error),
                    };
                    let (retry_plan, upstream_headers, upstream_body) = match retry_decision {
                        proxy::UpstreamRetryDecision::Forward(upstream) => {
                            break Ok(ImageUpstreamResponse::Response(upstream));
                        }
                        proxy::UpstreamRetryDecision::ForwardBuffered(upstream) => {
                            break Ok(ImageUpstreamResponse::Buffered(upstream));
                        }
                        proxy::UpstreamRetryDecision::Retry {
                            retry_plan,
                            upstream_headers,
                            upstream_body,
                        } => (retry_plan, upstream_headers, upstream_body),
                    };
                    proxy::record_retry_cooldown(&state, &ctx, &retry_plan);
                    retry_count += 1;
                    proxy::log_bad_gateway_retry_request(
                        &ctx.config,
                        &ctx,
                        &retry_plan,
                        retry_count,
                        proxy::BadGatewayRetryLog {
                            condition: "image_json_upstream_http_502_retry",
                            upstream_headers: Some(&upstream_headers),
                            upstream_body: upstream_body.as_deref(),
                            upstream_error: None,
                        },
                    );
                    logger::log_upstream_retry(logger::UpstreamRetryLog {
                        api_key: &ctx.api_key,
                        method: ctx.method.as_str(),
                        path: &ctx.path,
                        attempt: retry_count,
                        upstream_status: retry_plan.upstream_status,
                        retry_reason: retry_plan.reason,
                        delay: retry_plan.delay,
                        retry_after: retry_plan.retry_after,
                    });
                    let retry_sleep = tokio::time::sleep(retry_plan.delay);
                    tokio::pin!(retry_sleep);
                    loop {
                        tokio::select! {
                            _ = &mut retry_sleep => break,
                            _ = interval.tick() => {
                                log_image_debug_heartbeat(&ctx, "json_429_retry_wait");
                                yield json_heartbeat_bytes();
                            }
                        }
                    }
                }
                Err(error) => {
                    let Some(retry_plan) =
                        proxy::retry_plan_for_upstream_request_error(&ctx, retry_count)
                    else {
                        break Err(error);
                    };

                    proxy::record_retry_cooldown(&state, &ctx, &retry_plan);
                    retry_count += 1;
                    proxy::log_bad_gateway_retry_request(
                        &ctx.config,
                        &ctx,
                        &retry_plan,
                        retry_count,
                        proxy::BadGatewayRetryLog {
                            condition: "image_json_upstream_request_error_retry",
                            upstream_headers: None,
                            upstream_body: None,
                            upstream_error: Some(&logger::error_chain(&error)),
                        },
                    );
                    logger::log_upstream_retry(logger::UpstreamRetryLog {
                        api_key: &ctx.api_key,
                        method: ctx.method.as_str(),
                        path: &ctx.path,
                        attempt: retry_count,
                        upstream_status: retry_plan.upstream_status,
                        retry_reason: retry_plan.reason,
                        delay: retry_plan.delay,
                        retry_after: retry_plan.retry_after,
                    });
                    let retry_sleep = tokio::time::sleep(retry_plan.delay);
                    tokio::pin!(retry_sleep);
                    loop {
                        tokio::select! {
                            _ = &mut retry_sleep => break,
                            _ = interval.tick() => {
                                log_image_debug_heartbeat(&ctx, "json_502_retry_wait");
                                yield json_heartbeat_bytes();
                            }
                        }
                    }
                }
            }
        };

        match upstream {
            Ok(upstream) => {
                let upstream_status = upstream.status();
                let upstream_is_sse = proxy::is_event_stream_headers(upstream.headers());
                let upstream_headers = upstream.headers().clone();
                proxy::log_proxy_request(&ctx, StatusCode::OK.as_u16(), queued, Some(upstream_status));

                let mut body = Vec::new();
                let mut body_error = None;
                match upstream {
                    ImageUpstreamResponse::Response(upstream) => {
                        let mut upstream_stream = upstream.bytes_stream();
                        loop {
                            tokio::select! {
                                chunk = upstream_stream.next() => {
                                    match chunk {
                                        Some(Ok(chunk)) => body.extend_from_slice(&chunk),
                                        Some(Err(error)) => {
                                            body_error = Some(error.to_string());
                                            break;
                                        }
                                        None => break,
                                    }
                                }
                                _ = interval.tick() => {
                                    log_image_debug_heartbeat(&ctx, "json_body_wait");
                                    yield json_heartbeat_bytes();
                                }
                            }
                        }
                    }
                    ImageUpstreamResponse::Buffered(upstream) => {
                        body.extend_from_slice(&upstream.body);
                    }
                }

                if let Some(error) = body_error {
                    logger::log_debug_operation(
                        &ctx.config,
                        &ctx.api_key,
                        ctx.method.as_str(),
                        &ctx.path,
                        "image_json_body_error",
                        &error,
                    );
                    yield json_upstream_error_payload(&error);
                } else {
                    proxy::record_upstream_auth_failure(
                        &state,
                        &ctx,
                        upstream_status,
                        &upstream_headers,
                        &body,
                    );
                    if proxy::body_has_cyber_policy_error(&body)
                    || (upstream_is_sse && proxy::sse_body_has_cyber_policy_error(&body))
                    {
                        match proxy::block_cyber_policy_key(&ctx) {
                            proxy::CyberPolicyAction::WhitelistLog
                            | proxy::CyberPolicyAction::PassThrough => {
                                if upstream_is_sse {
                                    yield json_payload_from_sse_body(upstream_status, &body);
                                } else {
                                    yield Bytes::from(body);
                                }
                            }
                            proxy::CyberPolicyAction::Block => {
                                yield proxy::cyber_policy_json_error_payload();
                            }
                        }
                    } else if upstream_is_sse {
                        logger::log_debug_operation(
                            &ctx.config,
                            &ctx.api_key,
                            ctx.method.as_str(),
                            &ctx.path,
                            "image_json_from_upstream_sse",
                            format!("upstream_status={upstream_status} body_bytes={}", body.len()),
                        );
                        yield json_payload_from_sse_body(upstream_status, &body);
                    } else if (200..300).contains(&upstream_status) {
                        logger::log_debug_operation(
                            &ctx.config,
                            &ctx.api_key,
                            ctx.method.as_str(),
                            &ctx.path,
                            "image_json_success",
                            format!("upstream_status={upstream_status} body_bytes={}", body.len()),
                        );
                        yield Bytes::from(body);
                    } else {
                        if upstream_status == StatusCode::BAD_GATEWAY.as_u16() {
                            proxy::log_bad_gateway_final_request(
                                &ctx.config,
                                &ctx,
                                "image_json_upstream_http_502_final",
                                Some(upstream_status),
                                Some(&upstream_headers),
                                Some(&body),
                                None,
                            );
                        }
                        if upstream_status == StatusCode::SERVICE_UNAVAILABLE.as_u16() {
                            proxy::log_bad_gateway_final_request(
                                &ctx.config,
                                &ctx,
                                "image_json_upstream_http_503_final",
                                Some(upstream_status),
                                Some(&upstream_headers),
                                Some(&body),
                                None,
                            );
                        }
                        logger::log_debug_operation(
                            &ctx.config,
                            &ctx.api_key,
                            ctx.method.as_str(),
                            &ctx.path,
                            "image_json_upstream_status",
                            format!("upstream_status={upstream_status} body_bytes={}", body.len()),
                        );
                        yield json_upstream_status_payload(upstream_status, &body);
                    }
                }
            }
            Err(error) => {
                logger::log_debug_operation(
                    &ctx.config,
                    &ctx.api_key,
                    ctx.method.as_str(),
                    &ctx.path,
                    "image_upstream_error",
                    logger::error_chain(&error),
                );
                error!(error = %error, path = %ctx.path, "image upstream request failed");
                proxy::log_bad_gateway_final_request(
                    &ctx.config,
                    &ctx,
                    "image_json_upstream_request_error_final",
                    Some(StatusCode::BAD_GATEWAY.as_u16()),
                    None,
                    None,
                    Some(&logger::error_chain(&error)),
                );
                proxy::log_proxy_request(&ctx, StatusCode::BAD_GATEWAY.as_u16(), queued, None);
                yield json_upstream_error_payload(&error.to_string());
            }
        }
    };

    json_stream_response(stream)
}

fn image_sse_response(
    state: Arc<AppState>,
    ctx: ProxyRequest,
    initial_permit: Option<RateLimitPermit>,
) -> Response<Body> {
    let stream = try_stream! {
        let mut ctx = ctx;
        let queued = initial_permit.is_none();
        logger::log_debug_operation(
            &ctx.config,
            &ctx.api_key,
            ctx.method.as_str(),
            &ctx.path,
            "image_response_mode",
            format!("mode=sse queued={queued}"),
        );
        let mut interval = tokio::time::interval(ctx.config.heartbeat_interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

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
                        _ = interval.tick() => {
                            log_image_debug_heartbeat(&ctx, "sse_queue_wait");
                            yield proxy::heartbeat_bytes();
                        }
                    }
                }
            }
        };
        ctx.finish_queue_wait();
        let _permit = permit;

        let mut retry_count = 0_usize;
        let upstream = loop {
            let upstream_future = proxy::execute_upstream(&ctx);
            tokio::pin!(upstream_future);
            let upstream = loop {
                tokio::select! {
                    result = &mut upstream_future => break result,
                    _ = interval.tick() => {
                        log_image_debug_heartbeat(&ctx, "sse_upstream_wait");
                        yield proxy::heartbeat_bytes();
                    }
                }
            };

            match upstream {
                Ok(upstream) => {
                    let retry_decision = match proxy::classify_upstream_response_for_retry(
                        &ctx,
                        upstream,
                        retry_count,
                    )
                    .await
                    {
                        Ok(decision) => decision,
                        Err(error) => break Err(error),
                    };
                    let (retry_plan, upstream_headers, upstream_body) = match retry_decision {
                        proxy::UpstreamRetryDecision::Forward(upstream) => {
                            break Ok(ImageUpstreamResponse::Response(upstream));
                        }
                        proxy::UpstreamRetryDecision::ForwardBuffered(upstream) => {
                            break Ok(ImageUpstreamResponse::Buffered(upstream));
                        }
                        proxy::UpstreamRetryDecision::Retry {
                            retry_plan,
                            upstream_headers,
                            upstream_body,
                        } => (retry_plan, upstream_headers, upstream_body),
                    };
                    proxy::record_retry_cooldown(&state, &ctx, &retry_plan);
                    retry_count += 1;
                    proxy::log_bad_gateway_retry_request(
                        &ctx.config,
                        &ctx,
                        &retry_plan,
                        retry_count,
                        proxy::BadGatewayRetryLog {
                            condition: "image_sse_upstream_http_502_retry",
                            upstream_headers: Some(&upstream_headers),
                            upstream_body: upstream_body.as_deref(),
                            upstream_error: None,
                        },
                    );
                    logger::log_upstream_retry(logger::UpstreamRetryLog {
                        api_key: &ctx.api_key,
                        method: ctx.method.as_str(),
                        path: &ctx.path,
                        attempt: retry_count,
                        upstream_status: retry_plan.upstream_status,
                        retry_reason: retry_plan.reason,
                        delay: retry_plan.delay,
                        retry_after: retry_plan.retry_after,
                    });
                    let retry_sleep = tokio::time::sleep(retry_plan.delay);
                    tokio::pin!(retry_sleep);
                    loop {
                        tokio::select! {
                            _ = &mut retry_sleep => break,
                            _ = interval.tick() => {
                                log_image_debug_heartbeat(&ctx, "sse_429_retry_wait");
                                yield proxy::heartbeat_bytes();
                            }
                        }
                    }
                }
                Err(error) => {
                    let Some(retry_plan) =
                        proxy::retry_plan_for_upstream_request_error(&ctx, retry_count)
                    else {
                        break Err(error);
                    };

                    proxy::record_retry_cooldown(&state, &ctx, &retry_plan);
                    retry_count += 1;
                    proxy::log_bad_gateway_retry_request(
                        &ctx.config,
                        &ctx,
                        &retry_plan,
                        retry_count,
                        proxy::BadGatewayRetryLog {
                            condition: "image_sse_upstream_request_error_retry",
                            upstream_headers: None,
                            upstream_body: None,
                            upstream_error: Some(&logger::error_chain(&error)),
                        },
                    );
                    logger::log_upstream_retry(logger::UpstreamRetryLog {
                        api_key: &ctx.api_key,
                        method: ctx.method.as_str(),
                        path: &ctx.path,
                        attempt: retry_count,
                        upstream_status: retry_plan.upstream_status,
                        retry_reason: retry_plan.reason,
                        delay: retry_plan.delay,
                        retry_after: retry_plan.retry_after,
                    });
                    let retry_sleep = tokio::time::sleep(retry_plan.delay);
                    tokio::pin!(retry_sleep);
                    loop {
                        tokio::select! {
                            _ = &mut retry_sleep => break,
                            _ = interval.tick() => {
                                log_image_debug_heartbeat(&ctx, "sse_502_retry_wait");
                                yield proxy::heartbeat_bytes();
                            }
                        }
                    }
                }
            }
        };

        match upstream {
            Ok(upstream) => {
                let upstream_status = upstream.status();
                let upstream_is_sse = proxy::is_event_stream_headers(upstream.headers());
                let upstream_headers = upstream.headers().clone();
                proxy::log_proxy_request(
                    &ctx,
                    axum::http::StatusCode::OK.as_u16(),
                    queued,
                    Some(upstream_status),
                );

                if upstream_is_sse {
                    match upstream {
                        ImageUpstreamResponse::Response(upstream) => {
                            let timeout = proxy::stream_event_stall_timeout_for_request(&ctx);
                            let max_attempts = proxy::stream_event_stall_max_attempts(&ctx.config);
                            let sse_stream = proxy::process_sse_stream_with_stall_retry(
                                &ctx,
                                upstream,
                                timeout,
                                max_attempts,
                            ).await;
                            let sse_stream = proxy::record_auth_failure_on_stream_completion(
                                state.clone(),
                                ctx.clone(),
                                upstream_status,
                                upstream_headers.clone(),
                                sse_stream,
                            );
                            tokio::pin!(sse_stream);
                            while let Some(chunk) = sse_stream.next().await {
                                yield chunk?;
                            }
                        }
                        ImageUpstreamResponse::Buffered(upstream) => {
                            proxy::record_upstream_auth_failure(
                                &state,
                                &ctx,
                                upstream_status,
                                &upstream_headers,
                                &upstream.body,
                            );
                            if proxy::sse_body_has_cyber_policy_error(&upstream.body) {
                                match proxy::block_cyber_policy_key(&ctx) {
                                    proxy::CyberPolicyAction::WhitelistLog
                                    | proxy::CyberPolicyAction::PassThrough => {
                                        yield upstream.body;
                                    }
                                    proxy::CyberPolicyAction::Block => {
                                        yield proxy::cyber_policy_sse_error_event();
                                        yield proxy::done_bytes();
                                    }
                                }
                            } else {
                                yield upstream.body;
                            }
                        }
                    }
                } else {
                    let mut inspector = proxy::CyberPolicyJsonInspector::default();
                    let mut body_chunks = Vec::new();
                    let mut body_error = None;
                    let mut cyber_policy_action = None;
                    match upstream {
                        ImageUpstreamResponse::Response(upstream) => {
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
                                                    let action = proxy::block_cyber_policy_key(&ctx);
                                                    let should_stop = matches!(action, proxy::CyberPolicyAction::Block);
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
                                            None => break,
                                        }
                                    }
                                    _ = interval.tick() => {
                                        log_image_debug_heartbeat(&ctx, "sse_body_wait");
                                        yield proxy::heartbeat_bytes();
                                    }
                                }
                            }
                        }
                        ImageUpstreamResponse::Buffered(upstream) => {
                            let body = upstream.body;
                            let detected = inspector.push(&body);
                            if !body.is_empty() {
                                body_chunks.push(body);
                            }
                            if detected {
                                cyber_policy_action = Some(proxy::block_cyber_policy_key(&ctx));
                            }
                        }
                    }

                    if let Some(error) = body_error {
                        yield proxy::upstream_error_event(&error);
                        yield proxy::done_bytes();
                    } else {
                        let body = proxy::body_chunks_to_bytes(body_chunks);
                        proxy::record_upstream_auth_failure(
                            &state,
                            &ctx,
                            upstream_status,
                            &upstream_headers,
                            &body,
                        );
                        let cyber_policy_action = cyber_policy_action.or_else(|| {
                            (inspector.finish() || proxy::body_has_cyber_policy_error(&body))
                                .then(|| proxy::block_cyber_policy_key(&ctx))
                        });
                        if let Some(action) = cyber_policy_action {
                            match action {
                                proxy::CyberPolicyAction::WhitelistLog
                                | proxy::CyberPolicyAction::PassThrough => {
                                    let payload = json!({
                                        "upstream_status": upstream_status,
                                        "body": String::from_utf8_lossy(&body),
                                    }).to_string();
                                    yield proxy::sse_event_bytes("upstream_response", payload.as_bytes());
                                }
                                proxy::CyberPolicyAction::Block => {
                                    yield proxy::cyber_policy_sse_error_event();
                                }
                            }
                            yield proxy::done_bytes();
                        } else {
                            if upstream_status == StatusCode::BAD_GATEWAY.as_u16() {
                                proxy::log_bad_gateway_final_request(
                                    &ctx.config,
                                    &ctx,
                                    "image_sse_upstream_http_502_final",
                                    Some(upstream_status),
                                    Some(&upstream_headers),
                                    Some(&body),
                                    None,
                                );
                            }
                            if upstream_status == StatusCode::SERVICE_UNAVAILABLE.as_u16() {
                                proxy::log_bad_gateway_final_request(
                                    &ctx.config,
                                    &ctx,
                                    "image_sse_upstream_http_503_final",
                                    Some(upstream_status),
                                    Some(&upstream_headers),
                                    Some(&body),
                                    None,
                                );
                            }
                            let payload = if (200..300).contains(&upstream_status) {
                                body
                            } else {
                                let payload = json!({
                                    "upstream_status": upstream_status,
                                    "body": String::from_utf8_lossy(&body),
                                })
                                .to_string();
                                Bytes::from(payload)
                            };
                            yield proxy::sse_data_bytes(&payload);
                            yield proxy::done_bytes();
                        }
                    }
                }
            }
            Err(error) => {
                logger::log_debug_operation(
                    &ctx.config,
                    &ctx.api_key,
                    ctx.method.as_str(),
                    &ctx.path,
                    "image_upstream_error",
                    logger::error_chain(&error),
                );
                error!(error = %error, path = %ctx.path, "image upstream request failed");
                proxy::log_bad_gateway_final_request(
                    &ctx.config,
                    &ctx,
                    "image_sse_upstream_request_error_final",
                    Some(StatusCode::BAD_GATEWAY.as_u16()),
                    None,
                    None,
                    Some(&logger::error_chain(&error)),
                );
                proxy::log_proxy_request(
                    &ctx,
                    axum::http::StatusCode::BAD_GATEWAY.as_u16(),
                    queued,
                    None,
                );
                yield proxy::upstream_error_event(&error.to_string());
                yield proxy::done_bytes();
            }
        }
    };

    proxy::sse_response(stream)
}

fn json_stream_response(
    stream: impl futures_util::TryStream<Ok = Bytes, Error = std::io::Error> + Send + 'static,
) -> Response<Body> {
    let mut response = Response::new(Body::from_stream(
        stream.map_err(|error: std::io::Error| error),
    ));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

fn log_image_debug_heartbeat(ctx: &ProxyRequest, stage: &str) {
    logger::log_debug_operation(
        &ctx.config,
        &ctx.api_key,
        ctx.method.as_str(),
        &ctx.path,
        "image_heartbeat",
        stage,
    );
}

fn json_heartbeat_bytes() -> Bytes {
    Bytes::from_static(b"\n")
}

fn json_upstream_error_payload(error: &str) -> Bytes {
    Bytes::from(
        json!({
            "error": {
                "code": "upstream_error",
                "message": error,
            }
        })
        .to_string(),
    )
}

fn json_upstream_status_payload(upstream_status: u16, body: &[u8]) -> Bytes {
    Bytes::from(
        json!({
            "upstream_status": upstream_status,
            "body": String::from_utf8_lossy(body),
        })
        .to_string(),
    )
}

fn json_payload_from_sse_body(upstream_status: u16, body: &[u8]) -> Bytes {
    let Some(payload) = last_sse_json_payload(body) else {
        return json_upstream_status_payload(upstream_status, body);
    };

    Bytes::from(payload)
}

fn last_sse_json_payload(body: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(body);
    let mut current = Vec::new();
    let mut last_payload = None;

    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            if !current.is_empty() {
                let payload = current.join("\n");
                if payload.trim() != "[DONE]" {
                    last_payload = Some(payload);
                }
                current.clear();
            }
            continue;
        }

        if let Some(data) = line.strip_prefix("data:") {
            current.push(data.trim_start().to_owned());
        }
    }

    if !current.is_empty() {
        let payload = current.join("\n");
        if payload.trim() != "[DONE]" {
            last_payload = Some(payload);
        }
    }

    last_payload.filter(|payload| serde_json::from_str::<serde_json::Value>(payload).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_heartbeat_is_valid_leading_json_whitespace() {
        let body = [json_heartbeat_bytes().as_ref(), br#"{"ok":true}"#].concat();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(parsed, json!({ "ok": true }));
    }

    #[test]
    fn extracts_last_json_payload_from_sse_body() {
        let body = b": heartbeat\n\ndata: {\"created\":1,\"data\":[]}\n\ndata: [DONE]\n\n";

        assert_eq!(
            last_sse_json_payload(body).as_deref(),
            Some(r#"{"created":1,"data":[]}"#)
        );
    }

    #[test]
    fn ignores_sse_comment_heartbeats_when_extracting_json_payload() {
        let body = b": heartbeat\n\n: heartbeat\n\ndata: [DONE]\n\n";

        assert_eq!(last_sse_json_payload(body), None);
    }
}
