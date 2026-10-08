import { describe, expect, it } from 'vitest'

import { chatThreadHref, readThreadDeepLink } from './chat-thread-deep-link'

describe('readThreadDeepLink', () => {
  it('accepts the canonical durable thread query parameter', () => {
    expect(readThreadDeepLink('?thread_id=thread-personal-1')).toBe('thread-personal-1')
  })

  it('rejects conflicting aliases and control characters', () => {
    expect(readThreadDeepLink('?thread_id=thread-a&threadId=thread-b')).toBeNull()
    expect(readThreadDeepLink('?thread_id=thread%00bad')).toBeNull()
  })
})

describe('chatThreadHref', () => {
  const readBack = (href: string) => readThreadDeepLink(new URL(href, 'https://verevon.test').search)

  it('builds the canonical deep link for a plain ID', () => {
    expect(chatThreadHref('thread-personal-1')).toBe('/chat?thread_id=thread-personal-1')
  })

  it('round-trips through readThreadDeepLink, including IDs that need encoding', () => {
    for (const threadId of [
      'thread-personal-1',
      '0b1c9e0e-6f0a-4c39-9a52-1f1c1e6f2a11',
      'thread with spaces',
      'a&threadId=b',
      'a+b=c%20d#frag?x',
      'tråd/med/åøæ',
      'x'.repeat(200),
    ]) {
      const href = chatThreadHref(threadId)
      expect(href.startsWith('/chat?thread_id=')).toBe(true)
      expect(readBack(href)).toBe(threadId)
    }
  })

  it('never lets an ID smuggle in a second parameter or a fragment', () => {
    const url = new URL(chatThreadHref('a&threadId=b#frag'), 'https://verevon.test')
    expect([...url.searchParams.keys()]).toEqual(['thread_id'])
    expect(url.hash).toBe('')
  })

  it('falls back to plain /chat for an ID Chat would not read back', () => {
    for (const threadId of ['', '   ', ' padded ', 'x'.repeat(201), 'bad\u0000id', 'bad\nid', '\ud800']) {
      expect(chatThreadHref(threadId)).toBe('/chat')
    }
  })
})
