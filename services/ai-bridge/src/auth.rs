use std::{
    borrow::Cow,
    fs,
    io::{self, BufRead, BufReader, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    sync::Mutex,
};

use axum::http::HeaderMap;
use dashmap::DashMap;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::private_file::{atomic_write_private, create_private_dir_all, open_private_append};

const MISSING_KEY_IDENTITY: &str = "__missing_api_key__";
static BLACKLIST_FILE_LOCK: Mutex<()> = Mutex::new(());

/// In-memory mirror of the cyber-policy blacklist files.
///
/// The two blacklist files (API key + client IP) are the source of truth so
/// they survive process restarts and can be edited by operators. The cache
/// avoids re-scanning them on every request — reads become lock-free DashMap
/// lookups, and updates write to disk + memory atomically. Config reload first
/// prepares a full disk snapshot, then applies it only after every fallible
/// reload step has succeeded.
#[derive(Debug)]
pub struct BlacklistCache {
    mutation_lock: Mutex<()>,
    api_key_path: Mutex<PathBuf>,
    api_key_entries: DashMap<String, ()>,
    client_ip_path: Mutex<PathBuf>,
    client_ip_entries: DashMap<IpAddr, ()>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct AdminApiKeyEntry {
    pub id: String,
    pub masked: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct AdminBlacklistSnapshot {
    pub total_api_keys: usize,
    pub total_client_ips: usize,
    pub api_keys_truncated: bool,
    pub client_ips_truncated: bool,
    pub api_keys: Vec<AdminApiKeyEntry>,
    pub client_ips: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct AdminWhitelistSnapshot {
    pub total_api_keys: usize,
    pub api_keys_truncated: bool,
    pub api_keys: Vec<AdminApiKeyEntry>,
}

#[derive(Debug)]
pub(crate) struct BlacklistCacheUpdate {
    api_key_path: PathBuf,
    api_key_entries: Vec<String>,
    client_ip_path: PathBuf,
    client_ip_entries: Vec<IpAddr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdminListMutationResult {
    Created,
    Updated,
    Deleted,
    AlreadyExists,
    NotFound,
}

impl BlacklistCache {
    pub fn load(api_key_path: PathBuf, client_ip_path: PathBuf) -> io::Result<Self> {
        let api_key_entries = DashMap::new();
        for raw in read_blacklist_lines(&api_key_path)? {
            api_key_entries.insert(raw, ());
        }
        let client_ip_entries = DashMap::new();
        for raw in read_blacklist_lines(&client_ip_path)? {
            if let Ok(ip) = raw.parse::<IpAddr>() {
                client_ip_entries.insert(ip, ());
            }
        }

        Ok(Self {
            mutation_lock: Mutex::new(()),
            api_key_path: Mutex::new(api_key_path),
            api_key_entries,
            client_ip_path: Mutex::new(client_ip_path),
            client_ip_entries,
        })
    }

    #[cfg(test)]
    pub fn sync_from_disk(&self, api_key_path: &Path, client_ip_path: &Path) -> io::Result<()> {
        let update = self.prepare_sync_from_disk(api_key_path, client_ip_path)?;
        self.apply_sync_from_disk(update);
        Ok(())
    }

    pub(crate) fn prepare_sync_from_disk(
        &self,
        api_key_path: &Path,
        client_ip_path: &Path,
    ) -> io::Result<BlacklistCacheUpdate> {
        let api_key_entries =
            read_blacklist_lines_for_sync(api_key_path, self.api_key_entries.is_empty())?;
        let client_ip_entries =
            read_blacklist_lines_for_sync(client_ip_path, self.client_ip_entries.is_empty())?
                .into_iter()
                .filter_map(|raw| raw.parse::<IpAddr>().ok())
                .collect();

        Ok(BlacklistCacheUpdate {
            api_key_path: api_key_path.to_owned(),
            api_key_entries,
            client_ip_path: client_ip_path.to_owned(),
            client_ip_entries,
        })
    }

    pub(crate) fn apply_sync_from_disk(&self, update: BlacklistCacheUpdate) {
        replace_string_set(&self.api_key_entries, update.api_key_entries);
        replace_ip_set(&self.client_ip_entries, update.client_ip_entries);
        *self
            .api_key_path
            .lock()
            .expect("blacklist API key path lock should not be poisoned") = update.api_key_path;
        *self
            .client_ip_path
            .lock()
            .expect("blacklist client IP path lock should not be poisoned") = update.client_ip_path;
    }

    pub fn api_key_is_blacklisted(&self, api_key: &ApiKeyInfo) -> bool {
        api_key
            .raw_secret_for_blacklist()
            .is_some_and(|raw| self.api_key_entries.contains_key(raw))
    }

    pub fn client_ip_is_blacklisted(&self, client_ip: IpAddr) -> bool {
        self.client_ip_entries.contains_key(&client_ip)
    }

    pub(crate) fn admin_snapshot(&self, limit: usize) -> AdminBlacklistSnapshot {
        let total_api_keys = self.api_key_entries.len();
        let total_client_ips = self.client_ip_entries.len();
        let mut api_keys = self
            .api_key_entries
            .iter()
            .map(|entry| admin_api_key_entry(entry.key()))
            .collect::<Vec<_>>();
        let mut client_ips = self
            .client_ip_entries
            .iter()
            .map(|entry| entry.key().to_string())
            .collect::<Vec<_>>();
        api_keys.sort_by(|left, right| left.fingerprint.cmp(&right.fingerprint));
        client_ips.sort();
        api_keys.truncate(limit);
        client_ips.truncate(limit);

        AdminBlacklistSnapshot {
            total_api_keys,
            total_client_ips,
            api_keys_truncated: total_api_keys > limit,
            client_ips_truncated: total_client_ips > limit,
            api_keys,
            client_ips,
        }
    }

    pub fn append_api_key(&self, api_key: &ApiKeyInfo) -> io::Result<Option<bool>> {
        let Some(raw_key) = api_key.raw_secret_for_blacklist() else {
            return Ok(None);
        };
        let trimmed = raw_key.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        let _mutation = self
            .mutation_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.api_key_entries.contains_key(trimmed) {
            return Ok(Some(false));
        }

        let path = self
            .api_key_path
            .lock()
            .expect("blacklist API key path lock should not be poisoned")
            .clone();
        let inserted = append_raw_key_to_blacklist(&path, trimmed)?;
        if inserted {
            self.api_key_entries.insert(trimmed.to_owned(), ());
        }
        Ok(Some(inserted))
    }

    pub fn append_client_ip(&self, client_ip: IpAddr) -> io::Result<bool> {
        let _mutation = self
            .mutation_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.client_ip_entries.contains_key(&client_ip) {
            return Ok(false);
        }

        let path = self
            .client_ip_path
            .lock()
            .expect("blacklist client IP path lock should not be poisoned")
            .clone();
        let inserted = append_raw_key_to_blacklist(&path, &client_ip.to_string())?;
        if inserted {
            self.client_ip_entries.insert(client_ip, ());
        }
        Ok(inserted)
    }

    pub(crate) fn create_api_key(&self, raw: &str) -> io::Result<AdminListMutationResult> {
        self.mutate_api_keys(ListMutation::Create(raw))
    }

    pub(crate) fn update_api_key(
        &self,
        id: &str,
        raw: &str,
    ) -> io::Result<AdminListMutationResult> {
        self.mutate_api_keys(ListMutation::Update { id, value: raw })
    }

    pub(crate) fn delete_api_key(&self, id: &str) -> io::Result<AdminListMutationResult> {
        self.mutate_api_keys(ListMutation::Delete(id))
    }

    pub(crate) fn create_client_ip(&self, ip: IpAddr) -> io::Result<AdminListMutationResult> {
        let value = ip.to_string();
        self.mutate_client_ips(ListMutation::Create(&value))
    }

    pub(crate) fn update_client_ip(
        &self,
        current: IpAddr,
        replacement: IpAddr,
    ) -> io::Result<AdminListMutationResult> {
        let current = current.to_string();
        let replacement = replacement.to_string();
        self.mutate_client_ips(ListMutation::Update {
            id: &current,
            value: &replacement,
        })
    }

    pub(crate) fn delete_client_ip(&self, ip: IpAddr) -> io::Result<AdminListMutationResult> {
        let value = ip.to_string();
        self.mutate_client_ips(ListMutation::Delete(&value))
    }

    fn mutate_api_keys(&self, mutation: ListMutation<'_>) -> io::Result<AdminListMutationResult> {
        let _mutation = self
            .mutation_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = self
            .api_key_path
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let (result, entries) = mutate_list_file(&path, mutation, true)?;
        if result.changed() {
            replace_string_set(&self.api_key_entries, entries);
        }
        Ok(result)
    }

    fn mutate_client_ips(&self, mutation: ListMutation<'_>) -> io::Result<AdminListMutationResult> {
        let _mutation = self
            .mutation_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = self
            .client_ip_path
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let (result, entries) = mutate_list_file(&path, mutation, false)?;
        if result.changed() {
            replace_ip_set(
                &self.client_ip_entries,
                entries
                    .into_iter()
                    .filter_map(|entry| entry.parse::<IpAddr>().ok()),
            );
        }
        Ok(result)
    }

    #[cfg(test)]
    pub fn api_key_entry_count(&self) -> usize {
        self.api_key_entries.len()
    }

    #[cfg(test)]
    pub fn client_ip_entry_count(&self) -> usize {
        self.client_ip_entries.len()
    }
}

#[derive(Debug)]
pub struct WhitelistCache {
    mutation_lock: Mutex<()>,
    api_key_path: Mutex<PathBuf>,
    api_key_entries: DashMap<String, ()>,
}

#[derive(Debug)]
pub(crate) struct WhitelistCacheUpdate {
    api_key_path: PathBuf,
    api_key_entries: Vec<String>,
}

impl WhitelistCache {
    pub fn load(api_key_path: &Path) -> io::Result<Self> {
        let api_key_entries = DashMap::new();
        for raw in read_blacklist_lines(api_key_path)? {
            api_key_entries.insert(raw, ());
        }
        Ok(Self {
            mutation_lock: Mutex::new(()),
            api_key_path: Mutex::new(api_key_path.to_owned()),
            api_key_entries,
        })
    }

    #[cfg(test)]
    pub fn sync_from_disk(&self, api_key_path: &Path) -> io::Result<()> {
        let update = self.prepare_sync_from_disk(api_key_path)?;
        self.apply_sync_from_disk(update);
        Ok(())
    }

    pub(crate) fn prepare_sync_from_disk(
        &self,
        api_key_path: &Path,
    ) -> io::Result<WhitelistCacheUpdate> {
        let api_key_entries =
            read_blacklist_lines_for_sync(api_key_path, self.api_key_entries.is_empty())?;
        Ok(WhitelistCacheUpdate {
            api_key_path: api_key_path.to_owned(),
            api_key_entries,
        })
    }

    pub(crate) fn apply_sync_from_disk(&self, update: WhitelistCacheUpdate) {
        replace_string_set(&self.api_key_entries, update.api_key_entries);
        *self
            .api_key_path
            .lock()
            .expect("whitelist API key path lock should not be poisoned") = update.api_key_path;
    }

    pub fn api_key_is_whitelisted(&self, api_key: &ApiKeyInfo) -> bool {
        api_key
            .raw_secret_for_blacklist()
            .is_some_and(|raw| self.api_key_entries.contains_key(raw))
    }

    pub(crate) fn admin_snapshot(&self, limit: usize) -> AdminWhitelistSnapshot {
        let total_api_keys = self.api_key_entries.len();
        let mut api_keys = self
            .api_key_entries
            .iter()
            .map(|entry| admin_api_key_entry(entry.key()))
            .collect::<Vec<_>>();
        api_keys.sort_by(|left, right| left.fingerprint.cmp(&right.fingerprint));
        api_keys.truncate(limit);

        AdminWhitelistSnapshot {
            total_api_keys,
            api_keys_truncated: total_api_keys > limit,
            api_keys,
        }
    }

    pub fn api_key_is_whitelisted_in_file(
        &self,
        api_key_path: &Path,
        api_key: &ApiKeyInfo,
    ) -> io::Result<bool> {
        let Some(raw_key) = api_key.raw_secret_for_blacklist() else {
            return Ok(false);
        };

        blacklist_file_contains_key(api_key_path, raw_key)
    }

    pub(crate) fn create_api_key(&self, raw: &str) -> io::Result<AdminListMutationResult> {
        self.mutate_api_keys(ListMutation::Create(raw))
    }

    pub(crate) fn update_api_key(
        &self,
        id: &str,
        raw: &str,
    ) -> io::Result<AdminListMutationResult> {
        self.mutate_api_keys(ListMutation::Update { id, value: raw })
    }

    pub(crate) fn delete_api_key(&self, id: &str) -> io::Result<AdminListMutationResult> {
        self.mutate_api_keys(ListMutation::Delete(id))
    }

    fn mutate_api_keys(&self, mutation: ListMutation<'_>) -> io::Result<AdminListMutationResult> {
        let _mutation = self
            .mutation_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = self
            .api_key_path
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let (result, entries) = mutate_list_file(&path, mutation, true)?;
        if result.changed() {
            replace_string_set(&self.api_key_entries, entries);
        }
        Ok(result)
    }
}

#[derive(Debug, Clone)]
pub struct ApiKeyInfo {
    raw: Option<String>,
    // 使用 Cow 避免 missing key 场景的字符串分配
    // 正常请求有 API key 时分配 String，missing key 时使用静态字符串引用
    identity: Cow<'static, str>,
    masked: Cow<'static, str>,
    hash: Cow<'static, str>,
}

impl ApiKeyInfo {
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let raw = extract_bearer_token(headers);
        let identity = raw
            .as_ref()
            .map(|s| Cow::Owned(format!("key:{}", hash_api_key_full(s))))
            .unwrap_or_else(|| Cow::Borrowed(MISSING_KEY_IDENTITY));
        let masked = raw
            .as_deref()
            .map(|s| Cow::Owned(mask_api_key(s)))
            .unwrap_or_else(|| Cow::Borrowed("missing"));
        let hash = raw
            .as_deref()
            .map(|s| Cow::Owned(hash_api_key(s)))
            .unwrap_or_else(|| Cow::Borrowed("missing"));

        Self {
            raw,
            identity,
            masked,
            hash,
        }
    }

    pub fn from_headers_with_fallback(headers: &HeaderMap, client_ip: IpAddr) -> Self {
        let mut info = Self::from_headers(headers);
        if info.raw.is_some() {
            return info;
        }

        info.identity = Cow::Owned(missing_key_fallback_identity(headers, client_ip));
        info
    }

    pub fn identity(&self) -> &str {
        &self.identity
    }

    pub fn masked(&self) -> &str {
        &self.masked
    }

    pub fn hash(&self) -> &str {
        &self.hash
    }

    pub fn is_present(&self) -> bool {
        self.raw.is_some()
    }

    pub(crate) fn raw_secret_for_blacklist(&self) -> Option<&str> {
        self.raw.as_deref()
    }

    pub(crate) fn raw_secret_for_forensic_log(&self) -> Option<&str> {
        self.raw.as_deref()
    }

    pub(crate) fn forensic_snapshot(&self, include_raw_key: bool) -> Self {
        let mut snapshot = self.clone();
        if !include_raw_key {
            snapshot.raw = None;
        }
        snapshot
    }
}

pub fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .trim();
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }

    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_owned())
    }
}

