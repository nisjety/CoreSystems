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
  streamSearchAnswer: api.streamSearchAnswer,
}))
vi.mock('@/shared/api/navbar-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/navbar-client')>()),
  searchNavbar: api.searchNavbar,
}))
import { SearchPanel } from './DashboardSearchPanel'
import { searchSnapshotStorageKey } from '@/shared/session/home-local-content'
import { query, resetSearchSession, runSearch } from './dashboard-search-session'
import { webHistoryStorageKey } from './web-history'

type Identity = { user: { id: string } | null; activeOrg: { id: string } | null }
const [identity, setIdentity] = createStore<Identity>({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } })
const signIn = (user: string | null, org: string | null) => {
  setIdentity((state) => { state.user = user ? { id: user } : null; state.activeOrg = org ? { id: org } : null })
  flush()
}
const keyA = webHistoryStorageKey('user-a', 'org-a')!
const searchSessionKey = searchSnapshotStorageKey('user-a', 'org-a')!
const keyB = webHistoryStorageKey('user-b', 'org-a')!
const payload: WebSearchPayload = {
  results: [
    { hostname: 'example.com', snippet: 'All systems normal', title: 'Agent status overview', url: 'https://example.com/status' },
    { hostname: 'docs.example.org', snippet: 'Second page', title: 'Second result', url: 'https://docs.example.org/second' },
  ],
}
let queryClient: QueryClient
let openWindow: ReturnType<typeof vi.spyOn>

const stored = (key: string) => (JSON.parse(localStorage.getItem(key) ?? 'null') ?? []) as Array<{ kind: string; text: string; url?: string; at: string }>
const summary = (key: string) => stored(key).map((entry) => (entry.kind === 'page' ? `p:${entry.text}|${entry.url}` : `q:${entry.text}`))
// The compact box, or the "new search" box once the results are open.
const searchBox = () => (screen.queryByRole('combobox', { name: 'Search on the web or ask' }) ?? screen.getByPlaceholderText('Type a new search ...')) as HTMLInputElement
const type = (value: string) => { fireEvent.input(searchBox(), { target: { value } }); flush() }
// Links open a new tab; jsdom has nowhere to navigate, so a click on one is swallowed here.
const click = (element: Element, init?: MouseEventInit) => {
  element.addEventListener('click', (event) => event.preventDefault(), { once: true })
  fireEvent.click(element, init); flush()
}
const cardButton = () => screen.getByRole('button', { name: 'Open Agent status overview in Verevon' })
// jsdom has no auxclick helper; a middle-click is the event the browser sends for it.
const auxClick = (element: Element, button: number) => { fireEvent(element, new MouseEvent('auxclick', { bubbles: true, button, cancelable: true })); flush() }
const cardLink = () => screen.getByRole('link', { name: 'Open Agent status overview in a new tab' })

function setup(options: { remember?: boolean | undefined } = { remember: true }) {
  const [expanded, setExpanded] = createSignal(false)
  const [remember, setRemember] = createSignal<boolean | undefined>(options.remember)
  const view = render(() => (
    <QueryClientProvider client={queryClient}>
      <SearchPanel expanded={expanded()} onExpandedChange={setExpanded} onPreviewActiveChange={() => undefined} rememberSearches={remember()} />
    </QueryClientProvider>
  ))
  return { ...view, expanded, setRemember }
}

// Types a search and submits it; resolves once the results are on screen.
async function searchFor(text = 'agent status') {
  type(text)
  fireEvent.submit(searchBox().closest('form')!); flush()
  await waitFor(() => expect(cardButton()).toBeTruthy())
  await waitFor(() => expect(screen.getByRole('button', { name: 'Refresh' }).hasAttribute('disabled')).toBe(false))
}

beforeEach(() => {
  localStorage.clear(); sessionStorage.clear()
  resetSearchSession()
  signIn('user-a', 'org-a')
  api.runWebSearch.mockReset().mockResolvedValue(payload)
  api.loadSearchSuggestions.mockReset().mockResolvedValue({ suggestions: [] })
  api.loadQuerySuggestions.mockReset().mockResolvedValue({ correctedQuery: null, entity: null, relatedQueries: [] })
  api.loadSearchImages.mockReset().mockResolvedValue({ images: [] })
  api.loadSearchVideos.mockReset().mockResolvedValue({ videos: [] })
  api.findSimilar.mockReset().mockResolvedValue({ results: [] })
  api.searchNavbar.mockReset().mockResolvedValue({ results: [] })
  api.streamSearchAnswer.mockReset().mockResolvedValue(undefined)
  queryClient = new QueryClient({ defaultOptions: { queries: { gcTime: Infinity, retry: false } } })
  openWindow = vi.spyOn(window, 'open').mockReturnValue(null)
})
afterEach(() => {
  cleanup()
  resetSearchSession()
  vi.restoreAllMocks()
})

