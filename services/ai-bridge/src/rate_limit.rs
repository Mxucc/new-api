use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use dashmap::DashMap;
use serde::Serialize;
use tokio::{sync::Notify, task::JoinHandle, time};

use crate::auth::{admin_identity_fingerprint, admin_identity_id};

#[derive(Debug)]
pub struct RateLimiter {
    idle_ttl: Duration,
    cleanup_interval: Duration,
    max_entries: usize,
    entries: Arc<DashMap<String, Arc<KeyLimiter>>>,
    admission_lock: Mutex<()>,
    last_cleanup_millis: AtomicU64,
    rejected_admissions: AtomicU64,
    _cleanup_task: Option<JoinHandle<()>>,
}

#[derive(Debug)]
struct KeyLimiter {
    cooldown_until_millis: AtomicU64,
    cooldown_reason: Mutex<&'static str>,
    cooldown_changed: Notify,
    references: AtomicUsize,
    last_used_millis: AtomicU64,
}

#[derive(Debug)]
pub struct RateLimitPermit {
    entry: Option<Arc<KeyLimiter>>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct AdminRateLimitSnapshot {
    pub active: usize,
    pub tracked_keys: usize,
    pub cooldown_total: usize,
    pub cooldowns_truncated: bool,
    pub cooldowns: Vec<AdminRetryCooldown>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct AdminRetryCooldown {
    pub id: String,
    pub fingerprint: String,
    pub remaining_secs: u64,
    pub reason: &'static str,
}

struct EntryReference {
    entry: Option<Arc<KeyLimiter>>,
}

impl RateLimiter {
    #[cfg(test)]
    pub fn new(idle_ttl: Duration, cleanup_interval: Duration) -> Self {
        Self::with_max_entries(idle_ttl, cleanup_interval, 10_000)
    }

    pub fn with_max_entries(
        idle_ttl: Duration,
        cleanup_interval: Duration,
        max_entries: usize,
    ) -> Self {
        let entries = Arc::new(DashMap::new());
        let cleanup_entries = entries.clone();
        let cleanup_task = tokio::spawn(async move {
            loop {
                time::sleep(cleanup_interval).await;
                let now = now_millis();
                Self::cleanup_idle_entries_static(&cleanup_entries, idle_ttl, now);
            }
        });
        Self {
            idle_ttl,
            cleanup_interval,
            max_entries,
            entries,
            admission_lock: Mutex::new(()),
            last_cleanup_millis: AtomicU64::new(0),
            rejected_admissions: AtomicU64::new(0),
            _cleanup_task: Some(cleanup_task),
        }
    }

    pub fn try_acquire(&self, key: &str) -> Option<RateLimitPermit> {
        let Some(entry) = self.reference_entry(key) else {
            return Some(RateLimitPermit::untracked());
        };
        if !cooldown_delay_for_entry(entry.entry()).is_zero() {
            return None;
        }

        Some(entry.into_permit())
    }

    pub async fn acquire(&self, key: &str) -> RateLimitPermit {
        let Some(entry_reference) = self.reference_entry(key) else {
            return RateLimitPermit::untracked();
        };
        let entry = entry_reference.entry().clone();

        loop {
            let cooldown_changed = entry.cooldown_changed.notified();
            tokio::pin!(cooldown_changed);
            cooldown_changed.as_mut().enable();
            let delay = cooldown_delay_for_entry(&entry);
            if delay.is_zero() {
                return entry_reference.into_permit();
            }

            tokio::select! {
                () = time::sleep(delay) => {}
                () = &mut cooldown_changed => {}
            }
        }
    }

    pub async fn wait_for_retry_delay(&self, key: &str, delay: Duration) {
        if delay.is_zero() {
            return;
        }
        let Some(entry) = self.entries.get(key).map(|entry| entry.clone()) else {
            time::sleep(delay).await;
            return;
        };
        let cooldown_changed = entry.cooldown_changed.notified();
        tokio::pin!(cooldown_changed);
        cooldown_changed.as_mut().enable();
        if cooldown_delay_for_entry(&entry).is_zero() {
            time::sleep(delay).await;
            return;
        }
        tokio::select! {
            () = time::sleep(delay) => {}
            () = &mut cooldown_changed => {}
        }
    }

    pub fn force_acquire(&self, key: &str) -> RateLimitPermit {
        self.reference_entry(key)
            .map(EntryReference::into_permit)
            .unwrap_or_else(RateLimitPermit::untracked)
    }

    pub fn record_cooldown(&self, key: &str, delay: Duration, reason: &'static str) {
        if delay.is_zero() {
            return;
        }

        let Some(entry_reference) = self.reference_entry(key) else {
            return;
        };
        let cooldown_until = now_millis().saturating_add(duration_millis_u64(delay));
        let cooldown = &entry_reference.entry().cooldown_until_millis;
        let mut current = cooldown.load(Ordering::Acquire);

        while current < cooldown_until {
            match cooldown.compare_exchange(
                current,
                cooldown_until,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    *entry_reference
                        .entry()
                        .cooldown_reason
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = reason;
                    break;
                }
                Err(next) => current = next,
            }
        }
    }

    pub fn append_cooldown(&self, key: &str, delay: Duration, reason: &'static str) {
        if delay.is_zero() {
            return;
        }

        let Some(entry_reference) = self.reference_entry(key) else {
            return;
        };
        let delay_millis = duration_millis_u64(delay);
        let cooldown = &entry_reference.entry().cooldown_until_millis;
        let mut current = cooldown.load(Ordering::Acquire);

        loop {
            let now = now_millis();
            let base = current.max(now);
            let cooldown_until = base.saturating_add(delay_millis);

            match cooldown.compare_exchange(
                current,
                cooldown_until,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    *entry_reference
                        .entry()
                        .cooldown_reason
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = reason;
                    break;
                }
                Err(next) => current = next,
            }
        }
    }

