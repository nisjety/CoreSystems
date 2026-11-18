import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { applyServerRetention, isLocalRetentionAllowed, __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { __resetCommandsForTests } from '@/features/command/command-registry'
import { installHotkeys } from '@/features/command/hotkeys'
import { ApiError } from '@/shared/api/http'
import type { SpaceRosterMember, SpaceSearchHit, SpaceSearchPage, SpaceSearchRequest } from '@/shared/api/spaces-client'

import { clearSpaceRoomFeatures, publishSpaceRoomFeatures } from '../lib/space-room-features'
import { __resetRecentSearchesForTests, RECENT_SEARCHES_KEY, SEARCH_DEBOUNCE_MS } from '../lib/space-search-state'

// The room's search calls the gateway through the spaces client. Each request is held open here, so a case decides
// when, and whether, it answers, and can read the exact request and the abort signal the component handed over.
const search = vi.hoisted(() => ({ searchSpaceMessages: vi.fn() }))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  searchSpaceMessages: search.searchSpaceMessages,
}))

const { SpaceRoomSearch } = await import('./SpaceRoomSearch')

type Pending = {
  readonly request: SpaceSearchRequest
  readonly signal: AbortSignal
  readonly resolve: (page: SpaceSearchPage) => void
  readonly reject: (error: unknown) => void
}

let pending: Pending[] = []
let openThread: string | null = null
let onOpenHit = vi.fn()

const roster: SpaceRosterMember[] = [
  { subject_type: 'user', subject_id: 'sub-kari', role: 'owner', revision: 1, display_name: 'Kari' },
  { subject_type: 'user', subject_id: 'sub-ola', role: 'editor', revision: 1, display_name: 'Ola' },
]

function hit(messageId: string, overrides: Partial<SpaceSearchHit> = {}): SpaceSearchHit {
  return {
    messageId,
    threadId: 'thr-1',
    spaceId: 'space-room-a',
    role: 'user',
    authorSubjectId: 'sub-ola',
    agentName: undefined,
    createdAt: '2026-11-01T10:00:00.000Z',
    snippet: [
      { text: 'Vi møtes i ', matched: false },
      { text: 'kari', matched: true },
    ],
    threadTitle: 'Planning',
    ...overrides,
  }
}

function page(hits: readonly SpaceSearchHit[], overrides: Partial<SpaceSearchPage> = {}): SpaceSearchPage {
  return { hits, nextCursor: null, truncated: false, coverage: 'shared', excluded: [], ...overrides }
}

function lastPending(): Pending {
  const found = pending[pending.length - 1]
  if (!found) throw new Error('no search was sent')
  return found
}

function renderSearch(): void {
  render(() => (
    <SpaceRoomSearch
      spaceRef="space-room-a"
      roster={() => roster}
      viewerSubjectId={() => 'sub-kari'}
      openThreadId={() => openThread}
      onOpenHit={onOpenHit}
    />
  ))
  flush()
}

function trigger(): HTMLElement {
  return screen.getByRole('button', { name: 'Søk i meldinger' })
}

function openPanel(): HTMLInputElement {
  fireEvent.click(trigger())
  flush()
  return screen.getByRole('combobox', { name: 'Søk i meldinger' }) as HTMLInputElement
}

// Types one query and lets the debounce window pass, so the request leaves the page.
async function typeTo(input: HTMLInputElement, value: string): Promise<void> {
  fireEvent.input(input, { target: { value } })
  flush()
  await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS)
  flush()
}

