use super::*;

#[test]
fn retry_after_seconds_are_capped() {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::RETRY_AFTER, "120".parse().unwrap());
    let config = retry_test_config();

    let plan = retry_plan_from_headers(&headers, 0, &config);

    assert_eq!(plan.delay, std::time::Duration::from_secs(30));
    assert_eq!(plan.retry_after.as_deref(), Some("120"));
}

#[test]
fn exponential_retry_delay_is_capped() {
    let headers = reqwest::header::HeaderMap::new();
    let config = retry_test_config();

    let plan = retry_plan_from_headers(&headers, 10, &config);

    assert_eq!(plan.delay, std::time::Duration::from_secs(30));
}

#[test]
fn retry_reason_matches_openai_capacity_error_shape() {
    let body = br#"{"error":{"code":"model_at_capacity","message":"Selected model is at capacity. Please try a different model."}}"#;

    assert_eq!(
        retry_reason_for_body(StatusCode::BAD_REQUEST.as_u16(), body),
        Some(RETRY_REASON_CAPACITY)
    );
}

#[test]
fn retry_reason_matches_wrapped_upstream_capacity_messages() {
    let body = br#"{"error":"upstream failed: server overloaded"}"#;

    assert_eq!(
        retry_reason_for_body(StatusCode::BAD_GATEWAY.as_u16(), body),
        Some(RETRY_REASON_CAPACITY)
    );
}

#[test]
fn retry_reason_does_not_retry_503_capacity_body() {
    let body = br#"{"error":{"code":"model_at_capacity","message":"Selected model is at capacity. Please try a different model."}}"#;

    assert_eq!(
        retry_reason_for_body(StatusCode::SERVICE_UNAVAILABLE.as_u16(), body),
        None
    );
}

#[test]
fn retry_reason_does_not_match_success_text_body() {
    let body = br#"{"output_text":"the upstream failed yesterday"}"#;

    assert_eq!(retry_reason_for_body(StatusCode::OK.as_u16(), body), None);
}

#[test]
fn retry_reason_does_not_match_new_api_quota_error_body() {
    let body = br#"{"error":{"message":"upstream failed: not enough balance","type":"new_api_error","code":"insufficient_user_quota"}}"#;

    assert_eq!(
        retry_reason_for_body(StatusCode::BAD_GATEWAY.as_u16(), body),
        None
    );
}

#[test]
fn final_error_passthrough_keeps_plain_429_on_existing_wrapper_path() {
    let body = br#"{"error":{"message":"rate limited","type":"rate_limit_error","code":"rate_limit_exceeded"}}"#;

    assert!(!final_upstream_error_should_passthrough(
        StatusCode::TOO_MANY_REQUESTS.as_u16(),
        body,
    ));
}

#[test]
fn final_error_passthrough_includes_new_api_quota_even_when_429() {
    let body = br#"{"error":{"message":"not enough balance","type":"new_api_error","code":"insufficient_user_quota"}}"#;

    assert!(final_upstream_error_should_passthrough(
        StatusCode::TOO_MANY_REQUESTS.as_u16(),
        body,
    ));
}

#[test]
fn final_error_passthrough_includes_non_429_errors() {
    let body = br#"{"error":{"message":"final bad gateway"}}"#;

    assert!(final_upstream_error_should_passthrough(
        StatusCode::BAD_GATEWAY.as_u16(),
        body,
    ));
}

