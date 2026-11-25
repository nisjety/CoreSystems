import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { AttentionState } from '@/shared/attention/attention-state'
import {
  CHAT_ACTIVE_THREAD_KEY,
  CHAT_THREAD_HISTORY_KEY,
  CHAT_SIDEBAR_MIGRATED_KEY,
  CHAT_THREAD_TRANSCRIPTS_KEY,
  applyChatThreadStateLocally,
  chatSidebarMigrated,
  chatSidebarPagedListing,
  chatSidebarServerPagingEnabled,
  chatSidebarThreadStateEnabled,
  clearActiveChatThreadId,
  markChatSidebarMigrated,
  normalizeChatThreadHistoryItem,
  openForkedChatThread,
  pruneChatThreadTranscripts,
  clearChatThreadHistory,
  readActiveChatThreadId,
  readChatThreadHistory,
  readChatThreadTranscript,
  removeChatThreadHistoryItem,
  removeChatThreadTranscript,
  renameChatThreadLocally,
  replaceChatThreadHistory,
  selectChatThread,
  setActiveChatThreadId,
  togglePersonaEnabledChatThread,
  togglePinnedChatThread,
  upsertChatThreadHistory,
  upsertChatThreadTranscript,
} from '@/features/chat/lib/chat-thread-history'

function createStorageMock(): Storage {
  const entries = new Map<string, string>()

  return {
    clear: vi.fn(() => entries.clear()),
    getItem: vi.fn((key: string) => entries.get(key) ?? null),
    key: vi.fn((index: number) => Array.from(entries.keys())[index] ?? null),
    get length() {
      return entries.size
    },
    removeItem: vi.fn((key: string) => entries.delete(key)),
    setItem: vi.fn((key: string, value: string) => entries.set(key, value)),
  }
}

