#[allow(unused_imports)]
use super::request::*;
#[allow(unused_imports)]
use super::retry::*;
use super::sse::*;
#[allow(unused_imports)]
use super::tool_filter::*;
use super::*;
use axum::body::to_bytes;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    sync::mpsc,
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
};
use url::Url;

mod auth_failure;
mod cyber_policy;
mod request;
mod retry;
mod sse;
mod stream;
mod tool_filter;
mod upstream;

static TEST_TCP_BIND_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
static TEST_TEMP_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

// ─── Shared test helpers ───────────────────────────────────────────

fn retry_test_config() -> Config {
    Config {
        listen_addr: "127.0.0.1:0".parse().unwrap(),
        upstream_base_url: Url::parse("http://localhost:3000").unwrap(),
        admin_session_secret: None,
        admin_login_url: "/#/login".to_owned(),
        external_auth_cookie_secure: false,
        config_reload_interval: Some(std::time::Duration::from_secs(300)),
        auto_restart_interval: None,
        request_timeout: None,
        connect_timeout: std::time::Duration::from_secs(1),
        heartbeat_interval: std::time::Duration::from_secs(5),
        stream_event_stall_retry_enabled: true,
        stream_event_stall_timeout: Some(std::time::Duration::from_secs(60)),
        stream_event_stall_max_retries: 2,
        upstream_429_initial_delay: std::time::Duration::from_secs(2),
        upstream_429_max_delay: std::time::Duration::from_secs(30),
        upstream_429_max_retries: None,
        upstream_error_cooldown_enabled: true,
        upstream_502_cooldown_enabled: true,
        upstream_502_cooldown_accumulate_enabled: true,
        upstream_502_cooldown_delay: std::time::Duration::from_millis(1),
        upstream_502_max_retries: Some(3),
        auth_failure_cooldown_enabled: true,
        auth_failure_policy: crate::config::AuthFailurePolicy::Balanced,
        auth_failure_window: std::time::Duration::from_secs(60),
        auth_failure_threshold: 5,
        auth_failure_cooldown: std::time::Duration::from_secs(1800),
        auth_failure_ip_threshold: 20,
        auth_failure_max_entries: 10_000,
        auth_failure_error_body_max_bytes: 8 * 1024,
        rate_limiter_idle_ttl: std::time::Duration::from_secs(600),
        rate_limiter_cleanup_interval: std::time::Duration::from_secs(60),
        rate_limiter_max_entries: 10_000,
        max_body_bytes: 1024,
        max_upload_body_bytes: 8 * 1024,
        log_level: "info".to_owned(),
        log_format: crate::config::LogFormat::Json,
        log_file: None,
        bad_gateway_log_dir: std::env::temp_dir().join("ai-bridge-test-502-log"),
        service_unavailable_log_dir: std::env::temp_dir().join("ai-bridge-test-503-log"),
        forensic_queue_capacity: 256,
        forensic_max_body_bytes: 256 * 1024,
        forensic_max_records_per_minute: 120,
        forensic_include_api_key: false,
        internal_header_prefix: "x-ai-bridge-".to_owned(),
        image_response_mode: crate::config::ImageResponseMode::Auto,
        debug_output: false,
        cyber_policy_blacklist_file: unique_temp_dir("ai-bridge-test-blacklist")
            .with_extension("txt"),
        cyber_policy_black_log_file: unique_temp_dir("ai-bridge-test-black-log")
            .with_extension("jsonl"),
        cyber_policy_ip_blacklist_file: unique_temp_dir("ai-bridge-test-black-ip")
            .with_extension("txt"),
        cyber_policy_whitelist_file: unique_temp_dir("ai-bridge-test-whitelist")
            .with_extension("txt"),
        cyber_policy_white_log_file: unique_temp_dir("ai-bridge-test-white-log")
            .with_extension("jsonl"),
        cyber_policy_ban_enabled: true,
        cyber_policy_confirmation_rules_file: None,
        cyber_policy_confirmation_rules: CyberPolicyConfirmationRuleConfig::default_rules(),
        responses_nonstream_chat_fallback: false,
        stream_early_heartbeat: false,
        stream_synthetic_first_space: false,
        filter_tools_by_model: HashMap::new(),
        tool_filter_scan_limit: 1024,
    }
}

