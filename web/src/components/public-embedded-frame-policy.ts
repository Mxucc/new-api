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

// 只有与站点自身同源的嵌入地址才放开 same-origin：它加载的是本站资源；
// 其它来源（含子域、其它端口与 blob:）一律留在严格沙箱里，
// 这样仓库里不再需要登记任何私有部署域名。
export function getPublicEmbeddedFrameSandbox(src: string): string {
  try {
    // 只接受绝对地址：相对路径无法判定来源，一律留在严格沙箱里。
    const url = new URL(src)
    const isHttp = url.protocol === 'http:' || url.protocol === 'https:'
    if (isHttp && url.origin === window.location.origin) {
      return `${STRICT_PUBLIC_FRAME_SANDBOX} allow-same-origin`
    }
  } catch {
    // Invalid administrator-provided URLs keep the strict sandbox.
  }

  return STRICT_PUBLIC_FRAME_SANDBOX
}