fn missing_key_fallback_identity(headers: &HeaderMap, client_ip: IpAddr) -> String {
    if let Some(user_id) = visible_header_value(headers, "new-api-user") {
        return format!("new-api-user:{}", hash_api_key_full(&user_id));
    }

    if let Some(cookie) = visible_header_value(headers, axum::http::header::COOKIE.as_str()) {
        return format!("cookie:{}", hash_api_key_full(&cookie));
    }

    format!("ip:{client_ip}")
}

fn visible_header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub fn mask_api_key(api_key: &str) -> String {
    if api_key.is_ascii() {
        if api_key.len() <= 11 {
            return "***".to_owned();
        }
        let prefix_len = 7;
        let suffix_len = 4;
        let prefix = &api_key[..prefix_len];
        let suffix = &api_key[api_key.len() - suffix_len..];
        return format!("{prefix}...{suffix}");
    }

    let char_count = api_key.chars().count();
    if char_count <= 11 {
        return "***".to_owned();
    }
    let prefix = api_key.chars().take(7).collect::<String>();
    let suffix = api_key.chars().skip(char_count - 4).collect::<String>();
    format!("{prefix}...{suffix}")
}

pub fn hash_api_key(api_key: &str) -> String {
    let digest = Sha256::digest(api_key.as_bytes());
    hex::encode(&digest[..8])
}

