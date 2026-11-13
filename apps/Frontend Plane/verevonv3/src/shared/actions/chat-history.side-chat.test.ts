import { describe, expect, it } from 'vitest'
import { SIDE_CHAT_MODEL_MAX_CHARS, parseChatHistoryAction, threadSideChatAction } from './chat-history'

// The side question's action contract (F-t3-069, W4-BYO and DEC-V-R8): the question, and the model a connection thread
// is served with. The model is optional, because a platform thread names none; when it is named it is one model id.

describe('thread.side_chat action contract', () => {
  it('keeps a platform question as it was: the question alone, with no model key', () => {
    const parsed = parseChatHistoryAction({ type: 'thread.side_chat', threadId: ' thread-1 ', question: ' Hva ble sagt? ' })
    expect(parsed).toEqual({ type: 'thread.side_chat', threadId: 'thread-1', question: 'Hva ble sagt?' })
    expect(parsed).not.toHaveProperty('model')
  })

  it('takes the model a connection thread names, trimmed', () => {
    expect(
      parseChatHistoryAction({
        type: 'thread.side_chat',
        threadId: 'thread-1',
        question: 'Oppsummer',
        model: '  gpt-5-codex ',
      }),
    ).toEqual({ type: 'thread.side_chat', threadId: 'thread-1', question: 'Oppsummer', model: 'gpt-5-codex' })
  })

  it('refuses a blank model: a caller that names no model leaves the key out', () => {
    expect(() => threadSideChatAction.parse({ type: 'thread.side_chat', threadId: 'thread-1', question: 'Hei', model: '' })).toThrow()
    expect(() =>
      threadSideChatAction.parse({ type: 'thread.side_chat', threadId: 'thread-1', question: 'Hei', model: '   ' }),
    ).toThrow()
  })

  it('refuses a model id with a space or a control character in it', () => {
    expect(() =>
      threadSideChatAction.parse({ type: 'thread.side_chat', threadId: 'thread-1', question: 'Hei', model: 'gpt 5' }),
    ).toThrow()
    expect(() =>
      threadSideChatAction.parse({ type: 'thread.side_chat', threadId: 'thread-1', question: 'Hei', model: 'gpt\u0007-5' }),
    ).toThrow()
    expect(() =>
      threadSideChatAction.parse({ type: 'thread.side_chat', threadId: 'thread-1', question: 'Hei', model: 'gpt\n-5' }),
    ).toThrow()
  })

  it('takes a model of exactly the most characters, and refuses one longer', () => {
    expect(SIDE_CHAT_MODEL_MAX_CHARS).toBe(200)
    const atLimit = 'm'.repeat(SIDE_CHAT_MODEL_MAX_CHARS)
    expect(threadSideChatAction.parse({ type: 'thread.side_chat', threadId: 'thread-1', question: 'Hei', model: atLimit }).model).toBe(
      atLimit,
    )
    expect(() =>
      threadSideChatAction.parse({
        type: 'thread.side_chat',
        threadId: 'thread-1',
        question: 'Hei',
        model: 'm'.repeat(SIDE_CHAT_MODEL_MAX_CHARS + 1),
      }),
    ).toThrow()
  })

  it('counts the model limit in characters, as the gateway does, not in UTF-16 units', () => {
    // A character outside the basic plane is two UTF-16 units but one character: 200 of them are still within the limit.
    const astral = '\u{1D49C}'.repeat(SIDE_CHAT_MODEL_MAX_CHARS)
    expect(astral.length).toBe(SIDE_CHAT_MODEL_MAX_CHARS * 2)
    expect(threadSideChatAction.parse({ type: 'thread.side_chat', threadId: 'thread-1', question: 'Hei', model: astral }).model).toBe(
      astral,
    )
    expect(() =>
      threadSideChatAction.parse({
        type: 'thread.side_chat',
        threadId: 'thread-1',
        question: 'Hei',
        model: '\u{1D49C}'.repeat(SIDE_CHAT_MODEL_MAX_CHARS + 1),
      }),
    ).toThrow()
  })

  it('still bounds the question and needs a thread', () => {
    expect(() => parseChatHistoryAction({ type: 'thread.side_chat', threadId: 'thread-1', question: '   ' })).toThrow()
    expect(() =>
      parseChatHistoryAction({ type: 'thread.side_chat', threadId: 'thread-1', question: 'q'.repeat(2001) }),
    ).toThrow()
    expect(() => parseChatHistoryAction({ type: 'thread.side_chat', threadId: '  ', question: 'Hei' })).toThrow()
  })
})