describe('chat thread history', () => {
  beforeEach(() => {
    Object.defineProperty(window, 'localStorage', {
      configurable: true,
      value: createStorageMock(),
    })
    Object.defineProperty(window, 'sessionStorage', {
      configurable: true,
      value: createStorageMock(),
    })
    window.localStorage.clear()
    window.sessionStorage.clear()
  })

  afterEach(() => {
    clearActiveChatThreadId()
    clearChatThreadHistory()
  })

  it('stores active thread id in client storage', () => {
    setActiveChatThreadId(' thread-1 ')

    expect(readActiveChatThreadId()).toBe('thread-1')
    expect(window.localStorage.getItem(CHAT_ACTIVE_THREAD_KEY)).toBe('thread-1')
    expect(window.sessionStorage.getItem(CHAT_ACTIVE_THREAD_KEY)).toBe('thread-1')
  })

  it('prepends history entries and de-duplicates by thread id', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First question',
      preview: 'First answer',
      updatedAt: '2026-06-17T10:00:00.000Z',
    })
    upsertChatThreadHistory({
      threadId: 'thread-2',
      title: 'Second question',
      preview: 'Second answer',
      updatedAt: '2026-06-17T10:05:00.000Z',
    })
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First question updated',
      preview: 'Updated answer',
      updatedAt: '2026-06-17T10:10:00.000Z',
    })

    expect(readChatThreadHistory()).toEqual([
      {
        threadId: 'thread-1',
        title: 'First question updated',
        preview: 'Updated answer',
        updatedAt: '2026-06-17T10:10:00.000Z',
      },
      {
        threadId: 'thread-2',
        title: 'Second question',
        preview: 'Second answer',
        updatedAt: '2026-06-17T10:05:00.000Z',
      },
    ])
  })

  it('keeps an AI-generated title when a preview snapshot upserts over it', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'Visma fakturastatus',
      titleKind: 'generated',
      preview: 'Answer',
      updatedAt: '2026-06-17T10:00:00.000Z',
    })
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'Kan du sjekke i Visma om faktura 1234...',
      titleKind: 'preview',
      preview: 'Newer answer',
      updatedAt: '2026-06-17T10:05:00.000Z',
    })

    const item = readChatThreadHistory().find((candidate) => candidate.threadId === 'thread-1')
    expect(item?.title).toBe('Visma fakturastatus')
    expect(item?.titleKind).toBe('generated')
    // Everything except the locked title still updates.
    expect(item?.preview).toBe('Newer answer')
    expect(item?.updatedAt).toBe('2026-06-17T10:05:00.000Z')
  })

  it('keeps an AI-generated title against a legacy upsert without titleKind', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'Visma fakturastatus',
      titleKind: 'generated',
      preview: 'Answer',
      updatedAt: '2026-06-17T10:00:00.000Z',
    })
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'Kan du sjekke i Visma...',
      preview: 'Newer answer',
      updatedAt: '2026-06-17T10:05:00.000Z',
    })

    const item = readChatThreadHistory().find((candidate) => candidate.threadId === 'thread-1')
    expect(item?.title).toBe('Visma fakturastatus')
    expect(item?.titleKind).toBe('generated')
  })

  it('replaces an AI-generated title with a newer generated one', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'Visma fakturastatus',
      titleKind: 'generated',
      preview: 'Answer',
      updatedAt: '2026-06-17T10:00:00.000Z',
    })
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'Fakturaoppfølging mot Visma',
      titleKind: 'generated',
      preview: 'Answer',
      updatedAt: '2026-06-17T10:05:00.000Z',
    })

    const item = readChatThreadHistory().find((candidate) => candidate.threadId === 'thread-1')
    expect(item?.title).toBe('Fakturaoppfølging mot Visma')
    expect(item?.titleKind).toBe('generated')
  })

  it('still validates stored legacy items without titleKind', () => {
    window.localStorage.setItem(
      CHAT_THREAD_HISTORY_KEY,
      JSON.stringify([
        {
          threadId: 'thread-legacy',
          title: 'Old chat',
          preview: 'Saved before titleKind existed',
          updatedAt: '2026-06-17T09:00:00.000Z',
        },
      ]),
    )

    expect(readChatThreadHistory()).toEqual([
      {
        threadId: 'thread-legacy',
        title: 'Old chat',
        preview: 'Saved before titleKind existed',
        updatedAt: '2026-06-17T09:00:00.000Z',
      },
    ])
  })

  it('does not move a thread above newer ones when upserted with an older updatedAt', () => {
    upsertChatThreadHistory({
      threadId: 'thread-old',
      title: 'Older chat',
      preview: 'Old answer',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })
    upsertChatThreadHistory({
      threadId: 'thread-new',
      title: 'Newer chat',
      preview: 'New answer',
      updatedAt: '2026-06-17T10:00:00.000Z',
    })

    // Selection-shaped rewrite of the OLD thread: same timestamp, no new turn.
    upsertChatThreadHistory({
      threadId: 'thread-old',
      title: 'Older chat',
      preview: 'Old answer',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })

    expect(readChatThreadHistory().map((item) => item.threadId)).toEqual([
      'thread-new',
      'thread-old',
    ])
  })

  it('leaves order and timestamp identical on a click-shaped upsert', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First chat',
      preview: 'Answer one',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })
    upsertChatThreadHistory({
      threadId: 'thread-2',
      title: 'Second chat',
      preview: 'Answer two',
      updatedAt: '2026-06-17T10:00:00.000Z',
    })
    const before = readChatThreadHistory()

    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First chat',
      preview: 'Answer one',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })

    expect(readChatThreadHistory()).toEqual(before)
  })

  it('moves a thread to the top when its updatedAt is genuinely newer', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First chat',
      preview: 'Answer one',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })
    upsertChatThreadHistory({
      threadId: 'thread-2',
      title: 'Second chat',
      preview: 'Answer two',
      updatedAt: '2026-06-17T10:00:00.000Z',
    })

    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First chat',
      preview: 'A brand new message',
      updatedAt: '2026-06-17T11:00:00.000Z',
    })

    expect(readChatThreadHistory().map((item) => item.threadId)).toEqual([
      'thread-1',
      'thread-2',
    ])
  })

  it('removes one history entry without clearing the active thread', () => {
    setActiveChatThreadId('thread-1')
    upsertChatThreadHistory({ threadId: 'thread-1', title: 'One', preview: 'Saved' })
    upsertChatThreadHistory({ threadId: 'thread-2', title: 'Two', preview: 'Saved' })

    removeChatThreadHistoryItem('thread-2')

    expect(readActiveChatThreadId()).toBe('thread-1')
    expect(readChatThreadHistory().map((item) => item.threadId)).toEqual(['thread-1'])
    expect(window.localStorage.getItem(CHAT_THREAD_HISTORY_KEY)).toContain('thread-1')
  })

  it('replaces the local cache with the authoritative server history order', () => {
    upsertChatThreadHistory({
      threadId: 'thread-local',
      title: 'Local only',
      preview: 'Old',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })

    replaceChatThreadHistory([
      {
        threadId: 'thread-server-1',
        title: 'Server one',
        preview: 'Saved on the server',
        updatedAt: '2026-06-17T10:00:00.000Z',
      },
      {
        threadId: 'thread-server-2',
        title: 'Server two',
        preview: 'Also saved',
        updatedAt: '2026-06-17T09:30:00.000Z',
      },
    ])

    expect(readChatThreadHistory().map((item) => item.threadId)).toEqual([
      'thread-server-1',
      'thread-server-2',
    ])
    expect(window.localStorage.getItem(CHAT_THREAD_HISTORY_KEY)).not.toContain('thread-local')
  })

  it('stores and replaces a compact transcript per thread', () => {
    upsertChatThreadTranscript({
      threadId: 'thread-1',
      updatedAt: '2026-06-17T10:00:00.000Z',
      turns: [
        {
          id: 'user-1',
          role: 'user',
          content: 'hello',
          createdAt: '2026-06-17T10:00:00.000Z',
          tools: ['search', 'search'],
        },
        {
          id: 'asst-1',
          role: 'assistant',
          content: 'hi',
          createdAt: '2026-06-17T10:00:01.000Z',
          modelUsed: 'verevon-balance',
        },
      ],
    })
    upsertChatThreadTranscript({
      threadId: 'thread-1',
      updatedAt: '2026-06-17T10:01:00.000Z',
      turns: [
        {
          id: 'user-2',
          role: 'user',
          content: 'next',
          createdAt: '2026-06-17T10:01:00.000Z',
          provider: 'openai-codex-subscription',
          subscriptionConnectionId: 'conn_123',
        },
      ],
    })

    expect(readChatThreadTranscript('thread-1')).toEqual({
      threadId: 'thread-1',
      updatedAt: '2026-06-17T10:01:00.000Z',
      turns: [
        {
          id: 'user-2',
          role: 'user',
          content: 'next',
          createdAt: '2026-06-17T10:01:00.000Z',
          model: undefined,
          modelUsed: undefined,
          provider: 'openai-codex-subscription',
          subscriptionConnectionId: 'conn_123',
          tools: undefined,
          attachments: undefined,
        },
      ],
    })
    expect(window.localStorage.getItem(CHAT_THREAD_TRANSCRIPTS_KEY)).toContain('thread-1')
  })

  it('removes one transcript without deleting history or active thread', () => {
    setActiveChatThreadId('thread-1')
    upsertChatThreadHistory({ threadId: 'thread-1', title: 'One', preview: 'Saved' })
    upsertChatThreadTranscript({
      threadId: 'thread-1',
      turns: [{ id: 'user-1', role: 'user', content: 'hello', createdAt: '2026-06-17T10:00:00.000Z' }],
    })

    removeChatThreadTranscript('thread-1')

    expect(readActiveChatThreadId()).toBe('thread-1')
    expect(readChatThreadHistory().map((item) => item.threadId)).toEqual(['thread-1'])
    expect(readChatThreadTranscript('thread-1')).toBeNull()
  })

  it('keeps assistant citations and step activity with the transcript', () => {
    upsertChatThreadTranscript({
      threadId: 'thread-steps',
      updatedAt: '2026-06-17T10:02:00.000Z',
      turns: [
        {
          id: 'asst-1',
          role: 'assistant',
          content: 'answer',
          createdAt: '2026-06-17T10:02:00.000Z',
          citations: [{ id: 'c1', title: 'Source', url: 'https://example.com', snippet: 'Proof' }],
          toolCalls: [{ id: 'tool-1', name: 'web_search', output: '[{\"title\":\"Source\"}]' }],
        },
      ],
      taskSteps: [
        {
          id: 'asst-1:tool-search',
          title: 'Tool: Web search',
          detail: 'Web search captured 1 source.',
          status: 'done',
          createdAt: '2026-06-17T10:02:01.000Z',
          expandedDetail: '[{\"title\":\"Source\"}]',
          evidence: [{ id: 'c1', label: 'Source found', value: 'Source · example.com', href: 'https://example.com' }],
          turnId: 'asst-1',
          turnTitle: 'Answer: verify with web',
        },
      ],
    })

    expect(readChatThreadTranscript('thread-steps')).toMatchObject({
      threadId: 'thread-steps',
      turns: [
        {
          id: 'asst-1',
          citations: [{ id: 'c1', title: 'Source', url: 'https://example.com', snippet: 'Proof' }],
          toolCalls: [{ id: 'tool-1', name: 'web_search', output: '[{\"title\":\"Source\"}]' }],
        },
      ],
      taskSteps: [
        {
          id: 'asst-1:tool-search',
          title: 'Tool: Web search',
          evidence: [{ id: 'c1', label: 'Source found', value: 'Source · example.com', href: 'https://example.com' }],
        },
      ],
    })
  })

  it('selects a thread without rewriting its activity timestamp or order', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First question',
      preview: 'First answer',
      updatedAt: '2026-06-17T10:00:00.000Z',
    })
    upsertChatThreadHistory({
      threadId: 'thread-2',
      title: 'Second question',
      preview: 'Second answer',
      updatedAt: '2026-06-17T11:00:00.000Z',
    })
    const before = readChatThreadHistory()

    selectChatThread('thread-1')

    expect(readActiveChatThreadId()).toBe('thread-1')
    expect(readChatThreadHistory()).toEqual(before)
  })

  it('pins a thread to the top ahead of newer unpinned activity', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First chat',
      preview: 'Answer one',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })
    upsertChatThreadHistory({
      threadId: 'thread-2',
      title: 'Second chat',
      preview: 'Answer two',
      updatedAt: '2026-06-17T10:00:00.000Z',
    })

    togglePinnedChatThread('thread-1')

    expect(readChatThreadHistory().map((item) => item.threadId)).toEqual([
      'thread-1',
      'thread-2',
    ])
    expect(readChatThreadHistory()[0]).toMatchObject({ threadId: 'thread-1', pinned: true })
  })

  it('sorts multiple pinned items by their own updatedAt, ahead of all unpinned', () => {
    upsertChatThreadHistory({
      threadId: 'thread-a',
      title: 'A',
      preview: 'a',
      updatedAt: '2026-06-17T08:00:00.000Z',
    })
    upsertChatThreadHistory({
      threadId: 'thread-b',
      title: 'B',
      preview: 'b',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })
    upsertChatThreadHistory({
      threadId: 'thread-newest-unpinned',
      title: 'Newest unpinned',
      preview: 'c',
      updatedAt: '2026-06-17T12:00:00.000Z',
    })

    togglePinnedChatThread('thread-a')
    togglePinnedChatThread('thread-b')

    // Both pinned items sort ahead of the unpinned one despite it being the
    // most recently active thread; within the pinned tier, B (newer) leads A.
    expect(readChatThreadHistory().map((item) => item.threadId)).toEqual([
      'thread-b',
      'thread-a',
      'thread-newest-unpinned',
    ])
  })

  it('unpins on a second toggle without disturbing the activity sort', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First chat',
      preview: 'Answer one',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })
    upsertChatThreadHistory({
      threadId: 'thread-2',
      title: 'Second chat',
      preview: 'Answer two',
      updatedAt: '2026-06-17T10:00:00.000Z',
    })

    togglePinnedChatThread('thread-1')
    togglePinnedChatThread('thread-1')

    expect(readChatThreadHistory().find((item) => item.threadId === 'thread-1')?.pinned).toBeUndefined()
    expect(readChatThreadHistory().map((item) => item.threadId)).toEqual([
      'thread-2',
      'thread-1',
    ])
  })

  it('does not silently unpin a thread on a routine snapshot upsert that omits `pinned`', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First chat',
      preview: 'Answer one',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })
    togglePinnedChatThread('thread-1')

    // A routine periodic-snapshot-shaped write, exactly like
    // `writeThreadSnapshot` performs — it never mentions `pinned`.
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First chat',
      preview: 'A later answer',
      updatedAt: '2026-06-17T09:05:00.000Z',
    })

    expect(readChatThreadHistory().find((item) => item.threadId === 'thread-1')).toMatchObject({
      pinned: true,
      preview: 'A later answer',
    })
  })

  it('carries pin state across a resync that omits pinned entirely', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First chat',
      preview: 'Answer one',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })
    togglePinnedChatThread('thread-1')

    replaceChatThreadHistory([
      { threadId: 'thread-1', title: 'First chat', preview: 'Server-synced answer', updatedAt: '2026-06-17T09:10:00.000Z' },
      { threadId: 'thread-2', title: 'Second chat', preview: 'b', updatedAt: '2026-06-17T09:20:00.000Z' },
    ])

    expect(readChatThreadHistory().map((item) => item.threadId)).toEqual(['thread-1', 'thread-2'])
    expect(readChatThreadHistory()[0]).toMatchObject({ pinned: true, preview: 'Server-synced answer' })
  })

  /**
   * The test above omits `pinned`, which the REAL caller never does:
   * `CoreSidebar` passes `ChatThreadSession`s whose `pinned` is a hard boolean,
   * so production always takes the explicit branch and never the carry. This
   * covers what actually happens — and pins the intended semantics, since the
   * server owns the pin: an explicit `false` must win, because it may mean the
   * user unpinned the thread on another device.
   */
  it('lets an explicit server pinned:false win over a stale local pin', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First chat',
      preview: 'Answer one',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })
    togglePinnedChatThread('thread-1')
    expect(readChatThreadHistory()[0]).toMatchObject({ pinned: true })

    replaceChatThreadHistory([
      { threadId: 'thread-1', title: 'First chat', preview: 'Server-synced answer', updatedAt: '2026-06-17T09:10:00.000Z', pinned: false },
      { threadId: 'thread-2', title: 'Second chat', preview: 'b', updatedAt: '2026-06-17T09:20:00.000Z', pinned: true },
    ])

    const after = readChatThreadHistory()
    expect(after.find((item) => item.threadId === 'thread-1')?.pinned).toBeUndefined()
    expect(after.find((item) => item.threadId === 'thread-2')?.pinned).toBe(true)
  })

  // ── Persona spec: the per-thread override ──────────────────────────────
  // Same shape as the pin tests above, but with the INVERSE stored polarity
  // (`withPersonaEnabledCarry`): absent means enabled, so a toggle stores
  // `personaEnabled: false` and clears the key again on the way back to
  // enabled — the opposite of pinned's "store true, omit false".

  it('turns persona off for a thread, storing personaEnabled: false', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First chat',
      preview: 'Answer one',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })

    togglePersonaEnabledChatThread('thread-1')

    expect(readChatThreadHistory()[0]).toMatchObject({ threadId: 'thread-1', personaEnabled: false })
  })

  it('turns persona back on on a second toggle, clearing the stored key', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First chat',
      preview: 'Answer one',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })

    togglePersonaEnabledChatThread('thread-1')
    togglePersonaEnabledChatThread('thread-1')

    expect(readChatThreadHistory().find((item) => item.threadId === 'thread-1')?.personaEnabled).toBeUndefined()
  })

  it('does not silently re-enable persona on a routine snapshot upsert that omits `personaEnabled`', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First chat',
      preview: 'Answer one',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })
    togglePersonaEnabledChatThread('thread-1')

    // A routine periodic-snapshot-shaped write, exactly like
    // `writeThreadSnapshot` performs — it never mentions `personaEnabled`.
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First chat',
      preview: 'A later answer',
      updatedAt: '2026-06-17T09:05:00.000Z',
    })

    expect(readChatThreadHistory().find((item) => item.threadId === 'thread-1')).toMatchObject({
      personaEnabled: false,
      preview: 'A later answer',
    })
  })

  it('carries a disabled persona across a resync that omits personaEnabled entirely', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First chat',
      preview: 'Answer one',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })
    togglePersonaEnabledChatThread('thread-1')

    replaceChatThreadHistory([
      { threadId: 'thread-1', title: 'First chat', preview: 'Server-synced answer', updatedAt: '2026-06-17T09:10:00.000Z' },
      { threadId: 'thread-2', title: 'Second chat', preview: 'b', updatedAt: '2026-06-17T09:20:00.000Z' },
    ])

    expect(readChatThreadHistory().find((item) => item.threadId === 'thread-1')).toMatchObject({
      personaEnabled: false,
      preview: 'Server-synced answer',
    })
  })

  /**
   * Mirrors the pin test's own regression note: an explicit server value must
   * win over a stale local one, in EITHER direction — a device that never
   * saw the toggle must not resurrect it, and a device that has a stale
   * "disabled" must not keep it once the server reports enabled again.
   */
  it('lets an explicit server personaEnabled win over a stale local value in either direction', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First chat',
      preview: 'Answer one',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })
    togglePersonaEnabledChatThread('thread-1')
    expect(readChatThreadHistory()[0]).toMatchObject({ personaEnabled: false })

    replaceChatThreadHistory([
      { threadId: 'thread-1', title: 'First chat', preview: 'Server-synced answer', updatedAt: '2026-06-17T09:10:00.000Z', personaEnabled: true },
      { threadId: 'thread-2', title: 'Second chat', preview: 'b', updatedAt: '2026-06-17T09:20:00.000Z', personaEnabled: false },
    ])

    const after = readChatThreadHistory()
    expect(after.find((item) => item.threadId === 'thread-1')?.personaEnabled).toBeUndefined()
    expect(after.find((item) => item.threadId === 'thread-2')?.personaEnabled).toBe(false)
  })

  /**
   * `origin` (e.g. "mail_task" — see CoreSidebar's "Fra e-post" badge) is set
   * once at thread creation and never changes, unlike `pinned`. A snapshot
   * upsert (periodic saves while chatting) never mentions it, so it must
   * always carry forward from whatever was already stored rather than being
   * erased the moment a write omits it.
   */
  it('carries origin across an upsert that omits it entirely', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First chat',
      preview: 'Answer one',
      updatedAt: '2026-06-17T09:00:00.000Z',
      origin: 'mail_task',
    })
    expect(readChatThreadHistory()[0]).toMatchObject({ origin: 'mail_task' })

    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First chat',
      preview: 'Snapshot answer',
      updatedAt: '2026-06-17T09:05:00.000Z',
    })

    expect(readChatThreadHistory()[0]).toMatchObject({ origin: 'mail_task', preview: 'Snapshot answer' })
  })

  it('carries origin across a server resync (replaceChatThreadHistory) for a thread the resync omits it for', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First chat',
      preview: 'Answer one',
      updatedAt: '2026-06-17T09:00:00.000Z',
      origin: 'mail_task',
    })

    replaceChatThreadHistory([
      { threadId: 'thread-1', title: 'First chat', preview: 'Server-synced answer', updatedAt: '2026-06-17T09:10:00.000Z' },
      { threadId: 'thread-2', title: 'Second chat', preview: 'b', updatedAt: '2026-06-17T09:20:00.000Z', origin: 'chat' },
    ])

    const after = readChatThreadHistory()
    expect(after.find((item) => item.threadId === 'thread-1')).toMatchObject({ origin: 'mail_task', preview: 'Server-synced answer' })
    expect(after.find((item) => item.threadId === 'thread-2')).toMatchObject({ origin: 'chat' })
  })

  it('is a no-op when toggling a thread with no history entry', () => {
    expect(togglePinnedChatThread('does-not-exist')).toEqual([])
  })

  it('renames a thread and locks the new title against a later preview overwrite', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'Kan du sjekke i Visma om faktura 1234...',
      titleKind: 'preview',
      preview: 'Answer one',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })

    const next = renameChatThreadLocally('thread-1', 'Visma fakturastatus')

    expect(next.find((item) => item.threadId === 'thread-1')).toMatchObject({
      title: 'Visma fakturastatus',
      titleKind: 'generated',
    })

    // A routine preview snapshot after the rename -- exactly the shape a
    // periodic composer autosave writes -- must not silently revert it.
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'Kan du sjekke i Visma om faktura 1234...',
      titleKind: 'preview',
      preview: 'A later answer',
      updatedAt: '2026-06-17T09:05:00.000Z',
    })

    expect(readChatThreadHistory().find((item) => item.threadId === 'thread-1')).toMatchObject({
      title: 'Visma fakturastatus',
      titleKind: 'generated',
      preview: 'A later answer',
    })
  })

  it('lets a second rename revert the first, including after it locked the title', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'Original title',
      preview: 'Answer one',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })

    renameChatThreadLocally('thread-1', 'Renamed once')
    expect(readChatThreadHistory().find((item) => item.threadId === 'thread-1')?.title).toBe('Renamed once')

    // Simulates CoreSidebar reverting an optimistic rename after the server
    // write failed -- the revert has to go through the same lock the first
    // rename installed, or it would be blocked by its own titleKind.
    renameChatThreadLocally('thread-1', 'Original title')
    expect(readChatThreadHistory().find((item) => item.threadId === 'thread-1')?.title).toBe('Original title')
  })

  it('does not disturb pin state or preview when renaming', () => {
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'Original title',
      preview: 'Answer one',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })
    togglePinnedChatThread('thread-1')

    renameChatThreadLocally('thread-1', 'Renamed')

    expect(readChatThreadHistory().find((item) => item.threadId === 'thread-1')).toMatchObject({
      title: 'Renamed',
      pinned: true,
      preview: 'Answer one',
    })
  })

  it('is a no-op when renaming a thread with no history entry', () => {
    expect(renameChatThreadLocally('does-not-exist', 'New title')).toEqual([])
  })

  it('clears transcript storage with history', () => {
    upsertChatThreadTranscript({
      threadId: 'thread-1',
      turns: [{ id: 'user-1', role: 'user', content: 'hello', createdAt: '2026-06-17T10:00:00.000Z' }],
    })

    clearChatThreadHistory()

    expect(window.localStorage.getItem(CHAT_THREAD_HISTORY_KEY)).toBeNull()
    expect(window.localStorage.getItem(CHAT_THREAD_TRANSCRIPTS_KEY)).toBeNull()
  })
})

