// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  previewAction: vi.fn(),
  executeAction: vi.fn(),
  announceAttentionChanged: vi.fn(),
  archiveChatThread: vi.fn(),
  markChatThreadUnread: vi.fn(),
  setChatThreadStarred: vi.fn(),
}))

vi.mock('@/shared/actions/action-client', () => ({
  previewAction: mocks.previewAction,
  executeAction: mocks.executeAction,
}))
vi.mock('@/shared/api/attention-client', () => ({
  ATTENTION_CHANGED_EVENT: 'verevon:attention-changed',
  announceAttentionChanged: mocks.announceAttentionChanged,
}))
vi.mock('@/shared/api/chat-client', () => ({
  archiveChatThread: mocks.archiveChatThread,
  markChatThreadUnread: mocks.markChatThreadUnread,
  setChatThreadStarred: mocks.setChatThreadStarred,
}))

import { runPaletteAction } from './palette-providers'

describe('palette chat-thread star', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    // A low-risk, fully specified preview: the palette runs the action without asking first.
    mocks.previewAction.mockResolvedValue({ requiresApproval: false, risk: 'low', missingInputs: [] })
  })

  afterEach(() => {
    vi.unstubAllEnvs()
  })

  it('announces the attention change once the star lands, so the open chat re-reads its flags', async () => {
    vi.stubEnv('VITE_CHAT_SIDEBAR_THREAD_STATE', 'true')
    mocks.setChatThreadStarred.mockResolvedValue(null)

    await expect(runPaletteAction('chat.thread.star', { threadId: 'thread-1', starred: true })).resolves.toBe('done')

    expect(mocks.setChatThreadStarred).toHaveBeenCalledWith('thread-1', true)
    expect(mocks.announceAttentionChanged).toHaveBeenCalledTimes(1)
  })

  it('does not announce the star with the thread-state switch off', async () => {
    vi.stubEnv('VITE_CHAT_SIDEBAR_THREAD_STATE', 'false')
    mocks.setChatThreadStarred.mockResolvedValue(null)

    await expect(runPaletteAction('chat.thread.star', { threadId: 'thread-1', starred: true })).resolves.toBe('done')

    expect(mocks.setChatThreadStarred).toHaveBeenCalledTimes(1)
    expect(mocks.setChatThreadStarred).toHaveBeenCalledWith('thread-1', true)
    expect(mocks.announceAttentionChanged).not.toHaveBeenCalled()
  })

  it('does not announce a star the server refused', async () => {
    vi.stubEnv('VITE_CHAT_SIDEBAR_THREAD_STATE', 'true')
    mocks.setChatThreadStarred.mockRejectedValue(new Error('refused'))

    await expect(runPaletteAction('chat.thread.star', { threadId: 'thread-1', starred: true })).resolves.toBe('failed')

    expect(mocks.announceAttentionChanged).not.toHaveBeenCalled()
  })
})
