import { describe, expect, it } from 'vitest'

import { actionRegistry } from './action-registry'

// WS07-S6: the memory mode on chat.save_thread_snapshot is one of its two closed values, or absent.
describe('chat.save_thread_snapshot memoryMode', () => {
  const action = actionRegistry.find((candidate) => candidate.id === 'chat.save_thread_snapshot')
  const schema = action?.inputSchema

  it('is registered', () => {
    expect(schema).toBeDefined()
  })

  it('accepts inherit and off, and an input that names no mode', () => {
    expect(schema?.safeParse({ threadId: 'thread-1', memoryMode: 'inherit' }).success).toBe(true)
    expect(schema?.safeParse({ threadId: 'thread-1', memoryMode: 'off' }).success).toBe(true)
    expect(schema?.safeParse({ threadId: 'thread-1' }).success).toBe(true)
  })

  it('rejects any other memory mode', () => {
    for (const memoryMode of ['on', 'OFF', ' off', '', 'default', null, true]) {
      expect(schema?.safeParse({ threadId: 'thread-1', memoryMode }).success, JSON.stringify(memoryMode)).toBe(false)
    }
  })
})
