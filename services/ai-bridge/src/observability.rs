use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use serde_json::{Value, json};

const EVENT_CAPACITY: usize = 1_000;
const EVENT_QUERY_LIMIT: usize = 200;
static GLOBAL: OnceLock<Arc<Observability>> = OnceLock::new();

#[derive(Debug)]
pub(crate) struct Observability {
    next_id: AtomicU64,
    requests_total: AtomicU64,
    responses_2xx: AtomicU64,
    responses_4xx: AtomicU64,
    responses_5xx: AtomicU64,
    upstream_retries: AtomicU64,
    admin_mutations: AtomicU64,
    reload_successes: AtomicU64,
    reload_failures: AtomicU64,
    events: Mutex<VecDeque<AdminEvent>>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AdminEvent {
    pub id: u64,
    pub timestamp_ms: u64,
    pub level: &'static str,
    pub event: String,
    pub fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AdminObservabilitySnapshot {
    pub requests_total: u64,
    pub responses_2xx: u64,
    pub responses_4xx: u64,
    pub responses_5xx: u64,
    pub upstream_retries: u64,
    pub admin_mutations: u64,
    pub reload_successes: u64,
    pub reload_failures: u64,
    pub event_capacity: usize,
    pub events: Vec<AdminEvent>,
}

impl Default for Observability {
    fn default() -> Self {
        Self::new()
    }
}

impl Observability {
    pub(crate) fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            requests_total: AtomicU64::new(0),
            responses_2xx: AtomicU64::new(0),
            responses_4xx: AtomicU64::new(0),
            responses_5xx: AtomicU64::new(0),
            upstream_retries: AtomicU64::new(0),
            admin_mutations: AtomicU64::new(0),
            reload_successes: AtomicU64::new(0),
            reload_failures: AtomicU64::new(0),
            events: Mutex::new(VecDeque::with_capacity(EVENT_CAPACITY)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_request(
        &self,
        method: &str,
        path: &str,
        status: u16,
        upstream_status: Option<u16>,
        latency_ms: u64,
        queue_wait_ms: u64,
        key_fingerprint: &str,
    ) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
        match status {
            200..=299 => self.responses_2xx.fetch_add(1, Ordering::Relaxed),
            400..=499 => self.responses_4xx.fetch_add(1, Ordering::Relaxed),
            500..=599 => self.responses_5xx.fetch_add(1, Ordering::Relaxed),
            _ => 0,
        };
        self.push(
            if status >= 500 {
                "error"
            } else if status >= 400 {
                "warn"
            } else {
                "info"
            },
            "request",
            BTreeMap::from([
                ("method".to_owned(), json!(bounded(method, 16))),
                ("path".to_owned(), json!(bounded(path, 256))),
                ("status".to_owned(), json!(status)),
                ("upstream_status".to_owned(), json!(upstream_status)),
                ("latency_ms".to_owned(), json!(latency_ms)),
                ("queue_wait_ms".to_owned(), json!(queue_wait_ms)),
                (
                    "key_fingerprint".to_owned(),
                    json!(bounded(key_fingerprint, 32)),
                ),
            ]),
        );
    }

    pub(crate) fn record_retry(
        &self,
        path: &str,
        status: u16,
        attempt: usize,
        reason: &str,
        delay_ms: u64,
        key_fingerprint: &str,
    ) {
        self.upstream_retries.fetch_add(1, Ordering::Relaxed);
        self.push(
            "warn",
            "upstream_retry",
            BTreeMap::from([
                ("path".to_owned(), json!(bounded(path, 256))),
                ("upstream_status".to_owned(), json!(status)),
                ("attempt".to_owned(), json!(attempt)),
                ("reason".to_owned(), json!(bounded(reason, 64))),
                ("delay_ms".to_owned(), json!(delay_ms)),
                (
                    "key_fingerprint".to_owned(),
                    json!(bounded(key_fingerprint, 32)),
                ),
            ]),
        );
    }

    pub(crate) fn record_admin(&self, action: &str, subject: &str, outcome: &str) {
        self.admin_mutations.fetch_add(1, Ordering::Relaxed);
        self.push(
            if outcome == "success" { "info" } else { "warn" },
            "admin_mutation",
            BTreeMap::from([
                ("action".to_owned(), json!(bounded(action, 64))),
                ("subject".to_owned(), json!(bounded(subject, 64))),
                ("outcome".to_owned(), json!(bounded(outcome, 32))),
            ]),
        );
    }

    pub(crate) fn record_reload(&self, success: bool, source: &str) {
        if success {
            self.reload_successes.fetch_add(1, Ordering::Relaxed);
        } else {
            self.reload_failures.fetch_add(1, Ordering::Relaxed);
        }
        self.push(
            if success { "info" } else { "error" },
            "config_reload",
            BTreeMap::from([
                ("source".to_owned(), json!(bounded(source, 32))),
                (
                    "outcome".to_owned(),
                    json!(if success { "success" } else { "failed" }),
                ),
            ]),
        );
    }

    pub(crate) fn snapshot(&self, limit: usize) -> AdminObservabilitySnapshot {
        let events = self
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .rev()
            .take(limit.min(EVENT_QUERY_LIMIT))
            .cloned()
            .collect();
        AdminObservabilitySnapshot {
            requests_total: self.requests_total.load(Ordering::Relaxed),
            responses_2xx: self.responses_2xx.load(Ordering::Relaxed),
            responses_4xx: self.responses_4xx.load(Ordering::Relaxed),
            responses_5xx: self.responses_5xx.load(Ordering::Relaxed),
            upstream_retries: self.upstream_retries.load(Ordering::Relaxed),
            admin_mutations: self.admin_mutations.load(Ordering::Relaxed),
            reload_successes: self.reload_successes.load(Ordering::Relaxed),
            reload_failures: self.reload_failures.load(Ordering::Relaxed),
            event_capacity: EVENT_CAPACITY,
            events,
        }
    }

    fn push(&self, level: &'static str, event: &str, fields: BTreeMap<String, Value>) {
        let entry = AdminEvent {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            timestamp_ms: unix_time_millis(),
            level,
            event: event.to_owned(),
            fields,
        };
        let mut events = self
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if events.len() == EVENT_CAPACITY {
            events.pop_front();
        }
        events.push_back(entry);
    }
}

pub(crate) fn install_global(observability: Arc<Observability>) {
    let _ = GLOBAL.set(observability);
}

pub(crate) fn global() -> Option<&'static Observability> {
    GLOBAL.get().map(Arc::as_ref)
}

fn unix_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn bounded(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_are_bounded_and_newest_first() {
        let observability = Observability::new();
        observability.record_admin("create", "whitelist", "success");
        observability.record_admin("delete", "blacklist", "success");
        let snapshot = observability.snapshot(1);
        assert_eq!(snapshot.admin_mutations, 2);
        assert_eq!(snapshot.events.len(), 1);
        assert_eq!(snapshot.events[0].fields["action"], "delete");
    }

    #[test]
    fn request_metrics_are_classified_by_status() {
        let observability = Observability::new();
        observability.record_request("POST", "/v1/responses", 502, Some(502), 10, 0, "abc");
        let snapshot = observability.snapshot(10);
        assert_eq!(snapshot.requests_total, 1);
        assert_eq!(snapshot.responses_5xx, 1);
        assert_eq!(snapshot.events[0].fields["key_fingerprint"], "abc");
    }
}
