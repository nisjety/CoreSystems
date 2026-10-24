import { describe, expect, it } from 'vitest'
import { parseChatHistoryAction } from './chat-history'

describe('chat history action contract', () => {
  it('accepts archive and unarchive for one thread', () => {
    expect(parseChatHistoryAction({ type: 'thread.archive', threadId: ' thread-1 ' })).toEqual({
      type: 'thread.archive',
      threadId: 'thread-1',
    })
    expect(parseChatHistoryAction({ type: 'thread.unarchive', threadId: 'thread-1' }).type).toBe('thread.unarchive')
  })

  it('refuses an unknown action type and an empty thread id', () => {
    expect(() => parseChatHistoryAction({ type: 'thread.delete', threadId: 'thread-1' })).toThrow()
    expect(() => parseChatHistoryAction({ type: 'thread.archive', threadId: '   ' })).toThrow()
  })

  it('refuses a rewind that has not been confirmed', () => {
    expect(() =>
      parseChatHistoryAction({ type: 'thread.rewind', threadId: 'thread-1', messageId: 'message-4' }),
    ).toThrow()
    expect(
      parseChatHistoryAction({ type: 'thread.rewind', threadId: 'thread-1', messageId: 'message-4', confirm: true }).type,
    ).toBe('thread.rewind')
  })
})