struct CapturedRequest {
    head: String,
    body: Bytes,
}

fn test_state(upstream_base_url: Url) -> Arc<AppState> {
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    Arc::new(AppState::new(config).unwrap())
}

fn test_state_with_responses_chat_fallback(upstream_base_url: Url) -> Arc<AppState> {
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.responses_nonstream_chat_fallback = true;
    Arc::new(AppState::new(config).unwrap())
}

fn test_state_with_fast_retry(upstream_base_url: Url) -> Arc<AppState> {
    let mut config = retry_test_config();
    config.upstream_base_url = upstream_base_url;
    config.upstream_429_initial_delay = std::time::Duration::from_millis(1);
    config.upstream_429_max_delay = std::time::Duration::from_millis(1);
    config.upstream_429_max_retries = Some(1);
    Arc::new(AppState::new(config).unwrap())
}

fn test_proxy_request_with_state(
    path: &'static str,
    body: Bytes,
    state: &AppState,
) -> ProxyRequest {
    state.bind_runtime_snapshot(test_proxy_request(path, body))
}

fn test_proxy_request(path: &'static str, body: Bytes) -> ProxyRequest {
    let config = Arc::new(retry_test_config());
    let client = Client::builder()
        .connect_timeout(config.connect_timeout)
        .build()
        .unwrap();
    test_proxy_request_with_runtime(path, body, config, client)
}

fn test_proxy_request_with_runtime(
    path: &'static str,
    body: Bytes,
    config: Arc<Config>,
    client: Client,
) -> ProxyRequest {
    let forensic_writer = ForensicWriter::new(
        config.cyber_policy_black_log_file.clone(),
        config.cyber_policy_white_log_file.clone(),
    )
    .unwrap();
    test_proxy_request_with_runtime_and_writer(path, body, config, client, forensic_writer)
}

fn test_proxy_request_with_runtime_and_writer(
    path: &'static str,
    body: Bytes,
    config: Arc<Config>,
    client: Client,
    forensic_writer: Arc<ForensicWriter>,
) -> ProxyRequest {
    let cyber_policy_confirmation_rules =
        Arc::new(CyberPolicyConfirmationRules::new(&config).unwrap());
    let blacklist_cache = Arc::new(
        BlacklistCache::load(
            config.cyber_policy_blacklist_file.clone(),
            config.cyber_policy_ip_blacklist_file.clone(),
        )
        .unwrap(),
    );
    let whitelist_cache =
        Arc::new(WhitelistCache::load(&config.cyber_policy_whitelist_file).unwrap());
    let mut headers = HeaderMap::new();
    headers.insert(header::AUTHORIZATION, "Bearer sk-test".parse().unwrap());
    headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    let api_key = ApiKeyInfo::from_headers(&headers);

    ProxyRequest {
        config,
        client,
        method: Method::POST,
        uri: Uri::from_static(path),
        headers,
        original_body: body.clone(),
        body,
        client_ip: "198.51.100.7".parse().unwrap(),
        api_key,
        path: path.to_owned(),
        prompt_summary: PromptSummary::default(),
        cyber_policy_confirmation_rules,
        blacklist_cache,
        whitelist_cache,
        forensic_writer,
        started_at: Instant::now(),
        queue_started_at: None,
        queue_wait: Duration::ZERO,
        is_image_generation: false,
    }
}

fn default_test_cyber_policy_confirmation_rules() -> CyberPolicyConfirmationRules {
    CyberPolicyConfirmationRules::from_config(&CyberPolicyConfirmationRuleConfig::default_rules())
        .unwrap()
}

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let counter = TEST_TEMP_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        counter
    ))
}

fn spawn_single_response_server(
    content_type: &'static str,
    response_body: Bytes,
) -> (Url, mpsc::Receiver<CapturedRequest>, thread::JoinHandle<()>) {
    spawn_single_status_response_server(StatusCode::OK, content_type, response_body)
}

