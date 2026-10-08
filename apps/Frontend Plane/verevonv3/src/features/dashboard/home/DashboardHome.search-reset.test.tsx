// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/solid-query'
import { createRouter, memoryHistory, useLocation } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { SearchAnswerHandlers, WebSearchPayload } from '@/shared/api/search-client'

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
vi.mock('@/features/chat/lib/chat-appearance-preview', () => ({ chatAppearanceActive: () => true }))
vi.mock('@/features/chat/components/ChatPage', () => ({ default: () => null }))
vi.mock('@/shared/api/search-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/search-client')>()),
  findSimilar: api.findSimilar,
  loadQuerySuggestions: api.loadQuerySuggestions,
  loadSearchImages: api.loadSearchImages,
  loadSearchSuggestions: api.loadSearchSuggestions,
  loadSearchVideos: api.loadSearchVideos,
  runWebSearch: api.runWebSearch,
  streamSearchAnswer: api.streamSearchAnswer,
}))
vi.mock('@/shared/api/navbar-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/navbar-client')>()),
  searchNavbar: api.searchNavbar,
}))
vi.mock('./DashboardComposer', () => ({ DashboardComposer: (props: { message: string; onMessageChange: (value: string) => void }) =>
  <input id="verevon-dashboard-input" aria-label="Draft" value={props.message} onInput={(event) => props.onMessageChange(event.currentTarget.value)} /> }))
vi.mock('./KnowledgeComposer', () => ({ KnowledgeComposer: () => <p>Extract tool</p> }))
vi.mock('./DashboardCards', () => ({ DashboardCardsRail: () => null }))
vi.mock('./DashboardOverview', () => ({ DashboardOverview: () => <p>Overview</p> }))
vi.mock('./DashboardSectionOverview', () => ({ DashboardSectionOverview: () => null }))
import DashboardHome from './DashboardHome'
import { searchSnapshotStorageKey } from '@/shared/session/home-local-content'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'
import { query, resetSearchSession } from './dashboard-search-session'

// The saved snapshot belongs to the signed-in user and workspace.
const searchSessionKey = searchSnapshotStorageKey('user-a', 'org-a')!

const payload: WebSearchPayload = {
  results: [{ hostname: 'example.com', snippet: 'All systems normal', title: 'Agent status overview', url: 'https://example.com/status' }],
}
const savedSnapshot = {
  activeResultTab: 'Info',
  expanded: false,
  filters: { exactMatch: false, excludeDomains: [], includeDomains: [], timeRange: null, topic: null },
  previewResults: [{ hostname: 'example.com', snippet: 'Saved', title: 'Saved result', url: 'https://example.com/saved' }],
  query: 'saved query',
  savedAt: Date.now(),
  verevonResults: [],
  webAnswer: null,
  webCitations: [],
  webResults: [],
}
// Requests that never finish, so the test can see whether they were aborted.
const pending = { answer: [] as AbortSignal[], images: [] as AbortSignal[], navbar: [] as AbortSignal[] }
let queryClient: QueryClient

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  setSessionUser({ id: 'user-a', email: 'user-a@example.com', name: 'User A', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org-a', name: 'Org A', role: 'owner' })
  flush()
  resetSearchSession()
  for (const list of Object.values(pending)) list.length = 0
  api.runWebSearch.mockReset().mockResolvedValue(payload)
  api.loadSearchSuggestions.mockReset().mockResolvedValue({ suggestions: [] })
  api.loadQuerySuggestions.mockReset().mockResolvedValue({ correctedQuery: null, entity: null, relatedQueries: [] })
  api.loadSearchVideos.mockReset().mockResolvedValue({ videos: [] })
  api.findSimilar.mockReset().mockResolvedValue({ results: [] })
  api.loadSearchImages.mockReset().mockImplementation((_query: string, signal?: AbortSignal) => {
    if (signal) pending.images.push(signal)
    return new Promise(() => undefined)
  })
  api.searchNavbar.mockReset().mockImplementation((_query: string, signal?: AbortSignal) => {
    if (signal) pending.navbar.push(signal)
    return new Promise(() => undefined)
  })
  api.streamSearchAnswer.mockReset().mockImplementation((_query: string, handlers: SearchAnswerHandlers, signal?: AbortSignal) => {
    if (signal) pending.answer.push(signal)
    handlers.onDelta?.('Streaming answer')
    return new Promise(() => undefined)
  })
  queryClient = new QueryClient({ defaultOptions: { queries: { gcTime: Infinity, retry: false } } })
})
afterEach(() => {
  cleanup()
  resetSearchSession()
  clearSession()
})

