import { describe, expect, it } from 'vitest'
import { assistantChatDraftActionSchema, resolveAssistantChatDraft } from './assistant-chat-draft'

describe('assistant Chat draft intent', () => {
  it('extracts the requested text in Norwegian and English', () => {
    expect(resolveAssistantChatDraft('kan du åpne chat og skrive at jeg er best')).toEqual({
      type: 'assistant.chat.draft', href: '/chat', text: 'jeg er best', newChat: false,
    })
    expect(resolveAssistantChatDraft('Go to a new Chat and type this: Look at https://example.com.')).toEqual({
      type: 'assistant.chat.draft', href: '/chat', text: 'Look at https://example.com.', newChat: true,
    })
  })

  it('accepts only a bounded draft on the fixed Chat route', () => {
    expect(resolveAssistantChatDraft('Open Chat and type: ')).toBeNull()
    expect(resolveAssistantChatDraft('Open Chat and send that I am ready')).toBeNull()
    expect(resolveAssistantChatDraft(`Open Chat and type ${'x'.repeat(2000)}`)).toBeNull()
    expect(assistantChatDraftActionSchema.safeParse({ type: 'assistant.chat.draft', href: '/agents', text: 'Hello', newChat: false }).success).toBe(false)
  })
})