    pub(crate) fn admin_snapshot(&self, limit: usize) -> AdminRateLimitSnapshot {
        self.admin_snapshot_at(limit, now_millis())
    }

    fn admin_snapshot_at(&self, limit: usize, now: u64) -> AdminRateLimitSnapshot {
        let mut active = 0_usize;
        let mut cooldowns = Vec::new();

        for entry in &*self.entries {
            active = active.saturating_add(entry.references.load(Ordering::Acquire));
            let cooldown_until = entry.cooldown_until_millis.load(Ordering::Acquire);
            if cooldown_until > now {
                cooldowns.push(AdminRetryCooldown {
                    id: admin_identity_id(entry.key()),
                    fingerprint: admin_identity_fingerprint(entry.key()),
                    remaining_secs: remaining_secs(now, cooldown_until),
                    reason: *entry
                        .cooldown_reason
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()),
                });
            }
        }

        cooldowns.sort_by(|left, right| {
            right
                .remaining_secs
                .cmp(&left.remaining_secs)
                .then_with(|| left.fingerprint.cmp(&right.fingerprint))
        });
        let cooldown_total = cooldowns.len();
        cooldowns.truncate(limit);

        AdminRateLimitSnapshot {
            active,
            tracked_keys: self.entries.len(),
            cooldown_total,
            cooldowns_truncated: cooldown_total > limit,
            cooldowns,
        }
    }

    pub(crate) fn clear_cooldown(&self, id: &str) -> bool {
        let now = now_millis();
        for entry in &*self.entries {
            if admin_identity_id(entry.key()) != id {
                continue;
            }
            let was_active = entry.cooldown_until_millis.swap(0, Ordering::AcqRel) > now;
            *entry
                .cooldown_reason
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = "";
            entry.cooldown_changed.notify_waiters();
            return was_active;
        }
        false
    }

    #[cfg(test)]
    pub fn cooldown_delay(&self, key: &str) -> Duration {
        self.maybe_cleanup_idle_entries();

        let Some(entry) = self.entries.get(key) else {
            return Duration::ZERO;
        };

        cooldown_delay_for_entry(&entry)
    }

    #[cfg(test)]
    pub fn tracked_key_count(&self) -> usize {
        self.entries.len()
    }

    fn reference_entry(&self, key: &str) -> Option<EntryReference> {
        self.maybe_cleanup_idle_entries();

        let now = now_millis();
        if let Some(entry) = self.entries.get(key) {
            entry.references.fetch_add(1, Ordering::AcqRel);
            entry.last_used_millis.store(now, Ordering::Release);
            return Some(EntryReference {
                entry: Some(entry.clone()),
            });
        }

        let _guard = self
            .admission_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(entry) = self.entries.get(key) {
            entry.references.fetch_add(1, Ordering::AcqRel);
            entry.last_used_millis.store(now, Ordering::Release);
            return Some(EntryReference {
                entry: Some(entry.clone()),
            });
        }
        if self.entries.len() >= self.max_entries {
            self.cleanup_idle_entries(now);
        }
        if self.entries.len() >= self.max_entries {
            let rejected = self.rejected_admissions.fetch_add(1, Ordering::Relaxed) + 1;
            if rejected.is_power_of_two() {
                tracing::warn!(
                    event = "rate_limiter_state_saturated",
                    max_entries = self.max_entries,
                    rejected_admissions = rejected,
                    "rate-limiter state is saturated; serving a new identity without retained cooldown state"
                );
            }
            return None;
        }

        let entry = self.entries.entry(key.to_owned()).or_insert_with(|| {
            let limiter = Arc::new(KeyLimiter::new(now));
            limiter.references.fetch_add(1, Ordering::AcqRel);
            limiter.last_used_millis.store(now, Ordering::Release);
            limiter
        });

        Some(EntryReference {
            entry: Some(entry.clone()),
        })
    }

    fn maybe_cleanup_idle_entries(&self) {
        let now = now_millis();
        let cleanup_interval_millis = duration_millis_u64(self.cleanup_interval);
        let last_cleanup = self.last_cleanup_millis.load(Ordering::Acquire);

        if now.saturating_sub(last_cleanup) < cleanup_interval_millis {
            return;
        }

        if self
            .last_cleanup_millis
            .compare_exchange(last_cleanup, now, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        self.cleanup_idle_entries(now);
    }

    fn cleanup_idle_entries(&self, now: u64) {
        Self::cleanup_idle_entries_static(&self.entries, self.idle_ttl, now);
    }

    fn cleanup_idle_entries_static(
        entries: &DashMap<String, Arc<KeyLimiter>>,
        idle_ttl: Duration,
        now: u64,
    ) {
        let idle_ttl_millis = duration_millis_u64(idle_ttl);
        entries.retain(|_, entry| {
            if entry.references.load(Ordering::Acquire) != 0 {
                return true;
            }

            if entry.cooldown_until_millis.load(Ordering::Acquire) > now {
                return true;
            }

            let last_used = entry.last_used_millis.load(Ordering::Acquire);
            now.saturating_sub(last_used) < idle_ttl_millis
        });
    }
}