function setup(path = '/dashboard?tab=web') {
  const history = memoryHistory(path)
  const TestRouter = createRouter({ history, explicitLinks: true, routes: [{ path: '/*all', component: () => {
    const location = useLocation()
    return <><DashboardHome /><output data-testid="location">{location.pathname}{location.search}</output></>
  } }] })
  const view = render(() => <QueryClientProvider client={queryClient}><TestRouter>{(props) => props.children}</TestRouter></QueryClientProvider>)
  return { history, ...view }
}
const tab = (name: 'Chat' | 'Web' | 'Extract') => screen.getByRole('button', { name })
const pressed = () => (['Chat', 'Web', 'Extract'] as const).filter((name) => tab(name).getAttribute('aria-pressed') === 'true')
const searchBox = () => screen.getByRole('combobox', { name: 'Search on the web or ask' }) as HTMLInputElement
const draft = () => screen.getByLabelText('Draft') as HTMLInputElement
const stored = () => localStorage.getItem(searchSessionKey)
const type = (value: string) => { fireEvent.input(searchBox(), { target: { value } }); flush() }
const switchTo = async (name: 'Chat' | 'Web' | 'Extract', expected: 'Chat' | 'Web' | 'Extract' = name) => {
  fireEvent.click(tab(name), { detail: 1 }); flush()
  await waitFor(() => expect(pressed()).toEqual([expected]))
}
// A real double-click: click, click, dblclick.
const doubleClick = (name: 'Chat' | 'Web' | 'Extract') => {
  fireEvent.click(tab(name), { detail: 1 }); flush()
  fireEvent.click(tab(name), { detail: 2 }); flush()
  fireEvent.dblClick(tab(name), { detail: 2 }); flush()
}
const pause = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

async function openExpandedSearch(text = 'agent status') {
  type(text)
  await waitFor(() => expect(screen.getByText('Agent status overview')).toBeTruthy())
  fireEvent.click(screen.getByRole('button', { name: 'See more' })); flush()
  await waitFor(() => expect(screen.getByPlaceholderText('Type a new search ...')).toBeTruthy())
  await waitFor(() => expect(screen.getAllByText('Streaming answer').length).toBeGreaterThan(0))
}

describe('Double-clicking the active Nett tab', () => {
  it('starts the web search over: query, saved snapshot, in-flight requests and panel state', async () => {
    const { container } = setup()
    await openExpandedSearch()
    fireEvent.click(screen.getByRole('button', { name: 'Videos' })); flush()
    expect(screen.getByRole('button', { name: 'Videos' }).getAttribute('aria-pressed')).toBe('true')
    expect(query()).toBe('agent status')
    expect(JSON.parse(stored()!)).toMatchObject({ activeResultTab: 'Videos', expanded: true, query: 'agent status' })
    expect(container.querySelector('.verevon-home-bands--expanded')).not.toBeNull()
    expect(pending.answer.length + pending.navbar.length + pending.images.length).toBeGreaterThanOrEqual(3)
    expect([...pending.answer, ...pending.navbar, ...pending.images].every((signal) => !signal.aborted)).toBe(true)

    doubleClick('Web')

    expect(query()).toBe('')
    expect(stored()).toBeNull()
    expect(sessionStorage.getItem(searchSessionKey)).toBeNull()
    expect([...pending.answer, ...pending.navbar, ...pending.images].every((signal) => signal.aborted)).toBe(true)
    expect(container.querySelector('.verevon-home-bands--expanded')).toBeNull()
    expect(container.querySelector('.verevon-home-composer-expanded')).toBeNull()
    expect(screen.queryByPlaceholderText('Type a new search ...')).toBeNull()
    expect(screen.queryByText('Streaming answer')).toBeNull()
    expect(searchBox().value).toBe('')
    expect(pressed()).toEqual(['Web'])
    // A retired panel must not write its leftovers back once it is gone.
    await pause(300)
    expect(stored()).toBeNull()
  })

  it('does not bring the deleted search back when the panel mounts again', async () => {
    setup()
    await openExpandedSearch()
    doubleClick('Web')
    await pause(300)
    expect(stored()).toBeNull()

    await switchTo('Chat')
    await switchTo('Web')
    expect(searchBox().value).toBe('')
    expect(screen.queryByText('Agent status overview')).toBeNull()
    expect(stored()).toBeNull()
  })

  it('collapses an inline preview too, and leaves nothing to restore', async () => {
    const { container } = setup()
    type('agent status')
    await waitFor(() => expect(screen.getByText('Agent status overview')).toBeTruthy())
    expect(container.querySelector('.verevon-home-bands--results')).not.toBeNull()
    expect(stored()).not.toBeNull()

    doubleClick('Web')

    expect(container.querySelector('.verevon-home-bands--results')).toBeNull()
    expect(screen.queryByText('Agent status overview')).toBeNull()
    expect(searchBox().value).toBe('')
    expect(stored()).toBeNull()
  })

  it('starts with fresh panel state: the result tab is Info again on the next search', async () => {
    setup()
    await openExpandedSearch()
    fireEvent.click(screen.getByRole('button', { name: 'Videos' })); flush()
    doubleClick('Web')

    type('agent status')
    await waitFor(() => expect(screen.getByText('Agent status overview')).toBeTruthy())
    fireEvent.click(screen.getByRole('button', { name: 'See more' })); flush()
    await waitFor(() => expect(screen.getByPlaceholderText('Type a new search ...')).toBeTruthy())
    expect(screen.getByRole('button', { name: 'Info' }).getAttribute('aria-pressed')).toBe('true')
    expect(screen.getByRole('button', { name: 'Videos' }).getAttribute('aria-pressed')).toBe('false')
  })

  it('leaves the shared web-search cache alone, so the same query is still instant', async () => {
    setup()
    await openExpandedSearch()
    expect(api.runWebSearch).toHaveBeenCalledTimes(1)
    const cached = queryClient.getQueryCache().findAll().map((entry) => entry.queryKey)
    expect(cached.length).toBeGreaterThan(0)

    doubleClick('Web')

    expect(queryClient.getQueryCache().findAll().map((entry) => entry.queryKey)).toEqual(cached)
    type('agent status')
    // The preview comes straight from the cache: no request, no waiting.
    expect(screen.getByText('Agent status overview')).toBeTruthy()
    expect(api.runWebSearch).toHaveBeenCalledTimes(1)
  })

  it('does not touch the chat composer or its draft', async () => {
    setup()
    fireEvent.input(draft(), { target: { value: 'Keep this draft' } }); flush()
    const composer = draft()
    await openExpandedSearch()
    expect(composer.closest('[hidden]')).not.toBeNull()

    doubleClick('Web')

    expect(draft()).toBe(composer)
    expect(composer.value).toBe('Keep this draft')
    expect(composer.closest('[hidden]')).not.toBeNull()
    await switchTo('Chat')
    expect(draft()).toBe(composer)
    expect(composer.value).toBe('Keep this draft')
    expect(composer.closest('[hidden]')).toBeNull()
  })

  it('resets again after a new search, every time', async () => {
    setup()
    for (const text of ['agent status', 'second query']) {
      await openExpandedSearch(text)
      doubleClick('Web')
      expect(query()).toBe('')
      expect(stored()).toBeNull()
      expect(searchBox().value).toBe('')
    }
  })
})

