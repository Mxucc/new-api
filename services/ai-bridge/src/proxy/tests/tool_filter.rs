use super::*;

#[tokio::test]
async fn tool_filter_preserves_all_request_parameters() {
    // 测试工具过滤后是否保留所有请求参数（temperature, max_tokens, top_p 等）
    let request_body = Bytes::from_static(
            br#"{"model":"gpt-4","messages":[{"role":"user","content":"test"}],"tools":[{"type":"function","function":{"name":"test"}},{"type":"computer_20241022","name":"computer"}],"temperature":0.7,"max_tokens":100,"top_p":0.9,"frequency_penalty":0.5,"presence_penalty":0.3,"seed":42}"#,
        );
    let (upstream_base_url, captured_rx, server) =
        spawn_single_response_server("application/json", Bytes::from_static(b"{}"));
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config
        .filter_tools_by_model
        .insert("gpt-4".to_owned(), vec!["computer_20241022".to_owned()]);
    let state = Arc::new(AppState::new(config).unwrap());

    // 使用 handle_protocol 而不是 forward_with_permit，以测试完整的请求处理流程
    let request = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", "Bearer sk-test")
        .header("content-type", "application/json")
        .body(Body::from(request_body))
        .unwrap();

    let peer_addr = SocketAddr::from(([127, 0, 0, 1], 12345));
    let response = handle_protocol(State(state), ConnectInfo(peer_addr), request).await;

    assert_eq!(response.status(), StatusCode::OK);
    let captured = captured_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    let captured_json: Value = serde_json::from_slice(&captured.body).unwrap();

    // 验证所有参数都被保留
    assert_eq!(captured_json["model"], "gpt-4");
    assert_eq!(captured_json["temperature"], 0.7);
    assert_eq!(captured_json["max_tokens"], 100);
    assert_eq!(captured_json["top_p"], 0.9);
    assert_eq!(captured_json["frequency_penalty"], 0.5);
    assert_eq!(captured_json["presence_penalty"], 0.3);
    assert_eq!(captured_json["seed"], 42);

    // 验证工具被正确过滤
    let tools = captured_json["tools"].as_array().unwrap();
    assert_eq!(
        tools.len(),
        1,
        "Expected 1 tool after filtering, got {}",
        tools.len()
    );
    assert_eq!(tools[0]["type"], "function");

    server.join().unwrap();
}

#[test]
fn filter_tools_removes_matching_tool_types() {
    let mut config = HashMap::new();
    config.insert("gpt".to_owned(), vec!["image_generation".to_owned()]);
    let tool_filter_index = test_tool_filter_index(&config);

    let body = Bytes::from(
        r#"{"model":"gpt-4","tools":[{"type":"image_generation"},{"type":"code_interpreter"}]}"#,
    );
    let config_arc = Arc::new(retry_test_config());
    let api_key =
        ApiKeyInfo::from_headers_with_fallback(&HeaderMap::new(), "127.0.0.1".parse().unwrap());

    let filtered = filter_tools_by_model(
        &body,
        &config,
        &tool_filter_index,
        &config_arc,
        &api_key,
        "/v1/chat/completions",
    );

    assert!(filtered.is_some());
    let json: Value = serde_json::from_slice(&filtered.unwrap()).unwrap();
    let tools = json.get("tools").unwrap().as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(
        tools[0].get("type").unwrap().as_str().unwrap(),
        "code_interpreter"
    );
}

#[test]
fn filter_tools_removes_matching_tool_after_large_prompt_prefix() {
    let mut config = HashMap::new();
    config.insert("gpt".to_owned(), vec!["image_generation".to_owned()]);
    let tool_filter_index = test_tool_filter_index(&config);

    let body = Bytes::from(format!(
        r#"{{"model":"gpt-4","messages":[{{"role":"user","content":"{}"}}],"tools":[{{"type":"image_generation"}}]}}"#,
        "x".repeat(2048)
    ));
    let config_arc = Arc::new(retry_test_config());
    let api_key =
        ApiKeyInfo::from_headers_with_fallback(&HeaderMap::new(), "127.0.0.1".parse().unwrap());

    let filtered = filter_tools_by_model(
        &body,
        &config,
        &tool_filter_index,
        &config_arc,
        &api_key,
        "/v1/chat/completions",
    );

    assert!(filtered.is_some());
    let json: Value = serde_json::from_slice(&filtered.unwrap()).unwrap();
    assert!(json.get("tools").is_none());
}

