use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result};
use axum::http::{HeaderMap, Uri};
use dashmap::DashMap;
use serde_json::Value;
use tokio::sync::mpsc;
use tracing::error;

use crate::{
    auth::ApiKeyInfo,
    config::Config,
    logger::{
        BadGatewayRequestLog, BadGatewayTriggerLog, CyberPolicyBlackRequestLog,
        CyberPolicyWhiteRequestLog, PromptSummary, bad_gateway_request_json,
        bad_gateway_request_log_filename, cyber_policy_black_request_json,
        cyber_policy_white_request_json, unix_timestamp_parts,
    },
    private_file::{create_private_dir_all, open_private_append},
};

/// Lightweight, owned counterpart to [`CyberPolicyBlackRequestLog`].
///
/// We need an owned snapshot so the request hot path can return immediately
/// after enqueuing the work — the background writer task processes the entry
/// later, after the borrowed request context is gone.
struct CyberPolicyBlackJob {
    config: Arc<Config>,
    api_key: ApiKeyInfo,
    method: String,
    uri: Uri,
    path: String,
    headers: HeaderMap,
    original_body: Vec<u8>,
    body: Vec<u8>,
    client_ip: IpAddr,
    prompt_summary: PromptSummary,
    queue_wait: Duration,
    now: SystemTime,
}

struct CyberPolicyWhiteJob {
    config: Arc<Config>,
    api_key: ApiKeyInfo,
    method: String,
    uri: Uri,
    path: String,
    headers: HeaderMap,
    original_body: Vec<u8>,
    body: Vec<u8>,
    client_ip: IpAddr,
    prompt_summary: PromptSummary,
    queue_wait: Duration,
    trigger_count: usize,
    now: SystemTime,
}

struct BadGatewayJob {
    config: Arc<Config>,
    api_key: ApiKeyInfo,
    method: String,
    uri: Uri,
    path: String,
    headers: HeaderMap,
    original_body: Vec<u8>,
    body: Vec<u8>,
    client_ip: IpAddr,
    prompt_summary: PromptSummary,
    queue_wait: Duration,
    trigger: OwnedBadGatewayTrigger,
    now: SystemTime,
}

#[derive(Clone)]
struct OwnedBadGatewayTrigger {
    condition: &'static str,
    upstream_status: Option<u16>,
    upstream_headers: Option<HeaderMap>,
    upstream_body: Option<Vec<u8>>,
    upstream_error: Option<String>,
    retry_reason: Option<&'static str>,
    retry_attempt: Option<usize>,
    retry_delay: Option<Duration>,
    retry_after: Option<String>,
}

impl OwnedBadGatewayTrigger {
    fn from_borrowed(trigger: &BadGatewayTriggerLog<'_>, max_body_bytes: usize) -> Self {
        Self {
            condition: trigger.condition,
            upstream_status: trigger.upstream_status,
            upstream_headers: trigger.upstream_headers.cloned(),
            upstream_body: trigger
                .upstream_body
                .map(|body| bounded_copy(body, max_body_bytes)),
            upstream_error: trigger.upstream_error.map(str::to_owned),
            retry_reason: trigger.retry_reason,
            retry_attempt: trigger.retry_attempt,
            retry_delay: trigger.retry_delay,
            retry_after: trigger.retry_after.map(str::to_owned),
        }
    }

    fn as_borrowed(&self) -> BadGatewayTriggerLog<'_> {
        BadGatewayTriggerLog {
            condition: self.condition,
            upstream_status: self.upstream_status,
            upstream_headers: self.upstream_headers.as_ref(),
            upstream_body: self.upstream_body.as_deref(),
            upstream_error: self.upstream_error.as_deref(),
            retry_reason: self.retry_reason,
            retry_attempt: self.retry_attempt,
            retry_delay: self.retry_delay,
            retry_after: self.retry_after.as_deref(),
        }
    }
}

enum WriterMessage {
    CyberPolicy(Box<CyberPolicyBlackJob>),
    CyberPolicyWhite(Box<CyberPolicyWhiteJob>),
    BadGateway(Box<BadGatewayJob>),
    #[cfg(test)]
    Flush(tokio::sync::oneshot::Sender<()>),
}

/// Result of accepting a cyber-policy black record. Surfaces the in-memory
/// IP-count that PR1's original synchronous path used to derive from re-reading
/// the JSONL file — by tracking it in memory we no longer need that O(N) scan.
#[derive(Debug, Clone)]
pub struct CyberPolicyBlackEnqueue {
    pub path: PathBuf,
    pub client_ip_count: usize,
}

