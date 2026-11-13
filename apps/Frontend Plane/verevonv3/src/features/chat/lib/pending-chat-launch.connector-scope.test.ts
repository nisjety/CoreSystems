// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { consumePendingChatLaunch, writePendingChatLaunch } from './pending-chat-launch'

// W2-WS23-S6-a: the connected apps a person picked on Home survive the hand-off to the chat page. The ids are
// opaque connection ids (never credentials), normalised by the composer's own connector-scope rule.

const KEY = 'verevon.chat.pendingLaunch'

beforeEach(() => {
  window.sessionStorage.clear()
})

afterEach(() => {
  consumePendingChatLaunch()
})

describe('pending chat launch connector scope', () => {
  it('keeps the picked ids through a write and a read', async () => {
    await writePendingChatLaunch({ text: 'read my mail', connectorScope: ['conn_gmail', 'conn-drive'] })
    expect(consumePendingChatLaunch()).toMatchObject({
      text: 'read my mail',
      connectorScope: ['conn_gmail', 'conn-drive'],
    })
  })

  it('keeps the ids on a zero-retention launch, which stays in memory only', async () => {
    await writePendingChatLaunch({ text: 'read my mail', zdr: true, connectorScope: ['conn_gmail'] })
    expect(window.sessionStorage.getItem(KEY)).toBeNull()
    expect(consumePendingChatLaunch()).toMatchObject({ zdr: true, connectorScope: ['conn_gmail'] })
  })

  it.each([
    ['missing', undefined],
    ['empty', []],
    ['only ids the gateway would refuse', ['', '   ', 'has space', 'x'.repeat(129)]],
  ])('omits the key when the list is %s', async (_label, connectorScope) => {
    await writePendingChatLaunch({ text: 'hello', connectorScope })
    const launch = consumePendingChatLaunch()
    expect(launch).not.toBeNull()
    expect(launch).not.toHaveProperty('connectorScope')
  })

  it('normalises the ids as the composer does: trimmed, deduplicated, at most eight', async () => {
    await writePendingChatLaunch({
      text: 'hello',
      connectorScope: [' a ', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i'],
    })
    expect(consumePendingChatLaunch()?.connectorScope).toEqual(['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'])
  })

  it.each([
    ['a non-string entry', ['conn_gmail', 7]],
    ['a null entry', [null]],
    ['a string instead of a list', 'conn_gmail'],
    ['an object instead of a list', { 0: 'conn_gmail' }],
  ])('treats a stored launch with %s as unreadable', (_label, connectorScope) => {
    window.sessionStorage.setItem(KEY, JSON.stringify({ text: 'tampered', connectorScope }))
    expect(consumePendingChatLaunch()).toBeNull()
  })
})