describe('searches the panel runs', () => {
  it('remembers a search when it is submitted', async () => {
    setup()
    await searchFor('agent status')
    expect(summary(keyA)).toEqual(['q:agent status'])
    expect(Number.isNaN(Date.parse(stored(keyA)[0]!.at))).toBe(false)
  })

  it('does not remember typing, suggestions or the typeahead preview', async () => {
    api.loadSearchSuggestions.mockResolvedValue({ suggestions: [{ text: 'agent status page', source: 'history' }] })
    setup()
    type('agent'); type('agent s'); type('agent status')
    await waitFor(() => expect(api.runWebSearch).toHaveBeenCalled())
    await waitFor(() => expect(screen.getByText('Agent status overview')).toBeTruthy())
    await waitFor(() => expect(screen.getByText('Suggestions')).toBeTruthy())
    fireEvent.mouseDown(screen.getByText('agent status page').closest('button')!); flush()
    expect(localStorage.getItem(keyA)).toBeNull()
  })

  it('remembers a search started from a preview result or See more', async () => {
    setup()
    type('agent status')
    await waitFor(() => expect(screen.getByText('Agent status overview')).toBeTruthy())
    fireEvent.click(screen.getByText('Agent status overview').closest('button')!); flush()
    await waitFor(() => expect(cardButton()).toBeTruthy())
    expect(summary(keyA)).toEqual(['q:agent status'])
  })

  it('remembers a related search and a did-you-mean correction', async () => {
    api.loadQuerySuggestions.mockResolvedValue({ correctedQuery: 'agent statuses', entity: null, relatedQueries: ['agent metrics'] })
    setup()
    await searchFor('agent status')
    await waitFor(() => expect(screen.getByRole('button', { name: 'agent metrics' })).toBeTruthy())
    click(screen.getByRole('button', { name: 'agent metrics' }))
    await waitFor(() => expect(screen.getByRole('button', { name: 'agent statuses' })).toBeTruthy())
    click(screen.getByRole('button', { name: 'agent statuses' }))
    expect(summary(keyA)).toEqual(['q:agent statuses', 'q:agent metrics', 'q:agent status'])
  })

  it('moves a repeated search to the top instead of adding it again', async () => {
    setup()
    await searchFor('first search')
    await searchFor('second search')
    await searchFor('First   SEARCH')
    expect(summary(keyA)).toEqual(['q:First SEARCH', 'q:second search'])
  })

  it('remembers Refresh, which runs the search again, but not a filter-only re-run', async () => {
    // Both backends are written, so one save is two calls; only changes in the count matter.
    const write = [vi.spyOn(localStorage, 'setItem'), vi.spyOn(sessionStorage, 'setItem')]
    const historyWrites = () => write.flatMap((spy) => spy.mock.calls).filter(([key]) => key === keyA).length
    setup()
    await searchFor('agent status')
    expect(historyWrites()).toBeGreaterThan(0)
    const afterSearch = historyWrites()
    // A filter change re-runs the web list in place (no sidecars): the same search, not a new one.
    click(screen.getByRole('button', { name: 'Eksakt treff' }))
    expect(screen.getByRole('button', { name: 'Eksakt treff' }).getAttribute('aria-pressed')).toBe('true')
    await new Promise((resolve) => setTimeout(resolve, 50))
    expect(historyWrites()).toBe(afterSearch)
    click(screen.getByRole('button', { name: 'Refresh' }))
    expect(historyWrites()).toBeGreaterThan(afterSearch)
    expect(summary(keyA)).toEqual(['q:agent status'])
  })

  it('does not remember a search that is too short to run', async () => {
    setup()
    await searchFor('agent status')
    type('')
    click(screen.getByRole('button', { name: 'Refresh' }))
    expect(summary(keyA)).toEqual(['q:agent status'])
  })

  it('does not remember a search restored from the saved session', async () => {
    localStorage.setItem(searchSessionKey, JSON.stringify({
      activeResultTab: 'Info',
      expanded: true,
      filters: { exactMatch: false, excludeDomains: [], includeDomains: [], timeRange: null, topic: null },
      previewResults: [],
      query: 'saved query',
      savedAt: Date.now(),
      verevonResults: [],
      webAnswer: null,
      webCitations: [],
      webResults: payload.results!.map((result) => ({ ...result })),
    }))
    setup()
    await waitFor(() => expect(screen.getByRole('button', { name: 'Refresh' })).toBeTruthy())
    expect(query()).toBe('saved query')
    expect(localStorage.getItem(keyA)).toBeNull()
  })
})