/// Async background-writer for forensic JSONL files.
///
/// The forensic log paths (`cyber_policy_black_log_file`,
/// `bad_gateway_log_dir`, and `service_unavailable_log_dir`)
/// live on the request hot path today. PR2 moves the actual `OpenOptions::open
/// + writeln + flush` syscalls to a dedicated tokio task so handlers can
/// continue without paying for synchronous file I/O. The same task owns an
/// in-memory `client_ip` counter that replaces the previous "re-open + scan"
///   statistics path; the file itself remains the source of truth (replayed at
///   startup), preserving forensic semantics across restarts.
pub struct ForensicWriter {
    sender: mpsc::Sender<WriterMessage>,
    cyber_policy_log_path: PathBuf,
    cyber_policy_ip_counts: Arc<DashMap<IpAddr, usize>>,
    cyber_policy_white_key_counts: Arc<DashMap<String, usize>>,
    cyber_policy_budget: MinuteBudget,
    cyber_policy_white_budget: MinuteBudget,
    bad_gateway_budget: MinuteBudget,
    dropped_messages: AtomicU64,
    _task_handle: tokio::task::JoinHandle<()>,
}

struct MinuteBudget {
    state: Mutex<(u64, usize)>,
}

impl ForensicWriter {
    /// Build a writer rooted at `cyber_policy_black_log_file`.
    ///
    /// Replays the existing JSONL file once to seed the in-memory counter so
    /// IP-count assertions made immediately after startup (e.g. the 3-strikes
    /// blacklist threshold) still match the on-disk history.
    #[cfg(test)]
    pub fn new(
        cyber_policy_log_path: PathBuf,
        cyber_policy_white_log_path: PathBuf,
    ) -> Result<Arc<Self>> {
        Self::with_capacity(cyber_policy_log_path, cyber_policy_white_log_path, 256)
    }

    pub fn with_capacity(
        cyber_policy_log_path: PathBuf,
        cyber_policy_white_log_path: PathBuf,
        queue_capacity: usize,
    ) -> Result<Arc<Self>> {
        if queue_capacity == 0 {
            anyhow::bail!("forensic writer queue capacity must be greater than zero");
        }
        let counts: HashMap<IpAddr, usize> = replay_cyber_policy_ip_counts(&cyber_policy_log_path)
            .with_context(|| {
                format!(
                    "failed to replay cyber_policy black_log at {}",
                    cyber_policy_log_path.display()
                )
            })?;
        let cyber_policy_ip_counts: Arc<DashMap<IpAddr, usize>> = Arc::new(DashMap::new());
        for (ip, count) in counts {
            cyber_policy_ip_counts.insert(ip, count);
        }
        let white_counts = replay_cyber_policy_white_key_counts(&cyber_policy_white_log_path)
            .with_context(|| {
                format!(
                    "failed to replay cyber_policy white_log at {}",
                    cyber_policy_white_log_path.display()
                )
            })?;
        let cyber_policy_white_key_counts: Arc<DashMap<String, usize>> =
            Arc::new(white_counts.into_iter().collect());
        let (sender, receiver) = mpsc::channel(queue_capacity);
        let task_handle = spawn_writer_task(receiver);
        let writer = Arc::new(Self {
            sender,
            cyber_policy_log_path,
            cyber_policy_ip_counts,
            cyber_policy_white_key_counts,
            cyber_policy_budget: MinuteBudget::new(),
            cyber_policy_white_budget: MinuteBudget::new(),
            bad_gateway_budget: MinuteBudget::new(),
            dropped_messages: AtomicU64::new(0),
            _task_handle: task_handle,
        });
        Ok(writer)
    }

