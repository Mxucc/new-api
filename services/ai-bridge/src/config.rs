use std::{
    collections::HashMap,
    env,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use serde_json::Value;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Json,
    Pretty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageResponseMode {
    Auto,
    Json,
    Sse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthFailurePolicy {
    Off,
    Conservative,
    Balanced,
    Strict,
}

impl AuthFailurePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Conservative => "conservative",
            Self::Balanced => "balanced",
            Self::Strict => "strict",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" => Ok(Self::Off),
            "conservative" => Ok(Self::Conservative),
            "balanced" => Ok(Self::Balanced),
            "strict" => Ok(Self::Strict),
            _ => anyhow::bail!(
                "AI_BRIDGE_AUTH_FAILURE_POLICY must be off, conservative, balanced, or strict"
            ),
        }
    }
}

pub const DEFAULT_CONFIG_RELOAD_INTERVAL_SECS: u64 = 300;
pub const DEFAULT_CYBER_POLICY_CONFIRMATION_RULES_FILE: &str = "cyber_policy_rules.json";
pub const DEFAULT_UPSTREAM_429_MAX_RETRIES: usize = 3;
pub const DEFAULT_UPSTREAM_502_MAX_RETRIES: usize = 3;
pub const DEFAULT_AUTO_RESTART_INTERVAL_SECS: u64 = 0;
const CYBER_POLICY_CONFIRMATION_RULES_FILE_ENV: &str =
    "AI_BRIDGE_CYBER_POLICY_CONFIRMATION_RULES_FILE";
pub(crate) const ADMIN_CONFIG_FILE_ENV: &str = "AI_BRIDGE_ADMIN_CONFIG_FILE";
pub(crate) const ADMIN_EDITABLE_KEYS: &[&str] = &[
    "AI_BRIDGE_UPSTREAM_BASE_URL",
    "AI_BRIDGE_REQUEST_TIMEOUT_SECS",
    "AI_BRIDGE_CONNECT_TIMEOUT_SECS",
    "AI_BRIDGE_HEARTBEAT_INTERVAL_SECS",
    "AI_BRIDGE_STREAM_EVENT_STALL_RETRY_ENABLED",
    "AI_BRIDGE_STREAM_EVENT_STALL_TIMEOUT_SECS",
    "AI_BRIDGE_STREAM_EVENT_STALL_MAX_RETRIES",
    "AI_BRIDGE_UPSTREAM_429_INITIAL_DELAY_SECS",
    "AI_BRIDGE_UPSTREAM_429_MAX_DELAY_SECS",
    "AI_BRIDGE_UPSTREAM_429_MAX_RETRIES",
    "AI_BRIDGE_UPSTREAM_ERROR_COOLDOWN_ENABLED",
    "AI_BRIDGE_UPSTREAM_502_COOLDOWN_ENABLED",
    "AI_BRIDGE_UPSTREAM_502_COOLDOWN_ACCUMULATE_ENABLED",
    "AI_BRIDGE_UPSTREAM_502_COOLDOWN_SECS",
    "AI_BRIDGE_UPSTREAM_502_MAX_RETRIES",
    "AI_BRIDGE_AUTH_FAILURE_COOLDOWN_ENABLED",
    "AI_BRIDGE_AUTH_FAILURE_POLICY",
    "AI_BRIDGE_AUTH_FAILURE_WINDOW_SECS",
    "AI_BRIDGE_AUTH_FAILURE_THRESHOLD",
    "AI_BRIDGE_AUTH_FAILURE_COOLDOWN_SECS",
    "AI_BRIDGE_AUTH_FAILURE_IP_THRESHOLD",
    "AI_BRIDGE_AUTH_FAILURE_ERROR_BODY_MAX_BYTES",
    "AI_BRIDGE_MAX_BODY_BYTES",
    "AI_BRIDGE_MAX_UPLOAD_BODY_BYTES",
    "AI_BRIDGE_IMAGE_RESPONSE_MODE",
    "AI_BRIDGE_RESPONSES_NONSTREAM_CHAT_FALLBACK",
    "AI_BRIDGE_DEBUG_OUTPUT",
    "AI_BRIDGE_CYBER_POLICY_BAN_ENABLED",
    "AI_BRIDGE_CYBER_POLICY_BLACKLIST_FILE",
    "AI_BRIDGE_CYBER_POLICY_IP_BLACKLIST_FILE",
    "AI_BRIDGE_CYBER_POLICY_WHITELIST_FILE",
    "AI_BRIDGE_CYBER_POLICY_CONFIRMATION_RULES_FILE",
    "AI_BRIDGE_STREAM_EARLY_HEARTBEAT",
    "AI_BRIDGE_STREAM_SYNTHETIC_FIRST_SPACE",
    "AI_BRIDGE_FILTER_TOOLS_BY_MODEL",
    "AI_BRIDGE_TOOL_FILTER_SCAN_LIMIT",
];

