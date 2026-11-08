use std::{
    net::{IpAddr, SocketAddr},
    sync::{Arc, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use aho_corasick::AhoCorasick;
use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use axum::{
    body::{Body, to_bytes},
    extract::{ConnectInfo, State},
    http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode, Uri, header},
};
use bytes::Bytes;
use regex::RegexSet;
use reqwest::Client;
use serde_json::{Value, json};
use tracing::{error, warn};

use crate::{
    auth::{ApiKeyInfo, BlacklistCache, WhitelistCache},
    auth_failure::AuthFailureTracker,
    config::{Config, CyberPolicyConfirmationRuleConfig},
    forensic_writer::ForensicWriter,
    logger::{self, PromptSummary},
    observability::Observability,
    rate_limit::{RateLimitPermit, RateLimiter},
    responses_chat, sse_image, structured_outputs,
};

mod request;
mod retry;
mod sse;
mod stream;
mod tool_filter;
mod types;

use request::*;
pub(crate) use request::{
    cyber_policy_json_error_payload, cyber_policy_sse_error_event,
    stream_event_stall_timeout_for_request,
};
use retry::*;
pub(crate) use retry::{
    BufferedUpstreamResponse, RetryPlan, UpstreamRetryDecision, body_has_cyber_policy_error,
    classify_upstream_response_for_retry, retry_plan_for_upstream_request_error,
};
pub(crate) use sse::{
    CyberPolicyJsonInspector, done_bytes, heartbeat_bytes, sse_body_has_cyber_policy_error,
    sse_data_bytes, sse_event_bytes, sse_response, stream_event_stall_max_attempts,
    upstream_error_event,
};
use stream::*;
pub(crate) use stream::{
    process_sse_stream_with_stall_retry, record_auth_failure_on_stream_completion,
};
use tool_filter::*;
use types::*;

pub(crate) const CYBER_POLICY_CODE: &str = "cyber_policy";
const CYBER_POLICY_BLOCK_MESSAGE: &str = "网络滥用拦截：上游返回 cyber_policy，请检查请求内容。";
const CYBER_POLICY_IP_BLACKLIST_THRESHOLD: usize = 3;

const REQUEST_BODY_GPT_IMAGE_PATTERN: usize = 0;
const REQUEST_BODY_RESPONSE_FORMAT_PATTERN: usize = 1;
const REQUEST_BODY_FORMAT_PATTERN: usize = 2;
const REQUEST_BODY_MAX_TOKENS_PATTERN: usize = 3;

static REQUEST_BODY_MARKER_MATCHER: OnceLock<AhoCorasick> = OnceLock::new();

pub struct AppState {
    runtime: ArcSwap<RuntimeState>,
    started_at: Instant,
    pub rate_limiter: Arc<RateLimiter>,
    pub auth_failure_tracker: Arc<AuthFailureTracker>,
    pub blacklist_cache: Arc<BlacklistCache>,
    pub whitelist_cache: Arc<WhitelistCache>,
    pub forensic_writer: Arc<ForensicWriter>,
    pub(crate) observability: Arc<Observability>,
}

// Note: AppState does not implement Clone because:
// 1. It is always used as Arc<AppState> in production and tests
// 2. ArcSwap cannot be meaningfully cloned (would create independent snapshots)
// 3. Arc<T> implements Clone even when T doesn't

struct RuntimeState {
    config: Arc<Config>,
    client: Client,
    cyber_policy_confirmation_rules: Arc<CyberPolicyConfirmationRules>,
    tool_filter_index: Arc<ToolFilterIndex>,
}

struct RuntimeSnapshot {
    config: Arc<Config>,
    client: Client,
    cyber_policy_confirmation_rules: Arc<CyberPolicyConfirmationRules>,
    tool_filter_index: Arc<ToolFilterIndex>,
}

#[derive(Clone)]
pub struct ProxyRequest {
    pub config: Arc<Config>,
    client: Client,
    pub method: Method,
    pub uri: Uri,
    pub headers: HeaderMap,
    pub original_body: Bytes,
    pub body: Bytes,
    pub client_ip: IpAddr,
    pub api_key: ApiKeyInfo,
    pub path: String,
    pub prompt_summary: PromptSummary,
    pub cyber_policy_confirmation_rules: Arc<CyberPolicyConfirmationRules>,
    pub blacklist_cache: Arc<BlacklistCache>,
    pub whitelist_cache: Arc<WhitelistCache>,
    pub forensic_writer: Arc<ForensicWriter>,
    pub started_at: Instant,
    pub queue_started_at: Option<Instant>,
    pub queue_wait: Duration,
    pub is_image_generation: bool,
}

#[derive(Debug)]
pub struct CyberPolicyConfirmationRules {
    keyword_matcher: Option<AhoCorasick>,
    regex_set: RegexSet,
}

impl CyberPolicyConfirmationRules {
    fn new(config: &Config) -> Result<Self> {
        Self::from_config(&config.cyber_policy_confirmation_rules)
    }

    fn from_config(config: &CyberPolicyConfirmationRuleConfig) -> Result<Self> {
        // Build case-insensitive keyword matcher (None if no keywords)
        let keyword_matcher = if config.keywords.is_empty() {
            None
        } else {
            Some(
                AhoCorasick::builder()
                    .ascii_case_insensitive(true)
                    .build(&config.keywords)
                    .context("failed to build keyword matcher")?,
            )
        };

        // Build regex set
        let regex_set = if config.regex.is_empty() {
            RegexSet::empty()
        } else {
            RegexSet::new(&config.regex).with_context(|| {
                format!(
                    "invalid cyber-policy confirmation regex patterns: {:?}",
                    config.regex
                )
            })?
        };

        Ok(Self {
            keyword_matcher,
            regex_set,
        })
    }

    fn is_match(&self, text: &str) -> bool {
        // Fast path: check keywords with aho-corasick (no allocation, case-insensitive)
        if let Some(matcher) = &self.keyword_matcher
            && matcher.is_match(text)
        {
            return true;
        }
        // Check all regex patterns in one pass
        self.regex_set.is_match(text)
    }
}

impl AppState {
    #[cfg(test)]
    pub fn new(config: Config) -> Result<Self> {
        Self::new_with_observability(config, Arc::new(Observability::new()))
    }

    pub(crate) fn new_with_observability(
        config: Config,
        observability: Arc<Observability>,
    ) -> Result<Self> {
        let rate_limiter_idle_ttl = config.rate_limiter_idle_ttl;
        let rate_limiter_cleanup_interval = config.rate_limiter_cleanup_interval;
        let rate_limiter_max_entries = config.rate_limiter_max_entries;
        let blacklist_cache = Arc::new(
            BlacklistCache::load(
                config.cyber_policy_blacklist_file.clone(),
                config.cyber_policy_ip_blacklist_file.clone(),
            )
            .context("failed to load cyber-policy blacklist into memory")?,
        );
        let whitelist_cache = Arc::new(
            WhitelistCache::load(&config.cyber_policy_whitelist_file)
                .context("failed to load cyber-policy whitelist into memory")?,
        );
        let forensic_writer = ForensicWriter::with_capacity(
            config.cyber_policy_black_log_file.clone(),
            config.cyber_policy_white_log_file.clone(),
            config.forensic_queue_capacity,
        )
        .context("failed to start forensic writer")?;
        let runtime = RuntimeState::new(config)?;

        Ok(Self {
            runtime: ArcSwap::from_pointee(runtime),
            started_at: Instant::now(),
            rate_limiter: Arc::new(RateLimiter::with_max_entries(
                rate_limiter_idle_ttl,
                rate_limiter_cleanup_interval,
                rate_limiter_max_entries,
            )),
            auth_failure_tracker: Arc::new(AuthFailureTracker::new()),
            blacklist_cache,
            whitelist_cache,
            forensic_writer,
            observability,
        })
    }

    fn runtime_snapshot(&self) -> RuntimeSnapshot {
        let runtime = self.runtime.load();
        RuntimeSnapshot {
            config: runtime.config.clone(),
            client: runtime.client.clone(),
            cyber_policy_confirmation_rules: runtime.cyber_policy_confirmation_rules.clone(),
            tool_filter_index: runtime.tool_filter_index.clone(),
        }
    }

    pub(crate) fn config_snapshot(&self) -> Arc<Config> {
        self.runtime.load().config.clone()
    }

    pub(crate) fn uptime(&self) -> Duration {
        self.started_at.elapsed()
    }

    pub(crate) fn bind_runtime_snapshot(&self, mut ctx: ProxyRequest) -> ProxyRequest {
        let runtime = self.runtime_snapshot();
        ctx.config = runtime.config;
        ctx.client = runtime.client;
        ctx.cyber_policy_confirmation_rules = runtime.cyber_policy_confirmation_rules;
        ctx.blacklist_cache = self.blacklist_cache.clone();
        ctx.whitelist_cache = self.whitelist_cache.clone();
        ctx.forensic_writer = self.forensic_writer.clone();
        ctx
    }

    pub fn reload_runtime(&self, config: Config) -> Result<()> {
        let runtime = RuntimeState::new(config.clone())?;
        let blacklist_update = self
            .blacklist_cache
            .prepare_sync_from_disk(
                &config.cyber_policy_blacklist_file,
                &config.cyber_policy_ip_blacklist_file,
            )
            .context("failed to prepare blacklist cache sync from disk")?;
        let whitelist_update = self
            .whitelist_cache
            .prepare_sync_from_disk(&config.cyber_policy_whitelist_file)
            .context("failed to prepare whitelist cache sync from disk")?;

        self.blacklist_cache.apply_sync_from_disk(blacklist_update);
        self.whitelist_cache.apply_sync_from_disk(whitelist_update);
        self.runtime.store(Arc::new(runtime));
        Ok(())
    }
}

impl RuntimeState {
    fn new(config: Config) -> Result<Self> {
        // 启用 HTTP/2 多路复用以减少连接开销
        // HTTP/2 允许在单个 TCP 连接上并发多个请求，减少 20-40% 的连接建立开销
        // reqwest 0.12 默认支持 HTTP/2，通过 http2 feature 启用
        let mut client_builder = Client::builder()
            .connect_timeout(config.connect_timeout)
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .tcp_keepalive(std::time::Duration::from_secs(60));

        if let Some(timeout) = config.request_timeout {
            client_builder = client_builder.timeout(timeout);
        }

        let client = client_builder
            .build()
            .context("failed to build reqwest client")?;
        let cyber_policy_confirmation_rules = Arc::new(CyberPolicyConfirmationRules::new(&config)?);

        let tool_filter_index = Arc::new(ToolFilterIndex::new(
            &config.filter_tools_by_model,
            config.tool_filter_scan_limit,
        )?);

        Ok(Self {
            config: Arc::new(config),
            client,
            cyber_policy_confirmation_rules,
            tool_filter_index,
        })
    }
}

impl ProxyRequest {
    fn method_str(&self) -> &str {
        self.method.as_str()
    }

    pub(crate) fn start_queue_wait(&mut self) {
        self.queue_started_at = Some(Instant::now());
        self.queue_wait = Duration::ZERO;
    }

    pub(crate) fn finish_queue_wait(&mut self) {
        if let Some(queue_started_at) = self.queue_started_at.take() {
            self.queue_wait = queue_started_at.elapsed();
        }
    }

    fn with_queue_wait(mut self, queue_wait: Duration) -> Self {
        self.queue_started_at = None;
        self.queue_wait = queue_wait;
        self
    }
}

pub(crate) fn log_proxy_request(
    ctx: &ProxyRequest,
    status: u16,
    queued: bool,
    upstream_status: Option<u16>,
) {
    log_proxy_request_with_queue_wait(ctx, status, queued, upstream_status, ctx.queue_wait);
}

pub(crate) fn log_proxy_request_with_queue_wait(
    ctx: &ProxyRequest,
    status: u16,
    queued: bool,
    upstream_status: Option<u16>,
    queue_wait: Duration,
) {
    logger::log_request(logger::RequestLog {
        api_key: &ctx.api_key,
        method: ctx.method_str(),
        path: &ctx.path,
        status,
        latency: ctx.started_at.elapsed(),
        queue_wait,
        prompt_summary: &ctx.prompt_summary,
        queued,
        upstream_status,
    });
}

pub(crate) fn record_upstream_auth_failure(
    state: &AppState,
    ctx: &ProxyRequest,
    upstream_status: u16,
    upstream_headers: &HeaderMap,
    upstream_body: &[u8],
) {
    let outcome = state.auth_failure_tracker.record_upstream_response(
        &ctx.api_key,
        ctx.client_ip,
        upstream_status,
        upstream_headers,
        upstream_body,
        &ctx.config,
    );
    if !outcome.key_cooldown_opened && !outcome.ip_cooldown_opened {
        return;
    }

    warn!(
        event = "auth_failure_cooldown_opened",
        api_key_masked = ctx.api_key.masked(),
        api_key_hash = ctx.api_key.hash(),
        client_ip = %ctx.client_ip,
        method = ctx.method_str(),
        path = %ctx.path,
        upstream_status,
        key_cooldown_opened = outcome.key_cooldown_opened,
        ip_cooldown_opened = outcome.ip_cooldown_opened,
        cooldown_secs = ctx.config.auth_failure_cooldown.as_secs(),
        "repeated upstream authentication/authorization failures opened a local cooldown"
    );
}

pub(crate) fn record_upstream_error_cooldown(
    state: &AppState,
    ctx: &ProxyRequest,
    delay: Duration,
    reason: &'static str,
) {
    if !ctx.config.upstream_error_cooldown_enabled {
        return;
    }

    state
        .rate_limiter
        .record_cooldown(ctx.api_key.identity(), delay, reason);
}

pub(crate) fn record_retry_cooldown(state: &AppState, ctx: &ProxyRequest, retry_plan: &RetryPlan) {
    if retry_plan.reason == RETRY_REASON_BAD_GATEWAY {
        if ctx.whitelist_cache.api_key_is_whitelisted(&ctx.api_key) {
            return;
        }

        if ctx.config.upstream_502_cooldown_accumulate_enabled {
            state.rate_limiter.append_cooldown(
                ctx.api_key.identity(),
                retry_plan.delay,
                retry_plan.reason,
            );
        } else {
            state.rate_limiter.record_cooldown(
                ctx.api_key.identity(),
                retry_plan.delay,
                retry_plan.reason,
            );
        }
        return;
    }

    record_upstream_error_cooldown(state, ctx, retry_plan.delay, retry_plan.reason);
}

async fn acquire_permit_after_retry_delay(
    state: &AppState,
    ctx: &ProxyRequest,
    retry_reason: &str,
) -> RateLimitPermit {
    if retry_reason == RETRY_REASON_BAD_GATEWAY
        && ctx.whitelist_cache.api_key_is_whitelisted(&ctx.api_key)
    {
        return state.rate_limiter.force_acquire(ctx.api_key.identity());
    }

    state.rate_limiter.acquire(ctx.api_key.identity()).await
}

pub(crate) struct BadGatewayRetryLog<'a> {
    pub condition: &'static str,
    pub upstream_headers: Option<&'a HeaderMap>,
    pub upstream_body: Option<&'a [u8]>,
    pub upstream_error: Option<&'a str>,
}

