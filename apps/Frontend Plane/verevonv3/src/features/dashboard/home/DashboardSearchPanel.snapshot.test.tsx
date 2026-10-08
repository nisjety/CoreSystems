// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/solid-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, createStore, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { WebSearchPayload } from '@/shared/api/search-client'

const api = vi.hoisted(() => ({
  findSimilar: vi.fn(),
  loadQuerySuggestions: vi.fn(),
  loadSearchImages: vi.fn(),
  loadSearchSuggestions: vi.fn(),
  loadSearchVideos: vi.fn(),
  runWebSearch: vi.fn(),
  searchNavbar: vi.fn(),
  streamSearchAnswer: vi.fn(),
}))

vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }) }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))
vi.mock('@solidjs/router', () => ({ useNavigate: () => () => undefined }))
vi.mock('@/features/chat/lib/pending-chat-launch', () => ({ writePendingChatLaunch: vi.fn() }))
vi.mock('@/shared/api/search-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/search-client')>()),
  findSimilar: api.findSimilar,
  loadQuerySuggestions: api.loadQuerySuggestions,
  loadSearchImages: api.loadSearchImages,
  loadSearchSuggestions: api.loadSearchSuggestions,
  loadSearchVideos: api.loadSearchVideos,
  runWebSearch: api.runWebSearch,
  searchNavbar: api.searchNavbar,
  streamSearchAnswer: api.streamSearchAnswer,
}))
vi.mock('@/shared/api/navbar-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/navbar-client')>()),
  searchNavbar: api.searchNavbar,
}))
import { __resetRetentionForTests, applyServerRetention } from '@/features/chat/lib/chat-retention'
import { clearSearchSnapshots, searchSnapshotStorageKey } from '@/shared/session/home-local-content'
import { SearchPanel } from './DashboardSearchPanel'
import { query, resetSearchSession, runSearch } from './dashboard-search-session'

type Identity = { user: { id: string } | null; activeOrg: { id: string } | null }
const [identity, setIdentity] = createStore<Identity>({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } })
const signIn = (user: string | null, org: string | null) => {
  setIdentity((state) => { state.user = user ? { id: user } : null; state.activeOrg = org ? { id: org } : null })
  flush()
}
const keyA = searchSnapshotStorageKey('user-a', 'org-a')!
const keyB = searchSnapshotStorageKey('user-b', 'org-a')!
const keyAOrgB = searchSnapshotStorageKey('user-a', 'org-b')!
const legacyKey = 'verevon.dashboard.search.session'
const payload: WebSearchPayload = {
  results: [{ hostname: 'example.com', snippet: 'All systems normal', title: 'Agent status overview', url: 'https://example.com/status' }],
}
const snapshotFor = (text: string, expanded = false) => ({
  activeResultTab: 'Info',
  expanded,
  filters: { exactMatch: false, excludeDomains: [], includeDomains: [], timeRange: null, topic: null },
  previewResults: [],
  query: text,
  savedAt: Date.now(),
  verevonResults: [],
  webAnswer: null,
  webCitations: [],
  webResults: payload.results!.map((result) => ({ ...result })),
})
let queryClient: QueryClient

const compactBox = () => screen.getByRole('combobox', { name: 'Search on the web or ask' }) as HTMLInputElement
const expandedBox = () => screen.getByPlaceholderText('Type a new search ...') as HTMLInputElement
const currentBox = () => (screen.queryByRole('combobox', { name: 'Search on the web or ask' }) ?? screen.getByPlaceholderText('Type a new search ...')) as HTMLInputElement
const type = (value: string) => { fireEvent.input(currentBox(), { target: { value } }); flush() }
const stored = (key = keyA) => localStorage.getItem(key)
const storedQuery = (key = keyA) => (JSON.parse(stored(key) ?? 'null') as { query?: string } | null)?.query
const tick = () => new Promise<void>((resolve) => queueMicrotask(resolve))
const pause = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

function setup() {
  const [expanded, setExpanded] = createSignal(false)
  const view = render(() => (
    <QueryClientProvider client={queryClient}>
      <SearchPanel expanded={expanded()} onExpandedChange={setExpanded} onPreviewActiveChange={() => undefined} rememberSearches />
    </QueryClientProvider>
  ))
  return { ...view, expanded }
}