#[tokio::test]
async fn buffered_retry_plan_skips_new_api_quota_error_even_when_502() {
    let ctx = test_proxy_request(
        "/v1/responses",
        Bytes::from_static(br#"{"model":"demo","input":"hello"}"#),
    );
    let upstream = BufferedUpstreamResponse {
            upstream_status: StatusCode::BAD_GATEWAY.as_u16(),
            headers: reqwest::header::HeaderMap::new(),
            body: Bytes::from_static(
                br#"{"error":{"message":"upstream failed: not enough balance","type":"new_api_error","code":"insufficient_user_quota"}}"#,
            ),
        };

    assert!(retry_plan_for_buffered_upstream_response(&ctx, &upstream, 0).is_none());
}

#[tokio::test]
async fn buffered_retry_plan_skips_new_api_preconsume_quota_error() {
    let ctx = test_proxy_request(
        "/v1/responses",
        Bytes::from_static(br#"{"model":"demo","input":"hello"}"#),
    );
    let upstream = BufferedUpstreamResponse {
            upstream_status: StatusCode::BAD_GATEWAY.as_u16(),
            headers: reqwest::header::HeaderMap::new(),
            body: Bytes::from_static(
                br#"{"error":{"message":"pre consume token quota failed","type":"new_api_error","code":"pre_consume_token_quota_failed"}}"#,
            ),
        };

    assert!(retry_plan_for_buffered_upstream_response(&ctx, &upstream, 0).is_none());
}

#[tokio::test]
async fn buffered_bad_gateway_retry_plan_respects_502_max_retries() {
    let ctx = test_proxy_request(
        "/v1/responses",
        Bytes::from_static(br#"{"model":"demo","input":"hello"}"#),
    );
    let upstream = BufferedUpstreamResponse {
        upstream_status: StatusCode::BAD_GATEWAY.as_u16(),
        headers: reqwest::header::HeaderMap::new(),
        body: Bytes::from_static(br#"{"error":"temporary bad gateway"}"#),
    };

    assert!(retry_plan_for_buffered_upstream_response(&ctx, &upstream, 2).is_some());
    assert!(retry_plan_for_buffered_upstream_response(&ctx, &upstream, 3).is_none());
}

#[tokio::test]
async fn upstream_request_error_retry_plan_respects_502_max_retries() {
    let ctx = test_proxy_request(
        "/v1/responses",
        Bytes::from_static(br#"{"model":"demo","input":"hello"}"#),
    );

    assert!(retry_plan_for_upstream_request_error(&ctx, 2).is_some());
    assert!(retry_plan_for_upstream_request_error(&ctx, 3).is_none());
}

#[test]
fn retryable_capacity_text_matches_case_insensitively() {
    assert!(has_retryable_capacity_text("at capacity"));
    assert!(has_retryable_capacity_text("At Capacity"));
    assert!(has_retryable_capacity_text("AT CAPACITY"));
    assert!(has_retryable_capacity_text("at_capacity"));
    assert!(has_retryable_capacity_text("AT_CAPACITY"));

    assert!(has_retryable_capacity_text("server overloaded"));
    assert!(has_retryable_capacity_text("Server Overloaded"));
    assert!(has_retryable_capacity_text("SERVER OVERLOADED"));

    assert!(has_retryable_capacity_text("overloaded"));
    assert!(has_retryable_capacity_text("Overloaded"));
    assert!(has_retryable_capacity_text("OVERLOADED"));

    assert!(has_retryable_capacity_text("upstream failed"));
    assert!(has_retryable_capacity_text("Upstream Failed"));
    assert!(has_retryable_capacity_text("UPSTREAM FAILED"));
    assert!(has_retryable_capacity_text("upstream_failed"));
    assert!(has_retryable_capacity_text("UPSTREAM_FAILED"));

    // Non-matching
    assert!(!has_retryable_capacity_text("normal error"));
    assert!(!has_retryable_capacity_text("capacity"));
}

#[tokio::test]
async fn upstream_retry_cooldown_is_scoped_to_same_api_key() {
    let state = test_state(Url::parse("http://localhost:3000").unwrap());
    let ctx = test_proxy_request("/v1/chat/completions", Bytes::from_static(b"{}"));
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::RETRY_AFTER, "1".parse().unwrap());

    let retry_plan = retry_plan_for_reason(
        &ctx,
        &headers,
        StatusCode::TOO_MANY_REQUESTS.as_u16(),
        0,
        RETRY_REASON_RATE_LIMITED,
    )
    .unwrap();
    record_upstream_error_cooldown(&state, &ctx, retry_plan.delay, RETRY_REASON_RATE_LIMITED);

    assert!(
        state
            .rate_limiter
            .try_acquire(ctx.api_key.identity())
            .is_none()
    );
    assert!(state.rate_limiter.try_acquire("sk-different-key").is_some());
}

#[tokio::test]
async fn upstream_retry_cooldown_can_be_disabled() {
    let mut config = retry_test_config();
    config.upstream_error_cooldown_enabled = false;
    let state = Arc::new(AppState::new(config).unwrap());
    let ctx =
        test_proxy_request_with_state("/v1/chat/completions", Bytes::from_static(b"{}"), &state);

    record_upstream_error_cooldown(
        &state,
        &ctx,
        Duration::from_secs(1),
        RETRY_REASON_RATE_LIMITED,
    );

    assert!(
        state
            .rate_limiter
            .try_acquire(ctx.api_key.identity())
            .is_some()
    );
}

#[tokio::test]
async fn bad_gateway_retry_cooldown_accumulates_per_retry() {
    let mut config = retry_test_config();
    config.upstream_502_cooldown_delay = Duration::from_millis(30);
    let state = Arc::new(AppState::new(config).unwrap());
    let ctx =
        test_proxy_request_with_state("/v1/chat/completions", Bytes::from_static(b"{}"), &state);
    let retry_plan = retry_plan_for_bad_gateway(&ctx.config);

    record_retry_cooldown(&state, &ctx, &retry_plan);
    record_retry_cooldown(&state, &ctx, &retry_plan);

    assert!(state.rate_limiter.cooldown_delay(ctx.api_key.identity()) >= Duration::from_millis(55));
}

#[tokio::test]
async fn bad_gateway_retry_cooldown_can_disable_accumulation() {
    let mut config = retry_test_config();
    config.upstream_502_cooldown_delay = Duration::from_millis(30);
    config.upstream_502_cooldown_accumulate_enabled = false;
    let state = Arc::new(AppState::new(config).unwrap());
    let ctx =
        test_proxy_request_with_state("/v1/chat/completions", Bytes::from_static(b"{}"), &state);
    let retry_plan = retry_plan_for_bad_gateway(&ctx.config);

    record_retry_cooldown(&state, &ctx, &retry_plan);
    record_retry_cooldown(&state, &ctx, &retry_plan);

    assert!(state.rate_limiter.cooldown_delay(ctx.api_key.identity()) < Duration::from_millis(55));
    assert!(state.rate_limiter.cooldown_delay(ctx.api_key.identity()) >= Duration::from_millis(20));
}

#[tokio::test]
async fn bad_gateway_retry_cooldown_skips_whitelisted_key() {
    let mut config = retry_test_config();
    config.upstream_502_cooldown_delay = Duration::from_millis(30);
    let whitelist_file = config.cyber_policy_whitelist_file.clone();
    std::fs::write(&whitelist_file, "sk-test\n").unwrap();

    let state = Arc::new(AppState::new(config).unwrap());
    let ctx =
        test_proxy_request_with_state("/v1/chat/completions", Bytes::from_static(b"{}"), &state);
    let retry_plan = retry_plan_for_bad_gateway(&ctx.config);

    record_retry_cooldown(&state, &ctx, &retry_plan);
    record_retry_cooldown(&state, &ctx, &retry_plan);

    assert_eq!(
        state.rate_limiter.cooldown_delay(ctx.api_key.identity()),
        Duration::ZERO
    );
    assert!(
        state
            .rate_limiter
            .try_acquire(ctx.api_key.identity())
            .is_some()
    );

    let _ = std::fs::remove_file(&whitelist_file);
}