pub(crate) fn admin_identity_fingerprint(identity: &str) -> String {
    let value = identity.strip_prefix("key:").unwrap_or(identity);
    if value.len() >= 16 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return value[..16].to_ascii_lowercase();
    }
    hash_api_key(identity)
}

pub(crate) fn admin_identity_id(identity: &str) -> String {
    hash_api_key_full(identity)
}

fn admin_api_key_entry(api_key: &str) -> AdminApiKeyEntry {
    AdminApiKeyEntry {
        id: hash_api_key_full(api_key),
        masked: mask_api_key(api_key),
        fingerprint: hash_api_key(api_key),
    }
}

fn hash_api_key_full(api_key: &str) -> String {
    hex::encode(Sha256::digest(api_key.as_bytes()))
}

impl AdminListMutationResult {
    fn changed(self) -> bool {
        matches!(self, Self::Created | Self::Updated | Self::Deleted)
    }
}

enum ListMutation<'a> {
    Create(&'a str),
    Update { id: &'a str, value: &'a str },
    Delete(&'a str),
}

fn mutate_list_file(
    path: &Path,
    mutation: ListMutation<'_>,
    id_is_hash: bool,
) -> io::Result<(AdminListMutationResult, Vec<String>)> {
    const MAX_ADMIN_LIST_BYTES: usize = 16 * 1024 * 1024;
    let _file_guard = BLACKLIST_FILE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    if content.len() > MAX_ADMIN_LIST_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "list file is too large for administrator mutation",
        ));
    }

    let mut lines = content.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
    let identity_matches = |entry: &str, id: &str| {
        if id_is_hash {
            hash_api_key_full(entry) == id
        } else {
            entry == id
        }
    };

    let result = match mutation {
        ListMutation::Create(value) => {
            if list_entries(&lines).iter().any(|entry| entry == value) {
                AdminListMutationResult::AlreadyExists
            } else {
                lines.push(value.to_owned());
                AdminListMutationResult::Created
            }
        }
        ListMutation::Update { id, value } => {
            let Some(index) = lines.iter().position(|line| {
                blacklist_line_key(line).is_some_and(|entry| identity_matches(entry, id))
            }) else {
                return Ok((AdminListMutationResult::NotFound, list_entries(&lines)));
            };
            if list_entries(&lines)
                .iter()
                .any(|entry| entry == value && !identity_matches(entry, id))
            {
                AdminListMutationResult::AlreadyExists
            } else {
                lines[index] = value.to_owned();
                AdminListMutationResult::Updated
            }
        }
        ListMutation::Delete(id) => {
            let original_len = lines.len();
            lines.retain(|line| {
                !blacklist_line_key(line).is_some_and(|entry| identity_matches(entry, id))
            });
            if lines.len() == original_len {
                AdminListMutationResult::NotFound
            } else {
                AdminListMutationResult::Deleted
            }
        }
    };

    if result.changed() {
        let mut output = lines.join("\n");
        if !output.is_empty() {
            output.push('\n');
        }
        atomic_write_private(path, output.as_bytes())?;
    }
    Ok((result, list_entries(&lines)))
}