#[test]
fn filter_tools_removes_tools_field_when_empty() {
    let mut config = HashMap::new();
    config.insert("gpt".to_owned(), vec!["image_generation".to_owned()]);
    let tool_filter_index = test_tool_filter_index(&config);

    let body = Bytes::from(r#"{"model":"gpt-4","tools":[{"type":"image_generation"}]}"#);
    let config_arc = Arc::new(retry_test_config());
    let api_key =
        ApiKeyInfo::from_headers_with_fallback(&HeaderMap::new(), "127.0.0.1".parse().unwrap());

    let filtered = filter_tools_by_model(
        &body,
        &config,
        &tool_filter_index,
        &config_arc,
        &api_key,
        "/v1/chat/completions",
    );

    assert!(filtered.is_some());
    let json: Value = serde_json::from_slice(&filtered.unwrap()).unwrap();
    assert!(json.get("tools").is_none());
}

#[test]
fn filter_tools_uses_all_matching_model_prefixes() {
    let mut config = HashMap::new();
    config.insert("gpt".to_owned(), vec!["image_generation".to_owned()]);
    config.insert("gpt-4".to_owned(), vec!["code_interpreter".to_owned()]);
    let tool_filter_index = test_tool_filter_index(&config);

    let body = Bytes::from(
        r#"{"model":"gpt-4.1","tools":[{"type":"image_generation"},{"type":"code_interpreter"},{"type":"function","function":{"name":"lookup"}}]}"#,
    );
    let config_arc = Arc::new(retry_test_config());
    let api_key =
        ApiKeyInfo::from_headers_with_fallback(&HeaderMap::new(), "127.0.0.1".parse().unwrap());

    let filtered = filter_tools_by_model(
        &body,
        &config,
        &tool_filter_index,
        &config_arc,
        &api_key,
        "/v1/chat/completions",
    );

    assert!(filtered.is_some());
    let json: Value = serde_json::from_slice(&filtered.unwrap()).unwrap();
    let tools = json.get("tools").unwrap().as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].get("type").unwrap().as_str().unwrap(), "function");
}

