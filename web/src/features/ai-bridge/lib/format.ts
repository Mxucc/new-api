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
import type { AiBridgeEvent } from '../types'

/**
 * Compact duration used for uptime and cooldown countdowns. Units stay ASCII,
 * matching `formatUseTime` in `@/lib/format`.
 */
export function formatDuration(totalSeconds: number): string {
  if (!Number.isFinite(totalSeconds) || totalSeconds < 0) return '-'
  const seconds = Math.floor(totalSeconds)
  const days = Math.floor(seconds / 86_400)
  const hours = Math.floor((seconds % 86_400) / 3_600)
  const minutes = Math.floor((seconds % 3_600) / 60)
  const rest = seconds % 60
  if (days > 0) return `${days}d ${hours}h ${minutes}m`
  if (hours > 0) return `${hours}h ${minutes}m`
  if (minutes > 0) return `${minutes}m ${rest}s`
  return `${rest}s`
}

/** Values arriving from the observability stream, kept as `key=value` pairs. */
export function formatEventFields(fields: Record<string, unknown>): string {
  const parts: string[] = []
  for (const [key, value] of Object.entries(fields)) {
    if (value == null || value === '') continue
    if (typeof value === 'object') continue
    parts.push(`${key}=${String(value)}`)
  }
  return parts.join(' ')
}

export function formatEventSummary(event: AiBridgeEvent): string {
  return formatEventFields(event.fields)
}

const IPV4_PART = /^(?:0|[1-9]\d{0,2})$/

function isIpv4Address(value: string): boolean {
  const parts = value.split('.')
  if (parts.length !== 4) return false
  return parts.every((part) => IPV4_PART.test(part) && Number(part) <= 255)
}

function isIpv6Address(value: string): boolean {
  if (!value.includes(':')) return false
  try {
    const hostname = new URL(`http://[${value}]/`).hostname
    return hostname.startsWith('[') && hostname.endsWith(']')
  } catch {
    return false
  }
}

/** Mirrors the service's `IpAddr` parsing for the IP blacklist input. */
export function isIpAddress(value: string): boolean {
  const trimmed = value.trim()
  if (!trimmed) return false
  return isIpv4Address(trimmed) || isIpv6Address(trimmed)
}
