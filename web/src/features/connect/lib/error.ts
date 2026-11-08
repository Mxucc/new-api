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
import { getServerErrorStatus } from '@/lib/server-error-message'

const GENERIC_AXIOS_STATUS_MESSAGE = /^Request failed with status code \d+$/

/**
 * The Connect service reports failures as `{"error": "<message>"}`. Prefer that
 * text over the axios wrapper message so the service's own reason is shown.
 */
export function getConnectErrorMessage(
  error: unknown,
  fallback: string
): string {
  const responseMessage = (
    error as { response?: { data?: { error?: unknown } } }
  )?.response?.data?.error
  if (typeof responseMessage === 'string' && responseMessage.trim()) {
    return responseMessage
  }

  const errorMessage = (error as { message?: unknown })?.message
  if (
    typeof errorMessage === 'string' &&
    errorMessage.trim() &&
    !GENERIC_AXIOS_STATUS_MESSAGE.test(errorMessage)
  ) {
    return errorMessage
  }

  return fallback
}

export function getConnectErrorStatus(error: unknown): number | undefined {
  return getServerErrorStatus(error)
}

/**
 * 404 means this deployment does not mount the Connect service; 401 means the
 * visitor has no session for it. Both are setup states, not request failures.
 */
export function isConnectServiceUnavailable(error: unknown): boolean {
  const status = getServerErrorStatus(error)
  return status === 401 || status === 404
}