// Types a search and submits it; resolves once the results page is open.
async function searchFor(text = 'agent status') {
  type(text)
  fireEvent.submit(compactBox().closest('form')!); flush()
  await waitFor(() => expect(screen.getByRole('button', { name: 'Open Agent status overview in Verevon' })).toBeTruthy())
}

beforeEach(() => {
  localStorage.clear(); sessionStorage.clear()
  signIn('user-a', 'org-a')
  resetSearchSession(); flush()
  api.runWebSearch.mockReset().mockResolvedValue(payload)
  api.loadSearchSuggestions.mockReset().mockResolvedValue({ suggestions: [] })
  api.loadQuerySuggestions.mockReset().mockResolvedValue({ correctedQuery: null, entity: null, relatedQueries: [] })
  api.loadSearchImages.mockReset().mockResolvedValue({ images: [] })
  api.loadSearchVideos.mockReset().mockResolvedValue({ videos: [] })
  api.findSimilar.mockReset().mockResolvedValue({ results: [] })
  api.searchNavbar.mockReset().mockResolvedValue({ results: [] })
  api.streamSearchAnswer.mockReset().mockResolvedValue(undefined)
  queryClient = new QueryClient({ defaultOptions: { queries: { gcTime: Infinity, retry: false } } })
  vi.spyOn(window, 'open').mockReturnValue(null)
})
afterEach(() => {
  cleanup()
  resetSearchSession()
  __resetRetentionForTests()
  vi.restoreAllMocks()
})

describe('the saved search belongs to the signed-in user and workspace', () => {
  it('is saved under the current identity\'s key and nowhere else', async () => {
    setup()
    type('agent status')
    await waitFor(() => expect(storedQuery()).toBe('agent status'))
    expect(sessionStorage.getItem(keyA)).not.toBeNull()
    expect(stored(legacyKey)).toBeNull()
    expect(stored(keyB)).toBeNull()
    expect(stored(keyAOrgB)).toBeNull()
  })

  it('is restored for the same user and workspace', () => {
    localStorage.setItem(keyA, JSON.stringify(snapshotFor('saved query')))
    setup()
    expect(compactBox().value).toBe('saved query')
  })

  it('never restores another user\'s or another workspace\'s search, or the unscoped one older builds wrote', () => {
    localStorage.setItem(keyB, JSON.stringify(snapshotFor('another user')))
    localStorage.setItem(keyAOrgB, JSON.stringify(snapshotFor('another workspace')))
    localStorage.setItem(legacyKey, JSON.stringify(snapshotFor('unscoped')))
    setup()
    expect(compactBox().value).toBe('')
    expect(query()).toBe('')
  })

  it('reads and writes nothing until both the user and the workspace are known', async () => {
    localStorage.setItem(keyA, JSON.stringify(snapshotFor('saved query')))
    signIn('user-a', null)
    const write = vi.spyOn(localStorage, 'setItem')
    setup()
    expect(compactBox().value).toBe('')
    type('agent status')
    await pause(50)
    expect(write).not.toHaveBeenCalled()
    expect(storedQuery()).toBe('saved query')
  })

  it('writes to the identity that is current when the search changes, not the one the panel was created under', async () => {
    setup()
    type('as user a')
    await waitFor(() => expect(storedQuery(keyA)).toBe('as user a'))
    signIn('user-b', 'org-a')
    type('as user b')
    await waitFor(() => expect(storedQuery(keyB)).toBe('as user b'))
    expect(storedQuery(keyA)).toBe('as user a')
  })

  it('is removed with everyone\'s at sign-out, and the query in the box goes with it', async () => {
    localStorage.setItem(keyB, JSON.stringify(snapshotFor('another user')))
    setup()
    type('agent status')
    await waitFor(() => expect(storedQuery()).toBe('agent status'))

    signIn(null, null)
    clearSearchSnapshots(); flush()

    expect(stored(keyA)).toBeNull()
    expect(stored(keyB)).toBeNull()
    expect(query()).toBe('')
    await waitFor(() => expect(compactBox().value).toBe(''))
    await pause(50)
    expect(localStorage.length).toBe(0)
  })
})

