// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/solid-query'
import { createRouter, memoryHistory, useLocation } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createStore, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { WebSearchPayload } from '@/shared/api/search-client'

const api = vi.hoisted(() => ({
  findSimilar: vi.fn(),
  listChatThreads: vi.fn(),
  loadQuerySuggestions: vi.fn(),
  loadSearchImages: vi.fn(),
  loadSearchSuggestions: vi.fn(),
  loadSearchVideos: vi.fn(),
  runWebSearch: vi.fn(),
  searchNavbar: vi.fn(),
  streamSearchAnswer: vi.fn(),
}))
// The composer mock reports its zero-retention state the way the real one does.
const composer = vi.hoisted(() => ({ report: undefined as undefined | ((value: boolean | undefined) => void), reports: true }))

vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }) }))
vi.mock('@/shared/session/session-store', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/session/session-store')>()),
  getSession: () => identity,
}))
vi.mock('@/features/chat/lib/chat-appearance-preview', () => ({ chatAppearanceActive: () => true }))
vi.mock('@/features/chat/components/ChatPage', () => ({ default: () => null }))
vi.mock('@/shared/api/chat-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/chat-client')>()),
  listChatThreads: api.listChatThreads,
}))
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
vi.mock('./DashboardComposer', async () => {
  const { createEffect } = await import('solid-js')
  return { DashboardComposer: (props: { message: string; onMessageChange: (value: string) => void; onZeroRetentionChange?: (value: boolean | undefined) => void }) => {
    // Like the real composer: a statement, once on mount.
    createEffect(() => undefined, () => {
      const report = props.onZeroRetentionChange
      composer.report = report
      if (composer.reports) report?.(false)
    })
    return <input id="verevon-dashboard-input" aria-label="Draft" value={props.message} onInput={(event) => props.onMessageChange(event.currentTarget.value)} />
  } }
})
vi.mock('./KnowledgeComposer', () => ({ KnowledgeComposer: () => <p>Extract tool</p> }))
vi.mock('./DashboardCards', () => ({
  DashboardCardsRail: () => null,
  DashboardImageCard: () => <section data-information-card="weather">Weather card</section>,
}))
vi.mock('./DashboardUpdatesCard', () => ({ DashboardUpdatesCard: () => null }))
vi.mock('./DashboardSectionOverview', () => ({ DashboardSectionOverview: () => null }))
import DashboardHome from './DashboardHome'
import { __resetRetentionForTests, applyServerRetention } from '@/features/chat/lib/chat-retention'
import { homeDraftStorageKey, readHomeDraft, writeHomeDraft } from '@/shared/session/home-draft-storage'
import { searchSnapshotStorageKey } from '@/shared/session/home-local-content'
import { HOME_DRAFT_DEBOUNCE_MS } from './home-draft'
import { query, resetSearchSession } from './dashboard-search-session'
import { addWebHistory, webHistoryStorageKey } from './web-history'

type Identity = { user: { id: string } | null; activeOrg: { id: string } | null }
const [identity, setIdentity] = createStore<Identity>({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } })
const signIn = (user: string | null, org: string | null) => {
  setIdentity((state) => { state.user = user ? { id: user } : null; state.activeOrg = org ? { id: org } : null })
  flush()
}
const keyA = webHistoryStorageKey('user-a', 'org-a')!
const keyB = webHistoryStorageKey('user-b', 'org-a')!
const payload: WebSearchPayload = {
  results: [{ hostname: 'example.com', snippet: 'All systems normal', title: 'Agent status overview', url: 'https://example.com/status' }],
}
const at = (minute: number) => new Date(Date.UTC(2026, 8, 29, 10, minute))
const seedSearch = (text: string, minute: number, key = keyA) => addWebHistory(key, { kind: 'query', text }, at(minute))
const seedPage = (url: string, title: string, minute: number, key = keyA) => addWebHistory(key, { kind: 'page', text: title, url }, at(minute))
const stored = (key: string) => (JSON.parse(localStorage.getItem(key) ?? 'null') ?? []) as Array<{ kind: string; text: string }>
const storedTexts = (key: string) => stored(key).map((entry) => entry.text)
let queryClient: QueryClient

