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

import type { Message, MessageRole } from '../../types'
import { updateTargetAssistantMessage } from './message-update-utils'

function createMessage(key: string, from: MessageRole): Message {
  return {
    key,
    from,
    versions: [{ id: `${key}-version`, content: key }],
  }
}

function replaceContent(message: Message, content: string): Message {
  return {
    ...message,
    versions: [{ ...message.versions[0], content }],
  }
}

describe('target assistant message updates', () => {
  test('updates the bound assistant instead of the last assistant', () => {
    const messages = [
      createMessage('assistant-target', 'assistant'),
      createMessage('user-later', 'user'),
      createMessage('assistant-later', 'assistant'),
    ]

    const updated = updateTargetAssistantMessage(
      messages,
      'assistant-target',
      (message) => replaceContent(message, 'target response')
    )

    assert.equal(updated[0].versions[0].content, 'target response')
    assert.equal(updated[2].versions[0].content, 'assistant-later')
  })

  test('preserves messages when a supplied target no longer exists', () => {
    const messages = [createMessage('assistant-later', 'assistant')]

    const updated = updateTargetAssistantMessage(
      messages,
      'assistant-missing',
      (message) => replaceContent(message, 'unexpected response')
    )

    assert.strictEqual(updated, messages)
  })

  test('falls back to the last assistant for legacy callers without a key', () => {
    const messages = [createMessage('assistant-later', 'assistant')]

    const updated = updateTargetAssistantMessage(
      messages,
      undefined,
      (message) => replaceContent(message, 'legacy response')
    )

    assert.equal(updated[0].versions[0].content, 'legacy response')
  })
})