describe('Double-clicking other tabs', () => {
  it('only switches when the Nett tab was not active, and keeps the saved search', async () => {
    localStorage.setItem(searchSessionKey, JSON.stringify(savedSnapshot))
    setup('/dashboard')
    // A browser delivers click, click, dblclick about 100 ms apart, so the first
    // click has already switched the tab when the dblclick arrives.
    fireEvent.click(tab('Web'), { detail: 1 }); flush()
    await waitFor(() => expect(pressed()).toEqual(['Web']))
    fireEvent.click(tab('Web'), { detail: 2 }); flush()
    fireEvent.dblClick(tab('Web'), { detail: 2 }); flush()

    expect(searchBox().value).toBe('saved query')
    expect(stored()).not.toBeNull()
    expect(pressed()).toEqual(['Web'])
    expect(query()).toBe('saved query')
  })

  it('does nothing to the search when Hent ut is double-clicked, active or not', async () => {
    localStorage.setItem(searchSessionKey, JSON.stringify(savedSnapshot))
    setup('/dashboard?tab=web')
    await waitFor(() => expect(searchBox().value).toBe('saved query'))

    fireEvent.click(tab('Extract'), { detail: 1 }); flush()
    await waitFor(() => expect(pressed()).toEqual(['Extract']))
    fireEvent.click(tab('Extract'), { detail: 2 }); flush()
    fireEvent.dblClick(tab('Extract'), { detail: 2 }); flush()
    doubleClick('Extract')

    expect(screen.getByText('Extract tool')).toBeTruthy()
    expect(stored()).not.toBeNull()
    expect(query()).toBe('saved query')
    await switchTo('Web')
    expect(searchBox().value).toBe('saved query')
  })

  it('does nothing to the search when Chat is double-clicked, and keeps the draft', async () => {
    localStorage.setItem(searchSessionKey, JSON.stringify(savedSnapshot))
    setup('/dashboard?tab=web')
    await waitFor(() => expect(searchBox().value).toBe('saved query'))
    await switchTo('Chat')
    fireEvent.input(draft(), { target: { value: 'Unsent' } }); flush()

    doubleClick('Chat')

    expect(draft().value).toBe('Unsent')
    expect(stored()).not.toBeNull()
    expect(query()).toBe('saved query')
    await switchTo('Web')
    expect(searchBox().value).toBe('saved query')
  })
})
