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

const STRICT_PUBLIC_FRAME_SANDBOX =
  'allow-forms allow-popups allow-popups-to-escape-sandbox allow-scripts'

// The opaque-origin sandbox blocks Vite module assets on this deployment-owned site.
const TRUSTED_SAME_ORIGIN_FRAME_ORIGINS = new Set(['https://static.dli.li'])

export function getPublicEmbeddedFrameSandbox(src: string): string {
  try {
    const url = new URL(src)
    if (
      url.protocol === 'https:' &&
      TRUSTED_SAME_ORIGIN_FRAME_ORIGINS.has(url.origin)
    ) {
      return `${STRICT_PUBLIC_FRAME_SANDBOX} allow-same-origin`
    }
  } catch {
    // Invalid administrator-provided URLs keep the strict sandbox.
  }

  return STRICT_PUBLIC_FRAME_SANDBOX
}
