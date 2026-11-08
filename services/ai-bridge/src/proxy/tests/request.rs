use super::*;
use futures_util::StreamExt;

#[test]
fn builds_upstream_url_with_query() {
    let base = Url::parse("http://localhost:3000/api").unwrap();
    let uri = Uri::from_static("/v1/chat/completions?x=1");

    assert_eq!(
        build_upstream_url(&base, &uri).as_str(),
        "http://localhost:3000/api/v1/chat/completions?x=1"
    );
}

#[test]
fn upstream_request_headers_adds_client_ip_headers() {
    let headers = HeaderMap::new();
    let client_ip = "198.51.100.7".parse::<IpAddr>().unwrap();

    let forwarded = upstream_request_headers(&headers, "x-ai-bridge-", client_ip);

    assert_eq!(
        forwarded.get("x-forwarded-for").unwrap().to_str().unwrap(),
        "198.51.100.7"
    );
    assert_eq!(
        forwarded.get("x-real-ip").unwrap().to_str().unwrap(),
        "198.51.100.7"
    );
}

#[test]
fn upstream_request_headers_replaces_untrusted_forwarded_chain_with_resolved_client_ip() {
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-for", "203.0.113.8".parse().unwrap());
    headers.append("x-forwarded-for", "203.0.113.9".parse().unwrap());
    headers.insert("x-real-ip", "203.0.113.99".parse().unwrap());
    let client_ip = "198.51.100.7".parse::<IpAddr>().unwrap();

    let forwarded = upstream_request_headers(&headers, "x-ai-bridge-", client_ip);

    assert_eq!(
        forwarded.get("x-forwarded-for").unwrap().to_str().unwrap(),
        "198.51.100.7"
    );
    assert_eq!(
        forwarded.get("x-real-ip").unwrap().to_str().unwrap(),
        "198.51.100.7"
    );
}

#[test]
fn upstream_request_headers_filters_internal_and_hop_by_hop_headers() {
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer sk-test".parse().unwrap());
    headers.insert("host", "bridge.example".parse().unwrap());
    headers.insert("connection", "keep-alive".parse().unwrap());
    headers.insert("x-ai-bridge-debug", "1".parse().unwrap());
    let client_ip = "198.51.100.7".parse::<IpAddr>().unwrap();

    let forwarded = upstream_request_headers(&headers, "x-ai-bridge-", client_ip);

    assert!(forwarded.contains_key("authorization"));
    assert!(!forwarded.contains_key("host"));
    assert!(!forwarded.contains_key("connection"));
    assert!(!forwarded.contains_key("x-ai-bridge-debug"));
    assert_eq!(
        forwarded.get("x-forwarded-for").unwrap().to_str().unwrap(),
        "198.51.100.7"
    );
}

#[test]
fn upstream_request_headers_filters_connection_nominated_headers() {
    let mut headers = HeaderMap::new();
    headers.insert("connection", "x-remove, keep-alive".parse().unwrap());
    headers.insert("x-remove", "attacker-controlled".parse().unwrap());
    headers.insert("x-keep", "safe".parse().unwrap());

    let forwarded =
        upstream_request_headers(&headers, "x-ai-bridge-", "198.51.100.7".parse().unwrap());

    assert!(!forwarded.contains_key("x-remove"));
    assert_eq!(forwarded.get("x-keep").unwrap(), "safe");
}

#[test]
fn filtered_response_headers_filters_connection_nominated_headers() {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("connection", "x-remove".parse().unwrap());
    headers.insert("x-remove", "upstream-only".parse().unwrap());
    headers.insert("x-keep", "safe".parse().unwrap());

    let filtered = filtered_response_headers(&headers);

    assert!(!filtered.contains_key("connection"));
    assert!(!filtered.contains_key("x-remove"));
    assert_eq!(filtered.get("x-keep").unwrap(), "safe");
}

#[test]
fn effective_client_ip_uses_forwarded_for_from_trusted_peer() {
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-for", "203.0.113.8, 127.0.0.1".parse().unwrap());

    assert_eq!(
        effective_client_ip(&headers, "127.0.0.1".parse().unwrap()),
        "203.0.113.8".parse::<IpAddr>().unwrap()
    );
}

#[test]
fn effective_client_ip_falls_back_to_real_ip_from_trusted_peer() {
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-for", "unknown".parse().unwrap());
    headers.insert("x-real-ip", "203.0.113.9:443".parse().unwrap());

    assert_eq!(
        effective_client_ip(&headers, "127.0.0.1".parse().unwrap()),
        "203.0.113.9".parse::<IpAddr>().unwrap()
    );
}

