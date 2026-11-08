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

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object'
}

export interface AiBridgeErrorDetails {
  status?: number
  code?: string
}

/**
 * The ai-bridge service answers failures as
 * `{"error": {"code": "...", "message": "..."}}`.
 */
export function readAiBridgeError(error: unknown): AiBridgeErrorDetails {
  if (!isRecord(error)) return {}
  const response = isRecord(error.response) ? error.response : undefined
  if (!response) return {}
  const status =
    typeof response.status === 'number' ? response.status : undefined
  const data = isRecord(response.data) ? response.data : undefined
  const payload = data && isRecord(data.error) ? data.error : undefined
  const code = typeof payload?.code === 'string' ? payload.code : undefined
  return { status, code }
}

/** Server error code -> i18n message key shown to the administrator. */
export const AI_BRIDGE_ERROR_MESSAGE_KEYS: Record<string, string> = {
  list_entry_exists: 'This value already exists in this list.',
  list_entry_not_found: 'The list entry no longer exists.',
  invalid_list_value: 'The value is invalid for this list.',
  invalid_list_id: 'The list entry identifier is invalid.',
  unknown_list: 'This policy list is not available.',
  cooldown_not_found: 'This cooldown is no longer active.',
  invalid_cooldown_id: 'The cooldown identifier is invalid.',
  unknown_cooldown_kind: 'This cooldown type is not available.',
  invalid_admin_config: 'The configuration value is invalid.',
  config_update_failed:
    'The configuration was rejected and the previous file was restored.',
  reload_failed: 'Reloading the configuration failed.',
  admin_auth_required: 'Administrator session required',
  admin_access_forbidden: 'Administrator access only',
  admin_console_unavailable:
    'The AI Bridge console is not enabled on this deployment',
}

export function resolveAiBridgeErrorMessageKey(
  error: unknown,
  fallback: string
): string {
  const { code } = readAiBridgeError(error)
  return (code && AI_BRIDGE_ERROR_MESSAGE_KEYS[code]) || fallback
}

export type AiBridgeAccessState = 'unauthorized' | 'forbidden' | 'unavailable'

export const AI_BRIDGE_ACCESS_NOTICES: Record<
  AiBridgeAccessState,
  { title: string; description: string }
> = {
  unauthorized: {
    title: 'Administrator session required',
    description:
      'Sign in with an administrator account to open the AI Bridge console.',
  },
  forbidden: {
    title: 'Administrator access only',
    description:
      'This account does not have administrator access to the AI Bridge console.',
  },
  unavailable: {
    title: 'The AI Bridge console is not enabled on this deployment',
    description:
      'This deployment does not expose the AI Bridge administrator console, or the service is unreachable.',
  },
}

export function resolveAiBridgeAccessState(
  error: unknown
): AiBridgeAccessState {
  const { status } = readAiBridgeError(error)
  if (status === 401) return 'unauthorized'
  if (status === 403) return 'forbidden'
  return 'unavailable'
}