    /// Enqueue a cyber-policy black record and return the up-to-date in-memory
    /// IP count. Returns `None` only when the background task has shut down,
    /// in which case nothing will be persisted (caller is expected to skip the
    /// IP-blacklist escalation).
    pub fn enqueue_cyber_policy_black<'a>(
        &self,
        entry: &CyberPolicyBlackRequestLog<'a>,
    ) -> Option<CyberPolicyBlackEnqueue> {
        let now = SystemTime::now();
        let path = self.cyber_policy_log_path.clone();
        let mut count_entry = self
            .cyber_policy_ip_counts
            .entry(entry.client_ip)
            .or_insert(0);
        *count_entry += 1;
        let client_ip_count = *count_entry;
        drop(count_entry);

        if !self
            .cyber_policy_budget
            .allow(entry.config.forensic_max_records_per_minute, now)
        {
            self.log_drop("cyber_policy_black", "minute_budget_exhausted");
            return Some(CyberPolicyBlackEnqueue {
                path,
                client_ip_count,
            });
        }

        let job = CyberPolicyBlackJob {
            config: Arc::new(entry.config.clone()),
            api_key: entry
                .api_key
                .forensic_snapshot(entry.config.forensic_include_api_key),
            method: entry.method.to_owned(),
            uri: entry.uri.clone(),
            path: entry.path.to_owned(),
            headers: entry.headers.clone(),
            original_body: bounded_copy(entry.original_body, entry.config.forensic_max_body_bytes),
            body: bounded_copy(entry.body, entry.config.forensic_max_body_bytes),
            client_ip: entry.client_ip,
            prompt_summary: entry.prompt_summary.clone(),
            queue_wait: entry.queue_wait,
            now,
        };
        self.try_enqueue(
            WriterMessage::CyberPolicy(Box::new(job)),
            "cyber_policy_black",
        );
        Some(CyberPolicyBlackEnqueue {
            path,
            client_ip_count,
        })
    }

    /// Enqueue a cyber-policy white record for a whitelisted key and return the
    /// trigger count for this key. Returns `None` if the background task has shut down.
    pub fn enqueue_cyber_policy_white<'a>(
        &self,
        entry: &CyberPolicyWhiteRequestLog<'a>,
    ) -> Option<usize> {
        let now = SystemTime::now();
        let key_hash = entry.api_key.hash().to_owned();
        let mut count_entry = self
            .cyber_policy_white_key_counts
            .entry(key_hash.clone())
            .or_insert(0);
        *count_entry += 1;
        let trigger_count = *count_entry;
        drop(count_entry);

        if !self
            .cyber_policy_white_budget
            .allow(entry.config.forensic_max_records_per_minute, now)
        {
            self.log_drop("cyber_policy_white", "minute_budget_exhausted");
            return Some(trigger_count);
        }

        let job = CyberPolicyWhiteJob {
            config: Arc::new(entry.config.clone()),
            api_key: entry
                .api_key
                .forensic_snapshot(entry.config.forensic_include_api_key),
            method: entry.method.to_owned(),
            uri: entry.uri.clone(),
            path: entry.path.to_owned(),
            headers: entry.headers.clone(),
            original_body: bounded_copy(entry.original_body, entry.config.forensic_max_body_bytes),
            body: bounded_copy(entry.body, entry.config.forensic_max_body_bytes),
            client_ip: entry.client_ip,
            prompt_summary: entry.prompt_summary.clone(),
            queue_wait: entry.queue_wait,
            trigger_count,
            now,
        };
        self.try_enqueue(
            WriterMessage::CyberPolicyWhite(Box::new(job)),
            "cyber_policy_white",
        );
        Some(trigger_count)
    }

    /// Enqueue a 502/503 forensic record. Failures are logged but never panic —
    /// the request path still returns its non-forensic response either way.
    pub fn enqueue_bad_gateway<'a>(&self, entry: &BadGatewayRequestLog<'a>) {
        let now = SystemTime::now();
        if !self
            .bad_gateway_budget
            .allow(entry.config.forensic_max_records_per_minute, now)
        {
            self.log_drop("bad_gateway_request_forensic", "minute_budget_exhausted");
            return;
        }
        let job = BadGatewayJob {
            config: Arc::new(entry.config.clone()),
            api_key: entry
                .api_key
                .forensic_snapshot(entry.config.forensic_include_api_key),
            method: entry.method.to_owned(),
            uri: entry.uri.clone(),
            path: entry.path.to_owned(),
            headers: entry.headers.clone(),
            original_body: bounded_copy(entry.original_body, entry.config.forensic_max_body_bytes),
            body: bounded_copy(entry.body, entry.config.forensic_max_body_bytes),
            client_ip: entry.client_ip,
            prompt_summary: entry.prompt_summary.clone(),
            queue_wait: entry.queue_wait,
            trigger: OwnedBadGatewayTrigger::from_borrowed(
                &entry.trigger,
                entry.config.forensic_max_body_bytes,
            ),
            now,
        };
        self.try_enqueue(
            WriterMessage::BadGateway(Box::new(job)),
            "bad_gateway_request_forensic",
        );
    }

    fn try_enqueue(&self, message: WriterMessage, target: &'static str) -> bool {
        match self.sender.try_send(message) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.log_drop(target, "queue_full");
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.log_drop(target, "writer_closed");
                false
            }
        }
    }

    fn log_drop(&self, target: &'static str, reason: &'static str) {
        let dropped = self.dropped_messages.fetch_add(1, Ordering::Relaxed) + 1;
        if dropped.is_power_of_two() {
            error!(
                event = "forensic_writer_record_dropped",
                target,
                reason,
                dropped_records = dropped,
                "forensic record dropped to preserve bounded resource usage"
            );
        }
    }

    /// Block until every previously-enqueued message has been processed.
    ///
    /// Used by tests that read back the on-disk file immediately after
    /// triggering a write.
    #[cfg(test)]
    pub async fn flush(&self) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if self.sender.send(WriterMessage::Flush(tx)).await.is_err() {
            return;
        }
        let _ = rx.await;
    }
}

