import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { __resetRetentionForTests, applyServerRetention } from '@/features/chat/lib/chat-retention'
import { ApiError } from '@/shared/api/http'
import type { SpaceSearchHit, SpaceSearchPage, SpaceSearchRequest } from '@/shared/api/spaces-client'

import {
  __resetRecentSearchesForTests,
  createSpaceSearchController,
  readRecentSearches,
  RECENT_SEARCHES_KEY,
  rememberSearch,
  SEARCH_DEBOUNCE_MS,
  searchFailureKind,
  type SpaceSearchController,
  watchRecentSearchRetention,
} from './space-search-state'

type PendingSearch = {
  readonly request: SpaceSearchRequest
  readonly signal: AbortSignal
  readonly resolve: (page: SpaceSearchPage) => void
}

// Every search the controller starts is held open here, so a test decides when, and whether, it answers.
let openThread: string | null = null
let pending: PendingSearch[] = []

function controller(): SpaceSearchController {
  return createSpaceSearchController({
    spaceRef: () => 'room-a',
    threadId: () => openThread,
    search: (_spaceRef, request, signal) =>
      new Promise<SpaceSearchPage>((resolve) => {
        pending.push({ request, signal, resolve })
      }),
  })
}

// The search the controller started at this position. Asking for one it never started fails the test here.
function searchAt(index: number): PendingSearch {
  const found = pending[index]
  if (!found) throw new Error(`no search was started at position ${index}`)
  return found
}

function hit(messageId: string): SpaceSearchHit {
  return {
    messageId,
    threadId: 'thr-1',
    spaceId: 'space-room-a',
    role: 'user',
    authorSubjectId: 'sub-kari',
    agentName: undefined,
    createdAt: '2026-11-01T10:00:00.000Z',
    snippet: [{ text: `Hei ${messageId}`, matched: true }],
    threadTitle: 'Planning',
  }
}

function resultPage(hits: readonly SpaceSearchHit[], nextCursor: string | null = null): SpaceSearchPage {
  return { hits, nextCursor, truncated: false, coverage: 'shared', excluded: [] }
}

// Lets the controller's promise callbacks run, then applies Solid's pending signal writes.
async function settle(): Promise<void> {
  for (let i = 0; i < 5; i += 1) await Promise.resolve()
  flush()
}

// A keystroke's search starts once the typing pauses for the debounce window.
async function typed(c: SpaceSearchController, query: string): Promise<void> {
  c.setQuery(query)
  await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS)
}

beforeEach(() => {
  vi.useFakeTimers()
  openThread = null
  pending = []
  __resetRecentSearchesForTests()
  __resetRetentionForTests()
})

afterEach(() => {
  vi.useRealTimers()
  __resetRetentionForTests()
})

describe('in-room search controller (WS13-S5)', () => {
  it('drops a stale response, and a late answer to an older query never shows', async () => {
    const c = controller()
    await typed(c, 'kari')
    await typed(c, 'kari ola')
    expect(pending).toHaveLength(2)
    expect(searchAt(0).signal.aborted).toBe(true)

    searchAt(1).resolve(resultPage([hit('m-new')]))
    await settle()
    expect(c.hits().map((h) => h.messageId)).toEqual(['m-new'])
    expect(c.answeredQuery()).toBe('kari ola')

    searchAt(0).resolve(resultPage([hit('m-old')]))
    await settle()
    expect(c.hits().map((h) => h.messageId)).toEqual(['m-new'])
    expect(c.answeredQuery()).toBe('kari ola')
    expect(c.phase()).toBe('results')
  })

  it('loadMore appends the next page and drops a hit the list already has', async () => {
    const c = controller()
    await typed(c, 'kari')
    searchAt(0).resolve(resultPage([hit('m-1'), hit('m-2')], 'cur-2'))
    await settle()
    expect(c.hasMore()).toBe(true)

    c.loadMore()
    await settle()
    expect(pending).toHaveLength(2)
    expect(searchAt(1).request).toEqual({ ...searchAt(0).request, cursor: 'cur-2' })
    expect(c.loadingMore()).toBe(true)

    searchAt(1).resolve(resultPage([hit('m-2'), hit('m-3')], null))
    await settle()
    expect(c.hits().map((h) => h.messageId)).toEqual(['m-1', 'm-2', 'm-3'])
    expect(c.hasMore()).toBe(false)
    expect(c.loadingMore()).toBe(false)
  })

  it('scope thread reads as room when no thread is open, and as thread once one is', async () => {
    const c = controller()
    c.setScope('thread')
    await settle()
    expect(c.scope()).toBe('room')

    await typed(c, 'kari')
    expect(searchAt(0).request).toMatchObject({ scope: 'room', query: 'kari' })
    expect(searchAt(0).request.threadId).toBeUndefined()

    openThread = 'thr-1'
    await settle()
    expect(c.scope()).toBe('thread')
    await typed(c, 'kari ola')
    expect(searchAt(1).request).toMatchObject({ scope: 'thread', threadId: 'thr-1' })
  })

  it('reset aborts the request in flight and forgets the query and every result', async () => {
    const c = controller()
    await typed(c, 'kari')
    searchAt(0).resolve(resultPage([hit('m-1')]))
    await settle()
    expect(c.hits()).toHaveLength(1)

    await typed(c, 'kari ola')
    expect(pending).toHaveLength(2)
    c.reset()
    await settle()
    expect(searchAt(1).signal.aborted).toBe(true)
    expect(c.query()).toBe('')
    expect(c.hits()).toEqual([])
    expect(c.phase()).toBe('idle')

    searchAt(1).resolve(resultPage([hit('m-late')]))
    await settle()
    expect(c.hits()).toEqual([])
    expect(c.phase()).toBe('idle')

    // The debounce that was waiting when reset ran never fires.
    await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS * 4)
    expect(pending).toHaveLength(2)
  })
})

