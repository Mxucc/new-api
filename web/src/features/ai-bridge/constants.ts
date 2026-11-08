/*
Copyright (C) 2023-2026 QuantumNous

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU Affero General Public License as
published by the Free Software Foundation, either version 3 of the
License, or (at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
GNU Affero General Public License for more details.

You should have received a copy of the GNU Affero General Public License
along with this program. If not, see <https://www.gnu.org/licenses/>.

For commercial licensing, please contact support@quantumnous.com
*/
import {
  Activity,
  ScrollText,
  Settings,
  ShieldCheck,
  TimerReset,
  type LucideIcon,
} from 'lucide-react'

import type { AiBridgeCooldownKind, AiBridgeTab } from './types'

export const AI_BRIDGE_TABS: {
  value: AiBridgeTab
  label: string
  icon: LucideIcon
}[] = [
  { value: 'overview', label: 'Overview', icon: Activity },
  { value: 'cooldowns', label: 'Cooldowns', icon: TimerReset },
  { value: 'access', label: 'Access Lists', icon: ShieldCheck },
  { value: 'config', label: 'Configuration', icon: Settings },
  { value: 'logs', label: 'Logs', icon: ScrollText },
]

export const DEFAULT_AI_BRIDGE_TAB: AiBridgeTab = 'overview'

export function resolveAiBridgeTab(value: unknown): AiBridgeTab {
  const match = AI_BRIDGE_TABS.find((tab) => tab.value === value)
  return match?.value ?? DEFAULT_AI_BRIDGE_TAB
}

/** Server group value (hardcoded Chinese in the service) -> i18n group label. */
export const AI_BRIDGE_GROUP_LABEL_KEYS: Record<string, string> = {
  上游: 'Upstream',
  流式: 'Streaming',
  重试: 'Retry',
  鉴权: 'Authentication',
  请求: 'Request',
  兼容: 'Compatibility',
  观测: 'Observability',
  策略: 'Policy',
  工具: 'Tools',
}

/** Server config key -> i18n field label. */
export const AI_BRIDGE_FIELD_LABEL_KEYS: Record<string, string> = {
  AI_BRIDGE_UPSTREAM_BASE_URL: 'Upstream base URL',
  AI_BRIDGE_REQUEST_TIMEOUT_SECS: 'Request timeout (seconds)',
  AI_BRIDGE_CONNECT_TIMEOUT_SECS: 'Connect timeout (seconds)',
  AI_BRIDGE_HEARTBEAT_INTERVAL_SECS: 'Heartbeat interval (seconds)',
  AI_BRIDGE_STREAM_EVENT_STALL_RETRY_ENABLED: 'Retry stalled streams',
  AI_BRIDGE_STREAM_EVENT_STALL_TIMEOUT_SECS: 'Stream stall timeout (seconds)',
  AI_BRIDGE_STREAM_EVENT_STALL_MAX_RETRIES: 'Stream stall max retries',
  AI_BRIDGE_STREAM_EARLY_HEARTBEAT: 'Early heartbeat before the first byte',
  AI_BRIDGE_STREAM_SYNTHETIC_FIRST_SPACE: 'Synthesize a leading space',
  AI_BRIDGE_UPSTREAM_429_INITIAL_DELAY_SECS: '429 initial delay (seconds)',
  AI_BRIDGE_UPSTREAM_429_MAX_DELAY_SECS: '429 maximum delay (seconds)',
  AI_BRIDGE_UPSTREAM_429_MAX_RETRIES: '429 maximum retries (0 means unlimited)',
  AI_BRIDGE_UPSTREAM_ERROR_COOLDOWN_ENABLED: 'Cross-request cooldown after 429',
  AI_BRIDGE_UPSTREAM_502_COOLDOWN_ENABLED: 'Cooldown retry after 502',
  AI_BRIDGE_UPSTREAM_502_COOLDOWN_ACCUMULATE_ENABLED: 'Accumulate 502 cooldown',
  AI_BRIDGE_UPSTREAM_502_COOLDOWN_SECS: '502 cooldown (seconds)',
  AI_BRIDGE_UPSTREAM_502_MAX_RETRIES: '502 maximum retries (0 means unlimited)',
  AI_BRIDGE_AUTH_FAILURE_COOLDOWN_ENABLED:
    'Cooldown after authentication failures',
  AI_BRIDGE_AUTH_FAILURE_POLICY: 'Authentication cooldown policy',
  AI_BRIDGE_AUTH_FAILURE_WINDOW_SECS: 'Authentication failure window (seconds)',
  AI_BRIDGE_AUTH_FAILURE_THRESHOLD: 'API Key failure threshold',
  AI_BRIDGE_AUTH_FAILURE_COOLDOWN_SECS: 'API Key cooldown (seconds)',
  AI_BRIDGE_AUTH_FAILURE_IP_THRESHOLD: 'Client IP failure threshold',
  AI_BRIDGE_AUTH_FAILURE_ERROR_BODY_MAX_BYTES:
    'Authentication error body limit (bytes)',
  AI_BRIDGE_MAX_BODY_BYTES: 'Request body limit (bytes)',
  AI_BRIDGE_MAX_UPLOAD_BODY_BYTES: 'Upload body limit (bytes)',
  AI_BRIDGE_IMAGE_RESPONSE_MODE: 'Image response mode',
  AI_BRIDGE_RESPONSES_NONSTREAM_CHAT_FALLBACK:
    'Responses non-stream chat fallback',
  AI_BRIDGE_DEBUG_OUTPUT: 'Additional debug logging',
  AI_BRIDGE_CYBER_POLICY_BAN_ENABLED: 'Cyber Policy banning',
  AI_BRIDGE_CYBER_POLICY_BLACKLIST_FILE: 'API Key blacklist file',
  AI_BRIDGE_CYBER_POLICY_IP_BLACKLIST_FILE: 'IP blacklist file',
  AI_BRIDGE_CYBER_POLICY_WHITELIST_FILE: 'API Key whitelist file',
  AI_BRIDGE_CYBER_POLICY_CONFIRMATION_RULES_FILE: 'Confirmation rules file',
  AI_BRIDGE_FILTER_TOOLS_BY_MODEL: 'Per-model tool filter (JSON)',
  AI_BRIDGE_TOOL_FILTER_SCAN_LIMIT: 'Tool filter scan limit (bytes)',
}