#[test]
fn effective_client_ip_ignores_forwarded_headers_from_public_peer() {
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-for", "203.0.113.8".parse().unwrap());

    assert_eq!(
        effective_client_ip(&headers, "198.51.100.7".parse().unwrap()),
        "198.51.100.7".parse::<IpAddr>().unwrap()
    );
}

#[test]
fn stream_detection_uses_request_body_stream_flag() {
    let headers = HeaderMap::new();

    assert!(is_stream_request(
        &headers,
        &Bytes::from_static(br#"{"stream":true}"#)
    ));
    assert!(!is_stream_request(
        &headers,
        &Bytes::from_static(br#"{"stream":false}"#)
    ));
    assert!(!is_stream_request(
        &headers,
        &Bytes::from_static(br#"{"model":"demo"}"#)
    ));
}

#[test]
fn stream_detection_ignores_accept_event_stream_without_stream_flag() {
    let mut headers = HeaderMap::new();
    headers.insert(header::ACCEPT, "text/event-stream".parse().unwrap());

    assert!(!is_stream_request(
        &headers,
        &Bytes::from_static(br#"{"stream":false}"#)
    ));
    assert!(!is_stream_request(
        &headers,
        &Bytes::from_static(br#"{"model":"demo"}"#)
    ));
}

#[test]
fn stream_detection_recognizes_stream_options_as_streaming() {
    let headers = HeaderMap::new();

    assert!(is_stream_request(
        &headers,
        &Bytes::from_static(br#"{"model":"demo","stream_options":{"include_usage":true}}"#)
    ));

    assert!(is_stream_request(
        &headers,
        &Bytes::from_static(br#"{"stream":false,"stream_options":{"include_usage":true}}"#)
    ));

    assert!(is_stream_request(
        &headers,
        &Bytes::from_static(br#"{"stream":true,"stream_options":{"include_usage":true}}"#)
    ));
}

#[test]
fn large_upload_detection_only_streams_upload_like_requests() {
    let mut upload_headers = HeaderMap::new();
    upload_headers.insert(
        header::CONTENT_TYPE,
        "multipart/form-data; boundary=abc123".parse().unwrap(),
    );
    upload_headers.insert(header::CONTENT_LENGTH, "2048".parse().unwrap());
    assert!(should_stream_request_body_without_buffering(
        &Method::POST,
        "/v1/files",
        &upload_headers,
        1024
    ));

    let mut json_headers = HeaderMap::new();
    json_headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    json_headers.insert(header::CONTENT_LENGTH, "2048".parse().unwrap());
    assert!(!should_stream_request_body_without_buffering(
        &Method::POST,
        "/v1/responses",
        &json_headers,
        1024
    ));

    let mut small_upload_headers = upload_headers.clone();
    small_upload_headers.insert(header::CONTENT_LENGTH, "512".parse().unwrap());
    assert!(should_stream_request_body_without_buffering(
        &Method::POST,
        "/v1/files",
        &small_upload_headers,
        1024
    ));
}

#[test]
fn upload_path_forces_unbuffered_even_without_upload_headers() {
    let mut json_headers = HeaderMap::new();
    json_headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    json_headers.insert(header::CONTENT_LENGTH, "2048".parse().unwrap());

    assert!(should_stream_request_body_without_buffering(
        &Method::POST,
        "/v1/files",
        &json_headers,
        1024
    ));

    assert!(should_stream_request_body_without_buffering(
        &Method::POST,
        "/v1/images/edits",
        &json_headers,
        1024
    ));

    assert!(should_stream_request_body_without_buffering(
        &Method::POST,
        "/v1/audio/transcriptions",
        &json_headers,
        1024
    ));

    let mut no_length_headers = HeaderMap::new();
    no_length_headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    assert!(should_stream_request_body_without_buffering(
        &Method::POST,
        "/v1/files",
        &no_length_headers,
        1024
    ));

    assert!(!should_stream_request_body_without_buffering(
        &Method::POST,
        "/v1/responses",
        &json_headers,
        1024
    ));
}

#[tokio::test]
async fn limited_upload_body_stream_stops_after_configured_limit() {
    let body = Body::from(Bytes::from_static(b"12345678"));
    let mut stream = Box::pin(limited_upload_body_stream(body, 4));

    let result = stream.next().await.unwrap();

    assert!(matches!(
        result,
        Err(UploadBodyStreamError::LimitExceeded { limit: 4 })
    ));
    assert!(stream.next().await.is_none());
}

#[test]
fn should_block_image_model_on_wrong_path() {
    assert!(should_block_image_model_request(
        "gpt-image-2",
        "/v1/chat/completions"
    ));
    assert!(should_block_image_model_request(
        "gpt-image-1",
        "/v1/completions"
    ));
    assert!(should_block_image_model_request(
        "gpt-image",
        "/v1/embeddings"
    ));
}

#[test]
fn should_not_block_image_model_on_correct_path() {
    assert!(!should_block_image_model_request(
        "gpt-image-2",
        "/v1/images/generations"
    ));
    assert!(!should_block_image_model_request(
        "gpt-image-1",
        "/v1/images/edits"
    ));
}

#[test]
fn should_not_block_non_image_model() {
    assert!(!should_block_image_model_request(
        "gpt-4",
        "/v1/chat/completions"
    ));
    assert!(!should_block_image_model_request(
        "gpt-3.5-turbo",
        "/v1/completions"
    ));
    assert!(!should_block_image_model_request(
        "claude-3",
        "/v1/chat/completions"
    ));
}

#[test]
fn request_inspection_plan_skips_body_without_relevant_markers() {
    let config = HashMap::new();
    let tool_filter_index = test_tool_filter_index(&config);
    let body =
        Bytes::from_static(br#"{"model":"demo","messages":[{"role":"user","content":"hello"}]}"#);

    let plan = request_body_inspection_plan(
        &Method::POST,
        "/v1/chat/completions",
        &body,
        &tool_filter_index,
    );

    assert_eq!(plan, RequestBodyInspectionPlan::default());
    assert!(!plan.needs_metadata_parse());
}

#[test]
fn request_inspection_plan_combines_responses_body_markers() {
    let config = HashMap::new();
    let tool_filter_index = test_tool_filter_index(&config);
    let body = Bytes::from_static(
        br#"{"model":"demo","text":{"format":{"type":"json_schema"}},"max_tokens":16}"#,
    );

    let plan =
        request_body_inspection_plan(&Method::POST, "/v1/responses", &body, &tool_filter_index);

    assert!(plan.should_normalize_structured_output);
    assert!(plan.should_normalize_responses_compat);
    assert!(!plan.should_check_image_model);
    assert!(!plan.should_filter_tools);
    assert!(plan.needs_metadata_parse());
}

#[test]
fn request_inspection_plan_ignores_chat_format_without_response_format() {
    let config = HashMap::new();
    let tool_filter_index = test_tool_filter_index(&config);
    let body = Bytes::from_static(br#"{"model":"demo","format":{"type":"json_schema"}}"#);

    let plan = request_body_inspection_plan(
        &Method::POST,
        "/v1/chat/completions",
        &body,
        &tool_filter_index,
    );

    assert!(!plan.should_normalize_structured_output);
    assert!(!plan.needs_metadata_parse());
}

#[test]
fn request_inspection_plan_skips_image_model_check_on_image_endpoint() {
    let config = HashMap::new();
    let tool_filter_index = test_tool_filter_index(&config);
    let body = Bytes::from_static(br#"{"model":"gpt-image-1"}"#);

    let plan = request_body_inspection_plan(
        &Method::POST,
        "/v1/images/generations",
        &body,
        &tool_filter_index,
    );

    assert!(!plan.should_check_image_model);
    assert!(!plan.needs_metadata_parse());
}

#[test]
fn request_inspection_plan_preserves_tool_filter_prefilter() {
    let mut config = HashMap::new();
    config.insert("gpt".to_owned(), vec!["image_generation".to_owned()]);
    let tool_filter_index = test_tool_filter_index(&config);
    let body = Bytes::from_static(br#"{"model":"gpt-4","tools":[{"type":"image_generation"}]}"#);

    let plan = request_body_inspection_plan(
        &Method::POST,
        "/v1/chat/completions",
        &body,
        &tool_filter_index,
    );

    assert!(plan.should_filter_tools);
    assert!(plan.needs_metadata_parse());
}

#[test]
fn request_inspection_plan_accepts_tool_filter_markers_after_scan_limit() {
    let mut config = HashMap::new();
    config.insert("gpt".to_owned(), vec!["image_generation".to_owned()]);
    let tool_filter_index = test_tool_filter_index(&config);
    let body = Bytes::from(format!(
        r#"{{"model":"gpt-4","messages":[{{"role":"user","content":"{}"}}],"tools":[{{"type":"image_generation"}}]}}"#,
        "x".repeat(2048)
    ));

    let plan = request_body_inspection_plan(
        &Method::POST,
        "/v1/chat/completions",
        &body,
        &tool_filter_index,
    );

    assert!(plan.should_filter_tools);
    assert!(plan.needs_metadata_parse());
}