impl KeyLimiter {
    fn new(now: u64) -> Self {
        Self {
            cooldown_until_millis: AtomicU64::new(0),
            cooldown_reason: Mutex::new(""),
            cooldown_changed: Notify::new(),
            references: AtomicUsize::new(0),
            last_used_millis: AtomicU64::new(now),
        }
    }
}

impl EntryReference {
    fn entry(&self) -> &Arc<KeyLimiter> {
        self.entry
            .as_ref()
            .expect("entry reference should be present until converted or dropped")
    }

    fn into_permit(mut self) -> RateLimitPermit {
        RateLimitPermit {
            entry: self.entry.take(),
        }
    }
}

impl RateLimitPermit {
    fn untracked() -> Self {
        Self { entry: None }
    }

    #[cfg(test)]
    fn is_tracked(&self) -> bool {
        self.entry.is_some()
    }
}

impl Drop for EntryReference {
    fn drop(&mut self) {
        if let Some(entry) = self.entry.take() {
            release_entry_reference(&entry);
        }
    }
}

impl Drop for RateLimitPermit {
    fn drop(&mut self) {
        if let Some(entry) = self.entry.take() {
            release_entry_reference(&entry);
        }
    }
}

impl Drop for RateLimiter {
    fn drop(&mut self) {
        if let Some(task) = self._cleanup_task.take() {
            task.abort();
        }
    }
}