// Lets the component's promise callbacks run, then applies Solid's pending signal writes.
async function settle(): Promise<void> {
  await vi.advanceTimersByTimeAsync(0)
  flush()
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'Date'] })
  // A dev build may force the flags with VITE_*; the cases here read the room's own features, so no override applies.
  vi.stubEnv('VITE_SPACE_MESSAGE_SEARCH_UI', '')
  vi.stubEnv('VITE_SPACE_THREAD_PANE_UI', '')
  // jsdom has no CSS.escape; the component uses it to keep the active option in view.
  vi.stubGlobal('CSS', { escape: (value: string) => value })
  pending = []
  openThread = null
  onOpenHit = vi.fn()
  search.searchSpaceMessages.mockReset()
  search.searchSpaceMessages.mockImplementation(
    (_spaceRef: string, request: SpaceSearchRequest, signal: AbortSignal) =>
      new Promise<SpaceSearchPage>((resolve, reject) => {
        pending.push({ request, signal, resolve, reject })
      }),
  )
  publishSpaceRoomFeatures({ message_search: true, thread_pane: true })
  __resetRetentionForTests()
  __resetRecentSearchesForTests()
  __resetCommandsForTests()
})

afterEach(() => {
  cleanup()
  vi.useRealTimers()
  vi.unstubAllEnvs()
  vi.unstubAllGlobals()
  clearSpaceRoomFeatures()
  __resetRetentionForTests()
  __resetRecentSearchesForTests()
})