fn spawn_writer_task(mut receiver: mpsc::Receiver<WriterMessage>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(message) = receiver.recv().await {
            // Each write is a short blocking syscall; spawn_blocking keeps
            // the writer task itself responsive and avoids parking a runtime
            // worker behind disk I/O.
            match message {
                WriterMessage::CyberPolicy(job) => {
                    let result =
                        tokio::task::spawn_blocking(move || perform_cyber_policy_write(&job)).await;
                    if let Err(join_error) = result {
                        error!(
                            event = "forensic_writer_join_error",
                            target = "cyber_policy_black",
                            error = %join_error,
                            "cyber_policy black_log writer task panicked"
                        );
                    } else if let Ok(Err(error)) = result {
                        error!(
                            event = "forensic_writer_io_error",
                            target = "cyber_policy_black",
                            error = %error,
                            "failed to write cyber_policy black_log record"
                        );
                    }
                }
                WriterMessage::CyberPolicyWhite(job) => {
                    let result =
                        tokio::task::spawn_blocking(move || perform_cyber_policy_white_write(&job))
                            .await;
                    if let Err(join_error) = result {
                        error!(
                            event = "forensic_writer_join_error",
                            target = "cyber_policy_white",
                            error = %join_error,
                            "cyber_policy white_log writer task panicked"
                        );
                    } else if let Ok(Err(error)) = result {
                        error!(
                            event = "forensic_writer_io_error",
                            target = "cyber_policy_white",
                            error = %error,
                            "failed to write cyber_policy white_log record"
                        );
                    }
                }
                WriterMessage::BadGateway(job) => {
                    let result =
                        tokio::task::spawn_blocking(move || perform_bad_gateway_write(&job)).await;
                    if let Err(join_error) = result {
                        error!(
                            event = "forensic_writer_join_error",
                            target = "bad_gateway_request_forensic",
                            error = %join_error,
                            "bad_gateway forensic writer task panicked"
                        );
                    } else if let Ok(Err(error)) = result {
                        error!(
                            event = "forensic_writer_io_error",
                            target = "bad_gateway_request_forensic",
                            error = %error,
                            "failed to write bad_gateway forensic log record"
                        );
                    }
                }
                #[cfg(test)]
                WriterMessage::Flush(ack) => {
                    let _ = ack.send(());
                }
            }
        }
    })
}

impl MinuteBudget {
    fn new() -> Self {
        Self {
            state: Mutex::new((0, 0)),
        }
    }

    fn allow(&self, max_records_per_minute: usize, now: SystemTime) -> bool {
        if max_records_per_minute == 0 {
            return false;
        }
        let minute = now
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            / 60;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.0 != minute {
            *state = (minute, 0);
        }
        if state.1 >= max_records_per_minute {
            return false;
        }
        state.1 += 1;
        true
    }
}

fn bounded_copy(bytes: &[u8], max_bytes: usize) -> Vec<u8> {
    bytes[..bytes.len().min(max_bytes)].to_vec()
}

fn perform_cyber_policy_write(job: &CyberPolicyBlackJob) -> Result<PathBuf> {
    let path = job.config.cyber_policy_black_log_file.clone();
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        create_private_dir_all(parent).with_context(|| {
            format!(
                "failed to create cyber_policy black_log directory {}",
                parent.display()
            )
        })?;
    }

    let entry = CyberPolicyBlackRequestLog {
        config: &job.config,
        api_key: &job.api_key,
        method: &job.method,
        uri: &job.uri,
        path: &job.path,
        headers: &job.headers,
        original_body: &job.original_body,
        body: &job.body,
        client_ip: job.client_ip,
        prompt_summary: &job.prompt_summary,
        queue_wait: job.queue_wait,
    };
    let (now_seconds, now_millis) = unix_timestamp_parts(job.now);
    let line = serde_json::to_string(&cyber_policy_black_request_json(
        &entry,
        now_seconds,
        now_millis,
    ))
    .context("failed to serialize cyber_policy black_log record")?;

    let mut file = open_private_append(&path)
        .with_context(|| format!("failed to open cyber_policy black_log {}", path.display()))?;
    writeln!(file, "{line}")
        .with_context(|| format!("failed to write cyber_policy black_log {}", path.display()))?;
    file.flush()
        .with_context(|| format!("failed to flush cyber_policy black_log {}", path.display()))?;
    Ok(path)
}

