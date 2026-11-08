use super::*;

#[test]
fn cyber_policy_confirmation_detects_violation_terms_in_json_strings() {
    let rules = default_test_cyber_policy_confirmation_rules();

    assert!(request_body_confirms_cyber_policy_violation(
        br#"{"model":"demo","input":"phishing credential attack"}"#,
        &rules
    ));
    assert!(request_body_confirms_cyber_policy_violation(
        r#"{"model":"demo","messages":[{"role":"user","content":"网络攻击和漏洞利用"}]}"#
            .as_bytes(),
        &rules
    ));
}

#[test]
fn cyber_policy_confirmation_supports_configured_regex_rules() {
    let rules = CyberPolicyConfirmationRules::from_config(&CyberPolicyConfirmationRuleConfig {
        keywords: Vec::new(),
        regex: vec![r"(?i)credential\s+stuffing".to_owned()],
    })
    .unwrap();

    assert!(request_body_confirms_cyber_policy_violation(
        br#"{"model":"demo","input":"Credential    Stuffing workflow"}"#,
        &rules
    ));
}

#[test]
fn cyber_policy_confirmation_rejects_unrelated_json_strings() {
    let rules = default_test_cyber_policy_confirmation_rules();

    assert!(!request_body_confirms_cyber_policy_violation(
        br#"{"model":"demo","input":"hello"}"#,
        &rules
    ));
}