#[derive(Debug, Clone)]
pub struct Config {
    pub listen_addr: SocketAddr,
    pub upstream_base_url: Url,
    pub admin_session_secret: Option<String>,
    pub admin_login_url: String,
    pub external_auth_cookie_secure: bool,
    pub config_reload_interval: Option<Duration>,
    pub auto_restart_interval: Option<Duration>,
    pub request_timeout: Option<Duration>,
    pub connect_timeout: Duration,
    pub heartbeat_interval: Duration,
    pub stream_event_stall_retry_enabled: bool,
    pub stream_event_stall_timeout: Option<Duration>,
    pub stream_event_stall_max_retries: usize,
    pub upstream_429_initial_delay: Duration,
    pub upstream_429_max_delay: Duration,
    pub upstream_429_max_retries: Option<usize>,
    pub upstream_error_cooldown_enabled: bool,
    pub upstream_502_cooldown_enabled: bool,
    pub upstream_502_cooldown_accumulate_enabled: bool,
    pub upstream_502_cooldown_delay: Duration,
    pub upstream_502_max_retries: Option<usize>,
    pub auth_failure_cooldown_enabled: bool,
    pub auth_failure_policy: AuthFailurePolicy,
    pub auth_failure_window: Duration,
    pub auth_failure_threshold: usize,
    pub auth_failure_cooldown: Duration,
    pub auth_failure_ip_threshold: usize,
    pub auth_failure_max_entries: usize,
    pub auth_failure_error_body_max_bytes: usize,
    pub rate_limiter_idle_ttl: Duration,
    pub rate_limiter_cleanup_interval: Duration,
    pub rate_limiter_max_entries: usize,
    pub max_body_bytes: usize,
    pub max_upload_body_bytes: usize,
    pub log_level: String,
    pub log_format: LogFormat,
    pub log_file: Option<PathBuf>,
    /// Directory for 502 forensic JSONL records.
    ///
    /// These records intentionally include complete request bodies to make
    /// prompt-specific upstream failures reproducible, while credential headers
    /// remain redacted.
    pub bad_gateway_log_dir: PathBuf,
    /// Directory for 503 forensic JSONL records.
    pub service_unavailable_log_dir: PathBuf,
    pub forensic_queue_capacity: usize,
    pub forensic_max_body_bytes: usize,
    pub forensic_max_records_per_minute: usize,
    pub forensic_include_api_key: bool,
    pub internal_header_prefix: String,
    pub image_response_mode: ImageResponseMode,
    pub debug_output: bool,
    pub cyber_policy_blacklist_file: PathBuf,
    pub cyber_policy_black_log_file: PathBuf,
    pub cyber_policy_ip_blacklist_file: PathBuf,
    pub cyber_policy_whitelist_file: PathBuf,
    pub cyber_policy_white_log_file: PathBuf,
    pub cyber_policy_ban_enabled: bool,
    pub cyber_policy_confirmation_rules_file: Option<PathBuf>,
    pub cyber_policy_confirmation_rules: CyberPolicyConfirmationRuleConfig,
    pub responses_nonstream_chat_fallback: bool,
    pub stream_early_heartbeat: bool,
    pub stream_synthetic_first_space: bool,
    pub filter_tools_by_model: HashMap<String, Vec<String>>,
    pub tool_filter_scan_limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CyberPolicyConfirmationRuleConfig {
    pub keywords: Vec<String>,
    pub regex: Vec<String>,
}

impl CyberPolicyConfirmationRuleConfig {
    #[cfg(test)]
    pub fn default_rules() -> Self {
        Self::from_json_str(include_str!("../cyber_policy_rules.json"))
            .expect("default cyber_policy_rules.json must be valid")
    }

    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path).with_context(|| {
            format!(
                "failed to read cyber-policy confirmation rules file {}",
                path.display()
            )
        })?;
        Self::from_json_str(&content).with_context(|| {
            format!(
                "failed to parse cyber-policy confirmation rules file {}",
                path.display()
            )
        })
    }

    pub fn from_json_str(content: &str) -> Result<Self> {
        let value: Value =
            serde_json::from_str(content).context("rules file must be valid JSON")?;
        let object = value
            .as_object()
            .context("rules file root must be a JSON object")?;

        Ok(Self {
            keywords: string_array_field(object.get("keywords"), "keywords")?,
            regex: string_array_field(object.get("regex"), "regex")?,
        })
    }
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let source = EnvConfigSource::load()?;
        Self::from_source(source)
    }

    pub(crate) fn from_admin_overrides(overrides: HashMap<String, String>) -> Result<Self> {
        let mut source = EnvConfigSource::load_base()?;
        validate_admin_override_keys(&overrides)?;
        source.admin_override_values = overrides;
        Self::from_source(source)
    }

    pub(crate) fn load_admin_overrides() -> Result<(PathBuf, HashMap<String, String>)> {
        let source = EnvConfigSource::load()?;
        Ok((source.admin_config_file, source.admin_override_values))
    }

    pub(crate) fn admin_config_file_path() -> Result<PathBuf> {
        Ok(EnvConfigSource::load_base()?.admin_config_file)
    }

    fn from_source(source: EnvConfigSource) -> Result<Self> {
        let listen_addr = source
            .value_or("AI_BRIDGE_LISTEN_ADDR", "0.0.0.0:8080")
            .parse::<SocketAddr>()
            .context("AI_BRIDGE_LISTEN_ADDR must be a socket address, for example 0.0.0.0:8080")?;

        let upstream_base_url =
            Url::parse(&source.value_or("AI_BRIDGE_UPSTREAM_BASE_URL", "http://127.0.0.1:3000"))
                .context("AI_BRIDGE_UPSTREAM_BASE_URL must be an absolute URL")?;
        let admin_session_secret = source
            .value("AI_BRIDGE_ADMIN_SESSION_SECRET")
            .or_else(|| source.value("SESSION_SECRET"))
            .filter(|value| !value.trim().is_empty());
        let admin_login_url = source
            .value("AI_BRIDGE_ADMIN_LOGIN_URL")
            .or_else(|| source.value("LOGIN_URL"))
            .unwrap_or_else(|| "/sign-in".to_owned());
        let external_auth_cookie_secure =
            source.bool("AI_BRIDGE_EXTERNAL_AUTH_COOKIE_SECURE", true)?;

        let config_reload_interval_secs = source.parse(
            "AI_BRIDGE_CONFIG_RELOAD_INTERVAL_SECS",
            DEFAULT_CONFIG_RELOAD_INTERVAL_SECS,
        )?;
        let config_reload_interval = if config_reload_interval_secs == 0 {
            None
        } else {
            Some(Duration::from_secs(config_reload_interval_secs))
        };

        let auto_restart_interval = auto_restart_interval_from_source(&source)?;

        let request_timeout_secs = source.parse("AI_BRIDGE_REQUEST_TIMEOUT_SECS", 900_u64)?;
        let request_timeout = if request_timeout_secs == 0 {
            None
        } else {
            Some(Duration::from_secs(request_timeout_secs))
        };

        let connect_timeout =
            Duration::from_secs(source.parse("AI_BRIDGE_CONNECT_TIMEOUT_SECS", 30_u64)?);
        let heartbeat_interval =
            Duration::from_secs(source.parse("AI_BRIDGE_HEARTBEAT_INTERVAL_SECS", 5_u64)?);
        let stream_event_stall_retry_enabled =
            source.bool("AI_BRIDGE_STREAM_EVENT_STALL_RETRY_ENABLED", true)?;
        let stream_event_stall_timeout_secs =
            source.parse("AI_BRIDGE_STREAM_EVENT_STALL_TIMEOUT_SECS", 60_u64)?;
        let stream_event_stall_timeout =
            if !stream_event_stall_retry_enabled || stream_event_stall_timeout_secs == 0 {
                None
            } else {
                Some(Duration::from_secs(stream_event_stall_timeout_secs))
            };
        let stream_event_stall_max_retries =
            source.parse("AI_BRIDGE_STREAM_EVENT_STALL_MAX_RETRIES", 2_usize)?;
        let upstream_429_initial_delay =
            Duration::from_secs(source.parse("AI_BRIDGE_UPSTREAM_429_INITIAL_DELAY_SECS", 2_u64)?);
        let upstream_429_max_delay =
            Duration::from_secs(source.parse("AI_BRIDGE_UPSTREAM_429_MAX_DELAY_SECS", 30_u64)?);
        let upstream_429_max_retries = upstream_429_max_retries_from_source(&source)?;
        let upstream_error_cooldown_enabled =
            source.bool("AI_BRIDGE_UPSTREAM_ERROR_COOLDOWN_ENABLED", true)?;
        let upstream_502_cooldown_enabled =
            source.bool("AI_BRIDGE_UPSTREAM_502_COOLDOWN_ENABLED", true)?;
        let upstream_502_cooldown_accumulate_enabled =
            source.bool("AI_BRIDGE_UPSTREAM_502_COOLDOWN_ACCUMULATE_ENABLED", true)?;
        let upstream_502_cooldown_delay =
            Duration::from_secs(source.parse("AI_BRIDGE_UPSTREAM_502_COOLDOWN_SECS", 300_u64)?);
        let upstream_502_max_retries = upstream_502_max_retries_from_source(&source)?;
        let auth_failure_cooldown_enabled =
            source.bool("AI_BRIDGE_AUTH_FAILURE_COOLDOWN_ENABLED", true)?;
        let auth_failure_policy = AuthFailurePolicy::parse(
            &source.value_or("AI_BRIDGE_AUTH_FAILURE_POLICY", "balanced"),
        )?;
        let auth_failure_window =
            Duration::from_secs(source.parse("AI_BRIDGE_AUTH_FAILURE_WINDOW_SECS", 60_u64)?);
        let auth_failure_threshold = source.parse("AI_BRIDGE_AUTH_FAILURE_THRESHOLD", 5_usize)?;
        let auth_failure_cooldown =
            Duration::from_secs(source.parse("AI_BRIDGE_AUTH_FAILURE_COOLDOWN_SECS", 1800_u64)?);
        let auth_failure_ip_threshold =
            source.parse("AI_BRIDGE_AUTH_FAILURE_IP_THRESHOLD", 20_usize)?;
        let auth_failure_max_entries =
            source.parse("AI_BRIDGE_AUTH_FAILURE_MAX_ENTRIES", 10_000_usize)?;
        let auth_failure_error_body_max_bytes = source.parse(
            "AI_BRIDGE_AUTH_FAILURE_ERROR_BODY_MAX_BYTES",
            8 * 1024_usize,
        )?;
        let rate_limiter_idle_ttl =
            Duration::from_secs(source.parse("AI_BRIDGE_RATE_LIMITER_IDLE_TTL_SECS", 600_u64)?);
        let rate_limiter_cleanup_interval = Duration::from_secs(
            source.parse("AI_BRIDGE_RATE_LIMITER_CLEANUP_INTERVAL_SECS", 60_u64)?,
        );
        let rate_limiter_max_entries =
            source.parse("AI_BRIDGE_RATE_LIMITER_MAX_ENTRIES", 10_000_usize)?;
        let max_body_bytes = source.parse("AI_BRIDGE_MAX_BODY_BYTES", 64 * 1024 * 1024_usize)?;
        let max_upload_body_bytes =
            source.parse("AI_BRIDGE_MAX_UPLOAD_BODY_BYTES", 512 * 1024 * 1024_usize)?;
        let log_level = source.value_or("AI_BRIDGE_LOG_LEVEL", "info");
        let log_format = match source
            .value_or("AI_BRIDGE_LOG_FORMAT", "json")
            .to_ascii_lowercase()
            .as_str()
        {
            "pretty" | "text" | "plain" => LogFormat::Pretty,
            _ => LogFormat::Json,
        };
        let log_file = source.path("AI_BRIDGE_LOG_FILE");
        let bad_gateway_log_dir = source
            .path("AI_BRIDGE_502_LOG_DIR")
            .unwrap_or_else(|| PathBuf::from("log/502"));
        let service_unavailable_log_dir = source
            .path("AI_BRIDGE_503_LOG_DIR")
            .unwrap_or_else(|| PathBuf::from("log/503"));
        let forensic_queue_capacity =
            source.parse("AI_BRIDGE_FORENSIC_QUEUE_CAPACITY", 256_usize)?;
        let forensic_max_body_bytes =
            source.parse("AI_BRIDGE_FORENSIC_MAX_BODY_BYTES", 256 * 1024_usize)?;
        let forensic_max_records_per_minute =
            source.parse("AI_BRIDGE_FORENSIC_MAX_RECORDS_PER_MINUTE", 120_usize)?;
        let forensic_include_api_key = source.bool("AI_BRIDGE_FORENSIC_INCLUDE_API_KEY", false)?;
        let internal_header_prefix =
            source.value_or("AI_BRIDGE_INTERNAL_HEADER_PREFIX", "x-ai-bridge-");
        let debug_output = source.bool("AI_BRIDGE_DEBUG_OUTPUT", false)?;
        let cyber_policy_blacklist_file = match source.path("AI_BRIDGE_CYBER_POLICY_BLACKLIST_FILE")
        {
            Some(path) => path,
            None => default_executable_sibling_file("black.txt")?,
        };
        let cyber_policy_black_log_file = match source.path("AI_BRIDGE_CYBER_POLICY_BLACK_LOG_FILE")
        {
            Some(path) => path,
            None => default_executable_sibling_file("black_log")?,
        };
        let cyber_policy_ip_blacklist_file =
            match source.path("AI_BRIDGE_CYBER_POLICY_IP_BLACKLIST_FILE") {
                Some(path) => path,
                None => default_executable_sibling_file("black_ip")?,
            };
        let cyber_policy_whitelist_file = match source.path("AI_BRIDGE_CYBER_POLICY_WHITELIST_FILE")
        {
            Some(path) => path,
            None => default_executable_sibling_file("white.txt")?,
        };
        let cyber_policy_white_log_file = match source.path("AI_BRIDGE_CYBER_POLICY_WHITE_LOG_FILE")
        {
            Some(path) => path,
            None => default_executable_sibling_file("white_log.txt")?,
        };
        let cyber_policy_ban_enabled = source.bool("AI_BRIDGE_CYBER_POLICY_BAN_ENABLED", true)?;
        let cyber_policy_confirmation_rules_file =
            cyber_policy_confirmation_rules_file_from_source(&source);
        let cyber_policy_confirmation_rules =
            CyberPolicyConfirmationRuleConfig::from_file(&cyber_policy_confirmation_rules_file)?;
        let responses_nonstream_chat_fallback =
            source.bool("AI_BRIDGE_RESPONSES_NONSTREAM_CHAT_FALLBACK", false)?;
        let stream_early_heartbeat = source.bool("AI_BRIDGE_STREAM_EARLY_HEARTBEAT", true)?;
        let stream_synthetic_first_space =
            source.bool("AI_BRIDGE_STREAM_SYNTHETIC_FIRST_SPACE", false)?;
        let image_response_mode = match source
            .value_or("AI_BRIDGE_IMAGE_RESPONSE_MODE", "auto")
            .to_ascii_lowercase()
            .as_str()
        {
            "auto" => ImageResponseMode::Auto,
            "json" => ImageResponseMode::Json,
            "sse" => ImageResponseMode::Sse,
            _ => anyhow::bail!("AI_BRIDGE_IMAGE_RESPONSE_MODE must be one of: auto, json, sse"),
        };

        let filter_tools_by_model = match source.value("AI_BRIDGE_FILTER_TOOLS_BY_MODEL") {
            Some(json_str) => match serde_json::from_str::<HashMap<String, Vec<String>>>(&json_str)
            {
                Ok(map) => map,
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "AI_BRIDGE_FILTER_TOOLS_BY_MODEL is not valid JSON; using empty config"
                    );
                    HashMap::new()
                }
            },
            None => HashMap::new(),
        };

        let tool_filter_scan_limit =
            source.parse("AI_BRIDGE_TOOL_FILTER_SCAN_LIMIT", 1024_usize)?;

        if rate_limiter_idle_ttl.is_zero() {
            anyhow::bail!("AI_BRIDGE_RATE_LIMITER_IDLE_TTL_SECS must be greater than 0");
        }
        if rate_limiter_cleanup_interval.is_zero() {
            anyhow::bail!("AI_BRIDGE_RATE_LIMITER_CLEANUP_INTERVAL_SECS must be greater than 0");
        }
        if rate_limiter_max_entries == 0 {
            anyhow::bail!("AI_BRIDGE_RATE_LIMITER_MAX_ENTRIES must be greater than 0");
        }
        if forensic_queue_capacity == 0 {
            anyhow::bail!("AI_BRIDGE_FORENSIC_QUEUE_CAPACITY must be greater than 0");
        }
        if forensic_max_body_bytes == 0 {
            anyhow::bail!("AI_BRIDGE_FORENSIC_MAX_BODY_BYTES must be greater than 0");
        }
        if max_upload_body_bytes == 0 {
            anyhow::bail!("AI_BRIDGE_MAX_UPLOAD_BODY_BYTES must be greater than 0");
        }
        if config_reload_interval.is_some_and(|interval| interval.is_zero()) {
            anyhow::bail!(
                "AI_BRIDGE_CONFIG_RELOAD_INTERVAL_SECS must be greater than 0, or 0 to disable reload"
            );
        }
        if upstream_429_initial_delay.is_zero() {
            anyhow::bail!("AI_BRIDGE_UPSTREAM_429_INITIAL_DELAY_SECS must be greater than 0");
        }
        if upstream_429_max_delay < upstream_429_initial_delay {
            anyhow::bail!(
                "AI_BRIDGE_UPSTREAM_429_MAX_DELAY_SECS must be greater than or equal to AI_BRIDGE_UPSTREAM_429_INITIAL_DELAY_SECS"
            );
        }
        if upstream_502_cooldown_enabled && upstream_502_cooldown_delay.is_zero() {
            anyhow::bail!("AI_BRIDGE_UPSTREAM_502_COOLDOWN_SECS must be greater than 0");
        }
        if auth_failure_cooldown_enabled && auth_failure_policy != AuthFailurePolicy::Off {
            if auth_failure_window.is_zero() {
                anyhow::bail!("AI_BRIDGE_AUTH_FAILURE_WINDOW_SECS must be greater than 0");
            }
            if auth_failure_threshold == 0 {
                anyhow::bail!("AI_BRIDGE_AUTH_FAILURE_THRESHOLD must be greater than 0");
            }
            if auth_failure_cooldown.is_zero() {
                anyhow::bail!("AI_BRIDGE_AUTH_FAILURE_COOLDOWN_SECS must be greater than 0");
            }
            if auth_failure_ip_threshold != 0 && auth_failure_ip_threshold < auth_failure_threshold
            {
                anyhow::bail!(
                    "AI_BRIDGE_AUTH_FAILURE_IP_THRESHOLD must be 0 or greater than or equal to AI_BRIDGE_AUTH_FAILURE_THRESHOLD"
                );
            }
            if auth_failure_max_entries == 0 {
                anyhow::bail!("AI_BRIDGE_AUTH_FAILURE_MAX_ENTRIES must be greater than 0");
            }
            if auth_failure_error_body_max_bytes == 0 {
                anyhow::bail!("AI_BRIDGE_AUTH_FAILURE_ERROR_BODY_MAX_BYTES must be greater than 0");
            }
        }

        Ok(Self {
            listen_addr,
            upstream_base_url,
            admin_session_secret,
            admin_login_url,
            external_auth_cookie_secure,
            config_reload_interval,
            auto_restart_interval,
            request_timeout,
            connect_timeout,
            heartbeat_interval,
            stream_event_stall_retry_enabled,
            stream_event_stall_timeout,
            stream_event_stall_max_retries,
            upstream_429_initial_delay,
            upstream_429_max_delay,
            upstream_429_max_retries,
            upstream_error_cooldown_enabled,
            upstream_502_cooldown_enabled,
            upstream_502_cooldown_accumulate_enabled,
            upstream_502_cooldown_delay,
            upstream_502_max_retries,
            auth_failure_cooldown_enabled,
            auth_failure_policy,
            auth_failure_window,
            auth_failure_threshold,
            auth_failure_cooldown,
            auth_failure_ip_threshold,
            auth_failure_max_entries,
            auth_failure_error_body_max_bytes,
            rate_limiter_idle_ttl,
            rate_limiter_cleanup_interval,
            rate_limiter_max_entries,
            max_body_bytes,
            max_upload_body_bytes,
            log_level,
            log_format,
            log_file,
            bad_gateway_log_dir,
            service_unavailable_log_dir,
            forensic_queue_capacity,
            forensic_max_body_bytes,
            forensic_max_records_per_minute,
            forensic_include_api_key,
            internal_header_prefix,
            image_response_mode,
            debug_output,
            cyber_policy_blacklist_file,
            cyber_policy_black_log_file,
            cyber_policy_ip_blacklist_file,
            cyber_policy_whitelist_file,
            cyber_policy_white_log_file,
            cyber_policy_ban_enabled,
            cyber_policy_confirmation_rules_file: Some(cyber_policy_confirmation_rules_file),
            cyber_policy_confirmation_rules,
            responses_nonstream_chat_fallback,
            stream_early_heartbeat,
            stream_synthetic_first_space,
            filter_tools_by_model,
            tool_filter_scan_limit,
        })
    }
}

