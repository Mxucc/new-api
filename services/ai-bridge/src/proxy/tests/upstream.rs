use super::*;

#[tokio::test]
async fn non_stream_proxy_preserves_response_body_with_raw_response_arguments() {
    let request_body =
        Bytes::from_static(br#"{"model":"demo","input":"hello","include_obfuscation":false}"#);
    let upstream_body = Bytes::from_static(
            br#"{"id":"resp_test","object":"response","output":[{"type":"function_call","id":"fc_1","call_id":"call_1","name":"lookup","arguments":{"city":"Paris","days":0,"strict":false}}]}"#,
        );
    let (upstream_base_url, captured_rx, server) =
        spawn_single_response_server("application/json; charset=utf-8", upstream_body.clone());
    let state = test_state(upstream_base_url);
    let ctx = test_proxy_request("/v1/responses", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_with_permit(state, ctx, permit, false).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap(),
        "application/json; charset=utf-8"
    );
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(response_body, upstream_body);

    let captured = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert_eq!(captured.body, request_body);
    assert!(captured.head.contains("POST /v1/responses HTTP/1.1"));
    server.join().unwrap();
}

#[tokio::test]
async fn non_stream_proxy_retries_openai_capacity_error_body() {
    let request_body = Bytes::from_static(br#"{"model":"demo","input":"hello"}"#);
    let first_body = Bytes::from_static(
            br#"{"error":{"code":"model_at_capacity","message":"Selected model is at capacity. Please try a different model."}}"#,
        );
    let second_body = Bytes::from_static(br#"{"id":"resp_test","object":"response","output":[]}"#);
    let (upstream_base_url, captured_rx, server) = spawn_response_sequence_server(vec![
        TestUpstreamResponse::new(StatusCode::BAD_REQUEST, "application/json", first_body),
        TestUpstreamResponse::new(StatusCode::OK, "application/json", second_body.clone()),
    ]);
    let state = test_state_with_fast_retry(upstream_base_url);
    let ctx = test_proxy_request("/v1/responses", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_with_permit(state, ctx, permit, false).await;

    assert_eq!(response.status(), StatusCode::OK);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(response_body, second_body);
    let first = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    let second = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert_eq!(first.body, request_body);
    assert_eq!(second.body, request_body);
    server.join().unwrap();
}

#[tokio::test]
async fn non_stream_proxy_does_not_retry_503_invalid_response_format_error() {
    let request_body = Bytes::from_static(
            br#"{"model":"demo","input":"hello","text":{"format":{"type":"json_schema","schema":{"type":"object"}}}}"#,
        );
    let upstream_body = Bytes::from_static(
            br#"{"error":{"message":"Invalid value for 'text.format': schema must include name","type":"invalid_request_error","param":"text.format"}}"#,
        );
    let (upstream_base_url, captured_rx, server) = spawn_single_status_response_server(
        StatusCode::SERVICE_UNAVAILABLE,
        "application/json",
        upstream_body.clone(),
    );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.upstream_429_initial_delay = std::time::Duration::from_millis(1);
    config.upstream_429_max_delay = std::time::Duration::from_millis(1);
    config.upstream_429_max_retries = None;
    let state = Arc::new(AppState::new(config).unwrap());
    let ctx = test_proxy_request("/v1/responses", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        forward_with_permit(state, ctx, permit, false),
    )
    .await
    .expect("invalid request-format 503 should not enter unlimited retry");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(response_body, upstream_body);
    let captured = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert_eq!(captured.body, request_body);
    assert!(captured_rx.try_recv().is_err());
    server.join().unwrap();
}

#[tokio::test]
async fn non_stream_proxy_returns_new_api_quota_error_without_retrying_502() {
    let request_body = Bytes::from_static(br#"{"model":"demo","input":"hello"}"#);
    let upstream_body = Bytes::from_static(
            br#"{"error":{"message":"upstream failed: not enough balance","type":"new_api_error","code":"insufficient_user_quota"}}"#,
        );
    let (upstream_base_url, captured_rx, server) = spawn_single_status_response_server(
        StatusCode::BAD_GATEWAY,
        "application/json",
        upstream_body.clone(),
    );
    let state = test_state(upstream_base_url);
    let ctx = test_proxy_request("/v1/responses", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = tokio::time::timeout(
        Duration::from_secs(1),
        forward_with_permit(state, ctx, permit, false),
    )
    .await
    .expect("quota 502 should not enter transparent retry");

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(response_body, upstream_body);
    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    assert!(captured_rx.try_recv().is_err());
    server.join().unwrap();
}

#[tokio::test]
async fn handle_protocol_returns_new_api_insufficient_quota_403_unchanged() {
    let request_body =
        Bytes::from_static(br#"{"model":"demo","messages":[{"role":"user","content":"hello"}]}"#);
    let upstream_body = Bytes::from_static(
        br#"{"error":{"message":"user quota is insufficient","type":"new_api_error","param":"","code":"insufficient_user_quota"}}"#,
    );
    let (upstream_base_url, captured_rx, server) = spawn_single_status_response_server(
        StatusCode::FORBIDDEN,
        "application/json",
        upstream_body.clone(),
    );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    let state = Arc::new(AppState::new(config).unwrap());
    let request = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", "Bearer sk-test")
        .header("content-type", "application/json")
        .body(Body::from(request_body.clone()))
        .unwrap();
    let peer_addr = SocketAddr::from(([127, 0, 0, 1], 12345));

    let response = handle_protocol(State(state), ConnectInfo(peer_addr), request).await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(response_body, upstream_body);
    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    assert!(captured_rx.try_recv().is_err());
    server.join().unwrap();
}

#[tokio::test]
async fn handle_protocol_stream_surfaces_new_api_insufficient_quota_403_after_heartbeat() {
    let request_body = Bytes::from_static(
        br#"{"model":"demo","stream":true,"messages":[{"role":"user","content":"hello"}]}"#,
    );
    let upstream_body = Bytes::from_static(
        br#"{"error":{"message":"user quota is insufficient","type":"new_api_error","param":"","code":"insufficient_user_quota"}}"#,
    );
    let (upstream_base_url, captured_rx, server) = spawn_single_status_response_server(
        StatusCode::FORBIDDEN,
        "application/json",
        upstream_body,
    );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.stream_early_heartbeat = true;
    let state = Arc::new(AppState::new(config).unwrap());
    let request = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", "Bearer sk-test")
        .header("content-type", "application/json")
        .body(Body::from(request_body.clone()))
        .unwrap();
    let peer_addr = SocketAddr::from(([127, 0, 0, 1], 12345));

    let response = handle_protocol(State(state), ConnectInfo(peer_addr), request).await;

    assert_eq!(response.status(), StatusCode::OK);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let response_text = String::from_utf8(response_body.to_vec()).unwrap();
    assert!(response_text.starts_with(": heartbeat\n\n"));
    assert!(response_text.contains("data: {\"error\":{"));
    assert!(response_text.contains("user quota is insufficient"));
    assert!(response_text.contains("insufficient_user_quota"));
    assert!(!response_text.contains("event: upstream_response"));
    assert!(response_text.ends_with("data: [DONE]\n\n"));
    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    assert!(captured_rx.try_recv().is_err());
    server.join().unwrap();
}

#[tokio::test]
async fn stream_proxy_returns_new_api_quota_error_event_without_retrying_502() {
    let request_body = Bytes::from_static(
        br#"{"model":"demo","stream":true,"messages":[{"role":"user","content":"hello"}]}"#,
    );
    let upstream_body = Bytes::from_static(
            br#"{"error":{"message":"upstream failed: not enough balance","type":"new_api_error","code":"insufficient_user_quota"}}"#,
        );
    let (upstream_base_url, captured_rx, server) = spawn_single_status_response_server(
        StatusCode::BAD_GATEWAY,
        "application/json",
        upstream_body,
    );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.stream_early_heartbeat = true;
    let state = Arc::new(AppState::new(config).unwrap());
    let ctx = test_proxy_request("/v1/responses", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_stream_with_permit_or_retry_sse(state, ctx, permit).await;

    assert_eq!(response.status(), StatusCode::OK);
    let response_body = tokio::time::timeout(
        Duration::from_secs(1),
        to_bytes(response.into_body(), 1024 * 1024),
    )
    .await
    .expect("quota 502 stream response should finish without retry")
    .unwrap();
    let response_text = String::from_utf8(response_body.to_vec()).unwrap();
    assert!(response_text.contains(": heartbeat"));
    assert!(!response_text.contains("event: upstream_response"));
    assert!(response_text.contains("data: {\"error\":{"));
    assert!(response_text.contains("insufficient_user_quota"));
    assert!(response_text.contains("not enough balance"));
    assert!(response_text.ends_with("data: [DONE]\n\n"));
    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    assert!(captured_rx.try_recv().is_err());
    server.join().unwrap();
}

// CONTINUE_UPSTREAM_2

#[tokio::test]
async fn service_unavailable_503_returns_without_retry_and_writes_forensic_log() {
    let upstream_body = Bytes::from_static(
        br#"{"error":{"message":"Service temporarily unavailable","type":"server_error"}}"#,
    );
    let (upstream_base_url, captured_rx, server) = spawn_single_status_response_server(
        StatusCode::SERVICE_UNAVAILABLE,
        "application/json",
        upstream_body.clone(),
    );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.upstream_429_initial_delay = std::time::Duration::from_millis(1);
    config.upstream_429_max_delay = std::time::Duration::from_millis(1);
    config.upstream_429_max_retries = Some(1);
    config.bad_gateway_log_dir = temp_test_log_dir("service-unavailable-502");
    config.service_unavailable_log_dir = temp_test_log_dir("service-unavailable-503");
    let service_unavailable_log_dir = config.service_unavailable_log_dir.clone();
    let state = Arc::new(AppState::new(config).unwrap());
    let forensic_writer = state.forensic_writer.clone();
    let request_body =
        Bytes::from_static(br#"{"model":"gpt-4","messages":[{"role":"user","content":"test"}]}"#);
    let request = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", "Bearer sk-test")
        .header("content-type", "application/json")
        .body(Body::from(request_body.clone()))
        .unwrap();
    let peer_addr = SocketAddr::from(([127, 0, 0, 1], 12345));

    let response = handle_protocol(State(state), ConnectInfo(peer_addr), request).await;

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(response_body, upstream_body);
    let captured = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert_eq!(captured.body, request_body);
    assert!(captured_rx.try_recv().is_err());
    server.join().unwrap();
    forensic_writer.flush().await;
    let log_value = read_first_forensic_log_json(&service_unavailable_log_dir);
    assert_eq!(log_value["event"], "bad_gateway_request_forensic");
    assert_eq!(log_value["trigger"]["condition"], "upstream_http_503_final");
    assert_eq!(log_value["trigger"]["upstream_status"], 503);
}

#[tokio::test]
async fn gpt_5_3_codex_spark_with_text_format_forwards_correctly() {
    let request_body = Bytes::from_static(
            br#"{"model":"gpt-5.3-codex-spark","input":"test","text":{"format":{"type":"json_schema","schema":{"type":"object"}}},"temperature":0.7}"#,
        );
    let (upstream_base_url, captured_rx, server) =
        spawn_single_response_server("application/json", Bytes::from_static(b"{}"));
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    let state = Arc::new(AppState::new(config).unwrap());
    let request = Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("authorization", "Bearer sk-test")
        .header("content-type", "application/json")
        .body(Body::from(request_body.clone()))
        .unwrap();
    let peer_addr = SocketAddr::from(([127, 0, 0, 1], 12345));

    let response = handle_protocol(State(state), ConnectInfo(peer_addr), request).await;

    assert_eq!(response.status(), StatusCode::OK);
    let captured = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    let captured_json: Value = serde_json::from_slice(&captured.body).unwrap();
    assert_eq!(captured_json["model"], "gpt-5.3-codex-spark");
    assert_eq!(captured_json["input"], "test");
    assert_eq!(captured_json["temperature"], 0.7);
    assert_eq!(captured_json["text"]["format"]["type"], "json_schema");
    assert_eq!(captured_json["text"]["format"]["name"], "response");
    assert_eq!(
        captured_json["text"]["format"]["schema"]["additionalProperties"],
        false
    );
    server.join().unwrap();
}

#[tokio::test]
async fn gpt_5_3_codex_spark_responses_endpoint_forwards_correctly() {
    let request_body = Bytes::from_static(
            br#"{"model":"gpt-5.3-codex-spark","input":"test prompt","temperature":0.7,"max_tokens":100}"#,
        );
    let (upstream_base_url, captured_rx, server) =
        spawn_single_response_server("application/json", Bytes::from_static(b"{}"));
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    let state = Arc::new(AppState::new(config).unwrap());
    let request = Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("authorization", "Bearer sk-test")
        .header("content-type", "application/json")
        .body(Body::from(request_body.clone()))
        .unwrap();
    let peer_addr = SocketAddr::from(([127, 0, 0, 1], 12345));

    let response = handle_protocol(State(state), ConnectInfo(peer_addr), request).await;

    assert_eq!(response.status(), StatusCode::OK);
    let captured = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    let captured_json: Value = serde_json::from_slice(&captured.body).unwrap();
    assert_eq!(captured_json["model"], "gpt-5.3-codex-spark");
    assert_eq!(captured_json["input"], "test prompt");
    assert_eq!(captured_json["temperature"], 0.7);
    assert_eq!(captured_json["max_output_tokens"], 100);
    assert!(captured_json.get("max_tokens").is_none());
    server.join().unwrap();
}

#[tokio::test]
async fn gpt_5_3_codex_spark_request_forwards_correctly() {
    let request_body = Bytes::from_static(
            br#"{"model":"gpt-5.3-codex-spark","messages":[{"role":"user","content":"test"}],"temperature":0.7,"max_tokens":100}"#,
        );
    let (upstream_base_url, captured_rx, server) =
        spawn_single_response_server("application/json", Bytes::from_static(b"{}"));
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    let state = Arc::new(AppState::new(config).unwrap());
    let request = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", "Bearer sk-test")
        .header("content-type", "application/json")
        .body(Body::from(request_body.clone()))
        .unwrap();
    let peer_addr = SocketAddr::from(([127, 0, 0, 1], 12345));

    let response = handle_protocol(State(state), ConnectInfo(peer_addr), request).await;

    assert_eq!(response.status(), StatusCode::OK);
    let captured = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    let captured_json: Value = serde_json::from_slice(&captured.body).unwrap();
    assert_eq!(captured_json["model"], "gpt-5.3-codex-spark");
    assert_eq!(captured_json["temperature"], 0.7);
    assert_eq!(captured_json["max_tokens"], 100);
    assert_eq!(captured_json["messages"][0]["role"], "user");
    assert_eq!(captured_json["messages"][0]["content"], "test");
    server.join().unwrap();
}
// CONTINUE_UPSTREAM_3

#[tokio::test]
async fn non_stream_proxy_retries_502_with_configured_key_cooldown() {
    let request_body = Bytes::from_static(br#"{"model":"demo","input":"hello"}"#);
    let first_body = Bytes::from_static(br#"{"error":"temporary bad gateway"}"#);
    let second_body = Bytes::from_static(br#"{"id":"resp_test","object":"response","output":[]}"#);
    let (upstream_base_url, captured_rx, server) = spawn_response_sequence_server(vec![
        TestUpstreamResponse::new(StatusCode::BAD_GATEWAY, "application/json", first_body),
        TestUpstreamResponse::new(StatusCode::OK, "application/json", second_body.clone()),
    ]);
    let state = test_state(upstream_base_url);
    let ctx = test_proxy_request("/v1/responses", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_with_permit(state, ctx, permit, false).await;

    assert_eq!(response.status(), StatusCode::OK);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(response_body, second_body);
    let first = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    let second = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert_eq!(first.body, request_body);
    assert_eq!(second.body, request_body);
    server.join().unwrap();
}

#[tokio::test]
async fn non_stream_proxy_stops_retrying_502_after_configured_max_retries() {
    let request_body = Bytes::from_static(br#"{"model":"demo","input":"hello"}"#);
    let first_body = Bytes::from_static(br#"{"error":"bad gateway 1"}"#);
    let second_body = Bytes::from_static(br#"{"error":"bad gateway 2"}"#);
    let third_body = Bytes::from_static(br#"{"error":"bad gateway final"}"#);
    let (upstream_base_url, captured_rx, server) = spawn_response_sequence_server(vec![
        TestUpstreamResponse::new(StatusCode::BAD_GATEWAY, "application/json", first_body),
        TestUpstreamResponse::new(StatusCode::BAD_GATEWAY, "application/json", second_body),
        TestUpstreamResponse::new(
            StatusCode::BAD_GATEWAY,
            "application/json",
            third_body.clone(),
        ),
    ]);
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.upstream_502_cooldown_delay = std::time::Duration::from_millis(1);
    config.upstream_502_max_retries = Some(2);
    let state = Arc::new(AppState::new(config).unwrap());
    let ctx = test_proxy_request_with_state("/v1/responses", request_body.clone(), &state);
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_with_permit(state, ctx, permit, false).await;

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(response_body, third_body);
    for _ in 0..3 {
        let captured = captured_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        assert_eq!(captured.body, request_body);
    }
    assert!(captured_rx.try_recv().is_err());
    server.join().unwrap();
}

#[tokio::test]
async fn whitelisted_non_stream_502_retry_bypasses_existing_accumulated_cooldown() {
    let request_body = Bytes::from_static(br#"{"model":"demo","input":"hello"}"#);
    let first_body = Bytes::from_static(br#"{"error":"temporary bad gateway"}"#);
    let second_body = Bytes::from_static(br#"{"id":"resp_test","object":"response","output":[]}"#);
    let (upstream_base_url, captured_rx, server) = spawn_response_sequence_server(vec![
        TestUpstreamResponse::new(StatusCode::BAD_GATEWAY, "application/json", first_body),
        TestUpstreamResponse::new(StatusCode::OK, "application/json", second_body.clone()),
    ]);
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.upstream_502_cooldown_delay = std::time::Duration::from_millis(1);
    let whitelist_file = config.cyber_policy_whitelist_file.clone();
    std::fs::write(&whitelist_file, "sk-test\n").unwrap();
    let state = Arc::new(AppState::new(config).unwrap());
    let ctx = test_proxy_request_with_state("/v1/responses", request_body.clone(), &state);
    let identity = ctx.api_key.identity().to_owned();
    let permit = state.rate_limiter.force_acquire(ctx.api_key.identity());
    state.rate_limiter.append_cooldown(
        ctx.api_key.identity(),
        std::time::Duration::from_millis(200),
        "bad_gateway",
    );

    let started_at = std::time::Instant::now();
    let response = forward_with_permit(state.clone(), ctx, permit, false).await;

    assert!(
        started_at.elapsed() < std::time::Duration::from_millis(100),
        "whitelisted 502 retry should not wait for the pre-existing accumulated cooldown"
    );
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(response_body, second_body);
    let first = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    let second = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert_eq!(first.body, request_body);
    assert_eq!(second.body, request_body);
    assert!(
        state.rate_limiter.cooldown_delay(&identity) > std::time::Duration::ZERO,
        "test setup should leave the pre-existing cooldown in place instead of consuming it"
    );
    let _ = std::fs::remove_file(&whitelist_file);
    server.join().unwrap();
}

#[tokio::test]
async fn non_stream_proxy_writes_502_forensic_log_with_redacted_api_key() {
    let request_body = Bytes::from_static(br#"{"model":"demo","input":"full prompt"}"#);
    let upstream_body = Bytes::from_static(br#"{"error":"temporary bad gateway"}"#);
    let (upstream_base_url, captured_rx, server) = spawn_response_sequence_server(vec![
        TestUpstreamResponse::new(StatusCode::BAD_GATEWAY, "application/json", upstream_body),
        TestUpstreamResponse::new(
            StatusCode::OK,
            "application/json",
            Bytes::from_static(br#"{"ok":true}"#),
        ),
    ]);
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.bad_gateway_log_dir = unique_temp_dir("ai-bridge-502-forensic");
    let log_dir = config.bad_gateway_log_dir.clone();
    let state = Arc::new(AppState::new(config).unwrap());
    let forensic_writer = state.forensic_writer.clone();
    let ctx = test_proxy_request("/v1/responses", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_with_permit(state, ctx, permit, false).await;

    assert_eq!(response.status(), StatusCode::OK);
    let _ = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    let _ = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    server.join().unwrap();
    forensic_writer.flush().await;
    let entries = std::fs::read_dir(&log_dir)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(entries.len(), 1);
    let content = std::fs::read_to_string(entries[0].path()).unwrap();
    let line = content.lines().next().unwrap();
    let value: Value = serde_json::from_str(line).unwrap();
    assert_eq!(value["event"], "bad_gateway_request_forensic");
    assert_eq!(value["trigger"]["condition"], "upstream_http_502_retry");
    assert_eq!(
        value["request"]["body"]["utf8_lossy"],
        r#"{"model":"demo","input":"full prompt"}"#
    );
    assert_eq!(
        value["request"]["headers"]["authorization"][0]["redacted"],
        true
    );
    assert!(value["request"]["api_key_full"].is_null());
    assert_eq!(
        value["request"]["api_key_hash"],
        crate::auth::hash_api_key("sk-test")
    );
    assert_eq!(value["request"]["api_key_masked"], "***");
    assert!(value["request"].get("api_key_present").is_none());
    assert!(value["request"]["body"].get("hex").is_none());
    std::fs::remove_dir_all(log_dir).unwrap();
}

#[tokio::test]
async fn non_stream_proxy_returns_502_when_502_cooldown_is_disabled() {
    let request_body = Bytes::from_static(br#"{"model":"demo","input":"hello"}"#);
    let upstream_body = Bytes::from_static(br#"{"error":"temporary bad gateway"}"#);
    let (upstream_base_url, captured_rx, server) = spawn_single_status_response_server(
        StatusCode::BAD_GATEWAY,
        "application/json",
        upstream_body.clone(),
    );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.upstream_502_cooldown_enabled = false;
    let state = Arc::new(AppState::new(config).unwrap());
    let ctx = test_proxy_request("/v1/responses", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_with_permit(state, ctx, permit, false).await;

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(response_body, upstream_body);
    let captured = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert_eq!(captured.body, request_body);
    assert!(captured_rx.try_recv().is_err());
    server.join().unwrap();
}
// CONTINUE_UPSTREAM_4

#[tokio::test]
async fn non_stream_proxy_writes_502_forensic_log_with_original_request_body() {
    let request_body = Bytes::from_static(br#"{"model":"demo","input":"secret prompt"}"#);
    let upstream_body = Bytes::from_static(br#"{"error":"temporary bad gateway"}"#);
    let (upstream_base_url, _captured_rx, server) = spawn_single_status_response_server(
        StatusCode::BAD_GATEWAY,
        "application/json",
        upstream_body.clone(),
    );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.upstream_502_cooldown_enabled = false;
    config.bad_gateway_log_dir = temp_test_log_dir("original-request-body");
    let log_dir = config.bad_gateway_log_dir.clone();
    let state = Arc::new(AppState::new(config).unwrap());
    let forensic_writer = state.forensic_writer.clone();
    let mut ctx = test_proxy_request("/v1/responses", request_body.clone());
    ctx.headers.insert(
        header::AUTHORIZATION,
        "Bearer sk-secret-1234".parse().unwrap(),
    );
    ctx.api_key = ApiKeyInfo::from_headers(&ctx.headers);
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_with_permit(state, ctx, permit, false).await;

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let _ = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    forensic_writer.flush().await;
    let log_file = std::fs::read_dir(&log_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let line = std::fs::read_to_string(&log_file).unwrap();
    let log: Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(log["event"], "bad_gateway_request_forensic");
    assert_eq!(log["trigger"]["condition"], "upstream_http_502_final");
    assert_eq!(log["trigger"]["upstream_status"], 502);
    assert_eq!(
        log["request"]["body"]["utf8_lossy"],
        r#"{"model":"demo","input":"secret prompt"}"#
    );
    assert_eq!(
        log["request"]["original_body"]["utf8_lossy"],
        r#"{"model":"demo","input":"secret prompt"}"#
    );
    assert_eq!(
        log["trigger"]["upstream_body"]["utf8_lossy"],
        r#"{"error":"temporary bad gateway"}"#
    );
    assert_eq!(
        log["request"]["headers"]["authorization"][0]["redacted"],
        true
    );
    assert!(log["request"]["api_key_full"].is_null());
    assert_eq!(
        log["request"]["api_key_hash"],
        crate::auth::hash_api_key("sk-secret-1234")
    );
    let _ = std::fs::remove_dir_all(&log_dir);
    server.join().unwrap();
}

#[tokio::test]
async fn non_stream_proxy_normalizes_chat_response_format_schema_before_forwarding() {
    let request_body = Bytes::from_static(
            br#"{"model":"demo","messages":[{"role":"user","content":"hello"}],"response_format":{"type":"json_schema","json_schema":{"name":"DeepAnalysisLLMOut","strict":true,"schema":{"type":"object","properties":{"affected_entities":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}}},"required":["affected_entities"]}}}}"#,
        );
    let upstream_body = Bytes::from_static(br#"{"id":"chatcmpl_1","choices":[]}"#);
    let (upstream_base_url, captured_rx, server) =
        spawn_single_response_server("application/json; charset=utf-8", upstream_body);
    let state = test_state(upstream_base_url);
    let mut ctx = test_proxy_request("/v1/chat/completions", request_body);
    if let Some(normalized_body) =
        structured_outputs::normalize_request_body(&ctx.method, &ctx.path, &ctx.body)
    {
        ctx.body = normalized_body;
    }
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_with_permit(state, ctx, permit, false).await;

    assert_eq!(response.status(), StatusCode::OK);
    let captured = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    let captured_json: Value = serde_json::from_slice(&captured.body).unwrap();
    assert_eq!(
        captured_json["response_format"]["json_schema"]["schema"]["additionalProperties"],
        false
    );
    assert_eq!(
        captured_json["response_format"]["json_schema"]["schema"]["properties"]["affected_entities"]
            ["items"]["additionalProperties"],
        false
    );
    server.join().unwrap();
}
// CONTINUE_UPSTREAM_5

#[tokio::test]
async fn handle_protocol_normalizes_responses_text_format_schema_before_forwarding() {
    let request_body = Bytes::from_static(
            br#"{"model":"demo","input":"hello","text":{"format":{"type":"json_schema","name":"DeepAnalysisLLMOut","strict":true,"schema":{"type":"object","properties":{"affected_entities":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}}},"required":["affected_entities"]}}}}"#,
        );
    let upstream_body = Bytes::from_static(br#"{"id":"resp_1","output":[]}"#);
    let (upstream_base_url, captured_rx, server) =
        spawn_single_response_server("application/json; charset=utf-8", upstream_body);
    let state = test_state(upstream_base_url);
    let request = Request::builder()
        .method(Method::POST)
        .uri("/v1/responses")
        .header(header::AUTHORIZATION, "Bearer sk-test")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(request_body))
        .unwrap();
    let peer_addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();

    let response = handle_protocol(State(state), ConnectInfo(peer_addr), request).await;

    assert_eq!(response.status(), StatusCode::OK);
    let captured = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert!(captured.head.contains("POST /v1/responses HTTP/1.1"));
    let captured_json: Value = serde_json::from_slice(&captured.body).unwrap();
    assert_eq!(captured_json["input"], "hello");
    assert_eq!(
        captured_json["text"]["format"]["schema"]["additionalProperties"],
        false
    );
    assert_eq!(
        captured_json["text"]["format"]["schema"]["properties"]["affected_entities"]["items"]["additionalProperties"],
        false
    );
    server.join().unwrap();
}

#[tokio::test]
async fn responses_nonstream_chat_fallback_converts_request_path_and_response_body() {
    let request_body = Bytes::from_static(
        br#"{"model":"demo","instructions":"Be concise","input":"hello","max_output_tokens":12}"#,
    );
    let upstream_body = Bytes::from_static(
            br#"{"id":"chatcmpl_1","object":"chat.completion","created":1710000000,"model":"demo","choices":[{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}}"#,
        );
    let (upstream_base_url, captured_rx, server) =
        spawn_single_response_server("application/json; charset=utf-8", upstream_body);
    let state = test_state_with_responses_chat_fallback(upstream_base_url);
    let ctx = test_proxy_request("/v1/responses", request_body);
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_with_permit(state, ctx, permit, false).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap(),
        "application/json; charset=utf-8"
    );
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let response_json: Value = serde_json::from_slice(&response_body).unwrap();
    assert_eq!(response_json["object"], "response");
    assert_eq!(response_json["output_text"], "hi");
    assert_eq!(response_json["output"][0]["content"][0]["text"], "hi");
    assert_eq!(response_json["usage"]["input_tokens"], 2);
    assert_eq!(response_json["usage"]["output_tokens"], 1);
    let captured = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert!(captured.head.contains("POST /v1/chat/completions HTTP/1.1"));
    let captured_json: Value = serde_json::from_slice(&captured.body).unwrap();
    assert_eq!(captured_json["model"], "demo");
    assert_eq!(captured_json["stream"], false);
    assert_eq!(captured_json["max_tokens"], 12);
    assert_eq!(captured_json["messages"][0]["role"], "system");
    assert_eq!(captured_json["messages"][0]["content"], "Be concise");
    assert_eq!(captured_json["messages"][1]["role"], "user");
    assert_eq!(captured_json["messages"][1]["content"], "hello");
    server.join().unwrap();
}

#[tokio::test]
async fn responses_chat_fallback_normalizes_text_format_schema_before_forwarding() {
    let request_body = Bytes::from_static(
            br#"{"model":"demo","input":"hello","text":{"format":{"type":"json_schema","name":"DeepAnalysisLLMOut","strict":true,"schema":{"type":"object","properties":{"affected_entities":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}}},"required":["affected_entities"]}}}}"#,
        );
    let upstream_body = Bytes::from_static(
            br#"{"id":"chatcmpl_1","object":"chat.completion","created":1710000000,"model":"demo","choices":[{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}]}"#,
        );
    let (upstream_base_url, captured_rx, server) =
        spawn_single_response_server("application/json; charset=utf-8", upstream_body);
    let state = test_state_with_responses_chat_fallback(upstream_base_url);
    let ctx = test_proxy_request("/v1/responses", request_body);
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_with_permit(state, ctx, permit, false).await;

    assert_eq!(response.status(), StatusCode::OK);
    let captured = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    let captured_json: Value = serde_json::from_slice(&captured.body).unwrap();
    assert_eq!(captured_json["response_format"]["type"], "json_schema");
    assert_eq!(
        captured_json["response_format"]["json_schema"]["schema"]["additionalProperties"],
        false
    );
    assert_eq!(
        captured_json["response_format"]["json_schema"]["schema"]["properties"]["affected_entities"]
            ["items"]["additionalProperties"],
        false
    );
    server.join().unwrap();
}
// CONTINUE_UPSTREAM_6

#[tokio::test]
async fn responses_nonstream_chat_fallback_policy_is_default_off_and_exact() {
    let default_state = test_state(Url::parse("http://localhost:3000").unwrap());
    let ctx = default_state.bind_runtime_snapshot(test_proxy_request(
        "/v1/responses",
        Bytes::from_static(br#"{"model":"demo","input":"hello"}"#),
    ));
    assert!(!should_use_responses_nonstream_chat_fallback(&ctx));

    let state =
        test_state_with_responses_chat_fallback(Url::parse("http://localhost:3000").unwrap());
    let ctx = state.bind_runtime_snapshot(ctx);
    assert!(should_use_responses_nonstream_chat_fallback(&ctx));

    let stream_ctx = state.bind_runtime_snapshot(test_proxy_request(
        "/v1/responses",
        Bytes::from_static(br#"{"model":"demo","input":"hello","stream":true}"#),
    ));
    assert!(!should_use_responses_nonstream_chat_fallback(&stream_ctx));

    let compact_ctx = state.bind_runtime_snapshot(test_proxy_request(
        "/v1/responses/compact",
        Bytes::from_static(br#"{"model":"demo","input":"hello"}"#),
    ));
    assert!(!should_use_responses_nonstream_chat_fallback(&compact_ctx));

    let image_ctx = state.bind_runtime_snapshot(test_proxy_request(
        "/v1/responses",
        Bytes::from_static(
            br#"{"model":"demo","input":"draw","tools":[{"type":"image_generation"}]}"#,
        ),
    ));
    assert!(!should_use_responses_nonstream_chat_fallback(&image_ctx));
    assert!(is_responses_nonstream_image_generation_request(&image_ctx));
}

#[tokio::test]
async fn responses_nonstream_image_generation_uses_json_heartbeat_path() {
    let request_body = Bytes::from_static(
        br#"{"model":"demo","input":"draw","tools":[{"type":"image_generation"}]}"#,
    );
    let upstream_body = Bytes::from_static(
            br#"{"id":"resp_1","object":"response","output":[{"id":"ig_1","type":"image_generation_call","result":"aW1hZ2U="}],"output_text":"","status":"completed"}"#,
        );
    let (upstream_base_url, captured_rx, server) =
        spawn_single_response_server("application/json; charset=utf-8", upstream_body.clone());
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.heartbeat_interval = std::time::Duration::from_millis(10);
    config.image_response_mode = crate::config::ImageResponseMode::Sse;
    config.responses_nonstream_chat_fallback = true;
    let state = Arc::new(AppState::new(config).unwrap());
    let request = Request::builder()
        .method(Method::POST)
        .uri("/v1/responses")
        .header(header::AUTHORIZATION, "Bearer sk-test")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(request_body.clone()))
        .unwrap();
    let peer_addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();

    let response = handle_protocol(State(state), ConnectInfo(peer_addr), request).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap(),
        "application/json; charset=utf-8"
    );
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let response_json: Value = serde_json::from_slice(&response_body).unwrap();
    assert_eq!(response_json["output"][0]["type"], "image_generation_call");
    assert_eq!(response_json["output"][0]["result"], "aW1hZ2U=");
    let captured = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert!(captured.head.contains("POST /v1/responses HTTP/1.1"));
    assert_eq!(captured.body, request_body);
    server.join().unwrap();
}

#[tokio::test]
async fn image_json_returns_new_api_quota_429_after_one_upstream_request() {
    let request_body =
        Bytes::from_static(br#"{"model":"gpt-image-1","prompt":"draw a lighthouse"}"#);
    let upstream_body = Bytes::from_static(
        br#"{"error":{"message":"user quota is insufficient","type":"new_api_error","code":"insufficient_user_quota"}}"#,
    );
    let (upstream_base_url, captured_rx, server) = spawn_single_status_response_server(
        StatusCode::TOO_MANY_REQUESTS,
        "application/json; charset=utf-8",
        upstream_body,
    );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.upstream_429_initial_delay = Duration::from_millis(1);
    config.upstream_429_max_delay = Duration::from_millis(1);
    config.upstream_429_max_retries = Some(1);
    let state = Arc::new(AppState::new(config).unwrap());
    let request = Request::builder()
        .method(Method::POST)
        .uri("/v1/images/generations")
        .header(header::AUTHORIZATION, "Bearer sk-test")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(request_body.clone()))
        .unwrap();
    let peer_addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();

    let response = handle_protocol(State(state), ConnectInfo(peer_addr), request).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json; charset=utf-8"
    );
    let response_body = tokio::time::timeout(
        Duration::from_secs(1),
        to_bytes(response.into_body(), 1024 * 1024),
    )
    .await
    .expect("quota image JSON response should finish without retry")
    .unwrap();
    let response_json: Value = serde_json::from_slice(&response_body).unwrap();
    assert_eq!(response_json["upstream_status"], 429);
    assert!(
        response_json["body"]
            .as_str()
            .unwrap()
            .contains("insufficient_user_quota")
    );
    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    assert!(captured_rx.try_recv().is_err());
    server.join().unwrap();
}

#[tokio::test]
async fn image_auto_sse_returns_new_api_quota_429_after_one_upstream_request() {
    let request_body =
        Bytes::from_static(br#"{"model":"gpt-image-1","prompt":"draw a lighthouse"}"#);
    let upstream_body = Bytes::from_static(
        br#"{"error":{"message":"user quota is insufficient","type":"new_api_error","code":"insufficient_user_quota"}}"#,
    );
    let (upstream_base_url, captured_rx, server) = spawn_single_status_response_server(
        StatusCode::TOO_MANY_REQUESTS,
        "application/json; charset=utf-8",
        upstream_body,
    );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.upstream_429_initial_delay = Duration::from_millis(1);
    config.upstream_429_max_delay = Duration::from_millis(1);
    config.upstream_429_max_retries = Some(1);
    let state = Arc::new(AppState::new(config).unwrap());
    let request = Request::builder()
        .method(Method::POST)
        .uri("/v1/images/generations")
        .header(header::AUTHORIZATION, "Bearer sk-test")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "text/event-stream")
        .body(Body::from(request_body.clone()))
        .unwrap();
    let peer_addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();

    let response = handle_protocol(State(state), ConnectInfo(peer_addr), request).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/event-stream; charset=utf-8"
    );
    let response_body = tokio::time::timeout(
        Duration::from_secs(1),
        to_bytes(response.into_body(), 1024 * 1024),
    )
    .await
    .expect("quota image SSE response should finish without retry")
    .unwrap();
    let response_text = String::from_utf8(response_body.to_vec()).unwrap();
    assert!(response_text.contains("\"upstream_status\":429"));
    assert!(response_text.contains("insufficient_user_quota"));
    assert!(response_text.ends_with("data: [DONE]\n\n"));
    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    assert!(captured_rx.try_recv().is_err());
    server.join().unwrap();
}

#[tokio::test]
async fn handle_protocol_streams_upload_above_normal_limit_but_below_upload_limit() {
    let request_body = Bytes::from(vec![b'a'; 2048]);
    let upstream_body = Bytes::from_static(br#"{"id":"file_1","object":"file"}"#);
    let (upstream_base_url, captured_rx, server) =
        spawn_single_response_server("application/json; charset=utf-8", upstream_body);
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.max_body_bytes = 1024;
    let state = Arc::new(AppState::new(config).unwrap());
    let request = Request::builder()
        .method(Method::POST)
        .uri("/v1/files")
        .header(header::AUTHORIZATION, "Bearer sk-test")
        .header(header::CONTENT_TYPE, "multipart/form-data; boundary=abc123")
        .header(header::CONTENT_LENGTH, request_body.len().to_string())
        .body(Body::from(request_body.clone()))
        .unwrap();
    let peer_addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();

    let response = handle_protocol(State(state), ConnectInfo(peer_addr), request).await;

    assert_eq!(response.status(), StatusCode::OK);
    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(captured.head.contains("POST /v1/files HTTP/1.1"));
    assert_eq!(captured.body, request_body);
    server.join().unwrap();
}

#[tokio::test]
async fn handle_protocol_rejects_upload_content_length_above_upload_limit() {
    let mut config = retry_test_config();
    config.upstream_base_url = Url::parse("http://127.0.0.1:9").unwrap();
    config.max_body_bytes = 8;
    config.max_upload_body_bytes = 16;
    let state = Arc::new(AppState::new(config).unwrap());
    let request = Request::builder()
        .method(Method::POST)
        .uri("/v1/files")
        .header(header::AUTHORIZATION, "Bearer sk-test")
        .header(header::CONTENT_TYPE, "multipart/form-data; boundary=abc123")
        .header(header::CONTENT_LENGTH, "17")
        .body(Body::empty())
        .unwrap();
    let peer_addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();

    let response = handle_protocol(State(state), ConnectInfo(peer_addr), request).await;

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let response_json: Value = serde_json::from_slice(&response_body).unwrap();
    assert_eq!(response_json["code"], "payload_too_large");
}

#[tokio::test]
async fn handle_protocol_rejects_chunked_upload_above_upload_limit() {
    let _bind_guard = TEST_TCP_BIND_LOCK.lock().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    drop(_bind_guard);
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut accepted = None;
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((stream, _)) => {
                    accepted = Some(stream);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("failed to accept upload test connection: {error}"),
            }
        }

        if let Some(mut stream) = accepted {
            stream
                .set_read_timeout(Some(Duration::from_millis(250)))
                .unwrap();
            let mut received = Vec::new();
            let _ = stream.read_to_end(&mut received);
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}",
            );
            let _ = stream.flush();
        }
    });

    let mut config = retry_test_config();
    config.upstream_base_url = Url::parse(&format!("http://{addr}")).unwrap();
    config.max_body_bytes = 8;
    config.max_upload_body_bytes = 16;
    let state = Arc::new(AppState::new(config).unwrap());
    let request = Request::builder()
        .method(Method::POST)
        .uri("/v1/files")
        .header(header::AUTHORIZATION, "Bearer sk-test")
        .header(header::CONTENT_TYPE, "multipart/form-data; boundary=abc123")
        .body(Body::from(Bytes::from_static(b"12345678901234567")))
        .unwrap();
    let peer_addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();

    let response = tokio::time::timeout(
        Duration::from_secs(2),
        handle_protocol(State(state), ConnectInfo(peer_addr), request),
    )
    .await
    .expect("chunked upload limit should stop the upstream request");

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let response_json: Value = serde_json::from_slice(&response_body).unwrap();
    assert_eq!(response_json["code"], "payload_too_large");
    server.join().unwrap();
}

#[tokio::test]
async fn handle_protocol_rejects_large_non_upload_body_with_payload_too_large() {
    let mut config = retry_test_config();
    config.upstream_base_url = Url::parse("http://127.0.0.1:9").unwrap();
    config.max_body_bytes = 16;
    let state = Arc::new(AppState::new(config).unwrap());
    let request = Request::builder()
        .method(Method::POST)
        .uri("/v1/responses")
        .header(header::AUTHORIZATION, "Bearer sk-test")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(Bytes::from_static(
            br#"{"model":"demo","input":"this body is too large"}"#,
        )))
        .unwrap();
    let peer_addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();

    let response = handle_protocol(State(state), ConnectInfo(peer_addr), request).await;

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let response_json: Value = serde_json::from_slice(&response_body).unwrap();
    assert_eq!(response_json["code"], "payload_too_large");
}

#[tokio::test]
async fn handle_protocol_rejects_oversized_content_length_before_reading_body() {
    let mut config = retry_test_config();
    config.upstream_base_url = Url::parse("http://127.0.0.1:9").unwrap();
    config.max_body_bytes = 16;
    let state = Arc::new(AppState::new(config).unwrap());
    let request = Request::builder()
        .method(Method::POST)
        .uri("/v1/responses")
        .header(header::AUTHORIZATION, "Bearer sk-test")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CONTENT_LENGTH, "17")
        .body(Body::from(Bytes::from_static(br#"{}"#)))
        .unwrap();
    let peer_addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();

    let response = handle_protocol(State(state), ConnectInfo(peer_addr), request).await;

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let response_json: Value = serde_json::from_slice(&response_body).unwrap();
    assert_eq!(response_json["code"], "payload_too_large");
}
// CONTINUE_UPSTREAM_7

#[tokio::test]
async fn non_stream_proxy_writes_bounded_502_forensic_log_with_redacted_api_key() {
    let request_body = Bytes::from_static(br#"{"model":"demo","input":"full prompt text"}"#);
    let upstream_body = Bytes::from_static(br#"{"error":"temporary bad gateway"}"#);
    let (upstream_base_url, captured_rx, server) = spawn_single_status_response_server(
        StatusCode::BAD_GATEWAY,
        "application/json",
        upstream_body.clone(),
    );
    let log_dir = temp_test_log_dir("non-stream-502");
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.upstream_502_cooldown_enabled = false;
    config.bad_gateway_log_dir = log_dir.clone();
    let state = Arc::new(AppState::new(config).unwrap());
    let forensic_writer = state.forensic_writer.clone();
    let ctx = test_proxy_request("/v1/responses", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_with_permit(state, ctx, permit, false).await;

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let _ = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    server.join().unwrap();
    forensic_writer.flush().await;
    let log_value = read_first_forensic_log_json(&log_dir);
    assert_eq!(log_value["trigger"]["condition"], "upstream_http_502_final");
    assert_eq!(log_value["trigger"]["upstream_status"], 502);
    assert_eq!(
        log_value["trigger"]["upstream_body"]["utf8_lossy"],
        r#"{"error":"temporary bad gateway"}"#
    );
    assert_eq!(
        log_value["request"]["body"]["utf8_lossy"],
        r#"{"model":"demo","input":"full prompt text"}"#
    );
    assert_eq!(
        log_value["request"]["original_body"]["utf8_lossy"],
        r#"{"model":"demo","input":"full prompt text"}"#
    );
    assert_eq!(
        log_value["request"]["headers"]["authorization"][0]["redacted"],
        true
    );
    assert!(log_value["request"]["api_key_full"].is_null());
    assert_eq!(log_value["request"]["api_key_masked"], "***");
    assert_eq!(
        log_value["request"]["api_key_hash"],
        crate::auth::hash_api_key("sk-test")
    );
    assert!(log_value["request"].get("api_key_present").is_none());
    assert!(log_value["request"]["body"].get("hex").is_none());
    assert!(log_value["trigger"]["upstream_body"].get("hex").is_none());
    assert_eq!(
        log_value["request"]["body_was_modified_before_upstream"],
        false
    );
    let _ = std::fs::remove_dir_all(&log_dir);
}