describe('chat thread history: server flags, row limit and the sidebar switches', () => {
  const at = (minute: number) => `2026-06-17T09:${String(minute).padStart(2, '0')}:00.000Z`
  const manyThreads = (count: number) => Array.from({ length: count }, (_, index) => ({
    threadId: `thread-${index}`,
    title: `Chat ${index}`,
    preview: 'Preview',
    updatedAt: new Date(Date.UTC(2026, 5, 1) - index * 60_000).toISOString(),
  }))

  // The thread-state switch is on for these tests; the switch-off test turns it off.
  beforeEach(() => {
    vi.stubEnv('VITE_CHAT_SIDEBAR_THREAD_STATE', 'true')
  })

  beforeEach(() => {
    Object.defineProperty(window, 'localStorage', {
      configurable: true,
      value: createStorageMock(),
    })
    Object.defineProperty(window, 'sessionStorage', {
      configurable: true,
      value: createStorageMock(),
    })
    window.localStorage.clear()
    window.sessionStorage.clear()
  })

  afterEach(() => {
    vi.unstubAllEnvs()
    clearActiveChatThreadId()
    clearChatThreadHistory()
  })

  it('keeps the server star, unread reply and fork lineage from a server list, and drops blank or false ones', () => {
    const items = replaceChatThreadHistory([
      { threadId: 'thread-1', title: 'One', preview: 'p', updatedAt: at(1), starred: true, unread: true, forkedFromThreadId: ' parent-1 ' },
      { threadId: 'thread-2', title: 'Two', preview: 'p', updatedAt: at(2), starred: false, unread: false, forkedFromThreadId: '   ' },
    ])

    expect(items.find((item) => item.threadId === 'thread-1')).toMatchObject({ starred: true, unread: true, forkedFromThreadId: 'parent-1' })
    const second = items.find((item) => item.threadId === 'thread-2')
    expect(second?.starred).toBeUndefined()
    expect(second?.unread).toBeUndefined()
    expect(second?.forkedFromThreadId).toBeUndefined()
  })

  it('keeps the fork count of a server list above zero, lets a local write keep it, and clears it when a list omits it', () => {
    replaceChatThreadHistory([
      { threadId: 'thread-1', title: 'One', preview: 'p', updatedAt: at(1), forkCount: 2 },
      { threadId: 'thread-2', title: 'Two', preview: 'p', updatedAt: at(2), forkCount: 0 },
    ])
    expect(readChatThreadHistory().find((item) => item.threadId === 'thread-1')).toMatchObject({ forkCount: 2 })
    expect(readChatThreadHistory().find((item) => item.threadId === 'thread-2')).not.toHaveProperty('forkCount')

    upsertChatThreadHistory({ threadId: 'thread-1', title: 'One renamed', preview: 'p', updatedAt: at(3) })
    expect(readChatThreadHistory().find((item) => item.threadId === 'thread-1')).toMatchObject({ title: 'One renamed', forkCount: 2 })

    replaceChatThreadHistory([{ threadId: 'thread-1', title: 'One renamed', preview: 'p', updatedAt: at(3) }])
    expect(readChatThreadHistory().find((item) => item.threadId === 'thread-1')).not.toHaveProperty('forkCount')
  })

  it('lets a local write keep the stored flags, and a server list that omits them clear them', () => {
    replaceChatThreadHistory([{ threadId: 'thread-1', title: 'One', preview: 'p', updatedAt: at(1), starred: true, unread: true, forkedFromThreadId: 'parent-1' }])

    togglePinnedChatThread('thread-1')
    expect(readChatThreadHistory()[0]).toMatchObject({ starred: true, unread: true, forkedFromThreadId: 'parent-1' })

    replaceChatThreadHistory([{ threadId: 'thread-1', title: 'One', preview: 'p', updatedAt: at(1) }])
    const cleared = readChatThreadHistory()[0]
    expect(cleared?.starred).toBeUndefined()
    expect(cleared?.unread).toBeUndefined()
    expect(cleared?.forkedFromThreadId).toBeUndefined()
  })

  it('applies the server flags to one cached thread and leaves the others as they were', () => {
    replaceChatThreadHistory([
      { threadId: 'thread-1', title: 'One', preview: 'p', updatedAt: at(1), starred: true },
      { threadId: 'thread-2', title: 'Two', preview: 'p', updatedAt: at(2), starred: true },
    ])

    applyChatThreadStateLocally('thread-1', { unread: true })

    const byId = Object.fromEntries(readChatThreadHistory().map((item) => [item.threadId, item]))
    expect(byId['thread-1']).toMatchObject({ unread: true })
    expect(byId['thread-1']?.starred).toBeUndefined()
    expect(byId['thread-2']?.starred).toBe(true)
  })

  it('applies a fork count to one cached thread, drops it on a flag-only apply, and sets it again', () => {
    replaceChatThreadHistory([{ threadId: 'thread-1', title: 'One', preview: 'p', updatedAt: at(1), forkCount: 2 }])
    expect(readChatThreadHistory()[0]).toMatchObject({ forkCount: 2 })

    applyChatThreadStateLocally('thread-1', { unread: true })
    expect(readChatThreadHistory()[0]).toMatchObject({ unread: true })
    expect(readChatThreadHistory()[0]).not.toHaveProperty('forkCount')

    applyChatThreadStateLocally('thread-1', { forkCount: 3 })
    expect(readChatThreadHistory()[0]).toMatchObject({ forkCount: 3 })
  })

  it('leaves the cache alone for a thread it has no entry for', () => {
    replaceChatThreadHistory([{ threadId: 'thread-1', title: 'One', preview: 'p', updatedAt: at(1) }])

    expect(applyChatThreadStateLocally('thread-9', { starred: true })).toHaveLength(1)
    expect(readChatThreadHistory()[0]?.starred).toBeUndefined()
  })

  it('normalises one server session into an item, and refuses one with no thread id', () => {
    expect(normalizeChatThreadHistoryItem({ threadId: ' thread-1 ', title: 'One', starred: true, forkedFromThreadId: 'parent-1' }))
      .toMatchObject({ threadId: 'thread-1', starred: true, forkedFromThreadId: 'parent-1' })
    expect(normalizeChatThreadHistoryItem({ threadId: '  ', title: 'One' })).toBeNull()
  })

  it('keeps the first 40 rows while the paged sidebar is off', () => {
    replaceChatThreadHistory(manyThreads(45))

    expect(readChatThreadHistory()).toHaveLength(40)
  })

  it('keeps every loaded row, also through a local write, once the paged listing is in effect', () => {
    vi.stubEnv('VITE_CHAT_SIDEBAR_SERVER_PAGING', 'true')
    window.localStorage.setItem(CHAT_SIDEBAR_MIGRATED_KEY, 'true')
    replaceChatThreadHistory(manyThreads(45))
    expect(readChatThreadHistory()).toHaveLength(45)

    togglePinnedChatThread('thread-44')
    expect(readChatThreadHistory()).toHaveLength(45)
  })

  it('keeps the first 40 rows with the paged sidebar on while the local chats have not moved yet', () => {
    vi.stubEnv('VITE_CHAT_SIDEBAR_SERVER_PAGING', 'true')
    replaceChatThreadHistory(manyThreads(45))

    expect(readChatThreadHistory()).toHaveLength(40)
  })

  it('reads the two sidebar switches only as the literal true', () => {
    vi.stubEnv('VITE_CHAT_SIDEBAR_SERVER_PAGING', 'TRUE')
    vi.stubEnv('VITE_CHAT_SIDEBAR_THREAD_STATE', 'yes')
    expect(chatSidebarServerPagingEnabled()).toBe(false)
    expect(chatSidebarThreadStateEnabled()).toBe(false)

    vi.stubEnv('VITE_CHAT_SIDEBAR_SERVER_PAGING', 'true')
    vi.stubEnv('VITE_CHAT_SIDEBAR_THREAD_STATE', 'true')
    expect(chatSidebarServerPagingEnabled()).toBe(true)
    expect(chatSidebarThreadStateEnabled()).toBe(true)
  })

  it('records the one-time move in local storage, and treats blocked storage as not moved', () => {
    expect(chatSidebarMigrated()).toBe(false)

    markChatSidebarMigrated()
    expect(window.localStorage.getItem(CHAT_SIDEBAR_MIGRATED_KEY)).toBe('true')
    expect(chatSidebarMigrated()).toBe(true)

    vi.spyOn(window.localStorage, 'getItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    vi.spyOn(window.localStorage, 'setItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    expect(chatSidebarMigrated()).toBe(false)
    expect(() => markChatSidebarMigrated()).not.toThrow()
  })

  it('keeps no server flags while the thread-state switch is off, so the cache reads as it did before', () => {
    vi.stubEnv('VITE_CHAT_SIDEBAR_THREAD_STATE', 'false')
    replaceChatThreadHistory([{ threadId: 'thread-1', title: 'One', preview: 'p', updatedAt: at(1), starred: true, unread: true, forkedFromThreadId: 'parent-1' }])

    const item = readChatThreadHistory()[0]
    expect(item).toMatchObject({ threadId: 'thread-1', title: 'One' })
    expect(item).not.toHaveProperty('starred')
    expect(item).not.toHaveProperty('unread')
    expect(item).not.toHaveProperty('forkedFromThreadId')
  })

  it('keeps no fork count while the thread-state switch is off', () => {
    vi.stubEnv('VITE_CHAT_SIDEBAR_THREAD_STATE', 'false')
    replaceChatThreadHistory([{ threadId: 'thread-1', title: 'One', preview: 'p', updatedAt: at(1), forkCount: 3 }])

    expect(readChatThreadHistory()[0]).not.toHaveProperty('forkCount')
  })

  it('pages the sidebar only when the switch is on and the local chats have moved', () => {
    expect(chatSidebarPagedListing()).toBe(false)

    vi.stubEnv('VITE_CHAT_SIDEBAR_SERVER_PAGING', 'true')
    expect(chatSidebarPagedListing()).toBe(false)

    markChatSidebarMigrated()
    expect(chatSidebarPagedListing()).toBe(true)

    vi.stubEnv('VITE_CHAT_SIDEBAR_SERVER_PAGING', 'false')
    expect(chatSidebarPagedListing()).toBe(false)
  })

  it('drops the transcripts the cache no longer holds, and keeps the open chat and the cached ones', () => {
    replaceChatThreadHistory([{ threadId: 'thread-3', title: 'Three', preview: 'p', updatedAt: at(3) }])
    upsertChatThreadTranscript({ threadId: 'thread-1', turns: [], updatedAt: at(1) })
    upsertChatThreadTranscript({ threadId: 'thread-2', turns: [], updatedAt: at(2) })
    upsertChatThreadTranscript({ threadId: 'thread-3', turns: [], updatedAt: at(3) })
    setActiveChatThreadId('thread-2')

    const kept = pruneChatThreadTranscripts()

    expect(kept.map((item) => item.threadId).sort()).toEqual(['thread-2', 'thread-3'])
    expect(readChatThreadTranscript('thread-1')).toBeNull()
    expect(readChatThreadTranscript('thread-2')).not.toBeNull()
    expect(readChatThreadTranscript('thread-3')).not.toBeNull()
  })

  it('writes nothing when every stored transcript is still wanted', () => {
    replaceChatThreadHistory([{ threadId: 'thread-1', title: 'One', preview: 'p', updatedAt: at(1) }])
    upsertChatThreadTranscript({ threadId: 'thread-1', turns: [], updatedAt: at(1) })
    const setItem = vi.mocked(window.localStorage.setItem)
    setItem.mockClear()

    pruneChatThreadTranscripts()

    expect(setItem).not.toHaveBeenCalledWith(CHAT_THREAD_TRANSCRIPTS_KEY, expect.anything())
  })

  it('keeps the server attention state from a server list, and drops a value outside the closed set', () => {
    const unknownState = 'awaiting_approval' as unknown as AttentionState
    const items = replaceChatThreadHistory([
      { threadId: 'thread-1', title: 'One', preview: 'p', updatedAt: at(1), latestRunStatus: 'awaiting_approval', attentionState: 'working' },
      { threadId: 'thread-2', title: 'Two', preview: 'p', updatedAt: at(2), attentionState: unknownState },
    ])

    expect(items.find((item) => item.threadId === 'thread-1')).toMatchObject({ attentionState: 'working' })
    expect(items.find((item) => item.threadId === 'thread-2')?.attentionState).toBeUndefined()
    // The stored copy is read back through the validator, so the state survives a reload of the cache.
    expect(readChatThreadHistory().find((item) => item.threadId === 'thread-1')).toMatchObject({ attentionState: 'working' })
  })

  it('keeps the server attention state across a local write that leaves the run as it was, and drops it once the run moves', () => {
    replaceChatThreadHistory([{ threadId: 'thread-1', title: 'One', preview: 'p', updatedAt: at(1), latestRunId: 'run-1', latestRunStatus: 'awaiting_approval', attentionState: 'working' }])

    renameChatThreadLocally('thread-1', 'Renamed')
    expect(readChatThreadHistory()[0]).toMatchObject({ title: 'Renamed', attentionState: 'working' })

    upsertChatThreadHistory({ threadId: 'thread-1', title: 'Renamed', preview: 'p', updatedAt: at(2), latestRunId: 'run-2', latestRunStatus: 'running' })
    expect(readChatThreadHistory()[0]?.attentionState).toBeUndefined()
  })

  it('clears the attention state when a server list states none, so a switched-off server leaves no stale state', () => {
    replaceChatThreadHistory([{ threadId: 'thread-1', title: 'One', preview: 'p', updatedAt: at(1), attentionState: 'working' }])
    replaceChatThreadHistory([{ threadId: 'thread-1', title: 'One', preview: 'p', updatedAt: at(1) }])

    expect(readChatThreadHistory()[0]?.attentionState).toBeUndefined()
  })

  it('puts a fork copy into the cache from the session the fork returned, and opens it', () => {
    replaceChatThreadHistory([{ threadId: 'thread-source', title: 'Source', preview: 'p', updatedAt: at(1) }])

    openForkedChatThread('thread-fork', { title: 'Kopi av samtalen', preview: 'Forhåndsvisning', updatedAt: at(2) })

    expect(readChatThreadHistory().find((item) => item.threadId === 'thread-fork'))
      .toMatchObject({ title: 'Kopi av samtalen', preview: 'Forhåndsvisning' })
    expect(readActiveChatThreadId()).toBe('thread-fork')
  })

  it('opens a fork copy whose session states no details, without dropping the source thread', () => {
    replaceChatThreadHistory([{ threadId: 'thread-source', title: 'Source', preview: 'p', updatedAt: at(1) }])

    openForkedChatThread('thread-fork')

    expect(readChatThreadHistory().map((item) => item.threadId)).toEqual(expect.arrayContaining(['thread-source', 'thread-fork']))
    expect(readActiveChatThreadId()).toBe('thread-fork')
  })
})