#[derive(Debug, Default)]
struct EnvConfigSource {
    admin_override_values: HashMap<String, String>,
    admin_config_file: PathBuf,
    dotenv_values: HashMap<String, String>,
}

impl EnvConfigSource {
    fn load() -> Result<Self> {
        let mut source = Self::load_base()?;
        source.admin_override_values = read_admin_override_file(&source.admin_config_file)?;
        Ok(source)
    }

    fn load_base() -> Result<Self> {
        let mut dotenv_values = HashMap::new();
        match dotenvy::dotenv_iter() {
            Ok(iter) => {
                for item in iter {
                    let (key, value) =
                        item.context("failed to parse .env while loading configuration")?;
                    if key.is_empty() {
                        continue;
                    }
                    dotenv_values.insert(key, value);
                }
            }
            Err(error) if error.not_found() => {}
            Err(error) => return Err(error).context("failed to load .env configuration"),
        }

        let admin_config_file = dotenv_values
            .get(ADMIN_CONFIG_FILE_ENV)
            .cloned()
            .or_else(|| env::var(ADMIN_CONFIG_FILE_ENV).ok())
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(|| default_executable_sibling_file("admin-config.env"))?;
        Ok(Self {
            admin_override_values: HashMap::new(),
            admin_config_file,
            dotenv_values,
        })
    }