fn spawn_single_status_response_server(
    status: StatusCode,
    content_type: &'static str,
    response_body: Bytes,
) -> (Url, mpsc::Receiver<CapturedRequest>, thread::JoinHandle<()>) {
    let _bind_guard = TEST_TCP_BIND_LOCK.lock().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        tx.send(read_captured_request(&mut stream)).unwrap();
        write_test_response(
            &mut stream,
            &TestUpstreamResponse::new(status, content_type, response_body),
        );
    });

    (Url::parse(&format!("http://{addr}")).unwrap(), rx, server)
}

struct TestUpstreamResponse {
    status: StatusCode,
    content_type: &'static str,
    body: Bytes,
}

impl TestUpstreamResponse {
    fn new(status: StatusCode, content_type: &'static str, body: Bytes) -> Self {
        Self {
            status,
            content_type,
            body,
        }
    }
}

fn spawn_response_sequence_server(
    responses: Vec<TestUpstreamResponse>,
) -> (Url, mpsc::Receiver<CapturedRequest>, thread::JoinHandle<()>) {
    let _bind_guard = TEST_TCP_BIND_LOCK.lock().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    let server = thread::spawn(move || {
        for response in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let captured = read_captured_request(&mut stream);
            tx.send(captured).unwrap();
            write_test_response(&mut stream, &response);
        }
    });

    (Url::parse(&format!("http://{addr}")).unwrap(), rx, server)
}

fn spawn_delayed_streaming_response_server(
    content_type: &'static str,
    chunks: Vec<(Bytes, Duration)>,
    final_delay: Duration,
) -> (Url, mpsc::Receiver<CapturedRequest>, thread::JoinHandle<()>) {
    spawn_delayed_streaming_response_head_server(content_type, Duration::ZERO, chunks, final_delay)
}

fn spawn_delayed_streaming_response_head_server(
    content_type: &'static str,
    head_delay: Duration,
    chunks: Vec<(Bytes, Duration)>,
    final_delay: Duration,
) -> (Url, mpsc::Receiver<CapturedRequest>, thread::JoinHandle<()>) {
    let _bind_guard = TEST_TCP_BIND_LOCK.lock().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        tx.send(read_captured_request(&mut stream)).unwrap();
        write_delayed_streaming_response(
            &mut stream,
            DelayedStreamingResponse::new_with_head_delay(
                content_type,
                head_delay,
                chunks,
                final_delay,
            ),
        );
    });

    (Url::parse(&format!("http://{addr}")).unwrap(), rx, server)
}

fn spawn_disconnect_tolerant_delayed_streaming_response_server(
    content_type: &'static str,
    chunks: Vec<(Bytes, Duration)>,
    final_delay: Duration,
) -> (Url, mpsc::Receiver<CapturedRequest>, thread::JoinHandle<()>) {
    let _bind_guard = TEST_TCP_BIND_LOCK.lock().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        tx.send(read_captured_request(&mut stream)).unwrap();
        write_delayed_streaming_response_allow_disconnect(
            &mut stream,
            DelayedStreamingResponse::new(content_type, chunks, final_delay),
        );
    });

    (Url::parse(&format!("http://{addr}")).unwrap(), rx, server)
}

struct DelayedStreamingResponse {
    content_type: &'static str,
    head_delay: Duration,
    chunks: Vec<(Bytes, Duration)>,
    final_delay: Duration,
}

impl DelayedStreamingResponse {
    fn new(
        content_type: &'static str,
        chunks: Vec<(Bytes, Duration)>,
        final_delay: Duration,
    ) -> Self {
        Self::new_with_head_delay(content_type, Duration::ZERO, chunks, final_delay)
    }

    fn new_with_head_delay(
        content_type: &'static str,
        head_delay: Duration,
        chunks: Vec<(Bytes, Duration)>,
        final_delay: Duration,
    ) -> Self {
        Self {
            content_type,
            head_delay,
            chunks,
            final_delay,
        }
    }
}

