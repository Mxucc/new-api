use super::*;

#[tokio::test]
async fn synthetic_first_space_is_opt_in_and_only_for_simple_streaming_chat() {
    let mut config = retry_test_config();
    let eligible_ctx = test_proxy_request(
        "/v1/chat/completions",
        Bytes::from_static(
            br#"{"model":"demo","stream":true,"messages":[{"role":"user","content":"hello"}]}"#,
        ),
    );

    assert!(synthetic_first_space_event(&config, &eligible_ctx).is_none());

    config.stream_synthetic_first_space = true;
    let event = synthetic_first_space_event(&config, &eligible_ctx).unwrap();
    let event = String::from_utf8(event.to_vec()).unwrap();
    let payload = event
        .strip_prefix("data: ")
        .and_then(|value| value.strip_suffix("\n\n"))
        .unwrap();
    let payload_json: Value = serde_json::from_str(payload).unwrap();
    assert_eq!(payload_json["object"], "chat.completion.chunk");
    assert_eq!(payload_json["model"], "demo");
    assert_eq!(payload_json["choices"][0]["delta"]["role"], "assistant");
    assert_eq!(payload_json["choices"][0]["delta"]["content"], " ");
    assert_eq!(payload_json["choices"][0]["finish_reason"], Value::Null);
}

#[tokio::test]
async fn synthetic_first_space_rejects_json_and_tool_requests() {
    let mut config = retry_test_config();
    config.stream_synthetic_first_space = true;

    for body in [
            br#"{"model":"demo","stream":true,"messages":[],"response_format":{"type":"json_object"}}"#.as_slice(),
            br#"{"model":"demo","stream":true,"messages":[],"response_format":{"type":"json_schema","json_schema":{"name":"x","schema":{"type":"object"}}}}"#.as_slice(),
            br#"{"model":"demo","stream":true,"messages":[],"tools":[{"type":"function","function":{"name":"lookup","parameters":{"type":"object"}}}]}"#.as_slice(),
            br#"{"model":"demo","stream":true,"messages":[],"tool_choice":"auto"}"#.as_slice(),
            br#"{"model":"demo","stream":true,"messages":[],"n":2}"#.as_slice(),
        ] {
            let ctx = test_proxy_request("/v1/chat/completions", Bytes::copy_from_slice(body));
            assert!(synthetic_first_space_event(&config, &ctx).is_none());
        }
}

#[tokio::test]
async fn queue_wait_helpers_record_elapsed_wait_once() {
    let mut ctx = test_proxy_request("/v1/chat/completions", Bytes::from_static(b"{}"));

    ctx.start_queue_wait();
    std::thread::sleep(std::time::Duration::from_millis(1));
    ctx.finish_queue_wait();
    let first_wait = ctx.queue_wait;

    assert!(first_wait >= Duration::from_millis(1));
    assert!(ctx.queue_started_at.is_none());

    ctx.finish_queue_wait();
    assert_eq!(ctx.queue_wait, first_wait);
}

#[tokio::test]
async fn queue_wait_helper_can_set_known_duration_for_non_stream_path() {
    let ctx = test_proxy_request("/v1/responses", Bytes::from_static(b"{}"))
        .with_queue_wait(Duration::from_millis(42));

    assert_eq!(ctx.queue_wait, Duration::from_millis(42));
    assert!(ctx.queue_started_at.is_none());
}

#[tokio::test]
async fn direct_stream_proxy_preserves_upstream_sse_body() {
    let request_body = Bytes::from_static(
        br#"{"model":"demo","stream":true,"messages":[{"role":"user","content":"hello"}]}"#,
    );
    let upstream_body = Bytes::from_static(
            b"data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"delta\":{\"content\":\"hi\"},\"index\":0}]}\n\ndata: [DONE]\n\n",
        );
    let (upstream_base_url, captured_rx, server) =
        spawn_single_response_server("text/event-stream; charset=utf-8", upstream_body.clone());
    let state = test_state(upstream_base_url);
    let ctx = test_proxy_request("/v1/chat/completions", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_stream_with_permit_or_retry_sse(state, ctx, permit).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap(),
        "text/event-stream; charset=utf-8"
    );
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(response_body, upstream_body);

    let captured = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert_eq!(captured.body, request_body);
    assert!(captured.head.contains("POST /v1/chat/completions HTTP/1.1"));
    server.join().unwrap();
}