fn list_entries(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|line| blacklist_line_key(line).map(ToOwned::to_owned))
        .collect()
}

#[cfg(test)]
fn api_key_is_blacklisted(path: &Path, api_key: &ApiKeyInfo) -> io::Result<bool> {
    let Some(raw_key) = api_key.raw_secret_for_blacklist() else {
        return Ok(false);
    };

    blacklist_file_contains_key(path, raw_key)
}

#[cfg(test)]
fn append_api_key_to_blacklist(path: &Path, api_key: &ApiKeyInfo) -> io::Result<Option<bool>> {
    let Some(raw_key) = api_key.raw_secret_for_blacklist() else {
        return Ok(None);
    };

    append_raw_key_to_blacklist(path, raw_key).map(Some)
}

#[cfg(test)]
fn client_ip_is_blacklisted(path: &Path, client_ip: IpAddr) -> io::Result<bool> {
    let ip = client_ip.to_string();
    blacklist_file_contains_key(path, &ip)
}

#[cfg(test)]
fn append_client_ip_to_blacklist(path: &Path, client_ip: IpAddr) -> io::Result<bool> {
    let ip = client_ip.to_string();
    append_raw_key_to_blacklist(path, &ip)
}

fn blacklist_file_contains_key(path: &Path, raw_key: &str) -> io::Result<bool> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };

    for line in BufReader::new(file).lines() {
        if blacklist_line_key(&line?) == Some(raw_key) {
            return Ok(true);
        }
    }

    Ok(false)
}

