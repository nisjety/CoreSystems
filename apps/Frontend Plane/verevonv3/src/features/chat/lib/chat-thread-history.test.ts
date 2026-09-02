import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  CHAT_ACTIVE_THREAD_KEY,
  CHAT_THREAD_HISTORY_KEY,
  CHAT_THREAD_TRANSCRIPTS_KEY,
  clearActiveChatThreadId,
  clearChatThreadHistory,
  readActiveChatThreadId,
  readChatThreadHistory,
  readChatThreadTranscript,
  removeChatThreadHistoryItem,
  removeChatThreadTranscript,
  replaceChatThreadHistory,
  selectChatThread,
  setActiveChatThreadId,
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

  it('is a no-op when toggling a thread with no history entry', () => {
    expect(togglePinnedChatThread('does-not-exist')).toEqual([])
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