#[test]
fn filter_tools_returns_none_when_no_match() {
    let mut config = HashMap::new();
    config.insert("gpt".to_owned(), vec!["image_generation".to_owned()]);
    let tool_filter_index = test_tool_filter_index(&config);

    let body = Bytes::from(r#"{"model":"claude-3","tools":[{"type":"image_generation"}]}"#);
    let config_arc = Arc::new(retry_test_config());
    let api_key =
        ApiKeyInfo::from_headers_with_fallback(&HeaderMap::new(), "127.0.0.1".parse().unwrap());

    let filtered = filter_tools_by_model(
        &body,
        &config,
        &tool_filter_index,
        &config_arc,
        &api_key,
        "/v1/chat/completions",
    );

    assert!(filtered.is_none());
}

#[test]
fn filter_tools_returns_none_when_config_empty() {
    let config = HashMap::new();
    let tool_filter_index = test_tool_filter_index(&config);

    let body = Bytes::from(r#"{"model":"gpt-4","tools":[{"type":"image_generation"}]}"#);
    let config_arc = Arc::new(retry_test_config());
    let api_key =
        ApiKeyInfo::from_headers_with_fallback(&HeaderMap::new(), "127.0.0.1".parse().unwrap());

    let filtered = filter_tools_by_model(
        &body,
        &config,
        &tool_filter_index,
        &config_arc,
        &api_key,
        "/v1/chat/completions",
    );

    assert!(filtered.is_none());
}

#[test]
fn tool_filter_prefilter_skips_bodies_without_tools_marker() {
    let mut config = HashMap::new();
    config.insert("gpt".to_owned(), vec!["image_generation".to_owned()]);
    let tool_filter_index = test_tool_filter_index(&config);

    let body = Bytes::from(r#"{"model":"gpt-4","input":"image_generation"}"#);

    assert!(!request_body_may_need_tool_filter(
        &body,
        &tool_filter_index
    ));
}

#[test]
fn tool_filter_prefilter_skips_bodies_without_configured_tool_marker() {
    let mut config = HashMap::new();
    config.insert("gpt".to_owned(), vec!["image_generation".to_owned()]);
    let tool_filter_index = test_tool_filter_index(&config);

    let body = Bytes::from(r#"{"model":"gpt-4","tools":[{"type":"function"}]}"#);

    assert!(!request_body_may_need_tool_filter(
        &body,
        &tool_filter_index
    ));
}

#[test]
fn tool_filter_prefilter_accepts_matching_tool_marker() {
    let mut config = HashMap::new();
    config.insert("gpt".to_owned(), vec!["image_generation".to_owned()]);
    let tool_filter_index = test_tool_filter_index(&config);

    let body = Bytes::from(r#"{"model":"gpt-4","tools":[{"type":"image_generation"}]}"#);

    assert!(request_body_may_need_tool_filter(&body, &tool_filter_index));
}

#[test]
fn tool_filter_prefilter_accepts_matching_tool_marker_after_scan_limit() {
    let mut config = HashMap::new();
    config.insert("gpt".to_owned(), vec!["image_generation".to_owned()]);
    let tool_filter_index = test_tool_filter_index(&config);

    let body = Bytes::from(format!(
        r#"{{"model":"gpt-4","input":"{}","tools":[{{"type":"image_generation"}}]}}"#,
        "x".repeat(2048)
    ));

    assert!(request_body_may_need_tool_filter(&body, &tool_filter_index));
}

#[test]
fn tool_filter_metadata_gate_skips_unmatched_model_prefix() {
    let mut config = HashMap::new();
    config.insert("gpt".to_owned(), vec!["image_generation".to_owned()]);
    let tool_filter_index = test_tool_filter_index(&config);

    let meta: RequestMetadata<'_> =
        serde_json::from_slice(br#"{"model":"claude-3","tools":{"type":"image_generation"}}"#)
            .unwrap();

    assert!(!request_metadata_needs_tool_filter(
        &meta,
        &tool_filter_index,
        &config
    ));
}

#[test]
fn tool_filter_metadata_gate_accepts_matching_model_prefix() {
    let mut config = HashMap::new();
    config.insert("gpt".to_owned(), vec!["image_generation".to_owned()]);
    let tool_filter_index = test_tool_filter_index(&config);

    let meta: RequestMetadata<'_> =
        serde_json::from_slice(br#"{"model":"gpt-4.1","tools":[{"type":"image_generation"}]}"#)
            .unwrap();

    assert!(request_metadata_needs_tool_filter(
        &meta,
        &tool_filter_index,
        &config
    ));
}

#[test]
fn tool_filter_metadata_gate_checks_top_level_tool_types() {
    let mut config = HashMap::new();
    config.insert("gpt".to_owned(), vec!["image_generation".to_owned()]);
    let tool_filter_index = test_tool_filter_index(&config);

    let meta: RequestMetadata<'_> = serde_json::from_slice(
            br#"{"model":"gpt-4.1","messages":[{"role":"user","content":"image_generation"}],"tools":[{"type":"function","function":{"name":"lookup"}}]}"#,
        )
        .unwrap();

    assert!(!request_metadata_needs_tool_filter(
        &meta,
        &tool_filter_index,
        &config
    ));
}

#[test]
fn filter_tools_handles_invalid_json() {
    let mut config = HashMap::new();
    config.insert("gpt".to_owned(), vec!["image_generation".to_owned()]);
    let tool_filter_index = test_tool_filter_index(&config);

    let body = Bytes::from(r#"not valid json"#);
    let config_arc = Arc::new(retry_test_config());
    let api_key =
        ApiKeyInfo::from_headers_with_fallback(&HeaderMap::new(), "127.0.0.1".parse().unwrap());

    let filtered = filter_tools_by_model(
        &body,
        &config,
        &tool_filter_index,
        &config_arc,
        &api_key,
        "/v1/chat/completions",
    );

    assert!(filtered.is_none());
}

#[test]
fn filter_tools_handles_missing_model_field() {
    let mut config = HashMap::new();
    config.insert("gpt".to_owned(), vec!["image_generation".to_owned()]);
    let tool_filter_index = test_tool_filter_index(&config);

    let body = Bytes::from(r#"{"tools":[{"type":"image_generation"}]}"#);
    let config_arc = Arc::new(retry_test_config());
    let api_key =
        ApiKeyInfo::from_headers_with_fallback(&HeaderMap::new(), "127.0.0.1".parse().unwrap());

    let filtered = filter_tools_by_model(
        &body,
        &config,
        &tool_filter_index,
        &config_arc,
        &api_key,
        "/v1/chat/completions",
    );

    assert!(filtered.is_none());
}

#[test]
fn filter_tools_handles_missing_tools_field() {
    let mut config = HashMap::new();
    config.insert("gpt".to_owned(), vec!["image_generation".to_owned()]);
    let tool_filter_index = test_tool_filter_index(&config);

    let body = Bytes::from(r#"{"model":"gpt-4"}"#);
    let config_arc = Arc::new(retry_test_config());
    let api_key =
        ApiKeyInfo::from_headers_with_fallback(&HeaderMap::new(), "127.0.0.1".parse().unwrap());

    let filtered = filter_tools_by_model(
        &body,
        &config,
        &tool_filter_index,
        &config_arc,
        &api_key,
        "/v1/chat/completions",
    );

    assert!(filtered.is_none());
}