fn perform_cyber_policy_white_write(job: &CyberPolicyWhiteJob) -> Result<PathBuf> {
    let path = job.config.cyber_policy_white_log_file.clone();
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        create_private_dir_all(parent).with_context(|| {
            format!(
                "failed to create cyber_policy white_log directory {}",
                parent.display()
            )
        })?;
    }

    let entry = CyberPolicyWhiteRequestLog {
        config: &job.config,
        api_key: &job.api_key,
        method: &job.method,
        uri: &job.uri,
        path: &job.path,
        headers: &job.headers,
        original_body: &job.original_body,
        body: &job.body,
        client_ip: job.client_ip,
        prompt_summary: &job.prompt_summary,
        queue_wait: job.queue_wait,
        trigger_count: job.trigger_count,
    };
    let (now_seconds, now_millis) = unix_timestamp_parts(job.now);
    let line = serde_json::to_string(&cyber_policy_white_request_json(
        &entry,
        now_seconds,
        now_millis,
    ))
    .context("failed to serialize cyber_policy white_log record")?;

    let mut file = open_private_append(&path)
        .with_context(|| format!("failed to open cyber_policy white_log {}", path.display()))?;
    writeln!(file, "{line}")
        .with_context(|| format!("failed to write cyber_policy white_log {}", path.display()))?;
    file.flush()
        .with_context(|| format!("failed to flush cyber_policy white_log {}", path.display()))?;
    Ok(path)
}

fn perform_bad_gateway_write(job: &BadGatewayJob) -> Result<PathBuf> {
    let directory = upstream_error_log_dir(&job.config, job.trigger.upstream_status);
    create_private_dir_all(&directory).context("failed to create forensic log directory")?;
    let (now_seconds, now_millis) = unix_timestamp_parts(job.now);
    let path = directory.join(bad_gateway_request_log_filename(now_seconds));

    let trigger = job.trigger.as_borrowed();
    let entry = BadGatewayRequestLog {
        config: &job.config,
        api_key: &job.api_key,
        method: &job.method,
        uri: &job.uri,
        path: &job.path,
        headers: &job.headers,
        original_body: &job.original_body,
        body: &job.body,
        client_ip: job.client_ip,
        prompt_summary: &job.prompt_summary,
        queue_wait: job.queue_wait,
        trigger,
    };
    let line = serde_json::to_string(&bad_gateway_request_json(&entry, now_seconds, now_millis))
        .context("failed to serialize forensic request log")?;

    let mut file = open_private_append(&path)
        .with_context(|| format!("failed to open forensic log file {}", path.display()))?;
    writeln!(file, "{line}")
        .with_context(|| format!("failed to write forensic log file {}", path.display()))?;
    file.flush()
        .with_context(|| format!("failed to flush forensic log file {}", path.display()))?;
    Ok(path)
}

fn upstream_error_log_dir(config: &Config, upstream_status: Option<u16>) -> PathBuf {
    if upstream_status == Some(503) {
        return config.service_unavailable_log_dir.clone();
    }

    config.bad_gateway_log_dir.clone()
}

fn replay_cyber_policy_ip_counts(path: &Path) -> Result<HashMap<IpAddr, usize>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => {
            return Err(error).with_context(|| {
                format!("failed to open cyber_policy black_log {}", path.display())
            });
        }
    };

    let mut counts = HashMap::new();
    for line in BufReader::new(file).lines() {
        let line = line
            .with_context(|| format!("failed to read cyber_policy black_log {}", path.display()))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("event").and_then(Value::as_str) != Some("cyber_policy_black") {
            continue;
        }
        let Some(client_ip) = value
            .get("request")
            .and_then(|request| request.get("client_ip"))
            .and_then(Value::as_str)
            .and_then(|raw| raw.parse::<IpAddr>().ok())
        else {
            continue;
        };
        *counts.entry(client_ip).or_insert(0) += 1;
    }
    Ok(counts)
}

