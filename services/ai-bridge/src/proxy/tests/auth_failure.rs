use super::*;

fn auth_failure_request_to(key: &str, method: Method, path: &str, body: Bytes) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {key}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap()
}

async fn send_auth_failure_request(
    state: Arc<AppState>,
    key: &str,
    body: Bytes,
) -> (StatusCode, Bytes, HeaderMap) {
    send_auth_failure_request_to(state, key, Method::POST, "/v1/responses", body).await
}

async fn send_auth_failure_request_to(
    state: Arc<AppState>,
    key: &str,
    method: Method,
    path: &str,
    body: Bytes,
) -> (StatusCode, Bytes, HeaderMap) {
    let peer_addr: SocketAddr = "198.51.100.70:12345".parse().unwrap();
    let response = handle_protocol(
        State(state),
        ConnectInfo(peer_addr),
        auth_failure_request_to(key, method, path, body),
    )
    .await;
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, body, headers)
}

#[tokio::test]
async fn auth_failure_model_list_business_403_never_opens_cooldown() {
    let upstream_body = Bytes::from_static(
        br#"{"error":{"message":"This token has no access to any models","type":"new_api_error","code":"token_no_model_access"}}"#,
    );
    let responses = (0..6)
        .map(|_| {
            TestUpstreamResponse::new(
                StatusCode::FORBIDDEN,
                "application/json; charset=utf-8",
                upstream_body.clone(),
            )
        })
        .collect();
    let (upstream_base_url, captured_rx, server) = spawn_response_sequence_server(responses);
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.auth_failure_threshold = 5;
    config.auth_failure_ip_threshold = 0;
    config.upstream_502_cooldown_enabled = false;
    let state = Arc::new(AppState::new(config).unwrap());

    for _ in 0..6 {
        let (status, body, headers) = send_auth_failure_request_to(
            state.clone(),
            "sk-model-list",
            Method::GET,
            "/v1/models",
            Bytes::new(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body, upstream_body);
        assert!(!headers.contains_key(header::RETRY_AFTER));
    }

    for _ in 0..6 {
        captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    }
    assert!(captured_rx.try_recv().is_err());
    server.join().unwrap();
}

#[tokio::test]
async fn auth_failure_sixth_invalid_key_request_is_rejected_without_upstream_call() {
    let request_body = Bytes::from_static(br#"{"model":"demo","input":"hello"}"#);
    let upstream_body = Bytes::from_static(
        br#"{"error":{"message":"Invalid token","type":"new_api_error","code":""}}"#,
    );
    let responses = (0..5)
        .map(|_| {
            TestUpstreamResponse::new(
                StatusCode::UNAUTHORIZED,
                "application/json; charset=utf-8",
                upstream_body.clone(),
            )
        })
        .collect();
    let (upstream_base_url, captured_rx, server) = spawn_response_sequence_server(responses);
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.auth_failure_threshold = 5;
    config.auth_failure_ip_threshold = 0;
    config.upstream_502_cooldown_enabled = false;
    let state = Arc::new(AppState::new(config).unwrap());

    for _ in 0..5 {
        let (status, body, _) =
            send_auth_failure_request(state.clone(), "sk-auth-integration", request_body.clone())
                .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, upstream_body);
    }

    let oversized_request = Request::builder()
        .method(Method::POST)
        .uri("/v1/responses")
        .header(header::AUTHORIZATION, "Bearer sk-auth-integration")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CONTENT_LENGTH, "999999")
        .body(Body::empty())
        .unwrap();
    let peer_addr: SocketAddr = "198.51.100.70:12345".parse().unwrap();
    let response = handle_protocol(State(state), ConnectInfo(peer_addr), oversized_request).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response.headers().contains_key(header::RETRY_AFTER));
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(body, upstream_body);

    for _ in 0..5 {
        captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    }
    assert!(captured_rx.try_recv().is_err());
    server.join().unwrap();
}

#[tokio::test]
async fn auth_failure_quota_403_never_opens_cooldown() {
    let request_body = Bytes::from_static(br#"{"model":"demo","input":"hello"}"#);
    let upstream_body = Bytes::from_static(
        br#"{"error":{"message":"insufficient balance","type":"new_api_error","code":"insufficient_user_quota"}}"#,
    );
    let responses = (0..6)
        .map(|_| {
            TestUpstreamResponse::new(
                StatusCode::FORBIDDEN,
                "application/json",
                upstream_body.clone(),
            )
        })
        .collect();
    let (upstream_base_url, captured_rx, server) = spawn_response_sequence_server(responses);
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.auth_failure_threshold = 5;
    config.auth_failure_ip_threshold = 0;
    let state = Arc::new(AppState::new(config).unwrap());

    for _ in 0..6 {
        let (status, body, _) =
            send_auth_failure_request(state.clone(), "sk-quota", request_body.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body, upstream_body);
    }

    for _ in 0..6 {
        captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    }
    assert!(captured_rx.try_recv().is_err());
    server.join().unwrap();
}

#[tokio::test]
async fn auth_failure_channel_credential_403_never_opens_cooldown() {
    let request_body = Bytes::from_static(br#"{"model":"demo","input":"hello"}"#);
    let upstream_body = Bytes::from_static(
        br#"{"error":{"message":"channel credential rejected","type":"new_api_error","code":"channel:invalid_key"}}"#,
    );
    let responses = (0..6)
        .map(|_| {
            TestUpstreamResponse::new(
                StatusCode::FORBIDDEN,
                "application/json",
                upstream_body.clone(),
            )
        })
        .collect();
    let (upstream_base_url, captured_rx, server) = spawn_response_sequence_server(responses);
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.auth_failure_threshold = 5;
    config.auth_failure_ip_threshold = 0;
    let state = Arc::new(AppState::new(config).unwrap());

    for _ in 0..6 {
        let (status, body, _) =
            send_auth_failure_request(state.clone(), "sk-channel", request_body.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body, upstream_body);
    }

    for _ in 0..6 {
        captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    }
    assert!(captured_rx.try_recv().is_err());
    server.join().unwrap();
}

#[tokio::test]
async fn auth_failure_sse_business_403_never_opens_cooldown() {
    let request_body =
        Bytes::from_static(br#"{"model":"forbidden-model","stream":true,"input":"hello"}"#);
    let upstream_body = Bytes::from_static(
        b"data: {\"error\":{\"message\":\"model access denied\",\"type\":\"new_api_error\",\"code\":\"model_not_allowed\"}}\n\n",
    );
    let responses = (0..6)
        .map(|_| {
            TestUpstreamResponse::new(
                StatusCode::FORBIDDEN,
                "text/event-stream; charset=utf-8",
                upstream_body.clone(),
            )
        })
        .collect();
    let (upstream_base_url, captured_rx, server) = spawn_response_sequence_server(responses);
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.auth_failure_threshold = 5;
    config.auth_failure_ip_threshold = 0;
    config.stream_early_heartbeat = true;
    config.stream_event_stall_timeout = None;
    let state = Arc::new(AppState::new(config).unwrap());

    for _ in 0..6 {
        let (status, body, headers) =
            send_auth_failure_request(state.clone(), "sk-auth-sse", request_body.clone()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.starts_with(b": heartbeat\n\n"));
        assert!(body.ends_with(&upstream_body));
        assert!(!headers.contains_key(header::RETRY_AFTER));
    }

    for _ in 0..6 {
        captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    }
    assert!(captured_rx.try_recv().is_err());
    server.join().unwrap();
}