describe('in a zero-data-retention workspace', () => {
  it('saves nothing and restores nothing', async () => {
    localStorage.setItem(keyA, JSON.stringify(snapshotFor('saved query')))
    applyServerRetention(true)
    localStorage.setItem(keyA, JSON.stringify(snapshotFor('leftover')))
    const write = vi.spyOn(localStorage, 'setItem')
    setup()
    expect(compactBox().value).toBe('')
    type('agent status')
    await pause(50)
    expect(write).not.toHaveBeenCalled()
  })

  it('removes the saved search the moment the verdict arrives and does not save the next change', async () => {
    setup()
    type('agent status')
    await waitFor(() => expect(storedQuery()).toBe('agent status'))

    applyServerRetention(true); flush()
    expect(stored()).toBeNull()
    expect(sessionStorage.getItem(keyA)).toBeNull()

    type('agent status and more')
    await pause(50)
    expect(stored()).toBeNull()
  })

  it('saves again once the workspace allows local copies', async () => {
    setup()
    applyServerRetention(true); flush()
    applyServerRetention(false); flush()
    type('agent status')
    await waitFor(() => expect(storedQuery()).toBe('agent status'))
  })
})

describe('emptying the box', () => {
  it('removes the saved search, so it does not come back when the panel mounts again', async () => {
    const first = setup()
    type('haugesund vær')
    await waitFor(() => expect(storedQuery()).toBe('haugesund vær'))

    type('')

    expect(compactBox().value).toBe('')
    expect(query()).toBe('')
    await waitFor(() => expect(stored()).toBeNull())
    expect(sessionStorage.getItem(keyA)).toBeNull()
    first.unmount()

    setup()
    expect(compactBox().value).toBe('')
    expect(query()).toBe('')
  })

  it('removes the saved search of a finished search too, not only a typed one', async () => {
    const first = setup()
    await searchFor()
    expect(storedQuery()).toBe('agent status')
    fireEvent.click(screen.getByRole('button', { name: 'Compact' })); flush()
    await waitFor(() => expect(compactBox().value).toBe('agent status'))

    type('')

    await waitFor(() => expect(stored()).toBeNull())
    first.unmount()
    setup()
    expect(compactBox().value).toBe('')
    expect(screen.queryByPlaceholderText('Type a new search ...')).toBeNull()
  })

  it('leaves another account\'s saved search alone', async () => {
    localStorage.setItem(keyB, JSON.stringify(snapshotFor('another user')))
    setup()
    type('agent status')
    await waitFor(() => expect(storedQuery()).toBe('agent status'))
    type('')
    await waitFor(() => expect(stored()).toBeNull())
    expect(storedQuery(keyB)).toBe('another user')
  })
})

describe('keyboard focus when the search page opens', () => {
  it("does not move focus into the results page's search box on an ordinary submit, which would raise a touch keyboard over the results", async () => {
    setup()
    type('agent status')
    compactBox().focus()
    fireEvent.submit(compactBox().closest('form')!); flush()
    await waitFor(() => expect(screen.getByPlaceholderText('Type a new search ...')).toBeTruthy())
    await tick()
    expect(document.activeElement).not.toBe(expandedBox())
  })

  it("moves to the results page's search box when a keyboard press outside started the search and the control that started it is gone", async () => {
    setup()
    const opener = document.createElement('button')
    document.body.appendChild(opener)
    opener.focus()
    expect(document.activeElement).toBe(opener)

    expect(runSearch('agent status', { focus: true })).toBe(true)
    // Home hides the overview, and the row that was pressed goes with it.
    opener.remove()
    await waitFor(() => expect(screen.getByPlaceholderText('Type a new search ...')).toBeTruthy())
    await tick()
    expect(document.activeElement).toBe(expandedBox())
  })

  it('takes no focus for a search started from outside by a mouse or a touch', async () => {
    setup()
    const opener = document.createElement('button')
    document.body.appendChild(opener)
    opener.focus()

    runSearch('agent status')
    opener.remove()
    await waitFor(() => expect(screen.getByPlaceholderText('Type a new search ...')).toBeTruthy())
    await tick()

    expect(document.activeElement).not.toBe(expandedBox())
  })

  it('leaves focus alone when it has moved to something else that is still there', async () => {
    setup()
    const other = document.createElement('input')
    document.body.appendChild(other)
    other.focus()

    runSearch('agent status', { focus: true })
    await waitFor(() => expect(screen.getByPlaceholderText('Type a new search ...')).toBeTruthy())
    await tick()

    expect(document.activeElement).toBe(other)
    other.remove()
  })
})