pub(crate) fn log_bad_gateway_retry_request(
    config: &Config,
    ctx: &ProxyRequest,
    retry_plan: &RetryPlan,
    retry_attempt: usize,
    retry_log: BadGatewayRetryLog<'_>,
) {
    if retry_plan.reason != RETRY_REASON_BAD_GATEWAY {
        return;
    }

    log_bad_gateway_trigger_request(
        config,
        ctx,
        logger::BadGatewayTriggerLog {
            condition: retry_log.condition,
            upstream_status: Some(retry_plan.upstream_status),
            upstream_headers: retry_log.upstream_headers,
            upstream_body: retry_log.upstream_body,
            upstream_error: retry_log.upstream_error,
            retry_reason: Some(retry_plan.reason),
            retry_attempt: Some(retry_attempt),
            retry_delay: Some(retry_plan.delay),
            retry_after: retry_plan.retry_after.as_deref(),
        },
    );
}

pub(crate) fn log_bad_gateway_final_request(
    config: &Config,
    ctx: &ProxyRequest,
    condition: &'static str,
    upstream_status: Option<u16>,
    upstream_headers: Option<&HeaderMap>,
    upstream_body: Option<&[u8]>,
    upstream_error: Option<&str>,
) {
    log_bad_gateway_trigger_request(
        config,
        ctx,
        logger::BadGatewayTriggerLog {
            condition,
            upstream_status,
            upstream_headers,
            upstream_body,
            upstream_error,
            retry_reason: None,
            retry_attempt: None,
            retry_delay: None,
            retry_after: None,
        },
    );
}

