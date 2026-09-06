// @vitest-environment jsdom

import { beforeEach, describe, expect, it } from 'vitest'
import {
  MAX_PINNED_MESSAGES,
  clearPinnedMessages,
  pinnedMessagesFull,
  readPinnedMessages,
  togglePinnedMessage,
} from './chat-pinned-messages'

describe('pinned messages', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  it('starts empty and round-trips a pin', () => {
    expect(readPinnedMessages('t1')).toEqual([])
    expect(togglePinnedMessage('t1', 'm1')).toEqual(['m1'])
    expect(readPinnedMessages('t1')).toEqual(['m1'])
  })

  it('unpins the same message', () => {
    togglePinnedMessage('t1', 'm1')
    expect(togglePinnedMessage('t1', 'm1')).toEqual([])
    expect(readPinnedMessages('t1')).toEqual([])
  })

  it('keeps threads separate', () => {
    togglePinnedMessage('t1', 'm1')
    togglePinnedMessage('t2', 'm2')
    expect(readPinnedMessages('t1')).toEqual(['m1'])
    expect(readPinnedMessages('t2')).toEqual(['m2'])
  })

  /**
   * The order matters beyond tidiness: the server keeps the caller's order when
   * its own cap bites, so the earliest choices are the ones that survive.
   */
  it('preserves the order the user pinned in', () => {
    togglePinnedMessage('t1', 'm3')
    togglePinnedMessage('t1', 'm1')
    expect(readPinnedMessages('t1')).toEqual(['m3', 'm1'])
  })

  /**
   * Refused, not rotated. A pin silently disappearing because another was added
   * is not a behaviour a reader can predict.
   */
  it('refuses a pin past the cap instead of dropping the oldest', () => {
    for (let index = 0; index < MAX_PINNED_MESSAGES; index += 1) {
      togglePinnedMessage('t1', `m${index}`)
    }
    const full = readPinnedMessages('t1')
    expect(full).toHaveLength(MAX_PINNED_MESSAGES)
    expect(pinnedMessagesFull(full)).toBe(true)

    expect(togglePinnedMessage('t1', 'one-too-many')).toEqual(full)
    expect(readPinnedMessages('t1')).toEqual(full)
    // Unpinning still works while full — otherwise the cap would be a trap.
    expect(togglePinnedMessage('t1', 'm0')).not.toContain('m0')
  })

  it('ignores empty ids on either axis', () => {
    expect(togglePinnedMessage('', 'm1')).toEqual([])
    expect(togglePinnedMessage('t1', '  ')).toEqual([])
    expect(readPinnedMessages('')).toEqual([])
  })

  it('forgets a thread on request', () => {
    togglePinnedMessage('t1', 'm1')
    togglePinnedMessage('t2', 'm2')
    clearPinnedMessages('t1')
    expect(readPinnedMessages('t1')).toEqual([])
    expect(readPinnedMessages('t2')).toEqual(['m2'])
  })

  it('removes the key entirely once nothing is pinned', () => {
    togglePinnedMessage('t1', 'm1')
    togglePinnedMessage('t1', 'm1')
    expect(window.localStorage.getItem('verevon.chat.pinnedMessages.v1')).toBeNull()
  })

  /** A hand-edited or half-written value must not break the composer. */
  it('survives a corrupt stored value', () => {
    window.localStorage.setItem('verevon.chat.pinnedMessages.v1', '{not json')
    expect(readPinnedMessages('t1')).toEqual([])

    window.localStorage.setItem('verevon.chat.pinnedMessages.v1', '["an array, not an object"]')
    expect(readPinnedMessages('t1')).toEqual([])

    window.localStorage.setItem(
      'verevon.chat.pinnedMessages.v1',
      JSON.stringify({ t1: ['m1', 42, '', null, 'm2'] }),
    )
    expect(readPinnedMessages('t1')).toEqual(['m1', 'm2'])
  })

  /**
   * A stored list longer than the cap — written by an older build, or by hand —
   * is trimmed on read, so the UI never shows more pins than the server honours.
   */
  it('trims a stored list that exceeds the cap', () => {
    const tooMany = Array.from({ length: MAX_PINNED_MESSAGES + 3 }, (_, i) => `m${i}`)
    window.localStorage.setItem(
      'verevon.chat.pinnedMessages.v1',
      JSON.stringify({ t1: tooMany }),
    )
    expect(readPinnedMessages('t1')).toHaveLength(MAX_PINNED_MESSAGES)
  })
})