fn spawn_delayed_streaming_response_sequence_server(
    responses: Vec<DelayedStreamingResponse>,
) -> (Url, mpsc::Receiver<CapturedRequest>, thread::JoinHandle<()>) {
    let _bind_guard = TEST_TCP_BIND_LOCK.lock().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    let server = thread::spawn(move || {
        for response in responses {
            let (mut stream, _) = listener.accept().unwrap();
            tx.send(read_captured_request(&mut stream)).unwrap();
            write_delayed_streaming_response(&mut stream, response);
        }
    });

    (Url::parse(&format!("http://{addr}")).unwrap(), rx, server)
}

fn read_captured_request(stream: &mut std::net::TcpStream) -> CapturedRequest {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut head = String::new();

    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
        head.push_str(&line);
    }

    let content_length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    let mut body = vec![0_u8; content_length];
    reader.read_exact(&mut body).unwrap();

    CapturedRequest {
        head,
        body: Bytes::from(body),
    }
}

fn write_test_response(stream: &mut std::net::TcpStream, response: &TestUpstreamResponse) {
    let response_head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n",
        response.status.as_u16(),
        response.status.canonical_reason().unwrap_or("Unknown"),
        response.content_type,
        response.body.len()
    );
    stream.write_all(response_head.as_bytes()).unwrap();
    stream.write_all(&response.body).unwrap();
    stream.flush().unwrap();
}

fn write_delayed_streaming_response(
    stream: &mut std::net::TcpStream,
    response: DelayedStreamingResponse,
) {
    if !response.head_delay.is_zero() {
        thread::sleep(response.head_delay);
    }

    let response_head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nTransfer-Encoding: chunked\r\n\r\n",
        response.content_type
    );
    stream.write_all(response_head.as_bytes()).unwrap();
    stream.flush().unwrap();

    for (chunk, delay) in response.chunks {
        if !delay.is_zero() {
            thread::sleep(delay);
        }
        write_chunked_body_chunk(stream, &chunk);
    }

    if !response.final_delay.is_zero() {
        thread::sleep(response.final_delay);
    }
    stream.write_all(b"0\r\n\r\n").unwrap();
    stream.flush().unwrap();
}

fn write_delayed_streaming_response_allow_disconnect(
    stream: &mut std::net::TcpStream,
    response: DelayedStreamingResponse,
) {
    if !response.head_delay.is_zero() {
        thread::sleep(response.head_delay);
    }

    let response_head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nTransfer-Encoding: chunked\r\n\r\n",
        response.content_type
    );
    if stream.write_all(response_head.as_bytes()).is_err() || stream.flush().is_err() {
        return;
    }

    for (chunk, delay) in response.chunks {
        if !delay.is_zero() {
            thread::sleep(delay);
        }
        if write_chunked_body_chunk_result(stream, &chunk).is_err() {
            return;
        }
    }

    if !response.final_delay.is_zero() {
        thread::sleep(response.final_delay);
    }
    let _ = stream.write_all(b"0\r\n\r\n");
    let _ = stream.flush();
}

fn write_chunked_body_chunk(stream: &mut std::net::TcpStream, chunk: &[u8]) {
    let head = format!("{:x}\r\n", chunk.len());
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(chunk).unwrap();
    stream.write_all(b"\r\n").unwrap();
    stream.flush().unwrap();
}

fn write_chunked_body_chunk_result(
    stream: &mut std::net::TcpStream,
    chunk: &[u8],
) -> std::io::Result<()> {
    let head = format!("{:x}\r\n", chunk.len());
    stream.write_all(head.as_bytes())?;
    stream.write_all(chunk)?;
    stream.write_all(b"\r\n")?;
    stream.flush()
}

fn temp_test_log_dir(name: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("ai-bridge-{name}-{unique}"))
}

fn read_first_forensic_log_json(log_dir: &std::path::Path) -> Value {
    let mut entries = std::fs::read_dir(log_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    entries.sort();
    let content = std::fs::read_to_string(entries.first().unwrap()).unwrap();
    serde_json::from_str(content.lines().next().unwrap()).unwrap()
}

fn test_tool_filter_index(config: &HashMap<String, Vec<String>>) -> ToolFilterIndex {
    ToolFilterIndex::new(config, 1024).unwrap()
}
