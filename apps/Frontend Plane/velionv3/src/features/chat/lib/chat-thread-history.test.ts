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
          modelUsed: 'velion-balance',
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