describe('search failures (WS13-S5)', () => {
  it('names each typed failure code, and an unknown code by its status', () => {
    const kind = (code: string, status: number) => searchFailureKind(new ApiError(code, status, code))
    expect(kind('search_busy', 429)).toBe('busy')
    expect(kind('search_timeout', 503)).toBe('timeout')
    expect(kind('space_search_disabled', 404)).toBe('disabled')
    for (const code of ['active_membership_required', 'space_read_not_authorized', 'space_not_found']) {
      expect(kind(code, 403)).toBe('forbidden')
    }
    for (const code of ['invalid_query', 'invalid_filter', 'invalid_cursor']) {
      expect(kind(code, 400)).toBe('invalid')
    }
    for (const code of [
      'search_unavailable',
      'space_lifecycle_unavailable',
      'space_decision_unavailable',
      'delegated_auth_unavailable',
      'invalid_model_gateway_response',
    ]) {
      expect(kind(code, 503)).toBe('unavailable')
    }
    expect(kind('something_new', 500)).toBe('unavailable')
    expect(kind('something_new', 400)).toBe('other')
    expect(searchFailureKind(new Error('offline'))).toBe('other')
    expect(searchFailureKind(undefined)).toBe('other')
  })
})

describe('recent searches (WS13-S5)', () => {
  it('keeps the 8 most recent queries, newest first', () => {
    for (let i = 1; i <= 10; i += 1) rememberSearch(`sak-${String(i).padStart(2, '0')}`)
    const recents = readRecentSearches()
    expect(recents).toHaveLength(8)
    expect(recents[0]).toBe('sak-10')
    expect(recents[7]).toBe('sak-03')
    expect(JSON.parse(window.localStorage.getItem(RECENT_SEARCHES_KEY) ?? 'null')).toEqual([...recents])
  })

  it('writes them to storage while local retention is allowed', () => {
    rememberSearch('kari')
    expect(window.localStorage.getItem(RECENT_SEARCHES_KEY)).toBe(JSON.stringify(['kari']))
  })

  it('under ZDR nothing is written to storage, and the list still works for this session', () => {
    applyServerRetention(true)
    rememberSearch('kari')
    rememberSearch('ola')
    expect(window.localStorage.getItem(RECENT_SEARCHES_KEY)).toBeNull()
    expect(window.localStorage.length).toBe(0)
    expect(readRecentSearches()).toEqual(['ola', 'kari'])
  })

  it('a denial while the recents are mounted removes the stored copy at once', () => {
    rememberSearch('kari')
    expect(window.localStorage.getItem(RECENT_SEARCHES_KEY)).toBe(JSON.stringify(['kari']))
    const stop = watchRecentSearchRetention()
    try {
      applyServerRetention(true)
      expect(window.localStorage.getItem(RECENT_SEARCHES_KEY)).toBeNull()
    } finally {
      stop()
    }
  })
})