    fn value(&self, key: &str) -> Option<String> {
        self.admin_override_values
            .get(key)
            .cloned()
            .or_else(|| self.dotenv_values.get(key).cloned())
            .or_else(|| env::var(key).ok())
    }

    fn value_or(&self, key: &str, default: &str) -> String {
        self.value(key).unwrap_or_else(|| default.to_owned())
    }

    fn path(&self, key: &str) -> Option<PathBuf> {
        self.value(key)
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
    }

    fn parse<T>(&self, key: &str, default: T) -> Result<T>
    where
        T: std::str::FromStr,
        T::Err: std::error::Error + Send + Sync + 'static,
    {
        match self.value(key) {
            Some(value) => value
                .parse::<T>()
                .with_context(|| format!("{key} has an invalid value")),
            None => Ok(default),
        }
    }

    fn bool(&self, key: &str, default: bool) -> Result<bool> {
        match self.value(key) {
            Some(value) => parse_bool(&value).with_context(|| {
                format!("{key} must be a boolean: true/false, 1/0, yes/no, or on/off")
            }),
            None => Ok(default),
        }
    }
}

fn read_admin_override_file(path: &Path) -> Result<HashMap<String, String>> {
    let mut values = HashMap::new();
    let iter = match dotenvy::from_path_iter(path) {
        Ok(iter) => iter,
        Err(error) if error.not_found() => return Ok(values),
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "failed to load administrator config file {}",
                    path.display()
                )
            });
        }
    };
    for item in iter {
        let (key, value) = item.with_context(|| {
            format!(
                "failed to parse administrator config file {}",
                path.display()
            )
        })?;
        if !ADMIN_EDITABLE_KEYS.contains(&key.as_str()) {
            anyhow::bail!(
                "administrator config file {} contains unsupported key {key}",
                path.display()
            );
        }
        values.insert(key, value);
    }
    Ok(values)
}