fn log_bad_gateway_trigger_request(
    config: &Config,
    ctx: &ProxyRequest,
    trigger: logger::BadGatewayTriggerLog<'_>,
) {
    ctx.forensic_writer
        .enqueue_bad_gateway(&logger::BadGatewayRequestLog {
            config,
            api_key: &ctx.api_key,
            method: ctx.method_str(),
            uri: &ctx.uri,
            path: &ctx.path,
            headers: &ctx.headers,
            original_body: &ctx.original_body,
            body: &ctx.body,
            client_ip: ctx.client_ip,
            prompt_summary: &ctx.prompt_summary,
            queue_wait: ctx.queue_wait,
            trigger,
        });
}

fn blacklisted_api_key_should_be_blocked(
    config: &Config,
    cache: &BlacklistCache,
    api_key: &ApiKeyInfo,
    method: &str,
    path: &str,
) -> bool {
    if cache.api_key_is_blacklisted(api_key) {
        warn!(
            event = "cyber_policy_key_blocked",
            api_key_masked = api_key.masked(),
            api_key_hash = api_key.hash(),
            api_key_present = api_key.is_present(),
            method,
            path,
            blacklist_file = %config.cyber_policy_blacklist_file.display(),
            "API key is already listed in the cyber-policy blacklist"
        );
        return true;
    }
    false
}