describe('pages opened from the results', () => {
  it('remembers the page behind an opened result, with its title', async () => {
    setup()
    await searchFor()
    click(cardButton())
    expect(openWindow).toHaveBeenCalledWith('https://example.com/status', '_blank', 'noopener,noreferrer')
    expect(summary(keyA)).toEqual(['p:Agent status overview|https://example.com/status', 'q:agent status'])
  })

  it('remembers the New tab link on a click and on a middle-click, and not on a right-click', async () => {
    setup()
    await searchFor()
    auxClick(cardLink(), 2)
    expect(summary(keyA)).toEqual(['q:agent status'])
    auxClick(cardLink(), 1)
    expect(summary(keyA)).toEqual(['p:Agent status overview|https://example.com/status', 'q:agent status'])
    auxClick(screen.getByRole('link', { name: 'Open Second result in a new tab' }), 1)
    click(cardLink())
    expect(summary(keyA)).toEqual(['p:Agent status overview|https://example.com/status', 'p:Second result|https://docs.example.org/second', 'q:agent status'])
  })

  it('remembers a source opened from the sources rail', async () => {
    const view = setup()
    await searchFor()
    const source = view.container.querySelector<HTMLAnchorElement>('a.dashboard-xrail__source')!
    expect(source.getAttribute('href')).toBe('https://example.com/status')
    click(source)
    expect(summary(keyA)[0]).toBe('p:Agent status overview|https://example.com/status')
  })

  it('remembers a page opened from the Find similar list', async () => {
    api.findSimilar.mockResolvedValue({ results: [{ title: 'A neighbour', url: 'https://neighbour.example.net/page', snippet: 'Close by', hostname: 'neighbour.example.net' }] })
    const view = setup()
    await searchFor()
    click(view.container.querySelector('.dashboard-xresult__similar')!)
    await waitFor(() => expect(screen.getByText('A neighbour')).toBeTruthy())
    click(screen.getByText('A neighbour').closest('button')!)
    expect(openWindow).toHaveBeenCalledWith('https://neighbour.example.net/page', '_blank', 'noopener,noreferrer')
    expect(summary(keyA)[0]).toBe('p:A neighbour|https://neighbour.example.net/page')
  })

  it('remembers pages opened from the Images, Videos, Map and Shopping views', async () => {
    api.loadSearchImages.mockResolvedValue({ images: [{ imageUrl: 'https://img.example.org/a.png', thumbnailUrl: 'https://img.example.org/a-t.png', title: 'A diagram', url: 'https://images.example.org/page' }] })
    api.loadSearchVideos.mockResolvedValue({ videos: [{ author: null, embedUrl: 'https://videos.example.org/embed/1', length: null, thumbnailUrl: null, title: 'A demo', url: 'https://videos.example.org/watch/1' }] })
    const view = setup()
    await searchFor()

    click(screen.getByRole('button', { name: 'Images' }))
    await waitFor(() => expect(view.container.querySelector('a.dashboard-ximages__item')).not.toBeNull())
    click(view.container.querySelector('a.dashboard-ximages__item')!)
    expect(summary(keyA)[0]).toBe('p:A diagram|https://images.example.org/page')

    click(screen.getByRole('button', { name: 'Videos' }))
    await waitFor(() => expect(view.container.querySelector('a.dashboard-xvideo')).not.toBeNull())
    click(view.container.querySelector('a.dashboard-xvideo')!)
    expect(summary(keyA)[0]).toBe('p:A demo|https://videos.example.org/watch/1')

    click(screen.getByRole('button', { name: 'Map' }))
    click(view.container.querySelector('a.dashboard-xmap__item')!)
    expect(summary(keyA)[0]).toBe('p:Agent status overview|https://example.com/status')

    click(screen.getByRole('button', { name: 'Shopping' }))
    click(view.container.querySelectorAll('a.dashboard-xshop__item')[1]!)
    expect(summary(keyA)[0]).toBe('p:Second result|https://docs.example.org/second')
  })

  it('never stores an address that is not http or https, and strips credentials', async () => {
    api.runWebSearch.mockResolvedValue({ results: [
      { hostname: 'evil.example', title: 'Script link', url: 'javascript:alert(1)' },
      { hostname: 'example.com', title: 'Private page', url: 'https://alice:secret@example.com/inbox' },
    ] })
    setup()
    type('agent status')
    fireEvent.submit(searchBox().closest('form')!); flush()
    await waitFor(() => expect(screen.getByRole('button', { name: 'Open Private page in Verevon' })).toBeTruthy())
    click(screen.getByRole('button', { name: 'Open Script link in Verevon' }))
    click(screen.getByRole('button', { name: 'Open Private page in Verevon' }))
    expect(summary(keyA)).toEqual(['p:Private page|https://example.com/inbox', 'q:agent status'])
    expect(localStorage.getItem(keyA)).not.toContain('secret')
  })
})