fn validate_admin_override_keys(overrides: &HashMap<String, String>) -> Result<()> {
    if let Some(key) = overrides
        .keys()
        .find(|key| !ADMIN_EDITABLE_KEYS.contains(&key.as_str()))
    {
        anyhow::bail!("unsupported administrator configuration key {key}");
    }
    Ok(())
}

fn upstream_429_max_retries_from_source(source: &EnvConfigSource) -> Result<Option<usize>> {
    let retries = source.parse(
        "AI_BRIDGE_UPSTREAM_429_MAX_RETRIES",
        DEFAULT_UPSTREAM_429_MAX_RETRIES,
    )?;

    Ok(if retries == 0 { None } else { Some(retries) })
}

fn upstream_502_max_retries_from_source(source: &EnvConfigSource) -> Result<Option<usize>> {
    let retries = source.parse(
        "AI_BRIDGE_UPSTREAM_502_MAX_RETRIES",
        DEFAULT_UPSTREAM_502_MAX_RETRIES,
    )?;

    Ok(if retries == 0 { None } else { Some(retries) })
}

fn auto_restart_interval_from_source(source: &EnvConfigSource) -> Result<Option<Duration>> {
    let interval_secs = source.parse(
        "AI_BRIDGE_AUTO_RESTART_INTERVAL_SECS",
        DEFAULT_AUTO_RESTART_INTERVAL_SECS,
    )?;

    Ok(if interval_secs == 0 {
        None
    } else {
        Some(Duration::from_secs(interval_secs))
    })
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn string_array_field(value: Option<&Value>, name: &str) -> Result<Vec<String>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };

    let values = value
        .as_array()
        .with_context(|| format!("{name} must be an array of strings"))?;
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .with_context(|| format!("{name}[{index}] must be a non-empty string"))
        })
        .collect()
}