fn append_raw_key_to_blacklist(path: &Path, raw_key: &str) -> io::Result<bool> {
    let raw_key = raw_key.trim();
    if raw_key.is_empty() {
        return Ok(false);
    }

    let _guard = BLACKLIST_FILE_LOCK
        .lock()
        .expect("blacklist file lock should not be poisoned");

    if blacklist_file_contains_key(path, raw_key)? {
        return Ok(false);
    }

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        create_private_dir_all(parent)?;
    }

    let needs_leading_newline = match fs::read(path) {
        Ok(bytes) => !bytes.is_empty() && !bytes.ends_with(b"\n"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error),
    };

    let mut file = open_private_append(path)?;
    if needs_leading_newline {
        writeln!(file)?;
    }
    writeln!(file, "{raw_key}")?;
    Ok(true)
}

fn blacklist_line_key(line: &str) -> Option<&str> {
    let line = line.trim();
    let line = line.strip_prefix('\u{feff}').unwrap_or(line).trim();
    if line.is_empty() || line.starts_with('#') {
        None
    } else {
        Some(line)
    }
}

fn read_blacklist_lines(path: &Path) -> io::Result<Vec<String>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    read_blacklist_lines_from_reader(file)
}

fn read_blacklist_lines_for_sync(
    path: &Path,
    current_entries_empty: bool,
) -> io::Result<Vec<String>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound && current_entries_empty => {
            return Ok(Vec::new());
        }
        Err(error) => return Err(error),
    };

    read_blacklist_lines_from_reader(file)
}

fn read_blacklist_lines_from_reader(file: fs::File) -> io::Result<Vec<String>> {
    let mut entries = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if let Some(key) = blacklist_line_key(&line) {
            entries.push(key.to_owned());
        }
    }
    Ok(entries)
}

fn replace_string_set(set: &DashMap<String, ()>, entries: Vec<String>) {
    let parsed: std::collections::HashSet<String> = entries.into_iter().collect();
    for entry in &parsed {
        set.entry(entry.clone()).or_insert(());
    }
    set.retain(|key, _| parsed.contains(key));
}