describe('in-room message search (WS13-S5)', () => {
  it('debounces the query 250 ms, then sends one request, and a newer query aborts the one still in flight', async () => {
    renderSearch()
    const input = openPanel()
    fireEvent.input(input, { target: { value: 'kari' } })
    flush()
    await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS - 1)
    expect(search.searchSpaceMessages).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(1)
    expect(search.searchSpaceMessages).toHaveBeenCalledTimes(1)
    expect(pending[0]?.request).toMatchObject({ query: 'kari', prefix: true, scope: 'room' })
    expect(pending[0]?.signal.aborted).toBe(false)

    await typeTo(input, 'kari o')
    expect(search.searchSpaceMessages).toHaveBeenCalledTimes(2)
    expect(pending[0]?.signal.aborted).toBe(true)
    expect(pending[1]?.signal.aborted).toBe(false)
    expect(pending[1]?.request.query).toBe('kari o')
  })

  it('sends a typed word as a prefix match, and Enter sends it again as a finished word', async () => {
    renderSearch()
    const input = openPanel()
    await typeTo(input, 'kari')
    expect(pending).toHaveLength(1)
    expect(pending[0]?.request.prefix).toBe(true)

    fireEvent.keyDown(input, { key: 'Enter' })
    flush()
    expect(pending).toHaveLength(2)
    expect(pending[1]?.request).toMatchObject({ query: 'kari', prefix: false })
    expect(pending[0]?.signal.aborted).toBe(true)
  })

  it('sends the scope each chip names: this room, this thread, or one person once picked', async () => {
    openThread = 'thr-1'
    renderSearch()
    const input = openPanel()
    await typeTo(input, 'kari')
    expect(pending[0]?.request).toMatchObject({ scope: 'room', query: 'kari' })
    expect(pending[0]?.request.threadId).toBeUndefined()

    fireEvent.click(screen.getByRole('button', { name: 'Denne tråden' }))
    flush()
    expect(pending).toHaveLength(2)
    expect(pending[1]?.request).toMatchObject({ scope: 'thread', threadId: 'thr-1', query: 'kari' })
    expect(pending[0]?.signal.aborted).toBe(true)

    fireEvent.click(screen.getByRole('button', { name: 'Fra en person' }))
    flush()
    // A person scope waits for a person: nothing is sent until one is picked.
    expect(pending).toHaveLength(2)
    expect(screen.getByRole('button', { name: 'Fra en person' }).getAttribute('aria-pressed')).toBe('true')
    fireEvent.change(screen.getByRole('combobox', { name: 'Velg person' }), { target: { value: 'sub-ola' } })
    flush()
    expect(pending).toHaveLength(3)
    expect(pending[2]?.request).toMatchObject({ scope: 'person', authorSubjectId: 'sub-ola', query: 'kari' })
    expect(pending[2]?.request.threadId).toBeUndefined()
  })

  it('the arrows move the active hit (aria-activedescendant), and Enter opens that hit in context', async () => {
    renderSearch()
    const input = openPanel()
    await typeTo(input, 'kari')
    lastPending().resolve(page([hit('m-1'), hit('m-2', { authorSubjectId: 'sub-kari' })]))
    await settle()

    const options = screen.getAllByRole('option')
    expect(options).toHaveLength(2)
    expect(options[0]?.textContent).toContain('Ola')
    expect(options[1]?.textContent).toContain('Kari (deg)')
    expect(input.getAttribute('aria-expanded')).toBe('true')
    expect(input.getAttribute('aria-activedescendant')).toBeNull()

    fireEvent.keyDown(input, { key: 'ArrowDown' })
    flush()
    expect(input.getAttribute('aria-activedescendant')).toBe(options[0]?.id)
    fireEvent.keyDown(input, { key: 'ArrowDown' })
    flush()
    expect(input.getAttribute('aria-activedescendant')).toBe(options[1]?.id)
    fireEvent.keyDown(input, { key: 'ArrowUp' })
    flush()
    expect(options[0]?.getAttribute('aria-selected')).toBe('true')

    fireEvent.keyDown(input, { key: 'Enter' })
    flush()
    expect(onOpenHit).toHaveBeenCalledTimes(1)
    expect(onOpenHit.mock.calls[0]?.[0]).toMatchObject({ messageId: 'm-1' })
  })

  it('Esc closes the search, aborts what it had sent, and returns focus to the control that opened it', async () => {
    vi.useRealTimers()
    renderSearch()
    const opener = trigger() as HTMLButtonElement
    opener.focus()
    const input = openPanel()
    fireEvent.input(input, { target: { value: 'kari' } })
    flush()
    await waitFor(() => expect(search.searchSpaceMessages).toHaveBeenCalledTimes(1))

    fireEvent.keyDown(input, { key: 'Escape' })
    flush()
    expect(screen.queryByRole('search')).toBeNull()
    expect(pending[0]?.signal.aborted).toBe(true)
    await waitFor(() => expect(document.activeElement).toBe(trigger()))
  })

  it('a second Ctrl+F with the input focused is not taken, so the browser keeps its own find', async () => {
    const dispose = installHotkeys(() => ({
      spaceRef: 'space-room-a',
      chatThreadId: null,
      offline: false,
      navigate: vi.fn(),
      openPalette: vi.fn(),
      openShortcutSheet: vi.fn(),
    }))
    try {
      renderSearch()
      // The first press is taken: the command opens the search and the press is prevented.
      const first = fireEvent.keyDown(trigger(), { key: 'f', ctrlKey: true })
      flush()
      expect(first).toBe(false)
      const input = screen.getByRole('combobox', { name: 'Søk i meldinger' }) as HTMLInputElement
      input.focus()
      flush()
      // The command is hidden while the input has focus, so the press is left alone.
      const second = fireEvent.keyDown(input, { key: 'f', ctrlKey: true })
      flush()
      expect(second).toBe(true)
      expect(screen.getByRole('search')).toBeTruthy()
    } finally {
      dispose()
    }
  })

  it('renders snippets as text, with the matched words in <mark>, and a message that holds markup stays text', async () => {
    renderSearch()
    const input = openPanel()
    await typeTo(input, 'kari')
    lastPending().resolve(
      page([
        hit('m-1', {
          snippet: [
            { text: 'Hei <img src="x" onerror="window.__searchInjected = true"> ', matched: false },
            { text: 'kari', matched: true },
          ],
        }),
      ]),
    )
    await settle()

    const option = screen.getByRole('option')
    expect(option.querySelector('mark')?.textContent).toBe('kari')
    expect(option.querySelector('img')).toBeNull()
    expect(option.textContent).toContain('<img src="x"')
    expect((window as unknown as Record<string, unknown>).__searchInjected).toBeUndefined()
  })

  it('says when only the reader’s own conversations are searched, and when the list is cut short', async () => {
    renderSearch()
    const input = openPanel()
    await typeTo(input, 'kari')
    lastPending().resolve(page([hit('m-1')], { coverage: 'own_threads_only', truncated: true }))
    await settle()

    const notes = screen.getAllByRole('note').map((note) => note.textContent ?? '')
    expect(notes).toEqual([
      'Viser bare samtaler du selv har startet: organisasjonen har ikke slått på delt lesing i dette rommet.',
      'Viser de første 100 treffene. Snevre inn søket.',
    ])
  })

  it('names each failure with its own sentence, and offers a retry only where a retry can help', async () => {
    const cases = [
      { error: new ApiError('busy', 429, 'search_busy'), copy: 'Søket er opptatt akkurat nå.', retry: true },
      { error: new ApiError('timeout', 503, 'search_timeout'), copy: 'Søket tok for lang tid.', retry: true },
      { error: new ApiError('down', 503, 'search_unavailable'), copy: 'Søket er utilgjengelig akkurat nå.', retry: true },
      { error: new ApiError('off', 404, 'space_search_disabled'), copy: 'Meldingssøk er ikke slått på for dette rommet.', retry: false },
      { error: new ApiError('no access', 403, 'active_membership_required'), copy: 'Du har ikke lenger tilgang til å søke her.', retry: false },
    ]
    const sentences: string[] = []
    for (const { error, copy, retry } of cases) {
      renderSearch()
      const input = openPanel()
      await typeTo(input, 'kari')
      lastPending().reject(error)
      await settle()

      const state = document.querySelector('[data-state="error"]')
      expect(state?.textContent).toContain(copy)
      expect(Boolean(screen.queryByRole('button', { name: 'Prøv igjen' }))).toBe(retry)
      sentences.push(state?.textContent ?? '')
      cleanup()
      pending = []
    }
    expect(new Set(sentences).size).toBe(cases.length)
  })

  it('shows nothing and sends nothing while features.message_search is off', () => {
    publishSpaceRoomFeatures({ message_search: false, thread_pane: true })
    renderSearch()
    expect(screen.queryByRole('button', { name: 'Søk i meldinger' })).toBeNull()
    expect(search.searchSpaceMessages).not.toHaveBeenCalled()
  })
})

