import { afterEach, describe, expect, it } from 'vitest'
import {
  bindSupportChatThread,
  clearSupportChatThreads,
  isSupportChatThread,
  newSupportChatThreadId,
  readSupportChatThread,
} from './support-chat-thread'

afterEach(clearSupportChatThreads)

describe('support chat thread binding', () => {
  it('reuses a thread only for the exact user, organization, and conversation', () => {
    bindSupportChatThread({ userId: 'user-1', orgId: 'org-1', conversationId: 'conv-1' }, 'support_thread-1')

    expect(readSupportChatThread({ userId: 'user-1', orgId: 'org-1', conversationId: 'conv-1' })).toBe('support_thread-1')
    expect(readSupportChatThread({ userId: 'user-1', orgId: 'org-1', conversationId: 'conv-2' })).toBeNull()
    expect(readSupportChatThread({ userId: 'user-1', orgId: 'org-2', conversationId: 'conv-1' })).toBeNull()
    expect(readSupportChatThread({ userId: 'user-2', orgId: 'org-1', conversationId: 'conv-1' })).toBeNull()
    expect(isSupportChatThread('support_thread-1')).toBe(true)
  })

  it('removes the read-only thread marker with the authenticated session', () => {
    bindSupportChatThread({ userId: 'user-1', orgId: 'org-1', conversationId: 'conv-1' }, 'legacy-thread-1')
    clearSupportChatThreads()
    expect(isSupportChatThread('legacy-thread-1')).toBe(false)
  })

  it('persists the exact support binding for reload-safe answer hydration', () => {
    const scope = { userId: 'user-1', orgId: 'org-1', conversationId: 'conv-1' }
    bindSupportChatThread(scope, 'chat-thread-1')

    expect(window.sessionStorage.getItem('verevon.chat.supportThreadBindings.v1')).toContain('chat-thread-1')
    expect(readSupportChatThread(scope)).toBe('chat-thread-1')
  })

  it('generates the immutable support UUID namespace accepted by Session Core', () => {
    expect(newSupportChatThreadId()).toMatch(/^support_[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
  })
})
