import { afterEach, describe, expect, it, vi } from 'vitest'

import { searchSpaceMessages, spaceSearchPageFrom } from './message-search'

type FetchCall = { url: string; init: RequestInit | undefined }

function stubFetch(status: number, body: unknown): FetchCall[] {
  const calls: FetchCall[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url: String(url), init })
      return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
    }),
  )
  return calls
}

afterEach(() => {
  vi.unstubAllGlobals()
})

const sentBody = (call: FetchCall | undefined) => JSON.parse(String(call?.init?.body)) as Record<string, unknown>

const hit = (over: Record<string, unknown> = {}) => ({
  message_id: 'msg_1',
  thread_id: 'thr_1',
  space_id: 'space_1',
  role: 'assistant',
  author_subject_id: 'user_1',
  agent_name: 'Lagerhjelpen',
  created_at: '2026-11-02T10:00:00Z',
  snippet: [{ text: 'invoice', matched: true }],
  thread_title: 'Plan',
  ...over,
})

describe('spaceSearchPageFrom (WS13-S5)', () => {
  it('rejects a body with no hits array and a page with unknown coverage', () => {
    for (const body of [null, {}, { hits: 'none', coverage: 'shared' }, { coverage: 'shared' }]) {
      expect(() => spaceSearchPageFrom(body)).toThrow('invalid_space_search_response')
    }
    for (const coverage of ['everything', 'SHARED', undefined]) {
      expect(() => spaceSearchPageFrom({ hits: [], coverage })).toThrow('invalid_space_search_response')
    }
  })

  it('drops malformed hits and keeps the good ones', () => {
    const page = spaceSearchPageFrom({
      hits: [
        null,
        'x',
        hit({ message_id: '' }),
        hit({ thread_id: undefined }),
        hit({ space_id: '   ' }),
        hit({ message_id: 'msg_2' }),
      ],
      coverage: 'shared',
    })
    expect(page.hits).toEqual([
      {
        messageId: 'msg_2',
        threadId: 'thr_1',
        spaceId: 'space_1',
        role: 'assistant',
        authorSubjectId: 'user_1',
        agentName: 'Lagerhjelpen',
        createdAt: '2026-11-02T10:00:00Z',
        snippet: [{ text: 'invoice', matched: true }],
        threadTitle: 'Plan',
      },
    ])
  })

  it('caps the snippet text at 600 characters across its segments', () => {
    const page = spaceSearchPageFrom({
      hits: [
        hit({
          snippet: [
            { text: 'a'.repeat(500), matched: false },
            { text: 'b'.repeat(500), matched: true },
            { text: 'c', matched: false },
          ],
        }),
      ],
      coverage: 'own_threads_only',
    })
    expect(page.hits[0]?.snippet).toEqual([
      { text: 'a'.repeat(500), matched: false },
      { text: 'b'.repeat(100), matched: true },
    ])
  })
})

describe('searchSpaceMessages (WS13-S5)', () => {
  it('POSTs the query in a JSON body to the room search path, never in the URL', async () => {
    const calls = stubFetch(200, { data: { hits: [], coverage: 'shared', truncated: false, next_cursor: null } })
    await searchSpaceMessages('space 1', { query: 'faktura', prefix: true, scope: 'room', limit: 20, cursor: 'cur_1' })
    expect(calls[0]?.url.endsWith('/api/v1/spaces/space%201/search')).toBe(true)
    expect(calls[0]?.url).not.toContain('faktura')
    expect(calls[0]?.url).not.toContain('?')
    expect(calls[0]?.init?.method).toBe('POST')
    expect(calls[0]?.init?.cache).toBe('no-store')
    expect(sentBody(calls[0])).toEqual({ query: 'faktura', prefix: true, scope: 'room', limit: 20, cursor: 'cur_1' })
  })

  it('sends the thread id for a thread scope and the author for a person scope, and nothing scope-specific for a room', async () => {
    const calls = stubFetch(200, { data: { hits: [], coverage: 'shared' } })
    await searchSpaceMessages('room', {
      query: 'a',
      prefix: false,
      scope: 'thread',
      threadId: 'thr_1',
      authorSubjectId: 'user_9',
    })
    expect(sentBody(calls[0])).toEqual({ query: 'a', prefix: false, scope: 'thread', thread_id: 'thr_1' })
    await searchSpaceMessages('room', {
      query: 'a',
      prefix: false,
      scope: 'person',
      threadId: 'thr_1',
      authorSubjectId: 'user_9',
    })
    expect(sentBody(calls[1])).toEqual({ query: 'a', prefix: false, scope: 'person', author_subject_id: 'user_9' })
    await searchSpaceMessages('room', {
      query: 'a',
      prefix: false,
      scope: 'room',
      threadId: 'thr_1',
      authorSubjectId: 'user_9',
    })
    expect(sentBody(calls[2])).toEqual({ query: 'a', prefix: false, scope: 'room' })
  })
})