describe('in-room search under zero data retention (WS13-S5)', () => {
  it('under ZDR nothing is written to localStorage, the recents key included', async () => {
    applyServerRetention(true)
    expect(isLocalRetentionAllowed()).toBe(false)
    window.localStorage.clear()

    renderSearch()
    const input = openPanel()
    await typeTo(input, 'kari')
    lastPending().resolve(page([hit('m-1')]))
    await settle()
    // ArrowDown puts the first hit in focus; Enter opens it, and the finished query is remembered in memory only.
    fireEvent.keyDown(input, { key: 'ArrowDown' })
    flush()
    fireEvent.keyDown(input, { key: 'Enter' })
    flush()
    expect(onOpenHit).toHaveBeenCalledTimes(1)

    expect(window.localStorage.length).toBe(0)
    expect(window.localStorage.getItem(RECENT_SEARCHES_KEY)).toBeNull()
  })

  it('a retention denial removes the recents from storage and from the list on screen, at once', async () => {
    renderSearch()
    const input = openPanel()
    await typeTo(input, 'kari')
    lastPending().resolve(page([]))
    await settle()
    // Enter with no hit active sends the finished word; the search is remembered.
    fireEvent.keyDown(input, { key: 'Enter' })
    flush()
    lastPending().resolve(page([]))
    await settle()
    expect(window.localStorage.getItem(RECENT_SEARCHES_KEY)).toBe(JSON.stringify(['kari']))

    fireEvent.keyDown(input, { key: 'Escape' })
    flush()
    openPanel()
    expect(screen.getByRole('button', { name: 'kari' })).toBeTruthy()

    applyServerRetention(true)
    flush()
    expect(window.localStorage.getItem(RECENT_SEARCHES_KEY)).toBeNull()
    expect(screen.queryByRole('button', { name: 'kari' })).toBeNull()

    // Closing and reopening does not bring them back.
    fireEvent.keyDown(screen.getByRole('combobox', { name: 'Søk i meldinger' }), { key: 'Escape' })
    flush()
    openPanel()
    expect(screen.queryByRole('button', { name: 'kari' })).toBeNull()
  })
})
