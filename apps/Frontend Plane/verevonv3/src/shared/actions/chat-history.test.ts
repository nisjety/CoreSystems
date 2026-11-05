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

  it('accepts a search for a trimmed, non-empty query, and refuses an empty one', () => {
    expect(parseChatHistoryAction({ type: 'thread.search', query: '  receipt ' })).toEqual({
      type: 'thread.search',
      query: 'receipt',
    })
    expect(() => parseChatHistoryAction({ type: 'thread.search', query: '   ' })).toThrow()
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

  it('accepts an export of one thread in either format and profile, with the id trimmed', () => {
    expect(
      parseChatHistoryAction({ type: 'thread.export', threadId: ' thread-1 ', format: 'markdown', profile: 'redacted' }),
    ).toEqual({ type: 'thread.export', threadId: 'thread-1', format: 'markdown', profile: 'redacted' })
    expect(parseChatHistoryAction({ type: 'thread.export', threadId: 'thread-1', format: 'json', profile: 'full' }).type).toBe(
      'thread.export',
    )
  })

  it('refuses an export with a choice missing, an unknown format or profile, or a blank thread', () => {
    // Neither choice has a default, so a caller that leaves one out is refused.
    expect(() => parseChatHistoryAction({ type: 'thread.export', threadId: 'thread-1', format: 'json' })).toThrow()
    expect(() => parseChatHistoryAction({ type: 'thread.export', threadId: 'thread-1', profile: 'full' })).toThrow()
    expect(() => parseChatHistoryAction({ type: 'thread.export', threadId: 'thread-1', format: 'pdf', profile: 'full' })).toThrow()
    expect(() => parseChatHistoryAction({ type: 'thread.export', threadId: 'thread-1', format: 'json', profile: 'everything' })).toThrow()
    expect(() => parseChatHistoryAction({ type: 'thread.export', threadId: '   ', format: 'json', profile: 'full' })).toThrow()
  })
})