#[test]
fn cyber_policy_confirmation_rejects_invalid_regex_rules() {
    let error = CyberPolicyConfirmationRules::from_config(&CyberPolicyConfirmationRuleConfig {
        keywords: Vec::new(),
        regex: vec!["[".to_owned()],
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("invalid cyber-policy confirmation regex pattern"));
}

#[test]
fn detects_cyber_policy_json_error_shapes() {
    assert!(body_has_cyber_policy_error(
        br#"{"error":{"message":"network abuse","code":"cyber_policy"}}"#
    ));
    assert!(body_has_cyber_policy_error(
        br#"{"message":"network abuse","code":"cyber_policy"}"#
    ));
    assert!(!body_has_cyber_policy_error(
        br#"{"error":{"message":"rate limited","code":"rate_limit_exceeded"}}"#
    ));
}

#[test]
fn detects_cyber_policy_streaming_error_frames() {
    let body = b": heartbeat\n\ndata: {\"id\":\"ok\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: {\"error\":{\"message\":\"network abuse\",\"code\":\"cyber_policy\"}}\n\n";

    assert!(sse_body_has_cyber_policy_error(body));
}

#[test]
fn cyber_policy_confirmation_matches_case_insensitively() {
    let config = CyberPolicyConfirmationRuleConfig {
        keywords: vec!["phishing".to_owned(), "漏洞利用".to_owned()],
        regex: vec![r"(?i)credential\s+stuffing".to_owned()],
    };
    let rules = CyberPolicyConfirmationRules::from_config(&config).unwrap();

    // Keyword matching (case-insensitive)
    assert!(rules.is_match("This is a PHISHING attempt"));
    assert!(rules.is_match("phishing"));
    assert!(rules.is_match("Phishing"));
    assert!(rules.is_match("包含漏洞利用的文本"));

    // Regex matching
    assert!(rules.is_match("credential stuffing attack"));
    assert!(rules.is_match("Credential Stuffing"));

    // Non-matching
    assert!(!rules.is_match("normal request"));
    assert!(!rules.is_match("fishing")); // typo, should not match
}

#[test]
fn cyber_policy_confirmation_empty_rules_match_nothing() {
    // Empty rules should not match anything
    let config = CyberPolicyConfirmationRuleConfig {
        keywords: vec![],
        regex: vec![],
    };
    let rules = CyberPolicyConfirmationRules::from_config(&config).unwrap();

    assert!(!rules.is_match("phishing"));
    assert!(!rules.is_match("credential stuffing"));
    assert!(!rules.is_match("any text at all"));
    assert!(!rules.is_match(""));
}

#[tokio::test]
async fn non_stream_proxy_blacklists_key_on_cyber_policy_error() {
    let request_body =
        Bytes::from_static(br#"{"model":"demo","input":"phishing credential attack"}"#);
    let upstream_body = Bytes::from_static(
        br#"{"error":{"message":"network abuse detected","code":"cyber_policy"}}"#,
    );
    let (upstream_base_url, captured_rx, server) = spawn_single_status_response_server(
        StatusCode::BAD_REQUEST,
        "application/json",
        upstream_body,
    );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    let blacklist_file = config.cyber_policy_blacklist_file.clone();
    let black_log_file = config.cyber_policy_black_log_file.clone();
    let ip_blacklist_file = config.cyber_policy_ip_blacklist_file.clone();
    let state = Arc::new(AppState::new(config).unwrap());
    let forensic_writer = state.forensic_writer.clone();
    let ctx = test_proxy_request("/v1/responses", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_with_permit(state, ctx, permit, false).await;

    // Blacklist: return generic 403 error
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let response_json: Value = serde_json::from_slice(&response_body).unwrap();
    assert_eq!(response_json["code"], CYBER_POLICY_CODE);
    assert_eq!(response_json["error"], CYBER_POLICY_BLOCK_MESSAGE);
    forensic_writer.flush().await;
    assert_eq!(
        std::fs::read_to_string(&blacklist_file).unwrap(),
        "sk-test\n"
    );
    let black_log: Value =
        serde_json::from_str(std::fs::read_to_string(&black_log_file).unwrap().trim()).unwrap();
    assert_eq!(black_log["event"], "cyber_policy_black");
    assert_eq!(black_log["request"]["client_ip"], "198.51.100.7");
    assert_eq!(
        black_log["request"]["body"]["utf8_lossy"],
        r#"{"model":"demo","input":"phishing credential attack"}"#
    );
    assert_eq!(
        black_log["request"]["original_body"]["utf8_lossy"],
        r#"{"model":"demo","input":"phishing credential attack"}"#
    );
    assert_eq!(
        black_log["request"]["headers"]["authorization"][0]["redacted"],
        true
    );
    assert!(black_log["request"]["api_key_full"].is_null());
    assert_eq!(black_log["request"]["api_key_masked"], "***");
    assert_eq!(
        black_log["request"]["api_key_hash"],
        crate::auth::hash_api_key("sk-test")
    );
    assert!(black_log["request"].get("api_key_present").is_none());
    assert!(black_log["request"]["body"].get("hex").is_none());
    assert!(black_log["request"]["original_body"].get("hex").is_none());
    assert!(!ip_blacklist_file.exists());

    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    assert!(captured_rx.try_recv().is_err());
    let _ = std::fs::remove_file(&blacklist_file);
    let _ = std::fs::remove_file(&black_log_file);
    server.join().unwrap();
}

#[tokio::test]
async fn non_stream_proxy_skips_blacklist_when_cyber_policy_is_unconfirmed() {
    let request_body = Bytes::from_static(br#"{"model":"demo","input":"hello"}"#);
    let upstream_body = Bytes::from_static(
        br#"{"error":{"message":"network abuse detected","code":"cyber_policy"}}"#,
    );
    let (upstream_base_url, captured_rx, server) = spawn_single_status_response_server(
        StatusCode::BAD_REQUEST,
        "application/json",
        upstream_body,
    );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    let blacklist_file = config.cyber_policy_blacklist_file.clone();
    let black_log_file = config.cyber_policy_black_log_file.clone();
    let ip_blacklist_file = config.cyber_policy_ip_blacklist_file.clone();
    let state = Arc::new(AppState::new(config).unwrap());
    let ctx = test_proxy_request("/v1/responses", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_with_permit(state, ctx, permit, false).await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let response_json: Value = serde_json::from_slice(&response_body).unwrap();
    assert_eq!(response_json["code"], CYBER_POLICY_CODE);
    assert_eq!(response_json["error"], CYBER_POLICY_BLOCK_MESSAGE);
    assert!(!blacklist_file.exists());
    assert!(!black_log_file.exists());
    assert!(!ip_blacklist_file.exists());

    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    assert!(captured_rx.try_recv().is_err());
    server.join().unwrap();
}

#[tokio::test]
async fn non_stream_proxy_passes_through_cyber_policy_when_ban_is_disabled() {
    let request_body =
        Bytes::from_static(br#"{"model":"demo","input":"phishing credential attack"}"#);
    let upstream_body = Bytes::from_static(
        br#"{"error":{"message":"network abuse detected","code":"cyber_policy"}}"#,
    );
    let (upstream_base_url, captured_rx, server) = spawn_single_status_response_server(
        StatusCode::BAD_REQUEST,
        "application/json",
        upstream_body.clone(),
    );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.cyber_policy_ban_enabled = false;
    let blacklist_file = config.cyber_policy_blacklist_file.clone();
    let black_log_file = config.cyber_policy_black_log_file.clone();
    let ip_blacklist_file = config.cyber_policy_ip_blacklist_file.clone();
    let white_log_file = config.cyber_policy_white_log_file.clone();
    let state = Arc::new(AppState::new(config).unwrap());
    let forensic_writer = state.forensic_writer.clone();
    let ctx = test_proxy_request("/v1/responses", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_with_permit(state, ctx, permit, false).await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(response_body, upstream_body);
    forensic_writer.flush().await;
    assert!(!blacklist_file.exists());
    assert!(!black_log_file.exists());
    assert!(!ip_blacklist_file.exists());
    assert!(!white_log_file.exists());

    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    assert!(captured_rx.try_recv().is_err());
    server.join().unwrap();
}

#[tokio::test]
async fn non_stream_proxy_passes_through_response_for_whitelisted_key() {
    let request_body =
        Bytes::from_static(br#"{"model":"demo","input":"phishing credential attack"}"#);
    let upstream_body = Bytes::from_static(
        br#"{"error":{"message":"network abuse detected","code":"cyber_policy"}}"#,
    );
    let (upstream_base_url, captured_rx, server) = spawn_single_status_response_server(
        StatusCode::BAD_REQUEST,
        "application/json",
        upstream_body.clone(),
    );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    let blacklist_file = config.cyber_policy_blacklist_file.clone();
    let black_log_file = config.cyber_policy_black_log_file.clone();
    let whitelist_file = config.cyber_policy_whitelist_file.clone();
    let white_log_file = config.cyber_policy_white_log_file.clone();
    let _ = std::fs::remove_file(&blacklist_file);
    let _ = std::fs::remove_file(&black_log_file);
    let _ = std::fs::remove_file(&whitelist_file);
    let _ = std::fs::remove_file(&white_log_file);

    std::fs::write(&whitelist_file, "sk-test\n").unwrap();

    let state = Arc::new(AppState::new(config).unwrap());
    let forensic_writer = state.forensic_writer.clone();
    let ctx = test_proxy_request("/v1/responses", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_with_permit(state, ctx, permit, false).await;

    // Whitelist: pass through the original upstream response (400)
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let response_json: Value = serde_json::from_slice(&response_body).unwrap();
    assert_eq!(response_json["error"]["code"], CYBER_POLICY_CODE);
    assert_eq!(response_json["error"]["message"], "network abuse detected");

    forensic_writer.flush().await;

    // Key should NOT be blacklisted
    assert!(!blacklist_file.exists());
    assert!(!black_log_file.exists());

    // White log should exist
    assert!(white_log_file.exists());
    let white_log: Value =
        serde_json::from_str(std::fs::read_to_string(&white_log_file).unwrap().trim()).unwrap();
    assert_eq!(white_log["event"], "cyber_policy_white");
    assert_eq!(white_log["trigger_count"], 1);
    assert_eq!(white_log["request"]["client_ip"], "198.51.100.7");
    assert!(white_log["request"]["api_key_full"].is_null());
    assert_eq!(
        white_log["request"]["api_key_hash"],
        crate::auth::hash_api_key("sk-test")
    );

    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    assert!(captured_rx.try_recv().is_err());
    let _ = std::fs::remove_file(&whitelist_file);
    let _ = std::fs::remove_file(&white_log_file);
    server.join().unwrap();
}

#[tokio::test]
async fn handle_protocol_ignores_existing_blacklists_when_cyber_policy_ban_is_disabled() {
    let request_body = Bytes::from_static(br#"{"model":"demo","input":"hello"}"#);
    let upstream_body =
        Bytes::from_static(br#"{"id":"resp_test","object":"response","output":[]}"#);
    let (upstream_base_url, captured_rx, server) =
        spawn_single_response_server("application/json", upstream_body.clone());
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.cyber_policy_ban_enabled = false;
    std::fs::write(&config.cyber_policy_blacklist_file, "sk-test\n").unwrap();
    std::fs::write(&config.cyber_policy_ip_blacklist_file, "127.0.0.1\n").unwrap();
    let blacklist_file = config.cyber_policy_blacklist_file.clone();
    let ip_blacklist_file = config.cyber_policy_ip_blacklist_file.clone();
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
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(response_body, upstream_body);
    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    assert!(captured_rx.try_recv().is_err());

    let _ = std::fs::remove_file(blacklist_file);
    let _ = std::fs::remove_file(ip_blacklist_file);
    server.join().unwrap();
}

#[tokio::test]
async fn reload_runtime_missing_whitelist_file_preserves_existing_whitelist_cache() {
    let mut config = retry_test_config();
    let whitelist_file = config.cyber_policy_whitelist_file.clone();
    std::fs::write(&whitelist_file, "sk-test\n").unwrap();
    let state = AppState::new(config.clone()).unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(header::AUTHORIZATION, "Bearer sk-test".parse().unwrap());
    let api_key = ApiKeyInfo::from_headers(&headers);
    assert!(state.whitelist_cache.api_key_is_whitelisted(&api_key));

    config.cyber_policy_whitelist_file =
        unique_temp_dir("ai-bridge-test-missing-whitelist").with_extension("txt");
    let error = state.reload_runtime(config).unwrap_err().to_string();

    assert!(error.contains("failed to prepare whitelist cache sync from disk"));
    assert!(state.whitelist_cache.api_key_is_whitelisted(&api_key));

    let _ = std::fs::remove_file(whitelist_file);
}

#[tokio::test]
async fn reload_runtime_build_failure_does_not_mutate_whitelist_cache() {
    let mut config = retry_test_config();
    let whitelist_file = config.cyber_policy_whitelist_file.clone();
    std::fs::write(&whitelist_file, "sk-test\n").unwrap();
    let state = AppState::new(config.clone()).unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(header::AUTHORIZATION, "Bearer sk-test".parse().unwrap());
    let api_key = ApiKeyInfo::from_headers(&headers);
    assert!(state.whitelist_cache.api_key_is_whitelisted(&api_key));

    config.cyber_policy_whitelist_file =
        unique_temp_dir("ai-bridge-test-missing-whitelist-after-build").with_extension("txt");
    config.cyber_policy_confirmation_rules.regex = vec!["[".to_owned()];
    let error = state.reload_runtime(config).unwrap_err().to_string();

    assert!(error.contains("invalid cyber-policy confirmation regex patterns"));
    assert!(state.whitelist_cache.api_key_is_whitelisted(&api_key));

    let _ = std::fs::remove_file(whitelist_file);
}

#[tokio::test]
async fn cyber_policy_whitelist_file_recheck_prevents_blacklist_when_cache_misses() {
    let config = Arc::new(retry_test_config());
    let blacklist_file = config.cyber_policy_blacklist_file.clone();
    let white_log_file = config.cyber_policy_white_log_file.clone();
    let whitelist_file = config.cyber_policy_whitelist_file.clone();
    let client = Client::builder()
        .connect_timeout(config.connect_timeout)
        .build()
        .unwrap();
    let ctx = test_proxy_request_with_runtime(
        "/v1/responses",
        Bytes::from_static(br#"{"model":"demo","input":"phishing credential attack"}"#),
        config,
        client,
    );
    assert!(!ctx.whitelist_cache.api_key_is_whitelisted(&ctx.api_key));
    std::fs::write(&whitelist_file, "sk-test\n").unwrap();

    let action = block_cyber_policy_key(&ctx);

    assert_eq!(action, CyberPolicyAction::WhitelistLog);
    ctx.forensic_writer.flush().await;
    assert!(!blacklist_file.exists());
    assert!(white_log_file.exists());

    let _ = std::fs::remove_file(whitelist_file);
    let _ = std::fs::remove_file(white_log_file);
}

#[tokio::test]
async fn early_heartbeat_stream_wrapper_blocks_non_sse_cyber_policy_before_body_end() {
    let request_body = Bytes::from_static(
            br#"{"model":"demo","stream":true,"messages":[{"role":"user","content":"phishing credential attack"}]}"#,
        );
    let upstream_first = Bytes::from_static(
        br#"{"error":{"message":"upstream network abuse","code":"cyber_policy"}}"#,
    );
    let upstream_after = Bytes::from_static(br#"{"tail":"after"}"#);
    let (upstream_base_url, captured_rx, server) =
        spawn_disconnect_tolerant_delayed_streaming_response_server(
            "application/json; charset=utf-8",
            vec![
                (upstream_first, Duration::ZERO),
                (upstream_after, Duration::from_millis(250)),
            ],
            Duration::ZERO,
        );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.stream_early_heartbeat = true;
    let blacklist_file = config.cyber_policy_blacklist_file.clone();
    let state = Arc::new(AppState::new(config).unwrap());
    let ctx = test_proxy_request("/v1/chat/completions", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_stream_with_permit_or_retry_sse(state, ctx, permit).await;
    let response_body = tokio::time::timeout(
        Duration::from_millis(100),
        to_bytes(response.into_body(), 1024 * 1024),
    )
    .await
    .unwrap()
    .unwrap();
    let response_text = String::from_utf8(response_body.to_vec()).unwrap();

    assert!(response_text.starts_with(": heartbeat\n\n"));
    assert!(response_text.contains("event: error"));
    assert!(response_text.contains(CYBER_POLICY_BLOCK_MESSAGE));
    assert!(response_text.ends_with("data: [DONE]\n\n"));
    assert!(!response_text.contains("after"));
    assert_eq!(
        std::fs::read_to_string(&blacklist_file).unwrap(),
        "sk-test\n"
    );

    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    let _ = std::fs::remove_file(&blacklist_file);
    server.join().unwrap();
}

#[tokio::test]
async fn early_heartbeat_stream_wrapper_whitelist_keeps_detected_non_sse_chunk() {
    let request_body = Bytes::from_static(
            br#"{"model":"demo","stream":true,"messages":[{"role":"user","content":"phishing credential attack"}]}"#,
        );
    let upstream_first = Bytes::from_static(
        br#"{"error":{"message":"upstream network abuse","code":"cyber_policy"},"#,
    );
    let upstream_after = Bytes::from_static(br#""tail":"after"}"#);
    let (upstream_base_url, captured_rx, server) =
        spawn_disconnect_tolerant_delayed_streaming_response_server(
            "application/json; charset=utf-8",
            vec![
                (upstream_first, Duration::ZERO),
                (upstream_after, Duration::from_millis(10)),
            ],
            Duration::ZERO,
        );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.stream_early_heartbeat = true;
    let blacklist_file = config.cyber_policy_blacklist_file.clone();
    let whitelist_file = config.cyber_policy_whitelist_file.clone();
    let white_log_file = config.cyber_policy_white_log_file.clone();
    let _ = std::fs::remove_file(&blacklist_file);
    let _ = std::fs::remove_file(&whitelist_file);
    let _ = std::fs::remove_file(&white_log_file);
    std::fs::write(&whitelist_file, "sk-test\n").unwrap();

    let state = Arc::new(AppState::new(config).unwrap());
    let forensic_writer = state.forensic_writer.clone();
    let ctx = test_proxy_request("/v1/chat/completions", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_stream_with_permit_or_retry_sse(state, ctx, permit).await;
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let response_text = String::from_utf8(response_body.to_vec()).unwrap();

    assert!(response_text.contains("event: upstream_response"));
    assert!(response_text.contains("upstream network abuse"));
    assert!(response_text.contains(r#"\"code\":\"cyber_policy\""#));
    assert!(response_text.contains(r#"\"tail\":\"after\""#));
    assert!(!response_text.contains(CYBER_POLICY_BLOCK_MESSAGE));
    assert!(response_text.ends_with("data: [DONE]\n\n"));

    forensic_writer.flush().await;
    assert!(!blacklist_file.exists());
    assert!(white_log_file.exists());

    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    let _ = std::fs::remove_file(&whitelist_file);
    let _ = std::fs::remove_file(&white_log_file);
    server.join().unwrap();
}

#[tokio::test]
async fn image_sse_wrapper_blocks_non_sse_cyber_policy_before_body_end() {
    let request_body =
        Bytes::from_static(br#"{"model":"demo-image","prompt":"phishing credential attack"}"#);
    let upstream_first = Bytes::from_static(
        br#"{"error":{"message":"upstream network abuse","code":"cyber_policy"}}"#,
    );
    let upstream_after = Bytes::from_static(br#"{"tail":"after"}"#);
    let (upstream_base_url, captured_rx, server) =
        spawn_disconnect_tolerant_delayed_streaming_response_server(
            "application/json; charset=utf-8",
            vec![
                (upstream_first, Duration::ZERO),
                (upstream_after, Duration::from_millis(250)),
            ],
            Duration::ZERO,
        );
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.image_response_mode = crate::config::ImageResponseMode::Sse;
    let blacklist_file = config.cyber_policy_blacklist_file.clone();
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
    let response_body = tokio::time::timeout(
        Duration::from_millis(100),
        to_bytes(response.into_body(), 1024 * 1024),
    )
    .await
    .unwrap()
    .unwrap();
    let response_text = String::from_utf8(response_body.to_vec()).unwrap();

    assert!(response_text.contains("event: error"));
    assert!(response_text.contains(CYBER_POLICY_BLOCK_MESSAGE));
    assert!(response_text.ends_with("data: [DONE]\n\n"));
    assert!(!response_text.contains("after"));
    assert_eq!(
        std::fs::read_to_string(&blacklist_file).unwrap(),
        "sk-test\n"
    );

    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    let _ = std::fs::remove_file(&blacklist_file);
    server.join().unwrap();
}
#[tokio::test]
async fn cyber_policy_blacklists_client_ip_after_three_confirmed_black_log_records() {
    let config = Arc::new(retry_test_config());
    let blacklist_file = config.cyber_policy_blacklist_file.clone();
    let black_log_file = config.cyber_policy_black_log_file.clone();
    let ip_blacklist_file = config.cyber_policy_ip_blacklist_file.clone();
    let client = Client::builder()
        .connect_timeout(config.connect_timeout)
        .build()
        .unwrap();
    let forensic_writer = ForensicWriter::new(
        config.cyber_policy_black_log_file.clone(),
        config.cyber_policy_white_log_file.clone(),
    )
    .unwrap();

    let mut first = test_proxy_request_with_runtime_and_writer(
        "/v1/responses",
        Bytes::from_static(br#"{"model":"demo","input":"phishing first"}"#),
        config.clone(),
        client.clone(),
        forensic_writer.clone(),
    );
    first
        .headers
        .insert(header::AUTHORIZATION, "Bearer sk-first".parse().unwrap());
    first.api_key = ApiKeyInfo::from_headers(&first.headers);

    let mut second = test_proxy_request_with_runtime_and_writer(
        "/v1/responses",
        Bytes::from_static(br#"{"model":"demo","input":"malware second"}"#),
        config.clone(),
        client.clone(),
        forensic_writer.clone(),
    );
    second
        .headers
        .insert(header::AUTHORIZATION, "Bearer sk-second".parse().unwrap());
    second.api_key = ApiKeyInfo::from_headers(&second.headers);

    let mut third = test_proxy_request_with_runtime_and_writer(
        "/v1/responses",
        Bytes::from_static(br#"{"model":"demo","input":"ransomware third"}"#),
        config,
        client,
        forensic_writer.clone(),
    );
    third
        .headers
        .insert(header::AUTHORIZATION, "Bearer sk-third".parse().unwrap());
    third.api_key = ApiKeyInfo::from_headers(&third.headers);

    block_cyber_policy_key(&first);
    forensic_writer.flush().await;
    assert!(!ip_blacklist_file.exists());

    block_cyber_policy_key(&second);
    forensic_writer.flush().await;
    assert!(!ip_blacklist_file.exists());

    block_cyber_policy_key(&third);
    forensic_writer.flush().await;

    assert_eq!(
        std::fs::read_to_string(&ip_blacklist_file).unwrap(),
        "198.51.100.7\n"
    );
    assert_eq!(
        std::fs::read_to_string(&black_log_file)
            .unwrap()
            .lines()
            .count(),
        3
    );
    assert_eq!(
        std::fs::read_to_string(&blacklist_file)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        vec!["sk-first", "sk-second", "sk-third"]
    );

    let _ = std::fs::remove_file(&blacklist_file);
    let _ = std::fs::remove_file(&black_log_file);
    let _ = std::fs::remove_file(&ip_blacklist_file);
}

#[tokio::test]
async fn handle_protocol_blacklists_forwarded_client_ip_instead_of_loopback_peer() {
    let first_body = Bytes::from_static(br#"{"model":"demo","input":"phishing first"}"#);
    let second_body = Bytes::from_static(br#"{"model":"demo","input":"malware second"}"#);
    let third_body = Bytes::from_static(br#"{"model":"demo","input":"ransomware third"}"#);
    let upstream_body = Bytes::from_static(
        br#"{"error":{"message":"network abuse detected","code":"cyber_policy"}}"#,
    );
    let (upstream_base_url, captured_rx, server) = spawn_response_sequence_server(vec![
        TestUpstreamResponse::new(
            StatusCode::BAD_REQUEST,
            "application/json",
            upstream_body.clone(),
        ),
        TestUpstreamResponse::new(StatusCode::BAD_REQUEST, "application/json", upstream_body),
        TestUpstreamResponse::new(
            StatusCode::BAD_REQUEST,
            "application/json",
            Bytes::from_static(
                br#"{"error":{"message":"network abuse detected","code":"cyber_policy"}}"#,
            ),
        ),
    ]);
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    let blacklist_file = config.cyber_policy_blacklist_file.clone();
    let black_log_file = config.cyber_policy_black_log_file.clone();
    let ip_blacklist_file = config.cyber_policy_ip_blacklist_file.clone();
    let state = Arc::new(AppState::new(config).unwrap());
    let peer_addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();

    let first = Request::builder()
        .method(Method::POST)
        .uri("/v1/responses")
        .header(header::AUTHORIZATION, "Bearer sk-forwarded-one")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-forwarded-for", "203.0.113.44")
        .body(Body::from(first_body.clone()))
        .unwrap();
    let first_response = handle_protocol(State(state.clone()), ConnectInfo(peer_addr), first).await;
    assert_eq!(first_response.status(), StatusCode::FORBIDDEN);
    assert!(!ip_blacklist_file.exists());

    let second = Request::builder()
        .method(Method::POST)
        .uri("/v1/responses")
        .header(header::AUTHORIZATION, "Bearer sk-forwarded-two")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-forwarded-for", "203.0.113.44")
        .body(Body::from(second_body.clone()))
        .unwrap();
    let second_response =
        handle_protocol(State(state.clone()), ConnectInfo(peer_addr), second).await;
    assert_eq!(second_response.status(), StatusCode::FORBIDDEN);
    assert!(!ip_blacklist_file.exists());

    let third = Request::builder()
        .method(Method::POST)
        .uri("/v1/responses")
        .header(header::AUTHORIZATION, "Bearer sk-forwarded-three")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-forwarded-for", "203.0.113.44")
        .body(Body::from(third_body.clone()))
        .unwrap();
    let third_response = handle_protocol(State(state.clone()), ConnectInfo(peer_addr), third).await;
    assert_eq!(third_response.status(), StatusCode::FORBIDDEN);

    assert_eq!(
        std::fs::read_to_string(&ip_blacklist_file).unwrap(),
        "203.0.113.44\n"
    );
    state.forensic_writer.flush().await;
    let black_log_lines = std::fs::read_to_string(&black_log_file).unwrap();
    let black_log_values = black_log_lines
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(black_log_values.len(), 3);
    assert!(
        black_log_values
            .iter()
            .all(|value| value["request"]["client_ip"] == "203.0.113.44")
    );
    assert_eq!(
        std::fs::read_to_string(&blacklist_file)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        vec!["sk-forwarded-one", "sk-forwarded-two", "sk-forwarded-three"]
    );

    let first_captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let second_captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let third_captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(first_captured.body, first_body);
    assert_eq!(second_captured.body, second_body);
    assert_eq!(third_captured.body, third_body);
    assert!(
        first_captured
            .head
            .contains("x-forwarded-for: 203.0.113.44")
    );
    assert!(
        second_captured
            .head
            .contains("x-forwarded-for: 203.0.113.44")
    );
    assert!(
        third_captured
            .head
            .contains("x-forwarded-for: 203.0.113.44")
    );
    server.join().unwrap();

    let _ = std::fs::remove_file(&blacklist_file);
    let _ = std::fs::remove_file(&black_log_file);
    let _ = std::fs::remove_file(&ip_blacklist_file);
}

#[tokio::test]
async fn handle_protocol_blocks_key_already_in_cyber_policy_blacklist() {
    let mut config = retry_test_config();
    let blacklist_file = config.cyber_policy_blacklist_file.clone();
    std::fs::write(&blacklist_file, "sk-test\n").unwrap();
    config.upstream_base_url = Url::parse("http://127.0.0.1:9").unwrap();
    let state = Arc::new(AppState::new(config).unwrap());
    let request = Request::builder()
        .method(Method::POST)
        .uri("/v1/responses")
        .header(header::AUTHORIZATION, "Bearer sk-test")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(Bytes::from_static(
            br#"{"model":"demo","input":"hello"}"#,
        )))
        .unwrap();
    let peer_addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();

    let response = handle_protocol(State(state), ConnectInfo(peer_addr), request).await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let response_json: Value = serde_json::from_slice(&response_body).unwrap();
    assert_eq!(response_json["code"], CYBER_POLICY_CODE);
    assert_eq!(response_json["error"], CYBER_POLICY_BLOCK_MESSAGE);
    let _ = std::fs::remove_file(&blacklist_file);
}

#[tokio::test]
async fn handle_protocol_blocks_ip_already_in_cyber_policy_blacklist() {
    let mut config = retry_test_config();
    let ip_blacklist_file = config.cyber_policy_ip_blacklist_file.clone();
    std::fs::write(&ip_blacklist_file, "127.0.0.1\n").unwrap();
    config.upstream_base_url = Url::parse("http://127.0.0.1:9").unwrap();
    let state = Arc::new(AppState::new(config).unwrap());
    let request = Request::builder()
        .method(Method::POST)
        .uri("/v1/responses")
        .header(header::AUTHORIZATION, "Bearer sk-not-yet-key-blocked")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(Bytes::from_static(
            br#"{"model":"demo","input":"hello"}"#,
        )))
        .unwrap();
    let peer_addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();

    let response = handle_protocol(State(state), ConnectInfo(peer_addr), request).await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let response_json: Value = serde_json::from_slice(&response_body).unwrap();
    assert_eq!(response_json["code"], CYBER_POLICY_CODE);
    assert_eq!(response_json["error"], CYBER_POLICY_BLOCK_MESSAGE);
    let _ = std::fs::remove_file(&ip_blacklist_file);
}

#[tokio::test]
async fn handle_protocol_forwards_text_that_previously_matched_local_keyword_rules() {
    let request_body = Bytes::from_static(
        br#"{"model":"demo","input":"example blocked phrase should reach upstream"}"#,
    );
    let upstream_body = Bytes::from_static(br#"{"ok":true}"#);
    let (upstream_base_url, captured_rx, server) =
        spawn_single_response_server("application/json; charset=utf-8", upstream_body.clone());
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
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
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(response_body, upstream_body);
    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    server.join().unwrap();
}

#[tokio::test]
async fn stream_proxy_blacklists_key_on_midstream_cyber_policy_error() {
    let request_body = Bytes::from_static(
            br#"{"model":"demo","stream":true,"messages":[{"role":"user","content":"phishing credential attack"}]}"#,
        );
    let upstream_body = Bytes::from_static(
            b"data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"delta\":{\"content\":\"hi\"},\"index\":0}]}\n\ndata: {\"error\":{\"message\":\"upstream network abuse\",\"code\":\"cyber_policy\"}}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"after\"},\"index\":0}]}\n\n",
        );
    let (upstream_base_url, captured_rx, server) =
        spawn_single_response_server("text/event-stream; charset=utf-8", upstream_body);
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    let blacklist_file = config.cyber_policy_blacklist_file.clone();
    let state = Arc::new(AppState::new(config).unwrap());
    let ctx = test_proxy_request("/v1/chat/completions", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_stream_with_permit_or_retry_sse(state, ctx, permit).await;

    assert_eq!(response.status(), StatusCode::OK);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let response_text = String::from_utf8(response_body.to_vec()).unwrap();

    assert!(response_text.contains("\"content\":\"hi\""));
    assert!(response_text.contains("\"content\":\"after\""));
    assert!(response_text.contains("\"code\":\"cyber_policy\""));

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        std::fs::read_to_string(&blacklist_file).unwrap(),
        "sk-test\n"
    );

    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    let _ = std::fs::remove_file(&blacklist_file);
    server.join().unwrap();
}

#[tokio::test]
async fn stream_proxy_passes_through_response_for_whitelisted_key() {
    let request_body = Bytes::from_static(
            br#"{"model":"demo","stream":true,"messages":[{"role":"user","content":"phishing credential attack"}]}"#,
        );
    let upstream_body = Bytes::from_static(
            b"data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"delta\":{\"content\":\"hi\"},\"index\":0}]}\n\ndata: {\"error\":{\"message\":\"upstream network abuse\",\"code\":\"cyber_policy\"}}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"after\"},\"index\":0}]}\n\n",
        );
    let (upstream_base_url, captured_rx, server) =
        spawn_single_response_server("text/event-stream; charset=utf-8", upstream_body);
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    let blacklist_file = config.cyber_policy_blacklist_file.clone();
    let whitelist_file = config.cyber_policy_whitelist_file.clone();
    let white_log_file = config.cyber_policy_white_log_file.clone();
    let _ = std::fs::remove_file(&blacklist_file);
    let _ = std::fs::remove_file(&whitelist_file);
    let _ = std::fs::remove_file(&white_log_file);

    std::fs::write(&whitelist_file, "sk-test\n").unwrap();

    let state = Arc::new(AppState::new(config).unwrap());
    let forensic_writer = state.forensic_writer.clone();
    let ctx = test_proxy_request("/v1/chat/completions", request_body.clone());
    let permit = state
        .rate_limiter
        .try_acquire(ctx.api_key.identity())
        .unwrap();

    let response = forward_stream_with_permit_or_retry_sse(state, ctx, permit).await;

    assert_eq!(response.status(), StatusCode::OK);
    let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let response_text = String::from_utf8(response_body.to_vec()).unwrap();

    assert!(response_text.contains("\"content\":\"hi\""));
    assert!(response_text.contains("\"error\""));
    assert!(response_text.contains("upstream network abuse"));
    assert!(response_text.contains("\"content\":\"after\""));
    assert!(!response_text.contains(CYBER_POLICY_BLOCK_MESSAGE));

    forensic_writer.flush().await;

    assert!(!blacklist_file.exists());

    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(white_log_file.exists());
    let white_log: Value =
        serde_json::from_str(std::fs::read_to_string(&white_log_file).unwrap().trim()).unwrap();
    assert_eq!(white_log["event"], "cyber_policy_white");
    assert_eq!(white_log["trigger_count"], 1);

    let captured = captured_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(captured.body, request_body);
    let _ = std::fs::remove_file(&whitelist_file);
    let _ = std::fs::remove_file(&white_log_file);
    server.join().unwrap();
}