#[tokio::test]
async fn synthetic_first_space_returns_before_upstream_headers_for_direct_stream() {
    let request_body = Bytes::from_static(
        br#"{"model":"demo","stream":true,"messages":[{"role":"user","content":"hello"}]}"#,
    );
    let upstream_chunk = Bytes::from_static(
            b"data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"delta\":{\"content\":\"hi\"},\"index\":0}]}\n\ndata: [DONE]\n\n",
        );
    let (upstream_base_url, captured_rx, server) = spawn_delayed_streaming_response_head_server(
        "text/event-stream; charset=utf-8",
        Duration::from_millis(200),
        vec![(upstream_chunk.clone(), Duration::ZERO)],
        Duration::ZERO,
    );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.stream_synthetic_first_space = true;
    let state = Arc::new(AppState::new(config).unwrap());
    let ctx = test_proxy_request("/v1/chat/completions", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = tokio::time::timeout(
        Duration::from_millis(50),
        forward_stream_with_permit_or_retry_sse(state, ctx, permit),
    )
    .await
    .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap(),
        "text/event-stream; charset=utf-8"
    );
    let response_body = tokio::time::timeout(
        Duration::from_secs(1),
        to_bytes(response.into_body(), 1024 * 1024),
    )
    .await
    .unwrap()
    .unwrap();
    let response_text = String::from_utf8(response_body.to_vec()).unwrap();
    assert!(response_text.starts_with("data: "));
    assert!(response_text.contains("\"content\":\" \""));
    let synthetic_index = response_text
        .find("\"id\":\"chatcmpl-ai-bridge-synthetic-first-space\"")
        .unwrap();
    let upstream_index = response_text.find("\"id\":\"chatcmpl_1\"").unwrap();
    assert!(synthetic_index < upstream_index);
    assert!(response_text.contains("\"content\":\"hi\""));
    assert!(response_text.ends_with("data: [DONE]\n\n"));

    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    assert!(captured.head.contains("POST /v1/chat/completions HTTP/1.1"));
    server.join().unwrap();
}

