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
import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { getPublicEmbeddedFrameSandbox } from './public-embedded-frame-policy'

const STRICT_SANDBOX =
  'allow-forms allow-popups allow-popups-to-escape-sandbox allow-scripts'
const TRUSTED_SANDBOX = `${STRICT_SANDBOX} allow-same-origin`

describe('public embedded frame sandbox', () => {
  test('restores the origin only for the trusted static site', () => {
    const trustedUrls = [
      'https://static.dli.li/?embed=1&theme=dark',
      'https://static.dli.li:443/about/',
    ]

    for (const url of trustedUrls) {
      assert.equal(getPublicEmbeddedFrameSandbox(url), TRUSTED_SANDBOX, url)
    }
  })

  test('keeps arbitrary and deceptive URLs in the strict sandbox', () => {
    const untrustedUrls = [
      'https://example.com/',
      'https://dli.li/',
      'https://static.dli.li.example.com/',
      'http://static.dli.li/',
      'blob:https://static.dli.li/frame-id',
      'https://static.dli.li:444/',
      'not-a-url',
    ]

    for (const url of untrustedUrls) {
      assert.equal(getPublicEmbeddedFrameSandbox(url), STRICT_SANDBOX, url)
    }
  })
})
