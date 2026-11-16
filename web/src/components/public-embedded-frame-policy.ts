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
// trustedFrameOrigins 读取部署声明的可信嵌入来源（EMBEDDED_FRAME_ORIGINS，
// 由网关注入到 index.html 的 meta 标签）。站点把静态资源放在子域上时必须
// 放开 same-origin，否则 module 脚本在 opaque origin 里会被 CORS 拦掉。
function trustedFrameOrigins(): string[] {
  if (typeof document === 'undefined') {
    return []
  }
  const content = document
    .querySelector('meta[name="embedded-frame-origins"]')
    ?.getAttribute('content')
  if (!content) {
    return []
  }
  return content
    .split(',')
    .map((value) => value.trim())
    .filter((value) => value.length > 0)
}

export function getPublicEmbeddedFrameSandbox(src: string): string {
  try {
    // 只接受绝对地址：相对路径无法判定来源，一律留在严格沙箱里。
    const url = new URL(src)
    const isHttp = url.protocol === 'http:' || url.protocol === 'https:'
    const isTrusted =
      isHttp &&
      (url.origin === window.location.origin ||
        trustedFrameOrigins().includes(url.origin))
    if (isTrusted) {
      return `${STRICT_PUBLIC_FRAME_SANDBOX} allow-same-origin`
    }
  } catch {
    // Invalid administrator-provided URLs keep the strict sandbox.
  }

  return STRICT_PUBLIC_FRAME_SANDBOX
}