describe('when nothing may be remembered', () => {
  it.each([[false], [undefined]])('remembers no search and no page while rememberSearches is %s', async (remember) => {
    setup({ remember })
    await searchFor()
    click(cardButton())
    auxClick(cardLink(), 1)
    expect(openWindow).toHaveBeenCalled()
    expect(localStorage.getItem(keyA)).toBeNull()
    expect(sessionStorage.getItem(keyA)).toBeNull()
  })

  it('follows the latest report: a search while it is off leaves no trace, the next one after it is on does', async () => {
    const view = setup({ remember: true })
    await searchFor('kept search')
    view.setRemember(false); flush()
    await searchFor('private search')
    click(cardButton())
    expect(summary(keyA)).toEqual(['q:kept search'])
    view.setRemember(true); flush()
    await searchFor('later search')
    expect(summary(keyA)).toEqual(['q:later search', 'q:kept search'])
  })

  it('remembers nothing until the user and the workspace are known', async () => {
    for (const [user, org] of [[null, 'org-a'], ['user-a', null], [null, null]] as const) {
      signIn(user, org)
      const view = setup()
      await searchFor()
      click(cardButton())
      expect(openWindow).toHaveBeenCalled()
      view.unmount()
      cleanup()
      resetSearchSession()
    }
    expect(localStorage.getItem(keyA)).toBeNull()
    expect(Object.keys(localStorage).filter((key) => key.startsWith('verevon.home-web-history'))).toEqual([])
  })

  it('writes to the account that is current when the search or the click happens', async () => {
    setup()
    await searchFor('as user a')
    signIn('user-b', 'org-a')
    click(cardButton())
    await searchFor('as user b')
    expect(summary(keyA)).toEqual(['q:as user a'])
    expect(summary(keyB)).toEqual(['q:as user b', 'p:Agent status overview|https://example.com/status'])
  })
})

describe('running a search from outside the panel', () => {
  it('runs it through the panel\'s own submit flow: expanded results, a search request and a history entry', async () => {
    const view = setup()
    expect(runSearch('agent status')).toBe(true)
    flush()
    await waitFor(() => expect(cardButton()).toBeTruthy())
    expect(view.expanded()).toBe(true)
    expect(query()).toBe('agent status')
    expect(api.runWebSearch).toHaveBeenCalled()
    expect(api.runWebSearch.mock.calls.some(([input]) => input.query === 'agent status')).toBe(true)
    expect(screen.getByRole('button', { name: 'Info' }).getAttribute('aria-pressed')).toBe('true')
    expect(summary(keyA)).toEqual(['q:agent status'])
  })

  it('moves the searched text to the top of the history', async () => {
    setup()
    await searchFor('older search')
    await searchFor('newer search')
    click(screen.getByRole('button', { name: 'Compact' }))
    expect(runSearch('older search')).toBe(true)
    flush()
    await waitFor(() => expect(cardButton()).toBeTruthy())
    expect(summary(keyA)).toEqual(['q:older search', 'q:newer search'])
  })

  it('still runs it, without remembering it, while nothing may be remembered', async () => {
    const view = setup({ remember: false })
    expect(runSearch('agent status')).toBe(true)
    flush()
    await waitFor(() => expect(cardButton()).toBeTruthy())
    expect(view.expanded()).toBe(true)
    expect(localStorage.getItem(keyA)).toBeNull()
  })

  it('only fills the box when the text is too short to search', () => {
    const view = setup()
    expect(runSearch('ab')).toBe(true)
    flush()
    expect(query()).toBe('ab')
    expect(view.expanded()).toBe(false)
    expect(api.runWebSearch).not.toHaveBeenCalled()
  })

  it('has no way in once the panel is gone, and leaves nothing pending for a later one', async () => {
    const first = setup()
    first.unmount()
    cleanup()
    expect(runSearch('agent status')).toBe(false)
    flush()
    expect(query()).toBe('agent status')
    api.runWebSearch.mockClear()
    const second = setup()
    flush()
    expect(second.expanded()).toBe(false)
    expect(api.streamSearchAnswer).not.toHaveBeenCalled()
    expect(localStorage.getItem(keyA)).toBeNull()
  })
})