fn replay_cyber_policy_white_key_counts(path: &Path) -> Result<HashMap<String, usize>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => {
            return Err(error).with_context(|| {
                format!("failed to open cyber_policy white_log {}", path.display())
            });
        }
    };

    let mut counts = HashMap::new();
    for line in BufReader::new(file).lines() {
        let line = line
            .with_context(|| format!("failed to read cyber_policy white_log {}", path.display()))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("event").and_then(Value::as_str) != Some("cyber_policy_white") {
            continue;
        }
        let request = value.get("request");
        let Some(api_key_hash) = request
            .and_then(|request| request.get("api_key_hash"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| {
                request
                    .and_then(|request| request.get("api_key_full"))
                    .and_then(Value::as_str)
                    .map(crate::auth::hash_api_key)
            })
        else {
            continue;
        };
        *counts.entry(api_key_hash).or_insert(0) += 1;
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::ApiKeyInfo;
    use crate::config::CyberPolicyConfirmationRuleConfig;
    use axum::http::HeaderMap;
    use std::path::PathBuf;

    fn unique_temp_path(label: &str) -> PathBuf {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ai-bridge-forensic-{label}-{pid}-{nanos}.jsonl"))
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ai-bridge-forensic-{label}-{pid}-{nanos}"))
    }

    fn fixture_config(black_log: PathBuf) -> Config {
        Config {
            listen_addr: "127.0.0.1:0".parse().unwrap(),
            upstream_base_url: url::Url::parse("http://localhost:0").unwrap(),
            admin_session_secret: None,
            admin_login_url: "/#/login".to_owned(),
            external_auth_cookie_secure: false,
            config_reload_interval: None,
            auto_restart_interval: None,
            request_timeout: None,
            connect_timeout: Duration::from_secs(1),
            heartbeat_interval: Duration::from_secs(15),
            stream_event_stall_retry_enabled: true,
            stream_event_stall_timeout: Some(Duration::from_secs(60)),
            stream_event_stall_max_retries: 2,
            upstream_429_initial_delay: Duration::from_secs(2),
            upstream_429_max_delay: Duration::from_secs(30),
            upstream_429_max_retries: None,
            upstream_error_cooldown_enabled: true,
            upstream_502_cooldown_enabled: true,
            upstream_502_cooldown_accumulate_enabled: true,
            upstream_502_cooldown_delay: Duration::from_millis(1),
            upstream_502_max_retries: Some(3),
            auth_failure_cooldown_enabled: true,
            auth_failure_policy: crate::config::AuthFailurePolicy::Balanced,
            auth_failure_window: Duration::from_secs(60),
            auth_failure_threshold: 5,
            auth_failure_cooldown: Duration::from_secs(1800),
            auth_failure_ip_threshold: 20,
            auth_failure_max_entries: 10_000,
            auth_failure_error_body_max_bytes: 8 * 1024,
            rate_limiter_idle_ttl: Duration::from_secs(600),
            rate_limiter_cleanup_interval: Duration::from_secs(60),
            rate_limiter_max_entries: 10_000,
            max_body_bytes: 1024,
            max_upload_body_bytes: 8 * 1024,
            log_level: "info".to_owned(),
            log_format: crate::config::LogFormat::Json,
            log_file: None,
            bad_gateway_log_dir: unique_temp_dir("badgw"),
            service_unavailable_log_dir: unique_temp_dir("svcunavailable"),
            forensic_queue_capacity: 256,
            forensic_max_body_bytes: 256 * 1024,
            forensic_max_records_per_minute: 120,
            forensic_include_api_key: false,
            internal_header_prefix: "x-ai-bridge-".to_owned(),
            image_response_mode: crate::config::ImageResponseMode::Auto,
            debug_output: false,
            cyber_policy_blacklist_file: unique_temp_path("ckey"),
            cyber_policy_black_log_file: black_log,
            cyber_policy_ip_blacklist_file: unique_temp_path("cip"),
            cyber_policy_whitelist_file: unique_temp_path("cwhite"),
            cyber_policy_white_log_file: unique_temp_path("cwhitelog"),
            cyber_policy_ban_enabled: true,
            cyber_policy_confirmation_rules_file: None,
            cyber_policy_confirmation_rules: CyberPolicyConfirmationRuleConfig::default_rules(),
            responses_nonstream_chat_fallback: false,
            stream_early_heartbeat: false,
            stream_synthetic_first_space: false,
            filter_tools_by_model: std::collections::HashMap::new(),
            tool_filter_scan_limit: 1024,
        }
    }

    fn test_api_key() -> ApiKeyInfo {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer sk-forensic-test".parse().unwrap(),
        );
        ApiKeyInfo::from_headers(&headers)
    }

    #[tokio::test]
    async fn enqueue_cyber_policy_increments_in_memory_counter_and_persists_jsonl() {
        let path = unique_temp_path("enqueue-cp");
        let _ = std::fs::remove_file(&path);
        let white_path = unique_temp_path("enqueue-cp-white");
        let writer = ForensicWriter::new(path.clone(), white_path).unwrap();
        let config = fixture_config(path.clone());
        let api_key = test_api_key();
        let summary = PromptSummary::default();
        let headers = HeaderMap::new();
        let client_ip: IpAddr = "198.51.100.42".parse().unwrap();

        let entry = CyberPolicyBlackRequestLog {
            config: &config,
            api_key: &api_key,
            method: "POST",
            uri: &Uri::from_static("/v1/responses"),
            path: "/v1/responses",
            headers: &headers,
            original_body: b"{}",
            body: b"{}",
            client_ip,
            prompt_summary: &summary,
            queue_wait: Duration::ZERO,
        };

        let first = writer.enqueue_cyber_policy_black(&entry).unwrap();
        assert_eq!(first.client_ip_count, 1);
        let second = writer.enqueue_cyber_policy_black(&entry).unwrap();
        assert_eq!(second.client_ip_count, 2);
        let third = writer.enqueue_cyber_policy_black(&entry).unwrap();
        assert_eq!(third.client_ip_count, 3);

        writer.flush().await;
        let on_disk = std::fs::read_to_string(&path).unwrap();
        let line_count = on_disk.lines().count();
        assert_eq!(line_count, 3, "wrote one JSONL line per enqueue: {on_disk}");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn forensic_snapshot_is_truncated_and_redacts_api_key_by_default() {
        let path = unique_temp_path("bounded-redacted");
        let white_path = unique_temp_path("bounded-redacted-white");
        let writer = ForensicWriter::new(path.clone(), white_path).unwrap();
        let mut config = fixture_config(path.clone());
        config.forensic_max_body_bytes = 4;
        let api_key = test_api_key();
        let summary = PromptSummary::default();
        let headers = HeaderMap::new();
        let uri = Uri::from_static("/v1/responses");
        let entry = CyberPolicyBlackRequestLog {
            config: &config,
            api_key: &api_key,
            method: "POST",
            uri: &uri,
            path: "/v1/responses",
            headers: &headers,
            original_body: b"abcdefgh",
            body: b"ijklmnop",
            client_ip: "198.51.100.42".parse().unwrap(),
            prompt_summary: &summary,
            queue_wait: Duration::ZERO,
        };

        writer.enqueue_cyber_policy_black(&entry).unwrap();
        writer.flush().await;

        let on_disk = std::fs::read_to_string(&path).unwrap();
        let value: Value = serde_json::from_str(on_disk.lines().next().unwrap()).unwrap();
        assert_eq!(value["request"]["original_body"]["len"], 4);
        assert_eq!(value["request"]["original_body"]["utf8_lossy"], "abcd");
        assert_eq!(value["request"]["body"]["len"], 4);
        assert_eq!(value["request"]["body"]["utf8_lossy"], "ijkl");
        assert!(value["request"]["api_key_full"].is_null());
        assert_eq!(
            value["request"]["api_key_hash"],
            crate::auth::hash_api_key("sk-forensic-test")
        );
        assert!(!on_disk.contains("sk-forensic-test"));

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn forensic_snapshot_can_include_full_api_key_only_when_enabled() {
        let path = unique_temp_path("full-key-opt-in");
        let white_path = unique_temp_path("full-key-opt-in-white");
        let writer = ForensicWriter::new(path.clone(), white_path).unwrap();
        let mut config = fixture_config(path.clone());
        config.forensic_include_api_key = true;
        let api_key = test_api_key();
        let summary = PromptSummary::default();
        let headers = HeaderMap::new();
        let uri = Uri::from_static("/v1/responses");
        let entry = CyberPolicyBlackRequestLog {
            config: &config,
            api_key: &api_key,
            method: "POST",
            uri: &uri,
            path: "/v1/responses",
            headers: &headers,
            original_body: b"{}",
            body: b"{}",
            client_ip: "198.51.100.42".parse().unwrap(),
            prompt_summary: &summary,
            queue_wait: Duration::ZERO,
        };

        writer.enqueue_cyber_policy_black(&entry).unwrap();
        writer.flush().await;

        let on_disk = std::fs::read_to_string(&path).unwrap();
        let value: Value = serde_json::from_str(on_disk.lines().next().unwrap()).unwrap();
        assert_eq!(value["request"]["api_key_full"], "sk-forensic-test");
        assert!(value["request"]["api_key_masked"].is_null());
        assert!(value["request"]["api_key_hash"].is_null());

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn forensic_minute_budget_drops_excess_records_but_keeps_runtime_count() {
        let path = unique_temp_path("minute-budget");
        let white_path = unique_temp_path("minute-budget-white");
        let writer = ForensicWriter::new(path.clone(), white_path).unwrap();
        let mut config = fixture_config(path.clone());
        config.forensic_max_records_per_minute = 1;
        let api_key = test_api_key();
        let summary = PromptSummary::default();
        let headers = HeaderMap::new();
        let uri = Uri::from_static("/v1/responses");
        let entry = CyberPolicyBlackRequestLog {
            config: &config,
            api_key: &api_key,
            method: "POST",
            uri: &uri,
            path: "/v1/responses",
            headers: &headers,
            original_body: b"{}",
            body: b"{}",
            client_ip: "198.51.100.42".parse().unwrap(),
            prompt_summary: &summary,
            queue_wait: Duration::ZERO,
        };

        assert_eq!(
            writer
                .enqueue_cyber_policy_black(&entry)
                .unwrap()
                .client_ip_count,
            1
        );
        assert_eq!(
            writer
                .enqueue_cyber_policy_black(&entry)
                .unwrap()
                .client_ip_count,
            2
        );
        writer.flush().await;

        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert_eq!(on_disk.lines().count(), 1);

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn bounded_queue_rejects_new_records_when_full() {
        let (sender, _receiver) = mpsc::channel(1);
        let writer = ForensicWriter {
            sender,
            cyber_policy_log_path: unique_temp_path("queue-black"),
            cyber_policy_ip_counts: Arc::new(DashMap::new()),
            cyber_policy_white_key_counts: Arc::new(DashMap::new()),
            cyber_policy_budget: MinuteBudget::new(),
            cyber_policy_white_budget: MinuteBudget::new(),
            bad_gateway_budget: MinuteBudget::new(),
            dropped_messages: AtomicU64::new(0),
            _task_handle: tokio::spawn(async {}),
        };
        let (first, _first_rx) = tokio::sync::oneshot::channel();
        let (second, _second_rx) = tokio::sync::oneshot::channel();

        assert!(writer.try_enqueue(WriterMessage::Flush(first), "test"));
        assert!(!writer.try_enqueue(WriterMessage::Flush(second), "test"));
        assert_eq!(writer.dropped_messages.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn zero_queue_capacity_returns_error_instead_of_panicking() {
        assert!(
            ForensicWriter::with_capacity(
                unique_temp_path("zero-capacity-black"),
                unique_temp_path("zero-capacity-white"),
                0,
            )
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn newly_created_forensic_paths_use_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory = unique_temp_dir("private-mode");
        let path = directory.join("record.jsonl");
        create_private_dir_all(&directory).unwrap();
        drop(open_private_append(&path).unwrap());

        assert_eq!(
            std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let _ = std::fs::remove_dir_all(&directory);
    }

    #[tokio::test]
    async fn replay_seeds_counter_from_existing_black_log_file() {
        let path = unique_temp_path("replay");
        let _ = std::fs::remove_file(&path);
        let line = serde_json::json!({
            "event": "cyber_policy_black",
            "request": {"client_ip": "203.0.113.10"}
        })
        .to_string();
        let other_event = serde_json::json!({
            "event": "unrelated",
            "request": {"client_ip": "203.0.113.10"}
        })
        .to_string();
        std::fs::write(&path, format!("{line}\n{line}\n{other_event}\n")).unwrap();

        let white_path = unique_temp_path("replay-white");
        let writer = ForensicWriter::new(path.clone(), white_path).unwrap();
        let config = fixture_config(path.clone());
        let api_key = test_api_key();
        let summary = PromptSummary::default();
        let headers = HeaderMap::new();
        let client_ip: IpAddr = "203.0.113.10".parse().unwrap();

        let entry = CyberPolicyBlackRequestLog {
            config: &config,
            api_key: &api_key,
            method: "POST",
            uri: &Uri::from_static("/v1/responses"),
            path: "/v1/responses",
            headers: &headers,
            original_body: b"{}",
            body: b"{}",
            client_ip,
            prompt_summary: &summary,
            queue_wait: Duration::ZERO,
        };

        let result = writer.enqueue_cyber_policy_black(&entry).unwrap();
        // Two pre-existing matching records + the new enqueue = 3.
        assert_eq!(result.client_ip_count, 3);

        writer.flush().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn replay_seeds_counter_from_existing_white_log_file() {
        let black_path = unique_temp_path("replay-white-black");
        let white_path = unique_temp_path("replay-white");
        let _ = std::fs::remove_file(&black_path);
        let _ = std::fs::remove_file(&white_path);
        let line = serde_json::json!({
            "event": "cyber_policy_white",
            "request": {"api_key_full": "sk-forensic-test"}
        })
        .to_string();
        let other_event = serde_json::json!({
            "event": "cyber_policy_white",
            "request": {"api_key_full": "sk-other"}
        })
        .to_string();
        std::fs::write(&white_path, format!("{line}\n{line}\n{other_event}\n")).unwrap();

        let writer = ForensicWriter::new(black_path.clone(), white_path.clone()).unwrap();
        let mut config = fixture_config(black_path.clone());
        config.cyber_policy_white_log_file = white_path.clone();
        let api_key = test_api_key();
        let summary = PromptSummary::default();
        let headers = HeaderMap::new();
        let client_ip: IpAddr = "203.0.113.10".parse().unwrap();

        let entry = CyberPolicyWhiteRequestLog {
            config: &config,
            api_key: &api_key,
            method: "POST",
            uri: &Uri::from_static("/v1/responses"),
            path: "/v1/responses",
            headers: &headers,
            original_body: b"{}",
            body: b"{}",
            client_ip,
            prompt_summary: &summary,
            queue_wait: Duration::ZERO,
            trigger_count: 0,
        };

        let result = writer.enqueue_cyber_policy_white(&entry).unwrap();
        assert_eq!(result, 3);

        writer.flush().await;
        let _ = std::fs::remove_file(&black_path);
        let _ = std::fs::remove_file(&white_path);
    }
}
