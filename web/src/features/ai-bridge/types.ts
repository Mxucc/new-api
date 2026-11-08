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

export type AiBridgeTab =
  | 'overview'
  | 'cooldowns'
  | 'access'
  | 'config'
  | 'logs'

/** `GET /ai-bridge/api/me` payload. */
export interface AiBridgeIdentity {
  id: number
  username: string
  role: number
  csrf_token: string
}

export interface AiBridgeConcurrency {
  active: number
  tracked_keys: number
}

export interface AiBridgeRetryCooldown {
  id: string
  fingerprint: string
  remaining_secs: number
  reason: string
}

export interface AiBridgeRetryCooldowns {
  total: number
  truncated: boolean
  entries: AiBridgeRetryCooldown[]
}

export interface AiBridgeAuthCooldown {
  id: string
  scope: string
  identifier: string
  remaining_secs: number
  status: number
  reason: string
}

export interface AiBridgeAuthCooldowns {
  total: number
  truncated: boolean
  cooldowns: AiBridgeAuthCooldown[]
}

export interface AiBridgeKeyEntry {
  id: string
  masked: string
  fingerprint: string
}

export interface AiBridgeBlacklist {
  total_api_keys: number
  total_client_ips: number
  api_keys_truncated: boolean
  client_ips_truncated: boolean
  api_keys: AiBridgeKeyEntry[]
  client_ips: string[]
}

export interface AiBridgeWhitelist {
  total_api_keys: number
  api_keys_truncated: boolean
  api_keys: AiBridgeKeyEntry[]
}

export type AiBridgeEventLevel = 'info' | 'warn' | 'error'

export type AiBridgeEventName =
  | 'request'
  | 'upstream_retry'
  | 'admin_mutation'
  | 'config_reload'

export interface AiBridgeEvent {
  id: number
  timestamp_ms: number
  level: AiBridgeEventLevel | string
  event: AiBridgeEventName | string
  fields: Record<string, unknown>
}

export interface AiBridgeObservability {
  requests_total: number
  responses_2xx: number
  responses_4xx: number
  responses_5xx: number
  upstream_retries: number
  admin_mutations: number
  reload_successes: number
  reload_failures: number
  event_capacity: number
  events: AiBridgeEvent[]
}

export interface AiBridgeSnapshot {
  generated_at_ms: number
  uptime_secs: number
  concurrency: AiBridgeConcurrency
  retry_cooldowns: AiBridgeRetryCooldowns
  auth_cooldowns: AiBridgeAuthCooldowns
  blacklist: AiBridgeBlacklist
  whitelist: AiBridgeWhitelist
  observability: AiBridgeObservability
}

export type AiBridgeConfigFieldKind =
  | 'text'
  | 'number'
  | 'boolean'
  | 'select'
  | 'json'

export interface AiBridgeConfigField {
  key: string
  /** Server-side Chinese label; the console renders its own i18n label. */
  label: string
  /** Server-side Chinese group; the console renders its own i18n group. */
  group: string
  kind: AiBridgeConfigFieldKind | string
  value: string
  overridden: boolean
  min: number | null
  max: number | null
  options: string[]
}

export interface AiBridgePolicies {
  cyber_policy_ban_enabled: boolean
  auth_failure_cooldown_enabled: boolean
  auth_failure_policy: string
  upstream_error_cooldown_enabled: boolean
  upstream_502_cooldown_enabled: boolean
  stream_event_stall_retry_enabled: boolean
}

export interface AiBridgeOverrideFile {
  path: string
  exists: boolean
  writable: boolean
  overridden_fields: number
  issue: string | null
}

export interface AiBridgeFileDiagnostic {
  kind: string
  status: string
  path: string
  exists: boolean
  readable: boolean
  size_bytes: number | null
  modified_at_ms: number | null
  cached_entries: number
  issue: string | null
}

export interface AiBridgeControl {
  generated_at_ms: number
  override_file: AiBridgeOverrideFile
  fields: AiBridgeConfigField[]
  policies: AiBridgePolicies
  files: AiBridgeFileDiagnostic[]
}

export type AiBridgeCooldownKind = 'auth_api_key' | 'auth_client_ip' | 'retry'

export type AiBridgeListKind =
  | 'blacklist_key'
  | 'blacklist_ip'
  | 'whitelist_key'

export interface AiBridgeMutationResult {
  ok: boolean
  result: string
}

export interface AiBridgeListMutationResult {
  ok: boolean
  result: 'created' | 'updated' | 'deleted'
}