beforeEach(() => {
  localStorage.clear(); sessionStorage.clear()
  resetSearchSession()
  composer.report = undefined
  composer.reports = true
  signIn('user-a', 'org-a')
  api.runWebSearch.mockReset().mockResolvedValue(payload)
  api.listChatThreads.mockReset().mockResolvedValue([])
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

function setup(path = '/dashboard?tab=web') {
  const history = memoryHistory(path)
  const TestRouter = createRouter({ history, explicitLinks: true, routes: [{ path: '/*all', component: () => {
    const location = useLocation()
    return <><DashboardHome /><output data-testid="location">{location.pathname}{location.search}</output></>
  } }] })
  return render(() => <QueryClientProvider client={queryClient}><TestRouter>{(props) => props.children}</TestRouter></QueryClientProvider>)
}
type TabName = 'Chat' | 'Web' | 'Extract'
const tab = (name: TabName) => screen.getByRole('button', { name })
const pressed = () => (['Chat', 'Web', 'Extract'] as const).filter((name) => tab(name).getAttribute('aria-pressed') === 'true')
const switchTo = async (name: TabName) => {
  fireEvent.click(tab(name), { detail: 1 }); flush()
  await waitFor(() => expect(pressed()).toEqual([name]))
}
const doubleClick = (name: TabName) => {
  fireEvent.click(tab(name), { detail: 1 }); flush()
  fireEvent.click(tab(name), { detail: 2 }); flush()
  fireEvent.dblClick(tab(name), { detail: 2 }); flush()
}
// The compact box, or the "new search" box once the results are open.
const searchBox = () => (screen.queryByRole('combobox', { name: 'Search on the web or ask' }) ?? screen.getByPlaceholderText('Type a new search ...')) as HTMLInputElement
const emptySearchBox = () => { fireEvent.input(searchBox(), { target: { value: '' } }); flush() }
const submitSearch = async (text: string) => {
  fireEvent.input(searchBox(), { target: { value: text } }); flush()
  fireEvent.submit(searchBox().closest('form')!); flush()
  await waitFor(() => expect(screen.getByRole('button', { name: 'Open Agent status overview in Verevon' })).toBeTruthy())
}
const cardHeading = () => screen.queryByRole('heading', { level: 2, name: 'Pick up where you left off' })
const cardRows = (container: HTMLElement) => [...container.querySelectorAll('.home-recent__row strong')].map((row) => row.textContent)

describe('The Nett tab card', () => {
  it('lists the remembered searches and pages on Nett in place of the recent conversations', async () => {
    seedSearch('quarterly report', 1)
    seedPage('https://example.com/status', 'Agent status overview', 2)
    api.listChatThreads.mockResolvedValue([{ threadId: 't1', title: 'Quarterly plan', preview: '', updatedAt: '2026-09-28T10:00:00Z', pinned: false }])
    const view = setup()
    expect(pressed()).toEqual(['Web'])
    expect(cardRows(view.container)).toEqual(['Agent status overview', 'quarterly report'])
    expect(view.container.querySelector('.home-recent .home-eyebrow')?.textContent).toBe('Web')
    expect(screen.getAllByRole('heading', { level: 2 }).map((heading) => heading.textContent)).toEqual(['Pick up where you left off', 'Ready for what’s next'])
    expect(screen.getByText('Weather card')).toBeTruthy()
    // Home asks the server for the retention verdict once, but its answer is not shown here.
    await waitFor(() => expect(api.listChatThreads).toHaveBeenCalledTimes(1))
    await Promise.resolve()
    expect(screen.queryByText('Quarterly plan')).toBeNull()
    expect(cardRows(view.container)).toEqual(['Agent status overview', 'quarterly report'])
  })

  it('is the recent conversations on Chat, and the Nett card is gone there', async () => {
    seedSearch('quarterly report', 1)
    api.listChatThreads.mockResolvedValue([{ threadId: 't1', title: 'Quarterly plan', preview: '', updatedAt: '2026-09-28T10:00:00Z', pinned: false }])
    const view = setup('/dashboard')
    await waitFor(() => expect(screen.getByText('Quarterly plan')).toBeTruthy())
    expect(pressed()).toEqual(['Chat'])
    expect(screen.queryByText('quarterly report')).toBeNull()
    expect(view.container.querySelector('#home-web-history-title')).toBeNull()

    await switchTo('Web')
    expect(cardRows(view.container)).toEqual(['quarterly report'])
    expect(screen.queryByText('Quarterly plan')).toBeNull()
    await switchTo('Chat')
    await waitFor(() => expect(screen.getByText('Quarterly plan')).toBeTruthy())
    expect(screen.queryByText('quarterly report')).toBeNull()
  })

  it('is not on Hent ut', async () => {
    seedSearch('quarterly report', 1)
    const view = setup('/dashboard?tab=extract')
    expect(pressed()).toEqual(['Extract'])
    expect(view.container.querySelector('#home-web-history-title')).toBeNull()
    expect(screen.queryByText('quarterly report')).toBeNull()
  })

  it('explains itself when nothing has been searched yet', () => {
    setup()
    expect(cardHeading()).toBeTruthy()
    expect(screen.getByText('No searches yet.')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Clear' })).toBeNull()
  })
})

describe('Recording from the Nett tab', () => {
  it('remembers a search that was run and shows it once the results are put away', async () => {
    const view = setup()
    await submitSearch('agent status')
    expect(storedTexts(keyA)).toEqual(['agent status'])
    // The results own the stage; putting the query away brings the overview and its card back.
    fireEvent.click(screen.getByRole('button', { name: 'Compact' })); flush()
    emptySearchBox()
    await waitFor(() => expect(cardRows(view.container)).toEqual(['agent status']))
  })

  it('shows the searches that were run before, and a click on one runs it again', async () => {
    seedSearch('older search', 1)
    seedSearch('agent status', 2)
    const view = setup()
    expect(cardRows(view.container)).toEqual(['agent status', 'older search'])
    fireEvent.click(view.container.querySelectorAll<HTMLElement>('.home-recent__row')[1]!); flush()

    await waitFor(() => expect(screen.getByRole('button', { name: 'Open Agent status overview in Verevon' })).toBeTruthy())
    expect(screen.getByPlaceholderText('Type a new search ...')).toHaveProperty('value', 'older search')
    expect(query()).toBe('older search')
    expect(api.runWebSearch.mock.calls.some(([input]) => input.query === 'older search')).toBe(true)
    expect(storedTexts(keyA)).toEqual(['older search', 'agent status'])
  })

  it('keeps the history when the active Nett tab is double-clicked to start over', async () => {
    seedSearch('older search', 1)
    const view = setup()
    await submitSearch('agent status')
    expect(storedTexts(keyA)).toEqual(['agent status', 'older search'])

    doubleClick('Web')

    expect(query()).toBe('')
    expect(searchBox().value).toBe('')
    expect(storedTexts(keyA)).toEqual(['agent status', 'older search'])
    await waitFor(() => expect(cardRows(view.container)).toEqual(['agent status', 'older search']))
  })

  it('remembers nothing while the composer says the next turn is zero-retention, and nothing before it has said', async () => {
    composer.reports = false
    setup()
    await Promise.resolve()
    // The composer has not reported yet: fail closed.
    await submitSearch('unknown state')
    expect(localStorage.getItem(keyA)).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Open Agent status overview in Verevon' })); flush()

    composer.report?.(true); flush()
    await submitSearch('temporary chat on')
    fireEvent.click(screen.getByRole('button', { name: 'Open Agent status overview in Verevon' })); flush()
    expect(localStorage.getItem(keyA)).toBeNull()

    composer.report?.(false); flush()
    await submitSearch('temporary chat off')
    expect(storedTexts(keyA)).toEqual(['temporary chat off'])

    composer.report?.(undefined); flush()
    await submitSearch('composer gone')
    expect(storedTexts(keyA)).toEqual(['temporary chat off'])
  })

  it('does not record a search that is only typed, or shown as a preview', async () => {
    setup()
    fireEvent.input(searchBox(), { target: { value: 'agent status' } }); flush()
    await waitFor(() => expect(screen.getByText('Agent status overview')).toBeTruthy())
    expect(localStorage.getItem(keyA)).toBeNull()
  })
})

describe('Keyboard focus around the Nett card', () => {
  it('leaves focus alone when the row was pressed with a mouse or a finger, so no keyboard is raised', async () => {
    seedSearch('older search', 1)
    const view = setup()
    const row = view.container.querySelector<HTMLElement>('.home-recent__row')!
    row.focus()

    fireEvent.click(row, { detail: 1 }); flush()

    await waitFor(() => expect(screen.getByPlaceholderText('Type a new search ...')).toBeTruthy())
    await Promise.resolve()
    expect(document.activeElement).not.toBe(screen.getByPlaceholderText('Type a new search ...'))
  })

  it("lands in the results page's search box when a remembered search is run again from its row with the keyboard", async () => {
    seedSearch('older search', 1)
    const view = setup()
    const row = view.container.querySelector<HTMLElement>('.home-recent__row')!
    row.focus()
    expect(document.activeElement).toBe(row)

    fireEvent.click(row); flush()

    await waitFor(() => expect(screen.getByPlaceholderText('Type a new search ...')).toBeTruthy())
    await Promise.resolve()
    expect(row.isConnected).toBe(false)
    expect(document.activeElement).toBe(screen.getByPlaceholderText('Type a new search ...'))
  })

  it('does not lose focus to the top of the page when the history is cleared', () => {
    seedSearch('older search', 1)
    setup()
    const clear = screen.getByRole('button', { name: 'Clear' })
    clear.focus()

    fireEvent.click(clear); flush()

    expect(document.activeElement).toBe(cardHeading())
  })
})

describe('A zero-data-retention workspace', () => {
  it('shows recent conversations on Nett, and remembers no search or page', async () => {
    applyServerRetention(true)
    api.listChatThreads.mockResolvedValue([{ threadId: 't1', title: 'Quarterly plan', preview: '', updatedAt: '2026-09-28T10:00:00Z', pinned: false }])
    const view = setup()
    await waitFor(() => expect(screen.getByText('Quarterly plan')).toBeTruthy())
    expect(view.container.querySelector('#home-web-history-title')).toBeNull()

    await submitSearch('agent status')
    fireEvent.click(screen.getByRole('button', { name: 'Open Agent status overview in Verevon' })); flush()

    expect(localStorage.getItem(keyA)).toBeNull()
    expect(sessionStorage.getItem(keyA)).toBeNull()
  })

  it('drops what the Nett tab remembered when the verdict arrives, and shows recent conversations at once', async () => {
    seedSearch('salary negotiation lawyer', 1)
    api.listChatThreads.mockResolvedValue([{ threadId: 't1', title: 'Quarterly plan', preview: '', updatedAt: '2026-09-28T10:00:00Z', pinned: false }])
    const view = setup()
    expect(cardRows(view.container)).toEqual(['salary negotiation lawyer'])

    applyServerRetention(true); flush()

    await waitFor(() => expect(screen.getByText('Quarterly plan')).toBeTruthy())
    expect(view.container.querySelector('#home-web-history-title')).toBeNull()
    expect(localStorage.getItem(keyA)).toBeNull()
  })
})

// The verdict rides on a threads listing, and on these tabs the recent-work card, which
// makes the only listing Home makes by itself, is not on screen: Home has to ask.
describe('Learning the workspace\'s retention verdict on a cold start', () => {
  const draftKey = homeDraftStorageKey('user-a', 'org-a')!
  const snapshotKey = searchSnapshotStorageKey('user-a', 'org-a')!
  const seedSnapshot = () => localStorage.setItem(snapshotKey, JSON.stringify({
    activeResultTab: 'Info',
    expanded: false,
    filters: { exactMatch: false, excludeDomains: [], includeDomains: [], timeRange: null, topic: null },
    previewResults: [],
    query: 'salary negotiation lawyer',
    savedAt: Date.now(),
    verevonResults: [],
    webAnswer: null,
    webCitations: [],
    webResults: [],
  }))
  // A listing that states a zero-retention workspace once the test lets it answer.
  const zeroRetentionListing = () => {
    const listing: { answer: () => void } = { answer: () => undefined }
    api.listChatThreads.mockImplementation(() => new Promise((resolve) => {
      listing.answer = () => { applyServerRetention(true); resolve([]) }
    }))
    return listing
  }
  const seedEverything = () => {
    seedSearch('salary negotiation lawyer', 1)
    seedSnapshot()
    writeHomeDraft(draftKey, 'Unsent words')
  }
  const expectNothingLeft = () => {
    expect(localStorage.getItem(keyA)).toBeNull()
    expect(sessionStorage.getItem(keyA)).toBeNull()
    expect(localStorage.getItem(snapshotKey)).toBeNull()
    expect(sessionStorage.getItem(snapshotKey)).toBeNull()
    expect(readHomeDraft(draftKey)).toBe('')
  }

  it('asks on Nett, and a zero-retention answer removes the draft, the saved search and the history', async () => {
    seedEverything()
    const listing = zeroRetentionListing()
    const view = setup('/dashboard?tab=web')
    await waitFor(() => expect(api.listChatThreads).toHaveBeenCalledTimes(1))
    // Nothing has said no yet, so the gate is still open.
    expect(storedTexts(keyA)).toEqual(['salary negotiation lawyer'])
    expect(readHomeDraft(draftKey)).toBe('Unsent words')

    api.listChatThreads.mockReset().mockResolvedValue([])
    listing.answer(); flush()

    expectNothingLeft()
    // The restored search goes with its snapshot, which brings the overview back, as recent work.
    await waitFor(() => expect(view.container.querySelector('#home-recent-title')).not.toBeNull())
    expect(view.container.querySelector('#home-web-history-title')).toBeNull()

    // What is typed or searched from here on stays off the device too.
    fireEvent.input(screen.getByLabelText('Draft'), { target: { value: 'More words' } }); flush()
    await submitSearch('agent status')
    fireEvent.click(screen.getByRole('button', { name: 'Open Agent status overview in Verevon' })); flush()
    await new Promise((resolve) => setTimeout(resolve, HOME_DRAFT_DEBOUNCE_MS + 60))
    expectNothingLeft()
  })

  it('asks on Hent ut too', async () => {
    seedEverything()
    const listing = zeroRetentionListing()
    setup('/dashboard?tab=extract')
    await waitFor(() => expect(api.listChatThreads).toHaveBeenCalledTimes(1))

    listing.answer(); flush()

    expectNothingLeft()
  })

  it('keeps everything when the workspace allows local copies', async () => {
    seedEverything()
    api.listChatThreads.mockResolvedValue([])
    setup('/dashboard?tab=web')
    await waitFor(() => expect(api.listChatThreads).toHaveBeenCalledTimes(1))
    await Promise.resolve()

    expect(storedTexts(keyA)).toEqual(['salary negotiation lawyer'])
    expect(readHomeDraft(draftKey)).toBe('Unsent words')
    expect(localStorage.getItem(snapshotKey)).not.toBeNull()
  })

  it('changes nothing when the answer does not come', async () => {
    seedEverything()
    api.listChatThreads.mockRejectedValue(new Error('offline'))
    setup('/dashboard?tab=web')
    await waitFor(() => expect(api.listChatThreads).toHaveBeenCalledTimes(1))
    await Promise.resolve()

    expect(storedTexts(keyA)).toEqual(['salary negotiation lawyer'])
    expect(readHomeDraft(draftKey)).toBe('Unsent words')
    expect(localStorage.getItem(snapshotKey)).not.toBeNull()
  })

  it('asks once for a person and workspace, not on each tab change, and again for the next account', async () => {
    setup('/dashboard?tab=web')
    await waitFor(() => expect(api.listChatThreads).toHaveBeenCalledTimes(1))

    await switchTo('Extract')
    await switchTo('Web')
    await Promise.resolve()
    expect(api.listChatThreads).toHaveBeenCalledTimes(1)

    signIn('user-b', 'org-a')
    await waitFor(() => expect(api.listChatThreads).toHaveBeenCalledTimes(2))
    signIn(null, null)
    await Promise.resolve()
    expect(api.listChatThreads).toHaveBeenCalledTimes(2)
  })

  it('does not ask on top of the recent-work card on the Chat overview, nor when Nett is opened from there', async () => {
    api.listChatThreads.mockResolvedValue([{ threadId: 't1', title: 'Quarterly plan', preview: '', updatedAt: '2026-09-28T10:00:00Z', pinned: false }])
    setup('/dashboard')
    await waitFor(() => expect(screen.getByText('Quarterly plan')).toBeTruthy())
    expect(api.listChatThreads).toHaveBeenCalledTimes(1)

    await switchTo('Web')
    await Promise.resolve()
    expect(api.listChatThreads).toHaveBeenCalledTimes(1)
  })
})

describe('Clearing and account changes', () => {
  it('Clear removes this account\'s history and leaves the search itself alone', async () => {
    seedSearch('quarterly report', 1); seedSearch('theirs', 2, keyB)
    const view = setup()
    fireEvent.input(searchBox(), { target: { value: 'still typing' } }); flush()
    await waitFor(() => expect(screen.getByText('Agent status overview')).toBeTruthy())
    // Back to an empty box to see the card again.
    emptySearchBox()
    await waitFor(() => expect(cardRows(view.container)).toEqual(['quarterly report']))
    fireEvent.click(screen.getByRole('button', { name: 'Clear' })); flush()
    expect(screen.getByText('No searches yet.')).toBeTruthy()
    expect(localStorage.getItem(keyA)).toBeNull()
    expect(storedTexts(keyB)).toEqual(['theirs'])
  })

  it('shows each account its own history when the person or workspace changes', () => {
    seedSearch('as user a', 1); seedSearch('as user b', 2, keyB)
    const view = setup()
    expect(cardRows(view.container)).toEqual(['as user a'])
    signIn('user-b', 'org-a')
    expect(cardRows(view.container)).toEqual(['as user b'])
    signIn('user-b', 'org-b')
    expect(cardRows(view.container)).toEqual([])
    signIn(null, null)
    expect(cardRows(view.container)).toEqual([])
    signIn('user-a', 'org-a')
    expect(cardRows(view.container)).toEqual(['as user a'])
  })

  it('records a later search under the account that is signed in by then', async () => {
    setup()
    await submitSearch('as user a')
    signIn('user-b', 'org-a')
    await submitSearch('as user b')
    expect(storedTexts(keyA)).toEqual(['as user a'])
    expect(storedTexts(keyB)).toEqual(['as user b'])
  })
})