fn blacklisted_client_ip_should_be_blocked(
    config: &Config,
    cache: &BlacklistCache,
    client_ip: IpAddr,
    method: &str,
    path: &str,
) -> bool {
    if cache.client_ip_is_blacklisted(client_ip) {
        warn!(
            event = "cyber_policy_ip_blocked",
            client_ip = %client_ip,
            method,
            path,
            blacklist_file = %config.cyber_policy_ip_blacklist_file.display(),
            "client IP is already listed in the cyber-policy IP blacklist"
        );
        return true;
    }
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CyberPolicyAction {
    /// Return the generic cyber-policy block response. Permanent blacklist
    /// side effects are only written when local confirmation succeeds.
    Block,
    /// Log the whitelisted key trigger but do not blacklist; pass through the
    /// upstream cyber-policy response for monitoring/debugging.
    WhitelistLog,
    /// Cyber-policy banning is disabled; pass through the upstream response and
    /// skip permanent blacklist side effects.
    PassThrough,
}

pub(crate) fn block_cyber_policy_key(ctx: &ProxyRequest) -> CyberPolicyAction {
    if !ctx.config.cyber_policy_ban_enabled {
        warn!(
            event = "cyber_policy_ban_disabled",
            api_key_masked = ctx.api_key.masked(),
            api_key_hash = ctx.api_key.hash(),
            api_key_present = ctx.api_key.is_present(),
            method = ctx.method_str(),
            path = %ctx.path,
            client_ip = %ctx.client_ip,
            "upstream returned cyber_policy but cyber-policy banning is disabled; passing through"
        );
        return CyberPolicyAction::PassThrough;
    }

    if ctx.whitelist_cache.api_key_is_whitelisted(&ctx.api_key) {
        record_cyber_policy_white_log(ctx);
        warn!(
            event = "cyber_policy_whitelisted_key_triggered",
            api_key_masked = ctx.api_key.masked(),
            api_key_hash = ctx.api_key.hash(),
            api_key_present = ctx.api_key.is_present(),
            method = ctx.method_str(),
            path = %ctx.path,
            client_ip = %ctx.client_ip,
            "upstream returned cyber_policy for whitelisted key; logged but not blacklisted"
        );
        return CyberPolicyAction::WhitelistLog;
    }

    if !cyber_policy_request_confirms_violation(ctx) {
        warn!(
            event = "cyber_policy_confirmation_failed",
            api_key_masked = ctx.api_key.masked(),
            api_key_hash = ctx.api_key.hash(),
            api_key_present = ctx.api_key.is_present(),
            method = ctx.method_str(),
            path = %ctx.path,
            client_ip = %ctx.client_ip,
            "upstream returned cyber_policy but request content did not match local violation confirmation; skipping permanent blacklist"
        );
        return CyberPolicyAction::Block;
    }

    match ctx
        .whitelist_cache
        .api_key_is_whitelisted_in_file(&ctx.config.cyber_policy_whitelist_file, &ctx.api_key)
    {
        Ok(true) => {
            record_cyber_policy_white_log(ctx);
            warn!(
                event = "cyber_policy_whitelist_file_recheck_hit",
                api_key_masked = ctx.api_key.masked(),
                api_key_hash = ctx.api_key.hash(),
                api_key_present = ctx.api_key.is_present(),
                method = ctx.method_str(),
                path = %ctx.path,
                client_ip = %ctx.client_ip,
                whitelist_file = %ctx.config.cyber_policy_whitelist_file.display(),
                "upstream returned cyber_policy for key present in whitelist file; logged but not blacklisted"
            );
            return CyberPolicyAction::WhitelistLog;
        }
        Ok(false) => {}
        Err(error) => {
            error!(
                event = "cyber_policy_whitelist_file_recheck_error",
                api_key_masked = ctx.api_key.masked(),
                api_key_hash = ctx.api_key.hash(),
                api_key_present = ctx.api_key.is_present(),
                method = ctx.method_str(),
                path = %ctx.path,
                client_ip = %ctx.client_ip,
                whitelist_file = %ctx.config.cyber_policy_whitelist_file.display(),
                error = %error,
                "failed to recheck cyber-policy whitelist file; skipping permanent blacklist"
            );
            return CyberPolicyAction::Block;
        }
    }

    record_cyber_policy_black_log_and_maybe_blacklist_ip(ctx);

    match ctx.blacklist_cache.append_api_key(&ctx.api_key) {
        Ok(Some(inserted)) => {
            warn!(
                event = "cyber_policy_key_blacklisted",
                api_key_masked = ctx.api_key.masked(),
                api_key_hash = ctx.api_key.hash(),
                api_key_present = ctx.api_key.is_present(),
                method = ctx.method_str(),
                path = %ctx.path,
                blacklist_file = %ctx.config.cyber_policy_blacklist_file.display(),
                inserted,
                "upstream returned cyber_policy; API key is permanently blacklisted"
            );
        }
        Ok(None) => {
            warn!(
                event = "cyber_policy_key_blacklist_skipped",
                api_key_masked = ctx.api_key.masked(),
                api_key_hash = ctx.api_key.hash(),
                api_key_present = ctx.api_key.is_present(),
                method = ctx.method_str(),
                path = %ctx.path,
                blacklist_file = %ctx.config.cyber_policy_blacklist_file.display(),
                "upstream returned cyber_policy but no bearer API key was available to blacklist"
            );
        }
        Err(error) => {
            error!(
                event = "cyber_policy_key_blacklist_error",
                api_key_masked = ctx.api_key.masked(),
                api_key_hash = ctx.api_key.hash(),
                api_key_present = ctx.api_key.is_present(),
                method = ctx.method_str(),
                path = %ctx.path,
                blacklist_file = %ctx.config.cyber_policy_blacklist_file.display(),
                error = %error,
                "upstream returned cyber_policy but API key blacklist persistence failed"
            );
        }
    }

    CyberPolicyAction::Block
}

fn record_cyber_policy_black_log_and_maybe_blacklist_ip(ctx: &ProxyRequest) {
    let Some(write) =
        ctx.forensic_writer
            .enqueue_cyber_policy_black(&logger::CyberPolicyBlackRequestLog {
                config: &ctx.config,
                api_key: &ctx.api_key,
                method: ctx.method_str(),
                uri: &ctx.uri,
                path: &ctx.path,
                headers: &ctx.headers,
                original_body: &ctx.original_body,
                body: &ctx.body,
                client_ip: ctx.client_ip,
                prompt_summary: &ctx.prompt_summary,
                queue_wait: ctx.queue_wait,
            })
    else {
        return;
    };

    warn!(
        event = "cyber_policy_black_log",
        api_key_masked = ctx.api_key.masked(),
        api_key_hash = ctx.api_key.hash(),
        api_key_present = ctx.api_key.is_present(),
        method = ctx.method_str(),
        path = %ctx.path,
        client_ip = %ctx.client_ip,
        log_file = %write.path.display(),
        client_ip_black_count = write.client_ip_count,
        "enqueued full cyber_policy-blocked request to black_log writer"
    );

    if write.client_ip_count < CYBER_POLICY_IP_BLACKLIST_THRESHOLD {
        return;
    }

    match ctx.blacklist_cache.append_client_ip(ctx.client_ip) {
        Ok(inserted) => {
            warn!(
                event = "cyber_policy_ip_blacklisted",
                client_ip = %ctx.client_ip,
                method = ctx.method_str(),
                path = %ctx.path,
                black_log_file = %write.path.display(),
                client_ip_black_count = write.client_ip_count,
                threshold = CYBER_POLICY_IP_BLACKLIST_THRESHOLD,
                blacklist_file = %ctx.config.cyber_policy_ip_blacklist_file.display(),
                inserted,
                "same client IP reached cyber_policy blacklist threshold"
            );
        }
        Err(error) => {
            error!(
                event = "cyber_policy_ip_blacklist_error",
                client_ip = %ctx.client_ip,
                method = ctx.method_str(),
                path = %ctx.path,
                black_log_file = %write.path.display(),
                client_ip_black_count = write.client_ip_count,
                threshold = CYBER_POLICY_IP_BLACKLIST_THRESHOLD,
                blacklist_file = %ctx.config.cyber_policy_ip_blacklist_file.display(),
                error = %error,
                "same client IP reached cyber_policy blacklist threshold but IP blacklist persistence failed"
            );
        }
    }
}

fn record_cyber_policy_white_log(ctx: &ProxyRequest) {
    let Some(trigger_count) =
        ctx.forensic_writer
            .enqueue_cyber_policy_white(&logger::CyberPolicyWhiteRequestLog {
                config: &ctx.config,
                api_key: &ctx.api_key,
                method: ctx.method_str(),
                uri: &ctx.uri,
                path: &ctx.path,
                headers: &ctx.headers,
                original_body: &ctx.original_body,
                body: &ctx.body,
                client_ip: ctx.client_ip,
                prompt_summary: &ctx.prompt_summary,
                queue_wait: ctx.queue_wait,
                trigger_count: 0, // Will be set by enqueue_cyber_policy_white
            })
    else {
        return;
    };

    warn!(
        event = "cyber_policy_white_log",
        api_key_masked = ctx.api_key.masked(),
        api_key_hash = ctx.api_key.hash(),
        api_key_present = ctx.api_key.is_present(),
        method = ctx.method_str(),
        path = %ctx.path,
        client_ip = %ctx.client_ip,
        trigger_count,
        "enqueued whitelisted key cyber_policy trigger to white_log writer"
    );
}

fn cyber_policy_request_confirms_violation(ctx: &ProxyRequest) -> bool {
    request_body_confirms_cyber_policy_violation(
        &ctx.original_body,
        &ctx.cyber_policy_confirmation_rules,
    ) || (ctx.original_body != ctx.body
        && request_body_confirms_cyber_policy_violation(
            &ctx.body,
            &ctx.cyber_policy_confirmation_rules,
        ))
}

fn request_body_confirms_cyber_policy_violation(
    body: &[u8],
    rules: &CyberPolicyConfirmationRules,
) -> bool {
    if body.is_empty() {
        return false;
    }

    serde_json::from_slice::<Value>(body).map_or_else(
        |_| text_confirms_cyber_policy_violation(&String::from_utf8_lossy(body), rules),
        |value| json_value_confirms_cyber_policy_violation(&value, rules),
    )
}

fn json_value_confirms_cyber_policy_violation(
    value: &Value,
    rules: &CyberPolicyConfirmationRules,
) -> bool {
    match value {
        Value::String(text) => text_confirms_cyber_policy_violation(text, rules),
        Value::Array(items) => items
            .iter()
            .any(|item| json_value_confirms_cyber_policy_violation(item, rules)),
        Value::Object(object) => object
            .values()
            .any(|value| json_value_confirms_cyber_policy_violation(value, rules)),
        _ => false,
    }
}

fn text_confirms_cyber_policy_violation(text: &str, rules: &CyberPolicyConfirmationRules) -> bool {
    rules.is_match(text)
}

pub async fn handle_protocol(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    req: Request<Body>,
) -> Response<Body> {
    let started_at = Instant::now();
    let runtime = state.runtime_snapshot();
    let (parts, body) = req.into_parts();
    let method = parts.method;
    let uri = parts.uri;
    let headers = parts.headers;
    let path = uri.path().to_owned();
    let peer_ip = peer_addr.ip();
    let client_ip = effective_client_ip(&headers, peer_ip);
    let api_key = ApiKeyInfo::from_headers_with_fallback(&headers, client_ip);

    // Check whitelist first - whitelisted keys bypass blacklist checks
    let is_whitelisted = state.whitelist_cache.api_key_is_whitelisted(&api_key);

    if runtime.config.cyber_policy_ban_enabled
        && !is_whitelisted
        && blacklisted_client_ip_should_be_blocked(
            &runtime.config,
            &state.blacklist_cache,
            client_ip,
            method.as_str(),
            &path,
        )
    {
        let summary = PromptSummary::default();
        logger::log_request(logger::RequestLog {
            api_key: &api_key,
            method: method.as_str(),
            path: &path,
            status: StatusCode::FORBIDDEN.as_u16(),
            latency: started_at.elapsed(),
            queue_wait: Duration::ZERO,
            prompt_summary: &summary,
            queued: false,
            upstream_status: None,
        });
        return cyber_policy_json_error_response();
    }

    if runtime.config.cyber_policy_ban_enabled
        && !is_whitelisted
        && blacklisted_api_key_should_be_blocked(
            &runtime.config,
            &state.blacklist_cache,
            &api_key,
            method.as_str(),
            &path,
        )
    {
        let summary = PromptSummary::default();
        logger::log_request(logger::RequestLog {
            api_key: &api_key,
            method: method.as_str(),
            path: &path,
            status: StatusCode::FORBIDDEN.as_u16(),
            latency: started_at.elapsed(),
            queue_wait: Duration::ZERO,
            prompt_summary: &summary,
            queued: false,
            upstream_status: None,
        });
        return cyber_policy_json_error_response();
    }

    if let Some(cooldown) = state
        .auth_failure_tracker
        .check(&api_key, client_ip, &runtime.config)
    {
        let status = cooldown.status();
        warn!(
            event = "auth_failure_cooldown_rejected",
            api_key_masked = api_key.masked(),
            api_key_hash = api_key.hash(),
            client_ip = %client_ip,
            method = method.as_str(),
            path = %path,
            cooldown_scope = cooldown.scope().as_str(),
            cooldown_remaining_ms = cooldown.remaining().as_millis() as u64,
            "request rejected locally during repeated-auth-failure cooldown"
        );
        let summary = PromptSummary::default();
        logger::log_request(logger::RequestLog {
            api_key: &api_key,
            method: method.as_str(),
            path: &path,
            status: status.as_u16(),
            latency: started_at.elapsed(),
            queue_wait: Duration::ZERO,
            prompt_summary: &summary,
            queued: false,
            upstream_status: None,
        });
        return cooldown.into_response();
    }

    if should_stream_request_body_without_buffering(
        &method,
        &path,
        &headers,
        runtime.config.max_body_bytes,
    ) {
        if let Some(content_length) = request_content_length(&headers)
            && content_length > runtime.config.max_upload_body_bytes as u64
        {
            warn!(
                path = %path,
                content_length,
                max_upload_body_bytes = runtime.config.max_upload_body_bytes,
                "upload content-length exceeds streaming limit"
            );
            return payload_too_large_response(&api_key, &method, &path, started_at);
        }
        return forward_unbuffered_request_body(
            state, runtime, method, uri, headers, body, client_ip, api_key, path, started_at,
        )
        .await;
    }

    if let Some(content_length) =
        request_content_length_exceeds_limit(&headers, runtime.config.max_body_bytes)
    {
        warn!(
            path = %path,
            content_length,
            max_body_bytes = runtime.config.max_body_bytes,
            "request body content-length exceeds limit"
        );
        return payload_too_large_response(&api_key, &method, &path, started_at);
    }

    let mut body = match to_bytes(body, runtime.config.max_body_bytes).await {
        Ok(body) => body,
        Err(error) => {
            warn!(error = %error, path = %path, "failed to read request body");
            return payload_too_large_response(&api_key, &method, &path, started_at);
        }
    };

    let original_body = body.clone();

    let inspection =
        request_body_inspection_plan(&method, &path, &body, &runtime.tool_filter_index);

    // 两阶段 JSON 解析优化：
    // 第一阶段：轻量级解析（跳过大字段 messages/prompt/system）
    // 第二阶段：只在需要修改时才完整解析
    if inspection.needs_metadata_parse()
        && let Ok(meta) = serde_json::from_slice::<RequestMetadata<'_>>(&body)
    {
        // 图像模型检查（早期退出，无需完整解析）
        if inspection.should_check_image_model
            && let Some(model) = &meta.model
            && should_block_image_model_request(model, &path)
        {
            return image_model_blocked_response(
                &runtime.config,
                &api_key,
                &method,
                &path,
                model,
                started_at,
            );
        }

        // 判断是否需要完整解析（只在需要修改时）
        let needs_normalization = inspection.should_normalize_structured_output
            && request_metadata_needs_structured_output_normalization(&meta, &path);
        let needs_responses_compat_normalization = inspection.should_normalize_responses_compat
            && request_metadata_needs_responses_compat_normalization(&meta, &path);
        let needs_tool_filter = inspection.should_filter_tools
            && request_metadata_needs_tool_filter(
                &meta,
                &runtime.tool_filter_index,
                &runtime.config.filter_tools_by_model,
            );

        // 第二阶段：只在需要修改时才完整解析
        if (needs_normalization || needs_responses_compat_normalization || needs_tool_filter)
            && let Ok(mut json) = serde_json::from_slice::<Value>(&body)
        {
            let structured_output_schema_normalized = needs_normalization
                && structured_outputs::normalize_request_value(&method, &path, &mut json);

            let responses_compat_normalized = needs_responses_compat_normalization
                && normalize_responses_compat_request_value(&mut json);

            let tools_filtered = needs_tool_filter
                && filter_tools_json_by_model(
                    &mut json,
                    &runtime.config.filter_tools_by_model,
                    &runtime.tool_filter_index,
                    &runtime.config,
                    &api_key,
                    &path,
                );

            if (structured_output_schema_normalized
                || responses_compat_normalized
                || tools_filtered)
                && let Ok(updated_body) = serde_json::to_vec(&json)
            {
                let original_body_bytes = body.len();
                let model_for_log = meta.model.as_deref().unwrap_or("unknown").to_owned();
                drop(meta);
                body = Bytes::from(updated_body);

                if structured_output_schema_normalized {
                    logger::log_debug_operation(
                        &runtime.config,
                        &api_key,
                        method.as_str(),
                        &path,
                        "structured_output_schema_normalized",
                        format!(
                            "original_body_bytes={original_body_bytes} normalized_body_bytes={} model={:?}",
                            body.len(),
                            model_for_log
                        ),
                    );
                }

                if responses_compat_normalized {
                    logger::log_debug_operation(
                        &runtime.config,
                        &api_key,
                        method.as_str(),
                        &path,
                        "responses_compat_normalized",
                        format!(
                            "original_body_bytes={original_body_bytes} normalized_body_bytes={} model={:?}",
                            body.len(),
                            model_for_log
                        ),
                    );
                }

                if tools_filtered {
                    logger::log_debug_operation(
                        &runtime.config,
                        &api_key,
                        method.as_str(),
                        &path,
                        "tools_filtered",
                        format!(
                            "original_body_bytes={original_body_bytes} filtered_body_bytes={} model={:?}",
                            body.len(),
                            model_for_log
                        ),
                    );
                }
            }
        }
    }

    // 检测是否为图片生成请求（两种场景）：
    // 1. 直接图片接口：/v1/images/generations、/v1/images/edits
    // 2. Chat 接口使用 image_generation 工具
    let is_image_generation =
        is_image_model_path(&path) || responses_chat::uses_image_generation_tool(&body);

    let ctx = ProxyRequest {
        config: runtime.config,
        client: runtime.client,
        method,
        uri,
        headers,
        original_body,
        body,
        client_ip,
        api_key,
        path,
        prompt_summary: PromptSummary::default(),
        cyber_policy_confirmation_rules: runtime.cyber_policy_confirmation_rules,
        blacklist_cache: state.blacklist_cache.clone(),
        whitelist_cache: state.whitelist_cache.clone(),
        forensic_writer: state.forensic_writer.clone(),
        started_at,
        queue_started_at: None,
        queue_wait: Duration::ZERO,
        is_image_generation,
    };

    logger::log_debug_operation(
        &ctx.config,
        &ctx.api_key,
        ctx.method_str(),
        &ctx.path,
        "request_received",
        format!(
            "body_bytes={} headers={}",
            ctx.body.len(),
            ctx.headers.len()
        ),
    );

    if sse_image::is_image_endpoint(&ctx.path) {
        return sse_image::handle_image_request(state, ctx);
    }

    if is_responses_nonstream_image_generation_request(&ctx) {
        return sse_image::handle_image_json_request(state, ctx);
    }

    let is_whitelisted = ctx.whitelist_cache.api_key_is_whitelisted(&ctx.api_key);

    if is_stream_request(&ctx.headers, &ctx.body) {
        let permit = if is_whitelisted {
            Some(state.rate_limiter.force_acquire(ctx.api_key.identity()))
        } else {
            state.rate_limiter.try_acquire(ctx.api_key.identity())
        };
        if let Some(permit) = permit {
            return forward_stream_with_permit_or_retry_sse(state, ctx, permit).await;
        }
        return stream_response_with_optional_permit(state, ctx, None, None);
    }

    let (permit, queued, queue_wait) = if is_whitelisted {
        (
            state.rate_limiter.force_acquire(ctx.api_key.identity()),
            false,
            Duration::ZERO,
        )
    } else if let Some(permit) = state.rate_limiter.try_acquire(ctx.api_key.identity()) {
        (permit, false, Duration::ZERO)
    } else {
        let queue_started_at = Instant::now();
        (
            state.rate_limiter.acquire(ctx.api_key.identity()).await,
            true,
            queue_started_at.elapsed(),
        )
    };

    forward_with_permit(state, ctx.with_queue_wait(queue_wait), permit, queued).await
}

#[allow(clippy::too_many_arguments)]
async fn forward_unbuffered_request_body(
    state: Arc<AppState>,
    runtime: RuntimeSnapshot,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
    client_ip: IpAddr,
    api_key: ApiKeyInfo,
    path: String,
    started_at: Instant,
) -> Response<Body> {
    let ctx = ProxyRequest {
        config: runtime.config,
        client: runtime.client,
        method,
        uri,
        headers,
        original_body: Bytes::new(),
        body: Bytes::new(),
        client_ip,
        api_key,
        path,
        prompt_summary: PromptSummary::default(),
        cyber_policy_confirmation_rules: runtime.cyber_policy_confirmation_rules,
        blacklist_cache: state.blacklist_cache.clone(),
        whitelist_cache: state.whitelist_cache.clone(),
        forensic_writer: state.forensic_writer.clone(),
        started_at,
        queue_started_at: None,
        queue_wait: Duration::ZERO,
        is_image_generation: false,
    };
    logger::log_debug_operation(
        &ctx.config,
        &ctx.api_key,
        ctx.method_str(),
        &ctx.path,
        "request_body_streaming_bypass",
        format!(
            "content_length={:?} max_body_bytes={}",
            request_content_length(&ctx.headers),
            ctx.config.max_body_bytes
        ),
    );

    let (permit, queued, queue_wait) = if ctx.whitelist_cache.api_key_is_whitelisted(&ctx.api_key) {
        (
            state.rate_limiter.force_acquire(ctx.api_key.identity()),
            false,
            Duration::ZERO,
        )
    } else if let Some(permit) = state.rate_limiter.try_acquire(ctx.api_key.identity()) {
        (permit, false, Duration::ZERO)
    } else {
        let queue_started_at = Instant::now();
        (
            state.rate_limiter.acquire(ctx.api_key.identity()).await,
            true,
            queue_started_at.elapsed(),
        )
    };
    let ctx = state.bind_runtime_snapshot(ctx.with_queue_wait(queue_wait));

    let upstream =
        match execute_unbuffered_upstream(&ctx, body, ctx.config.max_upload_body_bytes).await {
            Ok(upstream) => upstream,
            Err(error) if request_error_is_upload_body_limit(&error) => {
                drop(permit);
                warn!(
                    path = %ctx.path,
                    max_upload_body_bytes = ctx.config.max_upload_body_bytes,
                    "streaming upload exceeded configured body limit"
                );
                return payload_too_large_response(
                    &ctx.api_key,
                    &ctx.method,
                    &ctx.path,
                    ctx.started_at,
                );
            }
            Err(error) => {
                drop(permit);
                error!(error = %error, path = %ctx.path, "unbuffered upstream request failed");
                log_bad_gateway_final_request(
                    &ctx.config,
                    &ctx,
                    "unbuffered_upstream_request_error_final",
                    Some(StatusCode::BAD_GATEWAY.as_u16()),
                    None,
                    None,
                    Some(&logger::error_chain(&error)),
                );
                log_proxy_request(&ctx, StatusCode::BAD_GATEWAY.as_u16(), queued, None);
                return json_error_response(
                    StatusCode::BAD_GATEWAY,
                    "upstream request failed",
                    "bad_gateway",
                );
            }
        };

    streamed_response_from_upstream(state, ctx, upstream, permit, queued).await
}

pub async fn forward_with_permit(
    state: Arc<AppState>,
    ctx: ProxyRequest,
    permit: RateLimitPermit,
    queued: bool,
) -> Response<Body> {
    let ctx = state.bind_runtime_snapshot(ctx);
    if should_use_responses_nonstream_chat_fallback(&ctx) {
        return forward_responses_nonstream_chat_fallback(state, ctx, permit, queued).await;
    }

    match execute_upstream_retrying_with_permit(&state, &ctx, permit).await {
        Ok((upstream, permit)) => {
            let config = ctx.config.clone();
            response_from_upstream(&state, &config, ctx, upstream, permit, queued)
        }
        Err(error) => {
            error!(error = %error, path = %ctx.path, "upstream request failed");
            log_bad_gateway_final_request(
                &ctx.config,
                &ctx,
                "upstream_request_error_final",
                Some(StatusCode::BAD_GATEWAY.as_u16()),
                None,
                None,
                Some(&logger::error_chain(&error)),
            );
            log_proxy_request(&ctx, StatusCode::BAD_GATEWAY.as_u16(), queued, None);
            json_error_response(
                StatusCode::BAD_GATEWAY,
                "upstream request failed",
                "bad_gateway",
            )
        }
    }
}

async fn forward_responses_nonstream_chat_fallback(
    state: Arc<AppState>,
    ctx: ProxyRequest,
    permit: RateLimitPermit,
    queued: bool,
) -> Response<Body> {
    let original_ctx = ctx.clone();
    let config = ctx.config.clone();
    let fallback_ctx = match responses_chat_fallback_request(&config, ctx) {
        Ok(ctx) => ctx,
        Err(error) => {
            drop(permit);
            error!(error = %error, path = %original_ctx.path, "failed to convert Responses request to Chat Completions");
            log_proxy_request(
                &original_ctx,
                StatusCode::BAD_REQUEST.as_u16(),
                queued,
                None,
            );
            return json_error_response(
                StatusCode::BAD_REQUEST,
                "failed to convert Responses request to Chat Completions",
                "responses_chat_fallback_request_error",
            );
        }
    };

    let fallback_config = fallback_ctx.config.clone();
    match execute_upstream_retrying_with_permit(&state, &fallback_ctx, permit).await {
        Ok((upstream, permit)) => {
            response_from_chat_fallback_upstream(
                &state,
                &fallback_config,
                fallback_ctx,
                upstream,
                permit,
                queued,
            )
            .await
        }
        Err(error) => {
            error!(error = %error, path = %fallback_ctx.path, "Responses chat fallback upstream request failed");
            log_bad_gateway_final_request(
                &fallback_config,
                &fallback_ctx,
                "responses_chat_fallback_upstream_request_error_final",
                Some(StatusCode::BAD_GATEWAY.as_u16()),
                None,
                None,
                Some(&logger::error_chain(&error)),
            );
            log_proxy_request(
                &fallback_ctx,
                StatusCode::BAD_GATEWAY.as_u16(),
                queued,
                None,
            );
            json_error_response(
                StatusCode::BAD_GATEWAY,
                "upstream request failed",
                "bad_gateway",
            )
        }
    }
}

fn should_use_responses_nonstream_chat_fallback(ctx: &ProxyRequest) -> bool {
    ctx.config.responses_nonstream_chat_fallback
        && responses_chat::is_responses_create_endpoint(&ctx.method, &ctx.path)
        && !is_stream_request(&ctx.headers, &ctx.body)
        && !responses_chat::uses_image_generation_tool(&ctx.body)
}

fn is_responses_nonstream_image_generation_request(ctx: &ProxyRequest) -> bool {
    responses_chat::is_responses_create_endpoint(&ctx.method, &ctx.path)
        && !is_stream_request(&ctx.headers, &ctx.body)
        && responses_chat::uses_image_generation_tool(&ctx.body)
}

fn responses_chat_fallback_request(
    config: &Config,
    mut ctx: ProxyRequest,
) -> anyhow::Result<ProxyRequest> {
    let original_body_bytes = ctx.body.len();
    ctx.body = responses_chat::responses_to_chat_body(&ctx.body)?;
    ctx.uri = responses_chat::chat_completions_uri_from_responses_uri(&ctx.uri);
    ctx.path = ctx.uri.path().to_owned();
    logger::log_debug_operation(
        config,
        &ctx.api_key,
        ctx.method_str(),
        &ctx.path,
        "responses_chat_fallback_request",
        format!(
            "original_body_bytes={original_body_bytes} converted_body_bytes={}",
            ctx.body.len()
        ),
    );
    Ok(ctx)
}

async fn response_from_chat_fallback_upstream(
    state: &AppState,
    config: &Config,
    ctx: ProxyRequest,
    upstream: BufferedUpstreamResponse,
    permit: RateLimitPermit,
    queued: bool,
) -> Response<Body> {
    let upstream_status = upstream.upstream_status;
    record_upstream_auth_failure(
        state,
        &ctx,
        upstream_status,
        &upstream.headers,
        &upstream.body,
    );
    if buffered_upstream_response_has_cyber_policy_error(&upstream) {
        let action = block_cyber_policy_key(&ctx);
        match action {
            CyberPolicyAction::WhitelistLog | CyberPolicyAction::PassThrough => {
                log_proxy_request(&ctx, upstream_status, queued, Some(upstream_status));
                drop(permit);
                let status = status_from_u16(upstream_status);
                let headers = filtered_response_headers(&upstream.headers);
                let mut response = Response::new(Body::from(upstream.body));
                *response.status_mut() = status;
                *response.headers_mut() = headers;
                return response;
            }
            CyberPolicyAction::Block => {
                log_proxy_request(
                    &ctx,
                    StatusCode::FORBIDDEN.as_u16(),
                    queued,
                    Some(upstream_status),
                );
                drop(permit);
                return cyber_policy_json_error_response();
            }
        }
    }

    let mut headers = filtered_response_headers(&upstream.headers);
    let status = status_from_u16(upstream_status);

    let converted_body = if (200..300).contains(&upstream_status) {
        match responses_chat::chat_to_responses_body(&upstream.body) {
            Ok(converted) => converted,
            Err(error) => {
                drop(permit);
                error!(error = %error, path = %ctx.path, "failed to convert Chat Completions response to Responses");
                log_proxy_request(
                    &ctx,
                    StatusCode::BAD_GATEWAY.as_u16(),
                    queued,
                    Some(upstream_status),
                );
                return json_error_response(
                    StatusCode::BAD_GATEWAY,
                    "failed to convert Chat Completions response to Responses",
                    "responses_chat_fallback_response_error",
                );
            }
        }
    } else {
        upstream.body
    };

    if status == StatusCode::BAD_GATEWAY {
        log_bad_gateway_final_request(
            config,
            &ctx,
            "responses_chat_fallback_upstream_http_502_final",
            Some(upstream_status),
            Some(&headers),
            Some(&converted_body),
            None,
        );
    }

    // 为 503 错误添加详细日志，与 502 保持一致
    if status == StatusCode::SERVICE_UNAVAILABLE {
        log_bad_gateway_final_request(
            config,
            &ctx,
            "responses_chat_fallback_upstream_http_503_final",
            Some(upstream_status),
            Some(&headers),
            Some(&converted_body),
            None,
        );
    }

    if (200..300).contains(&upstream_status) {
        headers.remove(header::CONTENT_LENGTH);
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
    }
    log_proxy_request(&ctx, status.as_u16(), queued, Some(upstream_status));
    logger::log_debug_operation(
        config,
        &ctx.api_key,
        ctx.method_str(),
        &ctx.path,
        "responses_chat_fallback_response",
        format!(
            "upstream_status={upstream_status} response_body_bytes={}",
            converted_body.len()
        ),
    );
    drop(permit);
    let mut response = Response::new(Body::from(converted_body));
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
}

async fn forward_stream_with_permit_or_retry_sse(
    state: Arc<AppState>,
    ctx: ProxyRequest,
    permit: RateLimitPermit,
) -> Response<Body> {
    let ctx = state.bind_runtime_snapshot(ctx);
    let initial_frame = initial_stream_frame(&ctx.config, &ctx);
    if initial_frame.is_some() {
        return stream_response_with_initial_frame(state, ctx, Some(permit), None, initial_frame);
    }

    match execute_upstream(&ctx).await {
        Ok(upstream) => {
            let retry_decision = match classify_upstream_response_for_retry(&ctx, upstream, 0).await
            {
                Ok(decision) => decision,
                Err(error) => {
                    drop(permit);
                    error!(error = %error, path = %ctx.path, "stream upstream response body read failed before retry decision");
                    log_bad_gateway_final_request(
                        &ctx.config,
                        &ctx,
                        "stream_upstream_body_error_final",
                        Some(StatusCode::BAD_GATEWAY.as_u16()),
                        None,
                        None,
                        Some(&logger::error_chain(&error)),
                    );
                    log_proxy_request(&ctx, StatusCode::BAD_GATEWAY.as_u16(), false, None);
                    return json_error_response(
                        StatusCode::BAD_GATEWAY,
                        "upstream request failed",
                        "bad_gateway",
                    );
                }
            };

            match retry_decision {
                UpstreamRetryDecision::Forward(upstream) => {
                    streamed_response_from_upstream(state, ctx, upstream, permit, false).await
                }
                UpstreamRetryDecision::ForwardBuffered(upstream) => {
                    let config = ctx.config.clone();
                    response_from_upstream(&state, &config, ctx, upstream, permit, false)
                }
                UpstreamRetryDecision::Retry {
                    retry_plan,
                    upstream_headers,
                    upstream_body,
                } => {
                    record_retry_cooldown(&state, &ctx, &retry_plan);
                    log_bad_gateway_retry_request(
                        &ctx.config,
                        &ctx,
                        &retry_plan,
                        1,
                        BadGatewayRetryLog {
                            condition: "stream_upstream_http_502_retry",
                            upstream_headers: Some(&upstream_headers),
                            upstream_body: upstream_body.as_deref(),
                            upstream_error: None,
                        },
                    );
                    stream_response_with_optional_permit(
                        state,
                        ctx,
                        Some(permit),
                        Some((1, retry_plan)),
                    )
                }
            }
        }
        Err(error) => {
            if let Some(retry_plan) = retry_plan_for_upstream_request_error(&ctx, 0) {
                record_retry_cooldown(&state, &ctx, &retry_plan);
                log_bad_gateway_retry_request(
                    &ctx.config,
                    &ctx,
                    &retry_plan,
                    1,
                    BadGatewayRetryLog {
                        condition: "stream_upstream_request_error_retry",
                        upstream_headers: None,
                        upstream_body: None,
                        upstream_error: Some(&logger::error_chain(&error)),
                    },
                );
                return stream_response_with_optional_permit(
                    state,
                    ctx,
                    Some(permit),
                    Some((1, retry_plan)),
                );
            }

            drop(permit);
            error!(error = %error, path = %ctx.path, "upstream streaming request failed");
            log_bad_gateway_final_request(
                &ctx.config,
                &ctx,
                "stream_upstream_request_error_final",
                Some(StatusCode::BAD_GATEWAY.as_u16()),
                None,
                None,
                Some(&logger::error_chain(&error)),
            );
            log_proxy_request(&ctx, StatusCode::BAD_GATEWAY.as_u16(), false, None);
            json_error_response(
                StatusCode::BAD_GATEWAY,
                "upstream request failed",
                "bad_gateway",
            )
        }
    }
}

pub(crate) async fn execute_upstream(ctx: &ProxyRequest) -> reqwest::Result<reqwest::Response> {
    let url = build_upstream_url(&ctx.config.upstream_base_url, &ctx.uri);
    let method = reqwest::Method::from_bytes(ctx.method.as_str().as_bytes())
        .expect("HTTP method from axum should be valid for reqwest");

    let headers = upstream_request_headers(
        &ctx.headers,
        &ctx.config.internal_header_prefix,
        ctx.client_ip,
    );
    let builder = ctx.client.request(method, url).headers(headers);

    logger::log_debug_operation(
        &ctx.config,
        &ctx.api_key,
        ctx.method_str(),
        &ctx.path,
        "upstream_request_start",
        format!("body_bytes={}", ctx.body.len()),
    );

    match builder.body(ctx.body.clone()).send().await {
        Ok(response) => {
            logger::log_debug_operation(
                &ctx.config,
                &ctx.api_key,
                ctx.method_str(),
                &ctx.path,
                "upstream_response_headers",
                format!("status={}", response.status().as_u16()),
            );
            Ok(response)
        }
        Err(error) => {
            logger::log_debug_operation(
                &ctx.config,
                &ctx.api_key,
                ctx.method_str(),
                &ctx.path,
                "upstream_request_error",
                logger::error_chain(&error),
            );
            Err(error)
        }
    }
}

async fn execute_unbuffered_upstream(
    ctx: &ProxyRequest,
    body: Body,
    max_upload_body_bytes: usize,
) -> reqwest::Result<reqwest::Response> {
    let url = build_upstream_url(&ctx.config.upstream_base_url, &ctx.uri);
    let method = reqwest::Method::from_bytes(ctx.method.as_str().as_bytes())
        .expect("HTTP method from axum should be valid for reqwest");

    let headers = upstream_request_headers(
        &ctx.headers,
        &ctx.config.internal_header_prefix,
        ctx.client_ip,
    );
    let mut builder = ctx.client.request(method, url).headers(headers);
    if let Some(content_length) = request_content_length(&ctx.headers) {
        builder = builder.header(reqwest::header::CONTENT_LENGTH, content_length);
    }

    logger::log_debug_operation(
        &ctx.config,
        &ctx.api_key,
        ctx.method_str(),
        &ctx.path,
        "unbuffered_upstream_request_start",
        "body_stream=true",
    );

    match builder
        .body(reqwest::Body::wrap_stream(limited_upload_body_stream(
            body,
            max_upload_body_bytes,
        )))
        .send()
        .await
    {
        Ok(response) => {
            logger::log_debug_operation(
                &ctx.config,
                &ctx.api_key,
                ctx.method_str(),
                &ctx.path,
                "upstream_response_headers",
                format!("status={}", response.status().as_u16()),
            );
            Ok(response)
        }
        Err(error) => {
            logger::log_debug_operation(
                &ctx.config,
                &ctx.api_key,
                ctx.method_str(),
                &ctx.path,
                "unbuffered_upstream_request_error",
                logger::error_chain(&error),
            );
            Err(error)
        }
    }
}

/// 带 permit 管理的重试版本：重试前会释放旧 permit 并重新获取，确保遵守速率限制
pub(crate) async fn execute_upstream_retrying_with_permit(
    state: &AppState,
    ctx: &ProxyRequest,
    mut permit: RateLimitPermit,
) -> reqwest::Result<(BufferedUpstreamResponse, RateLimitPermit)> {
    let mut retry_count = 0_usize;

    loop {
        let upstream = match execute_upstream(ctx).await {
            Ok(upstream) => upstream,
            Err(error) => {
                let Some(retry_plan) = retry_plan_for_upstream_request_error(ctx, retry_count)
                else {
                    return Err(error);
                };

                record_retry_cooldown(state, ctx, &retry_plan);
                retry_count += 1;
                log_bad_gateway_retry_request(
                    &ctx.config,
                    ctx,
                    &retry_plan,
                    retry_count,
                    BadGatewayRetryLog {
                        condition: "upstream_request_error_retry",
                        upstream_headers: None,
                        upstream_body: None,
                        upstream_error: Some(&logger::error_chain(&error)),
                    },
                );
                logger::log_upstream_retry(logger::UpstreamRetryLog {
                    api_key: &ctx.api_key,
                    method: ctx.method_str(),
                    path: &ctx.path,
                    attempt: retry_count,
                    upstream_status: retry_plan.upstream_status,
                    retry_reason: retry_plan.reason,
                    delay: retry_plan.delay,
                    retry_after: retry_plan.retry_after,
                });

                // 关键：释放旧 permit，等待，然后重新获取
                drop(permit);
                state
                    .rate_limiter
                    .wait_for_retry_delay(ctx.api_key.identity(), retry_plan.delay)
                    .await;
                permit = acquire_permit_after_retry_delay(state, ctx, retry_plan.reason).await;

                continue;
            }
        };
        let upstream = BufferedUpstreamResponse::from_response(upstream).await?;
        let Some(retry_plan) =
            retry_plan_for_buffered_upstream_response(ctx, &upstream, retry_count)
        else {
            return Ok((upstream, permit));
        };

        record_retry_cooldown(state, ctx, &retry_plan);
        retry_count += 1;
        log_bad_gateway_retry_request(
            &ctx.config,
            ctx,
            &retry_plan,
            retry_count,
            BadGatewayRetryLog {
                condition: "upstream_http_502_retry",
                upstream_headers: Some(&upstream.headers),
                upstream_body: Some(&upstream.body),
                upstream_error: None,
            },
        );
        logger::log_upstream_retry(logger::UpstreamRetryLog {
            api_key: &ctx.api_key,
            method: ctx.method_str(),
            path: &ctx.path,
            attempt: retry_count,
            upstream_status: retry_plan.upstream_status,
            retry_reason: retry_plan.reason,
            delay: retry_plan.delay,
            retry_after: retry_plan.retry_after,
        });

        // 关键：释放旧 permit，等待，然后重新获取
        drop(permit);
        state
            .rate_limiter
            .wait_for_retry_delay(ctx.api_key.identity(), retry_plan.delay)
            .await;
        permit = acquire_permit_after_retry_delay(state, ctx, retry_plan.reason).await;
    }
}

pub(crate) fn body_chunks_to_bytes(chunks: Vec<Bytes>) -> Bytes {
    let total_len: usize = chunks.iter().map(|chunk| chunk.len()).sum();
    let mut body = bytes::BytesMut::with_capacity(total_len);
    for chunk in chunks {
        body.extend_from_slice(&chunk);
    }
    body.freeze()
}

pub(crate) fn is_event_stream_headers(headers: &HeaderMap) -> bool {
    header_contains(headers, header::CONTENT_TYPE, "text/event-stream")
}

fn synthetic_first_space_event(config: &Config, ctx: &ProxyRequest) -> Option<Bytes> {
    if !config.stream_synthetic_first_space {
        return None;
    }

    if !is_synthetic_first_space_endpoint(&ctx.method, &ctx.path) {
        return None;
    }

    let value = serde_json::from_slice::<Value>(&ctx.body).ok()?;
    if !is_synthetic_first_space_eligible(&ctx.method, &ctx.path, &value) {
        return None;
    }

    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let payload = json!({
        "id": "chatcmpl-ai-bridge-synthetic-first-space",
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [
            {
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "content": " "
                },
                "finish_reason": null
            }
        ]
    })
    .to_string();

    Some(sse_data_bytes(payload.as_bytes()))
}