fn cyber_policy_confirmation_rules_file_from_source(source: &EnvConfigSource) -> PathBuf {
    source
        .path(CYBER_POLICY_CONFIRMATION_RULES_FILE_ENV)
        .unwrap_or_else(default_cyber_policy_confirmation_rules_file)
}

fn default_cyber_policy_confirmation_rules_file() -> PathBuf {
    PathBuf::from(DEFAULT_CYBER_POLICY_CONFIRMATION_RULES_FILE)
}

fn default_executable_sibling_file(filename: &str) -> Result<PathBuf> {
    let executable = env::current_exe().context("failed to resolve current executable path")?;
    let directory = executable
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .context("current executable path has no parent directory")?;
    Ok(directory.join(filename))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_debug_bool_values() {
        assert_eq!(parse_bool("true"), Some(true));
        assert_eq!(parse_bool("1"), Some(true));
        assert_eq!(parse_bool("YES"), Some(true));
        assert_eq!(parse_bool("off"), Some(false));
        assert_eq!(parse_bool("0"), Some(false));
        assert_eq!(parse_bool("maybe"), None);
    }

    #[test]
    fn parses_responses_chat_fallback_bool_values() {
        assert_eq!(parse_bool("on"), Some(true));
        assert_eq!(parse_bool("OFF"), Some(false));
        assert_eq!(parse_bool("unexpected"), None);
    }

    #[test]
    fn parses_upstream_error_cooldown_bool_values() {
        assert_eq!(parse_bool("true"), Some(true));
        assert_eq!(parse_bool("0"), Some(false));
        assert_eq!(parse_bool("unexpected"), None);
    }

    #[test]
    fn upstream_429_max_retries_defaults_to_bounded_retry() {
        let source = EnvConfigSource::default();

        assert_eq!(
            upstream_429_max_retries_from_source(&source).unwrap(),
            Some(DEFAULT_UPSTREAM_429_MAX_RETRIES)
        );
    }

    #[test]
    fn auth_failure_policy_accepts_all_modes_and_rejects_unknown_values() {
        assert_eq!(
            AuthFailurePolicy::parse("off").unwrap(),
            AuthFailurePolicy::Off
        );
        assert_eq!(
            AuthFailurePolicy::parse("conservative").unwrap(),
            AuthFailurePolicy::Conservative
        );
        assert_eq!(
            AuthFailurePolicy::parse("balanced").unwrap(),
            AuthFailurePolicy::Balanced
        );
        assert_eq!(
            AuthFailurePolicy::parse("strict").unwrap(),
            AuthFailurePolicy::Strict
        );
        assert!(AuthFailurePolicy::parse("aggressive").is_err());
    }

    #[test]
    fn upstream_429_max_retries_zero_remains_explicit_unlimited() {
        let mut source = EnvConfigSource::default();
        source.dotenv_values.insert(
            "AI_BRIDGE_UPSTREAM_429_MAX_RETRIES".to_owned(),
            "0".to_owned(),
        );

        assert_eq!(upstream_429_max_retries_from_source(&source).unwrap(), None);
    }

    #[test]
    fn upstream_502_max_retries_defaults_to_bounded_retry() {
        let source = EnvConfigSource::default();

        assert_eq!(
            upstream_502_max_retries_from_source(&source).unwrap(),
            Some(DEFAULT_UPSTREAM_502_MAX_RETRIES)
        );
    }

    #[test]
    fn upstream_502_max_retries_zero_remains_explicit_unlimited() {
        let mut source = EnvConfigSource::default();
        source.dotenv_values.insert(
            "AI_BRIDGE_UPSTREAM_502_MAX_RETRIES".to_owned(),
            "0".to_owned(),
        );

        assert_eq!(upstream_502_max_retries_from_source(&source).unwrap(), None);
    }

    #[test]
    fn auto_restart_interval_defaults_to_disabled() {
        let source = EnvConfigSource::default();

        assert_eq!(auto_restart_interval_from_source(&source).unwrap(), None);
    }

    #[test]
    fn auto_restart_interval_accepts_24_hours() {
        let mut source = EnvConfigSource::default();
        source.dotenv_values.insert(
            "AI_BRIDGE_AUTO_RESTART_INTERVAL_SECS".to_owned(),
            "86400".to_owned(),
        );

        assert_eq!(
            auto_restart_interval_from_source(&source).unwrap(),
            Some(Duration::from_secs(86_400))
        );
    }

    #[test]
    fn dotenv_values_are_read_before_process_environment_for_hot_reload() {
        let mut source = EnvConfigSource::default();
        source
            .dotenv_values
            .insert("AI_BRIDGE_EXAMPLE".to_owned(), "from_dotenv".to_owned());

        assert_eq!(
            source.value_or("AI_BRIDGE_EXAMPLE", "default"),
            "from_dotenv"
        );
    }

    #[test]
    fn dotenv_accepts_quoted_filter_tools_json_with_spaces() {
        let line = "AI_BRIDGE_FILTER_TOOLS_BY_MODEL='{\"gpt\": [\"image_generation\"]}'\n";
        let mut iter = dotenvy::from_read_iter(line.as_bytes());
        let (key, value) = iter.next().unwrap().unwrap();

        assert_eq!(key, "AI_BRIDGE_FILTER_TOOLS_BY_MODEL");
        assert_eq!(value, r#"{"gpt": ["image_generation"]}"#);

        let parsed: HashMap<String, Vec<String>> = serde_json::from_str(&value).unwrap();
        assert_eq!(
            parsed.get("gpt").unwrap(),
            &vec!["image_generation".to_owned()]
        );
        assert!(iter.next().is_none());
    }

    #[test]
    fn cyber_policy_confirmation_rules_file_defaults_to_root_json() {
        let source = EnvConfigSource::default();

        assert_eq!(
            cyber_policy_confirmation_rules_file_from_source(&source),
            PathBuf::from(DEFAULT_CYBER_POLICY_CONFIRMATION_RULES_FILE)
        );
    }

    #[test]
    fn cyber_policy_confirmation_rules_file_honors_configured_path() {
        let mut source = EnvConfigSource::default();
        source.dotenv_values.insert(
            CYBER_POLICY_CONFIRMATION_RULES_FILE_ENV.to_owned(),
            "/tmp/custom-cyber-policy-rules.json".to_owned(),
        );

        assert_eq!(
            cyber_policy_confirmation_rules_file_from_source(&source),
            PathBuf::from("/tmp/custom-cyber-policy-rules.json")
        );
    }

    #[test]
    fn default_cyber_policy_confirmation_rules_come_from_json_file() {
        let from_embedded_file = CyberPolicyConfirmationRuleConfig::from_json_str(include_str!(
            "../cyber_policy_rules.json"
        ))
        .unwrap();

        assert_eq!(
            CyberPolicyConfirmationRuleConfig::default_rules(),
            from_embedded_file
        );
    }

    #[test]
    fn parses_cyber_policy_confirmation_rules_json() {
        let rules = CyberPolicyConfirmationRuleConfig::from_json_str(
            r#"{
                "keywords": ["phishing", "漏洞利用"],
                "regex": ["(?i)credential\\s+stuffing"]
            }"#,
        )
        .unwrap();

        assert_eq!(
            rules,
            CyberPolicyConfirmationRuleConfig {
                keywords: vec!["phishing".to_owned(), "漏洞利用".to_owned()],
                regex: vec![r"(?i)credential\s+stuffing".to_owned()],
            }
        );
    }

    #[test]
    fn rejects_non_string_cyber_policy_confirmation_rules() {
        let error = CyberPolicyConfirmationRuleConfig::from_json_str(r#"{"keywords":["ok", 1]}"#)
            .unwrap_err()
            .to_string();

        assert!(error.contains("keywords[1]"));
    }

    #[test]
    fn administrator_overrides_take_precedence_without_accepting_secrets() {
        let mut source = EnvConfigSource::default();
        source.dotenv_values.insert(
            "AI_BRIDGE_UPSTREAM_BASE_URL".to_owned(),
            "http://dotenv.example".to_owned(),
        );
        source.admin_override_values.insert(
            "AI_BRIDGE_UPSTREAM_BASE_URL".to_owned(),
            "http://admin.example".to_owned(),
        );

        assert_eq!(
            source.value("AI_BRIDGE_UPSTREAM_BASE_URL").as_deref(),
            Some("http://admin.example")
        );
        assert!(!ADMIN_EDITABLE_KEYS.contains(&"AI_BRIDGE_ADMIN_SESSION_SECRET"));
        assert!(
            validate_admin_override_keys(&HashMap::from([(
                "AI_BRIDGE_ADMIN_SESSION_SECRET".to_owned(),
                "must-not-be-written".to_owned(),
            )]))
            .is_err()
        );
    }
}
