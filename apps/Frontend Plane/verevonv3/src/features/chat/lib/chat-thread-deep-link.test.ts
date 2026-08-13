import { describe, expect, it } from 'vitest'

import { readThreadDeepLink } from './chat-thread-deep-link'

describe('readThreadDeepLink', () => {
  it('accepts the canonical durable thread query parameter', () => {
    expect(readThreadDeepLink('?thread_id=thread-personal-1')).toBe('thread-personal-1')
  })

  it('rejects conflicting aliases and control characters', () => {
    expect(readThreadDeepLink('?thread_id=thread-a&threadId=thread-b')).toBeNull()
    expect(readThreadDeepLink('?thread_id=thread%00bad')).toBeNull()
  })
})