fn is_synthetic_first_space_eligible(method: &Method, path: &str, value: &Value) -> bool {
    is_synthetic_first_space_endpoint(method, path) && is_synthetic_first_space_body_eligible(value)
}

fn is_synthetic_first_space_endpoint(method: &Method, path: &str) -> bool {
    method == Method::POST && path.trim_end_matches('/') == "/v1/chat/completions"
}

fn is_synthetic_first_space_body_eligible(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };

    if object.get("stream").and_then(Value::as_bool) != Some(true) {
        return false;
    }

    let Some(messages) = object.get("messages").and_then(Value::as_array) else {
        return false;
    };
    if messages.is_empty() {
        return false;
    }

    if object.get("tools").is_some_and(non_empty_array_or_object)
        || object
            .get("functions")
            .is_some_and(non_empty_array_or_object)
        || object.contains_key("tool_choice")
        || object.contains_key("function_call")
    {
        return false;
    }

    if object
        .get("response_format")
        .is_some_and(is_json_response_format)
    {
        return false;
    }

    if object
        .get("n")
        .and_then(Value::as_u64)
        .is_some_and(|n| n > 1)
    {
        return false;
    }

    if object
        .get("modalities")
        .is_some_and(|modalities| !is_text_only_modalities(modalities))
    {
        return false;
    }

    true
}

fn non_empty_array_or_object(value: &Value) -> bool {
    value.as_array().is_some_and(|items| !items.is_empty())
        || value.as_object().is_some_and(|object| !object.is_empty())
}

fn is_json_response_format(value: &Value) -> bool {
    value
        .get("type")
        .and_then(Value::as_str)
        .map(|format_type| {
            format_type.eq_ignore_ascii_case("json_object")
                || format_type.eq_ignore_ascii_case("json_schema")
        })
        .unwrap_or(true)
}

fn is_text_only_modalities(value: &Value) -> bool {
    let Some(modalities) = value.as_array() else {
        return false;
    };

    !modalities.is_empty()
        && modalities
            .iter()
            .all(|modality| modality.as_str().is_some_and(|item| item == "text"))
}

fn status_from_u16(status: u16) -> StatusCode {
    StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY)
}

fn io_error(error: reqwest::Error) -> std::io::Error {
    std::io::Error::other(error)
}

// Helper function to check if haystack contains needle (case-insensitive ASCII)
fn contains_substring_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }

    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

#[cfg(test)]
mod tests;