fn replace_ip_set(set: &DashMap<IpAddr, ()>, entries: impl IntoIterator<Item = IpAddr>) {
    let parsed: std::collections::HashSet<IpAddr> = entries.into_iter().collect();
    for ip in &parsed {
        set.entry(*ip).or_insert(());
    }
    set.retain(|key, _| parsed.contains(key));
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn extracts_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer sk-test-123"),
        );

        assert_eq!(
            extract_bearer_token(&headers),
            Some("sk-test-123".to_owned())
        );
    }

    #[test]
    fn bearer_token_identity_takes_precedence_over_fallback_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer sk-test-123"),
        );
        headers.insert("new-api-user", HeaderValue::from_static("42"));

        let api_key =
            ApiKeyInfo::from_headers_with_fallback(&headers, "198.51.100.7".parse().unwrap());

        assert!(api_key.identity().starts_with("key:"));
        assert!(!api_key.identity().contains("sk-test-123"));
        assert!(api_key.is_present());
    }

    #[test]
    fn missing_bearer_identity_uses_new_api_user_header() {
        let mut headers = HeaderMap::new();
        headers.insert("new-api-user", HeaderValue::from_static("42"));

        let api_key =
            ApiKeyInfo::from_headers_with_fallback(&headers, "198.51.100.7".parse().unwrap());

        assert!(api_key.identity().starts_with("new-api-user:"));
        assert!(!api_key.identity().contains("42"));
        assert!(!api_key.is_present());
        assert_eq!(api_key.masked(), "missing");
        assert_eq!(api_key.hash(), "missing");
    }

    #[test]
    fn missing_bearer_identity_hashes_cookie_before_ip_fallback() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            HeaderValue::from_static("new-api-session=abc"),
        );

        let first =
            ApiKeyInfo::from_headers_with_fallback(&headers, "198.51.100.7".parse().unwrap());
        let second =
            ApiKeyInfo::from_headers_with_fallback(&headers, "198.51.100.9".parse().unwrap());

        assert_eq!(first.identity(), second.identity());
        assert!(first.identity().starts_with("cookie:"));
        assert!(!first.identity().contains("new-api-session=abc"));
        assert!(!first.is_present());
    }

    #[test]
    fn missing_bearer_identity_falls_back_to_client_ip() {
        let headers = HeaderMap::new();

        let api_key =
            ApiKeyInfo::from_headers_with_fallback(&headers, "198.51.100.7".parse().unwrap());

        assert_eq!(api_key.identity(), "ip:198.51.100.7");
        assert!(!api_key.is_present());
    }

    #[test]
    fn unicode_api_key_masking_does_not_slice_inside_utf8_character() {
        let key = "sk-\u{51ed}\u{8bc1}-\u{5b89}\u{5168}-123456";
        let masked = mask_api_key(key);

        assert!(masked.contains("..."));
        assert!(masked.ends_with("3456"));
    }

    #[test]
    fn short_api_key_masking_never_reveals_the_full_secret() {
        for key in ["a", "sk-test", "12345678901", "短密钥"] {
            let masked = mask_api_key(key);
            assert_eq!(masked, "***");
            assert!(!masked.contains(key));
        }
    }

    #[test]
    fn admin_snapshots_are_bounded_and_never_serialize_raw_keys() {
        let base = std::env::temp_dir().join(format!(
            "ai-bridge-auth-admin-snapshot-{}",
            std::process::id()
        ));
        let black_keys = base.with_extension("black-keys");
        let black_ips = base.with_extension("black-ips");
        let white_keys = base.with_extension("white-keys");
        let blocked = "sk-blocked-super-secret-value";
        let allowed = "sk-allowed-super-secret-value";
        std::fs::write(
            &black_keys,
            format!("{blocked}\nsk-second-blocked-secret\n"),
        )
        .unwrap();
        std::fs::write(&black_ips, "198.51.100.2\n198.51.100.1\n").unwrap();
        std::fs::write(&white_keys, format!("{allowed}\n")).unwrap();

        let blacklist = BlacklistCache::load(black_keys.clone(), black_ips.clone()).unwrap();
        let whitelist = WhitelistCache::load(&white_keys).unwrap();
        let black_snapshot = blacklist.admin_snapshot(1);
        let white_snapshot = whitelist.admin_snapshot(1);
        assert_eq!(black_snapshot.total_api_keys, 2);
        assert_eq!(black_snapshot.total_client_ips, 2);
        assert!(black_snapshot.api_keys_truncated);
        assert!(black_snapshot.client_ips_truncated);
        assert_eq!(white_snapshot.total_api_keys, 1);

        let json = serde_json::to_string(&(black_snapshot, white_snapshot)).unwrap();
        assert!(!json.contains(blocked));
        assert!(!json.contains(allowed));
        assert!(json.contains("198.51.100.1") || json.contains("198.51.100.2"));

        let _ = std::fs::remove_file(black_keys);
        let _ = std::fs::remove_file(black_ips);
        let _ = std::fs::remove_file(white_keys);
    }

    #[test]
    fn blacklist_file_lookup_ignores_empty_lines_and_comments() {
        let path = std::env::temp_dir().join(format!(
            "ai-bridge-auth-blacklist-{}-lookup.txt",
            std::process::id()
        ));
        std::fs::write(&path, "\n# comment\nsk-blocked\n").unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer sk-blocked"),
        );
        let api_key = ApiKeyInfo::from_headers(&headers);

        assert!(api_key_is_blacklisted(&path, &api_key).unwrap());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn blacklist_file_lookup_ignores_utf8_bom_on_first_line() {
        let path = std::env::temp_dir().join(format!(
            "ai-bridge-auth-blacklist-{}-bom.txt",
            std::process::id()
        ));
        std::fs::write(&path, "\u{feff}sk-bom\n").unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer sk-bom"),
        );
        let api_key = ApiKeyInfo::from_headers(&headers);

        assert!(api_key_is_blacklisted(&path, &api_key).unwrap());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn blacklist_append_is_idempotent() {
        let path = std::env::temp_dir().join(format!(
            "ai-bridge-auth-blacklist-{}-append.txt",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer sk-newly-blocked"),
        );
        let api_key = ApiKeyInfo::from_headers(&headers);

        assert_eq!(
            append_api_key_to_blacklist(&path, &api_key).unwrap(),
            Some(true)
        );
        assert_eq!(
            append_api_key_to_blacklist(&path, &api_key).unwrap(),
            Some(false)
        );

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            content.lines().collect::<Vec<_>>(),
            vec!["sk-newly-blocked"]
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn client_ip_blacklist_append_is_idempotent() {
        let path = std::env::temp_dir().join(format!(
            "ai-bridge-auth-ip-blacklist-{}-append.txt",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let client_ip = "198.51.100.7".parse().unwrap();

        assert!(append_client_ip_to_blacklist(&path, client_ip).unwrap());
        assert!(!append_client_ip_to_blacklist(&path, client_ip).unwrap());
        assert!(client_ip_is_blacklisted(&path, client_ip).unwrap());

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content.lines().collect::<Vec<_>>(), vec!["198.51.100.7"]);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn administrator_api_key_crud_is_atomic_and_never_returns_raw_key() {
        let (api_key_path, client_ip_path) = unique_blacklist_paths("admin-key-crud");
        std::fs::write(&api_key_path, "# preserved\nsk-original\n").unwrap();
        let cache = BlacklistCache::load(api_key_path.clone(), client_ip_path.clone()).unwrap();
        let original_id = hash_api_key_full("sk-original");

        assert_eq!(
            cache.create_api_key("sk-created").unwrap(),
            AdminListMutationResult::Created
        );
        assert_eq!(
            cache.create_api_key("sk-created").unwrap(),
            AdminListMutationResult::AlreadyExists
        );
        assert_eq!(
            cache.update_api_key(&original_id, "sk-updated").unwrap(),
            AdminListMutationResult::Updated
        );
        let snapshot = cache.admin_snapshot(10);
        assert_eq!(snapshot.total_api_keys, 2);
        assert!(
            snapshot
                .api_keys
                .iter()
                .all(|entry| !entry.id.contains("sk-"))
        );
        assert!(
            snapshot
                .api_keys
                .iter()
                .all(|entry| !entry.masked.contains("created"))
        );

        let updated_id = hash_api_key_full("sk-updated");
        assert_eq!(
            cache.delete_api_key(&updated_id).unwrap(),
            AdminListMutationResult::Deleted
        );
        assert_eq!(
            cache.delete_api_key(&updated_id).unwrap(),
            AdminListMutationResult::NotFound
        );
        let content = std::fs::read_to_string(&api_key_path).unwrap();
        assert!(content.contains("# preserved"));
        assert!(content.contains("sk-created"));
        assert!(!content.contains("sk-original"));

        let _ = std::fs::remove_file(api_key_path);
        let _ = std::fs::remove_file(client_ip_path);
    }

    #[test]
    fn administrator_ip_and_whitelist_crud_sync_memory_immediately() {
        let (whitelist_path, ip_path) = unique_blacklist_paths("admin-other-crud");
        let blacklist =
            BlacklistCache::load(PathBuf::from("/nonexistent"), ip_path.clone()).unwrap();
        let whitelist = WhitelistCache::load(&whitelist_path).unwrap();
        let first_ip: IpAddr = "198.51.100.10".parse().unwrap();
        let second_ip: IpAddr = "2001:db8::10".parse().unwrap();

        assert_eq!(
            blacklist.create_client_ip(first_ip).unwrap(),
            AdminListMutationResult::Created
        );
        assert_eq!(
            blacklist.update_client_ip(first_ip, second_ip).unwrap(),
            AdminListMutationResult::Updated
        );
        assert!(!blacklist.client_ip_is_blacklisted(first_ip));
        assert!(blacklist.client_ip_is_blacklisted(second_ip));

        assert_eq!(
            whitelist.create_api_key("sk-white").unwrap(),
            AdminListMutationResult::Created
        );
        let id = hash_api_key_full("sk-white");
        assert_eq!(
            whitelist.update_api_key(&id, "sk-white-new").unwrap(),
            AdminListMutationResult::Updated
        );
        let id = hash_api_key_full("sk-white-new");
        assert_eq!(
            whitelist.delete_api_key(&id).unwrap(),
            AdminListMutationResult::Deleted
        );
        assert_eq!(whitelist.admin_snapshot(10).total_api_keys, 0);

        let _ = std::fs::remove_file(whitelist_path);
        let _ = std::fs::remove_file(ip_path);
    }

    fn unique_blacklist_paths(label: &str) -> (PathBuf, PathBuf) {
        let pid = std::process::id();
        let api_key_path =
            std::env::temp_dir().join(format!("ai-bridge-auth-cache-{label}-{pid}-key.txt",));
        let client_ip_path =
            std::env::temp_dir().join(format!("ai-bridge-auth-cache-{label}-{pid}-ip.txt",));
        let _ = std::fs::remove_file(&api_key_path);
        let _ = std::fs::remove_file(&client_ip_path);
        (api_key_path, client_ip_path)
    }

    #[test]
    fn blacklist_cache_loads_existing_entries_from_disk() {
        let (api_key_path, client_ip_path) = unique_blacklist_paths("load");
        std::fs::write(&api_key_path, "# comment\nsk-existing-1\n\nsk-existing-2\n").unwrap();
        std::fs::write(&client_ip_path, "198.51.100.7\n# blocked\n203.0.113.10\n").unwrap();

        let cache = BlacklistCache::load(api_key_path.clone(), client_ip_path.clone()).unwrap();

        assert_eq!(cache.api_key_entry_count(), 2);
        assert_eq!(cache.client_ip_entry_count(), 2);

        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer sk-existing-2"),
        );
        let api_key = ApiKeyInfo::from_headers(&headers);
        assert!(cache.api_key_is_blacklisted(&api_key));
        assert!(cache.client_ip_is_blacklisted("198.51.100.7".parse().unwrap()));
        assert!(!cache.client_ip_is_blacklisted("198.51.100.99".parse().unwrap()));

        let _ = std::fs::remove_file(api_key_path);
        let _ = std::fs::remove_file(client_ip_path);
    }

    #[test]
    fn blacklist_cache_append_persists_to_disk_and_memory() {
        let (api_key_path, client_ip_path) = unique_blacklist_paths("append");
        let cache = BlacklistCache::load(api_key_path.clone(), client_ip_path.clone()).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer sk-new-1"),
        );
        let api_key = ApiKeyInfo::from_headers(&headers);

        assert_eq!(cache.append_api_key(&api_key).unwrap(), Some(true));
        assert_eq!(cache.append_api_key(&api_key).unwrap(), Some(false));
        assert!(cache.api_key_is_blacklisted(&api_key));
        let on_disk = std::fs::read_to_string(&api_key_path).unwrap();
        assert_eq!(on_disk.lines().collect::<Vec<_>>(), vec!["sk-new-1"]);

        let ip: IpAddr = "198.51.100.42".parse().unwrap();
        assert!(cache.append_client_ip(ip).unwrap());
        assert!(!cache.append_client_ip(ip).unwrap());
        assert!(cache.client_ip_is_blacklisted(ip));

        let _ = std::fs::remove_file(api_key_path);
        let _ = std::fs::remove_file(client_ip_path);
    }

    #[test]
    fn blacklist_cache_sync_picks_up_external_additions_and_removals() {
        let (api_key_path, client_ip_path) = unique_blacklist_paths("sync");
        std::fs::write(&api_key_path, "sk-original\n").unwrap();
        std::fs::write(&client_ip_path, "198.51.100.1\n").unwrap();

        let cache = BlacklistCache::load(api_key_path.clone(), client_ip_path.clone()).unwrap();
        assert_eq!(cache.api_key_entry_count(), 1);
        assert_eq!(cache.client_ip_entry_count(), 1);

        // External edit: replace the file contents (simulating an operator).
        std::fs::write(&api_key_path, "sk-new-after-sync\n").unwrap();
        std::fs::write(&client_ip_path, "203.0.113.99\n").unwrap();
        cache
            .sync_from_disk(&api_key_path, &client_ip_path)
            .unwrap();

        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer sk-original"),
        );
        let original = ApiKeyInfo::from_headers(&headers);
        let mut new_headers = HeaderMap::new();
        new_headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer sk-new-after-sync"),
        );
        let new_api_key = ApiKeyInfo::from_headers(&new_headers);

        assert!(!cache.api_key_is_blacklisted(&original));
        assert!(cache.api_key_is_blacklisted(&new_api_key));
        assert!(!cache.client_ip_is_blacklisted("198.51.100.1".parse().unwrap()));
        assert!(cache.client_ip_is_blacklisted("203.0.113.99".parse().unwrap()));

        let _ = std::fs::remove_file(api_key_path);
        let _ = std::fs::remove_file(client_ip_path);
    }

    #[test]
    fn blacklist_cache_sync_updates_append_paths() {
        let (api_key_path, client_ip_path) = unique_blacklist_paths("path-update-original");
        let (new_api_key_path, new_client_ip_path) = unique_blacklist_paths("path-update-new");
        let cache = BlacklistCache::load(api_key_path.clone(), client_ip_path.clone()).unwrap();

        cache
            .sync_from_disk(&new_api_key_path, &new_client_ip_path)
            .unwrap();

        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer sk-after-path-update"),
        );
        let api_key = ApiKeyInfo::from_headers(&headers);
        assert_eq!(cache.append_api_key(&api_key).unwrap(), Some(true));

        let ip: IpAddr = "203.0.113.8".parse().unwrap();
        assert!(cache.append_client_ip(ip).unwrap());

        assert!(!api_key_path.exists());
        assert!(!client_ip_path.exists());
        assert_eq!(
            std::fs::read_to_string(&new_api_key_path).unwrap(),
            "sk-after-path-update\n"
        );
        assert_eq!(
            std::fs::read_to_string(&new_client_ip_path).unwrap(),
            "203.0.113.8\n"
        );

        let _ = std::fs::remove_file(new_api_key_path);
        let _ = std::fs::remove_file(new_client_ip_path);
    }

    #[test]
    fn whitelist_cache_sync_does_not_clear_existing_entries_when_file_is_missing() {
        let path = std::env::temp_dir().join(format!(
            "ai-bridge-auth-whitelist-{}-missing.txt",
            std::process::id()
        ));
        std::fs::write(&path, "sk-whitelisted\n").unwrap();
        let cache = WhitelistCache::load(&path).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer sk-whitelisted"),
        );
        let api_key = ApiKeyInfo::from_headers(&headers);
        assert!(cache.api_key_is_whitelisted(&api_key));

        std::fs::remove_file(&path).unwrap();
        let error = cache.sync_from_disk(&path).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(cache.api_key_is_whitelisted(&api_key));
    }
}