#[tokio::test]
async fn early_heartbeat_returns_before_upstream_headers_without_synthetic_content() {
    let request_body = Bytes::from_static(
            br#"{"model":"demo","stream":true,"messages":[{"role":"user","content":"hello"}],"response_format":{"type":"json_object"}}"#,
        );
    let upstream_chunk = Bytes::from_static(
            b"data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"delta\":{\"content\":\"{\\\"ok\\\":true}\"},\"index\":0}]}\n\ndata: [DONE]\n\n",
        );
    let (upstream_base_url, captured_rx, server) = spawn_delayed_streaming_response_head_server(
        "text/event-stream; charset=utf-8",
        Duration::from_millis(200),
        vec![(upstream_chunk.clone(), Duration::ZERO)],
        Duration::ZERO,
    );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.stream_early_heartbeat = true;
    config.stream_synthetic_first_space = true;
    let state = Arc::new(AppState::new(config).unwrap());
    let ctx = test_proxy_request("/v1/chat/completions", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = tokio::time::timeout(
        Duration::from_millis(50),
        forward_stream_with_permit_or_retry_sse(state, ctx, permit),
    )
    .await
    .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let response_body = tokio::time::timeout(
        Duration::from_secs(1),
        to_bytes(response.into_body(), 1024 * 1024),
    )
    .await
    .unwrap()
    .unwrap();
    let response_text = String::from_utf8(response_body.to_vec()).unwrap();
    assert!(response_text.starts_with(": heartbeat\n\n"));
    assert!(!response_text.contains("chatcmpl-ai-bridge-synthetic-first-space"));
    assert!(response_text.contains("\"id\":\"chatcmpl_1\""));
    assert!(response_text.ends_with("data: [DONE]\n\n"));

    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    assert!(captured.head.contains("POST /v1/chat/completions HTTP/1.1"));
    server.join().unwrap();
}

#[tokio::test]
async fn direct_pg_stream_proxy_preserves_upstream_sse_body_without_stall_retry() {
    let request_body = Bytes::from_static(
        br#"{"model":"demo","stream":true,"messages":[{"role":"user","content":"hello"}]}"#,
    );
    let ping = Bytes::from_static(b": PING\n\n");
    let role_only = Bytes::from_static(
            b"data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"delta\":{\"role\":\"assistant\"},\"index\":0,\"finish_reason\":null}]}\n\n",
        );
    let content = Bytes::from_static(
            b"data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"delta\":{\"content\":\"hi\"},\"index\":0,\"finish_reason\":null}]}\n\ndata: [DONE]\n\n",
        );
    let (upstream_base_url, captured_rx, server) = spawn_delayed_streaming_response_server(
        "text/event-stream; charset=utf-8",
        vec![
            (ping.clone(), Duration::ZERO),
            (role_only.clone(), Duration::ZERO),
            (content.clone(), Duration::from_millis(90)),
        ],
        Duration::ZERO,
    );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.stream_event_stall_timeout = Some(Duration::from_millis(50));
    config.stream_event_stall_max_retries = 1;
    let state = Arc::new(AppState::new(config).unwrap());
    let ctx = test_proxy_request("/pg/chat/completions", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_stream_with_permit_or_retry_sse(state, ctx, permit).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap(),
        "text/event-stream; charset=utf-8"
    );
    let response_body = tokio::time::timeout(
        Duration::from_secs(1),
        to_bytes(response.into_body(), 1024 * 1024),
    )
    .await
    .unwrap()
    .unwrap();
    let expected = [ping.as_ref(), role_only.as_ref(), content.as_ref()].concat();
    assert_eq!(response_body, Bytes::from(expected));

    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    assert!(captured.head.contains("POST /pg/chat/completions HTTP/1.1"));
    assert!(captured_rx.try_recv().is_err());
    server.join().unwrap();
}

#[tokio::test]
async fn direct_stream_event_stall_timeout_retries_before_progress() {
    let request_body = Bytes::from_static(
        br#"{"model":"demo","stream":true,"messages":[{"role":"user","content":"hello"}]}"#,
    );
    let first_created = Bytes::from_static(b"data: {\"type\":\"response.created\"}\n\n");
    let second_created =
        Bytes::from_static(b"data: {\"type\":\"response.created\",\"id\":\"resp_2\"}\n\n");
    let second_delta =
        Bytes::from_static(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}\n\n");
    let (upstream_base_url, captured_rx, server) =
        spawn_delayed_streaming_response_sequence_server(vec![
            DelayedStreamingResponse::new(
                "text/event-stream; charset=utf-8",
                vec![
                    (first_created.clone(), Duration::ZERO),
                    (
                        Bytes::from_static(b"data: {\"type\":\"response.output_text.delta\""),
                        Duration::from_millis(90),
                    ),
                ],
                Duration::ZERO,
            ),
            DelayedStreamingResponse::new(
                "text/event-stream; charset=utf-8",
                vec![
                    (second_created.clone(), Duration::ZERO),
                    (second_delta.clone(), Duration::from_millis(10)),
                ],
                Duration::ZERO,
            ),
        ]);
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.stream_event_stall_timeout = Some(Duration::from_millis(50));
    config.stream_event_stall_max_retries = 1;
    let state = Arc::new(AppState::new(config).unwrap());
    let ctx = test_proxy_request("/v1/responses", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_stream_with_permit_or_retry_sse(state, ctx, permit).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get(header::CONTENT_LENGTH).is_none());
    let response_body = tokio::time::timeout(
        Duration::from_secs(1),
        to_bytes(response.into_body(), 1024 * 1024),
    )
    .await
    .unwrap()
    .unwrap();
    let expected = [
        first_created.as_ref(),
        second_created.as_ref(),
        second_delta.as_ref(),
    ]
    .concat();
    assert_eq!(response_body, Bytes::from(expected));

    let first = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let second = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(first.body, request_body);
    assert_eq!(second.body, request_body);
    server.join().unwrap();
}

#[tokio::test]
async fn direct_stream_event_stall_timeout_errors_after_retries_exhausted() {
    let request_body = Bytes::from_static(
        br#"{"model":"demo","stream":true,"messages":[{"role":"user","content":"hello"}]}"#,
    );
    let (upstream_base_url, captured_rx, server) = spawn_delayed_streaming_response_server(
        "text/event-stream; charset=utf-8",
        vec![
            (
                Bytes::from_static(b"data: {\"type\":\"response.created\"}\n\n"),
                Duration::ZERO,
            ),
            (
                Bytes::from_static(b"data: {\"type\":\"response.output_text.delta\""),
                Duration::from_millis(90),
            ),
        ],
        Duration::ZERO,
    );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.stream_event_stall_timeout = Some(Duration::from_millis(50));
    config.stream_event_stall_max_retries = 0;
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
    .unwrap()
    .unwrap();
    let response_text = String::from_utf8(response_body.to_vec()).unwrap();

    assert!(response_text.contains("response.created"));
    assert!(response_text.contains("event: error"));
    assert!(response_text.contains("\"code\":\"stream_event_stall_timeout\""));
    assert!(response_text.ends_with("data: [DONE]\n\n"));
    assert!(!response_text.contains("response.output_text.delta"));

    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    server.join().unwrap();
}

#[tokio::test]
async fn direct_stream_event_stall_timeout_allows_complete_sse_events() {
    let request_body = Bytes::from_static(
        br#"{"model":"demo","stream":true,"messages":[{"role":"user","content":"hello"}]}"#,
    );
    let first = Bytes::from_static(b"data: {\"type\":\"response.created\"}\n\n");
    let second = Bytes::from_static(b"data: {\"type\":\"response.completed\"}\n\n");
    let (upstream_base_url, _captured_rx, server) = spawn_delayed_streaming_response_server(
        "text/event-stream; charset=utf-8",
        vec![
            (first.clone(), Duration::ZERO),
            (second.clone(), Duration::from_millis(20)),
        ],
        Duration::ZERO,
    );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.stream_event_stall_timeout = Some(Duration::from_millis(80));
    let state = Arc::new(AppState::new(config).unwrap());
    let ctx = test_proxy_request("/v1/responses", request_body);
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
    .unwrap()
    .unwrap();
    let expected = [first.as_ref(), second.as_ref()].concat();
    assert_eq!(response_body, Bytes::from(expected));
    server.join().unwrap();
}