fn release_entry_reference(entry: &KeyLimiter) {
    entry
        .last_used_millis
        .store(now_millis(), Ordering::Release);
    entry.references.fetch_sub(1, Ordering::AcqRel);
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn duration_millis_u64(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn cooldown_delay_for_entry(entry: &KeyLimiter) -> Duration {
    cooldown_delay_until(
        now_millis(),
        entry.cooldown_until_millis.load(Ordering::Acquire),
    )
}

fn cooldown_delay_until(now: u64, cooldown_until: u64) -> Duration {
    if cooldown_until <= now {
        return Duration::ZERO;
    }

    Duration::from_millis(cooldown_until - now)
}

fn remaining_secs(now: u64, until: u64) -> u64 {
    until.saturating_sub(now).saturating_add(999) / 1_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn records_cooldown_per_key() {
        let limiter = RateLimiter::new(Duration::from_millis(50), Duration::from_millis(1));
        limiter.record_cooldown("key-a", Duration::from_millis(20), "rate_limited");

        assert!(limiter.cooldown_delay("key-a") > Duration::ZERO);
        assert_eq!(limiter.cooldown_delay("key-b"), Duration::ZERO);
    }

    #[tokio::test]
    async fn clearing_cooldown_immediately_wakes_waiting_requests() {
        let limiter = Arc::new(RateLimiter::new(
            Duration::from_secs(60),
            Duration::from_secs(60),
        ));
        limiter.record_cooldown("key-a", Duration::from_secs(30), "rate_limited");
        let id = admin_identity_id("key-a");
        let waiting = {
            let limiter = limiter.clone();
            tokio::spawn(async move { limiter.acquire("key-a").await })
        };
        tokio::task::yield_now().await;

        assert!(limiter.clear_cooldown(&id));
        let permit = tokio::time::timeout(Duration::from_millis(100), waiting)
            .await
            .expect("cleared cooldown should wake the waiter")
            .unwrap();
        drop(permit);
    }

    #[tokio::test]
    async fn clearing_cooldown_interrupts_current_retry_delay() {
        let limiter = Arc::new(RateLimiter::new(
            Duration::from_secs(60),
            Duration::from_secs(60),
        ));
        limiter.record_cooldown("key-a", Duration::from_secs(30), "rate_limited");
        let waiting = {
            let limiter = limiter.clone();
            tokio::spawn(async move {
                limiter
                    .wait_for_retry_delay("key-a", Duration::from_secs(30))
                    .await;
            })
        };
        tokio::task::yield_now().await;

        assert!(limiter.clear_cooldown(&admin_identity_id("key-a")));
        tokio::time::timeout(Duration::from_millis(100), waiting)
            .await
            .expect("cleared cooldown should interrupt the retry delay")
            .unwrap();
    }

    #[tokio::test]
    async fn keeps_longer_cooldown_when_shorter_retry_arrives() {
        let limiter = RateLimiter::new(Duration::from_millis(50), Duration::from_millis(1));
        limiter.record_cooldown("key-a", Duration::from_millis(30), "rate_limited");
        limiter.record_cooldown("key-a", Duration::from_millis(5), "capacity");

        assert!(limiter.cooldown_delay("key-a") >= Duration::from_millis(20));
    }

    #[tokio::test]
    async fn append_cooldown_adds_each_retry_to_existing_window() {
        let limiter = RateLimiter::new(Duration::from_millis(100), Duration::from_millis(1));
        limiter.append_cooldown("key-a", Duration::from_millis(30), "bad_gateway");
        limiter.append_cooldown("key-a", Duration::from_millis(30), "bad_gateway");

        assert!(limiter.cooldown_delay("key-a") >= Duration::from_millis(55));
    }

    #[tokio::test]
    async fn cooldown_expires() {
        let limiter = RateLimiter::new(Duration::from_millis(50), Duration::from_millis(1));
        limiter.record_cooldown("key-a", Duration::from_millis(5), "rate_limited");

        tokio::time::sleep(Duration::from_millis(10)).await;

        assert_eq!(limiter.cooldown_delay("key-a"), Duration::ZERO);
    }

    #[tokio::test]
    async fn try_acquire_waits_only_for_key_in_cooldown() {
        let limiter = RateLimiter::new(Duration::from_millis(50), Duration::from_millis(1));
        limiter.record_cooldown("key-a", Duration::from_millis(5), "rate_limited");

        assert!(limiter.try_acquire("key-a").is_none());
        assert!(limiter.try_acquire("key-b").is_some());

        let started_at = std::time::Instant::now();
        let _permit = limiter.acquire("key-a").await;

        assert!(started_at.elapsed() >= Duration::from_millis(5));
    }

    #[tokio::test]
    async fn cleans_idle_entries_after_ttl() {
        let limiter = RateLimiter::new(Duration::from_millis(5), Duration::from_millis(1));
        limiter.record_cooldown("key-a", Duration::from_millis(1), "rate_limited");
        assert_eq!(limiter.tracked_key_count(), 1);

        tokio::time::sleep(Duration::from_millis(10)).await;
        limiter.record_cooldown("key-b", Duration::from_millis(1), "rate_limited");

        assert_eq!(limiter.tracked_key_count(), 1);
    }

    #[tokio::test]
    async fn keeps_active_cooldown_during_cleanup() {
        let limiter = RateLimiter::new(Duration::from_millis(5), Duration::from_millis(1));
        limiter.record_cooldown("key-a", Duration::from_millis(50), "rate_limited");

        tokio::time::sleep(Duration::from_millis(10)).await;
        limiter.record_cooldown("key-b", Duration::from_millis(1), "rate_limited");

        assert_eq!(limiter.tracked_key_count(), 2);
    }

    #[tokio::test]
    async fn concurrent_reference_and_cleanup_preserves_entries() {
        let limiter = Arc::new(RateLimiter::new(
            Duration::from_millis(10),
            Duration::from_millis(1),
        ));

        let limiter_clone = limiter.clone();
        let reference_task = tokio::spawn(async move {
            for i in 0..100 {
                let key = format!("key-{}", i % 10);
                let _entry = limiter_clone.reference_entry(&key);
                tokio::time::sleep(Duration::from_micros(100)).await;
            }
        });

        let limiter_clone = limiter.clone();
        let cleanup_task = tokio::spawn(async move {
            for _ in 0..50 {
                limiter_clone.maybe_cleanup_idle_entries();
                tokio::time::sleep(Duration::from_micros(200)).await;
            }
        });

        reference_task.await.unwrap();
        cleanup_task.await.unwrap();

        assert!(limiter.tracked_key_count() <= 10);
    }

    #[tokio::test]
    async fn background_cleanup_removes_idle_entries() {
        let limiter = RateLimiter::new(Duration::from_millis(10), Duration::from_millis(5));

        {
            let _permit_a = limiter.try_acquire("key-a").unwrap();
            let _permit_b = limiter.try_acquire("key-b").unwrap();
        }

        assert_eq!(limiter.tracked_key_count(), 2);

        tokio::time::timeout(Duration::from_millis(250), async {
            loop {
                if limiter.tracked_key_count() == 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("background cleanup should remove idle entries");

        assert_eq!(limiter.tracked_key_count(), 0);
    }

    #[tokio::test]
    async fn bounds_state_and_returns_untracked_permit_when_saturated() {
        let limiter =
            RateLimiter::with_max_entries(Duration::from_secs(60), Duration::from_secs(60), 2);
        let first = limiter.try_acquire("key-a").unwrap();
        let second = limiter.try_acquire("key-b").unwrap();
        assert!(first.is_tracked());
        assert!(second.is_tracked());

        let third = limiter.try_acquire("key-c").unwrap();
        assert!(!third.is_tracked());
        assert_eq!(limiter.tracked_key_count(), 2);
    }

    #[tokio::test]
    async fn admin_snapshot_reports_concurrency_and_masks_cooldown_identity() {
        let limiter = RateLimiter::new(Duration::from_secs(60), Duration::from_secs(60));
        let identity = format!("key:{}", "a".repeat(64));
        let permit = limiter.try_acquire(&identity).unwrap();
        limiter.record_cooldown(&identity, Duration::from_secs(30), "capacity");

        let snapshot = limiter.admin_snapshot(10);
        assert_eq!(snapshot.active, 1);
        assert_eq!(snapshot.tracked_keys, 1);
        assert_eq!(snapshot.cooldown_total, 1);
        assert_eq!(snapshot.cooldowns[0].fingerprint, "a".repeat(16));
        assert_eq!(snapshot.cooldowns[0].reason, "capacity");
        assert!(limiter.clear_cooldown(&snapshot.cooldowns[0].id));
        assert_eq!(limiter.admin_snapshot(10).cooldown_total, 0);
        assert!(
            !serde_json::to_string(&snapshot)
                .unwrap()
                .contains(&identity)
        );

        drop(permit);
        assert_eq!(limiter.admin_snapshot(10).active, 0);
    }
}