export const AI_BRIDGE_POLICY_LABEL_KEYS = {
  cyber_policy_ban_enabled: 'Cyber Policy banning',
  auth_failure_cooldown_enabled: 'Cooldown after authentication failures',
  auth_failure_policy: 'Authentication cooldown policy',
  upstream_error_cooldown_enabled: 'Cross-request cooldown after 429',
  upstream_502_cooldown_enabled: 'Cooldown retry after 502',
  stream_event_stall_retry_enabled: 'Retry stalled streams',
} as const

export const AI_BRIDGE_AUTH_POLICY_LABEL_KEYS: Record<string, string> = {
  off: 'Off',
  conservative: 'Conservative',
  balanced: 'Balanced',
  strict: 'Strict',
}

export const AI_BRIDGE_COOLDOWN_SCOPE_LABEL_KEYS: Record<string, string> = {
  api_key: 'API Key',
  client_ip: 'Client IP',
}

export const AI_BRIDGE_COOLDOWN_REASON_LABEL_KEYS: Record<string, string> = {
  rate_limited: 'Rate limited by the upstream provider',
  capacity: 'Upstream capacity is exhausted',
  bad_gateway: 'Upstream gateway error',
  repeated_upstream_auth_failure: 'Repeated upstream authentication failures',
}

/** Retry cooldowns are keyed by a hashed identity, never by a raw API Key. */
export const AI_BRIDGE_COOLDOWN_KIND_BY_SCOPE: Record<
  string,
  AiBridgeCooldownKind
> = {
  api_key: 'auth_api_key',
  client_ip: 'auth_client_ip',
}

export const AI_BRIDGE_FILE_KIND_LABEL_KEYS: Record<string, string> = {
  blacklist_key: 'API Key blacklist',
  blacklist_ip: 'IP blacklist',
  whitelist_key: 'API Key whitelist',
  confirmation_rules: 'Confirmation rules',
}

export const AI_BRIDGE_FILE_STATUS_LABEL_KEYS: Record<string, string> = {
  ok: 'Available',
  missing: 'Not created',
  error: 'Error',
}

export const AI_BRIDGE_SNAPSHOT_REFETCH_INTERVAL_MS = 5_000
