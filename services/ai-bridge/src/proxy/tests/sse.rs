use super::*;

#[test]
fn sse_frame_cyber_policy_check_short_circuits_without_keyword() {
    // Frames without "cyber_policy" substring should short-circuit before JSON parsing
    let normal_frame =
        b"data: {\"id\":\"chatcmpl-123\",\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n";
    assert!(!sse_frame_has_cyber_policy_error(normal_frame));

    let heartbeat_frame = b": heartbeat\n\n";
    assert!(!sse_frame_has_cyber_policy_error(heartbeat_frame));

    // Frame with "cyber_policy" should be detected
    let error_frame = b"data: {\"error\":{\"code\":\"cyber_policy\",\"message\":\"abuse\"}}\n\n";
    assert!(sse_frame_has_cyber_policy_error(error_frame));
}

#[test]
fn sse_frame_fields_parse_comments_crlf_and_multiline_data() {
    let fields = sse_frame_fields(
            b": heartbeat\r\nevent: response.output_text.delta\r\ndata: {\"type\":\"response.output_text.delta\"\r\ndata: ,\"delta\":\"hi\"}\r\n\r\n",
        )
        .map(|field| (field.name, field.value))
        .collect::<Vec<_>>();

    assert_eq!(
        fields,
        vec![
            ("event", "response.output_text.delta"),
            ("data", "{\"type\":\"response.output_text.delta\""),
            ("data", ",\"delta\":\"hi\"}"),
        ]
    );
}

#[test]
fn sse_frame_progress_detects_event_name_and_done_frame() {
    assert!(sse_frame_is_stream_progress(
        b"event: response.output_text.delta\n\n"
    ));
    assert!(sse_frame_is_stream_progress(b"data: [DONE]\n\n"));
}

#[test]
fn upstream_response_error_payload_preserves_json_object() {
    let body = br#"{"error":{"message":"not enough balance","type":"new_api_error","code":"insufficient_user_quota"}}"#;

    assert_eq!(
        upstream_response_error_payload(StatusCode::BAD_GATEWAY.as_u16(), body),
        Bytes::from_static(body),
    );
}

#[test]
fn upstream_response_error_payload_wraps_non_json_body() {
    let payload = upstream_response_error_payload(
        StatusCode::BAD_GATEWAY.as_u16(),
        b"gateway connection failed",
    );
    let value: serde_json::Value = serde_json::from_slice(&payload).unwrap();

    assert_eq!(value["error"]["message"], "gateway connection failed");
    assert_eq!(value["error"]["code"], "upstream_error");
    assert_eq!(value["upstream_status"], 502);
}

#[test]
fn sse_event_buffer_waits_for_complete_event_boundaries() {
    let mut buffer = SseEventBuffer::default();

    assert!(buffer.push(b"data: {\"type\":\"response.").is_empty());
    assert!(buffer.push(b"created\"}").is_empty());

    let frames = buffer.push(b"\n\n");
    assert_eq!(frames.len(), 1);
    assert_eq!(
        frames[0].bytes,
        Bytes::from_static(b"data: {\"type\":\"response.created\"}\n\n")
    );
    assert_eq!(buffer.buffered_len(), 0);
}

#[tokio::test]
async fn stream_event_stall_timeout_is_disabled_for_playground_paths() {
    let ctx = test_proxy_request("/pg/chat/completions", Bytes::from_static(b"{}"));

    assert_eq!(stream_event_stall_timeout_for_request(&ctx), None);

    let ctx = test_proxy_request("/v1/responses", Bytes::from_static(b"{}"));
    assert_eq!(
        stream_event_stall_timeout_for_request(&ctx),
        Some(Duration::from_secs(60))
    );
}

#[tokio::test]
async fn stream_event_stall_timeout_is_disabled_for_image_generation() {
    // 测试直接图片接口：/v1/images/generations
    let mut ctx = test_proxy_request(
        "/v1/images/generations",
        Bytes::from_static(b"{\"prompt\":\"a cat\"}"),
    );
    ctx.is_image_generation = true;
    assert_eq!(stream_event_stall_timeout_for_request(&ctx), None);

    // 测试直接图片接口：/v1/images/edits
    let mut ctx = test_proxy_request(
        "/v1/images/edits",
        Bytes::from_static(b"{\"prompt\":\"a dog\"}"),
    );
    ctx.is_image_generation = true;
    assert_eq!(stream_event_stall_timeout_for_request(&ctx), None);

    // 测试 Chat 接口使用 image_generation 工具
    let body = Bytes::from_static(
            br#"{"model":"gpt-4","messages":[{"role":"user","content":"draw a cat"}],"tools":[{"type":"image_generation"}]}"#,
        );
    let mut ctx = test_proxy_request("/v1/responses", body);
    ctx.is_image_generation = true;
    assert_eq!(stream_event_stall_timeout_for_request(&ctx), None);

    // 测试普通请求仍然有超时
    let ctx = test_proxy_request(
        "/v1/chat/completions",
        Bytes::from_static(b"{\"model\":\"gpt-4\",\"messages\":[]}"),
    );
    assert_eq!(
        stream_event_stall_timeout_for_request(&ctx),
        Some(Duration::from_secs(60))
    );
}
