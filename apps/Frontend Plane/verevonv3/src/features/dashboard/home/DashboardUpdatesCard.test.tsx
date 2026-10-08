// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { createStore, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }) }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))
vi.mock('@/shared/api/home-updates-client', () => ({ loadAquatiqUpdates: vi.fn() }))
vi.mock('@/shared/api/information-client', () => ({
  loadDashboardInformationSnapshot: vi.fn(), loadNews: vi.fn(), loadTraffic: vi.fn(),
  newsCategoryOptions: [{ value: 'all', label: 'Alle' }, { value: 'technology', label: 'Technology' }],
  formatDerivedTrafficMetadata: () => 'Oslo', formatTrafficObservation: () => 'Unavailable',
}))
import { loadAquatiqUpdates } from '@/shared/api/home-updates-client'
import { loadDashboardInformationSnapshot, loadNews, loadTraffic, type InformationNewsPayload } from '@/shared/api/information-client'
import { DashboardUpdatesCard } from './DashboardUpdatesCard'
import { homeLayoutStorageKey } from './home-layout-state'
const [identity, setIdentity] = createStore({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } })
const news: InformationNewsPayload = { articles: [{ id: 'a', title: 'Morning news', source: 'NRK', link: 'https://nrk.no/story', description: 'A longer story', categories: [], publishDate: '2026-09-28' }], totalCount: 1, hasMore: false, lastUpdated: '2026-09-28' }
beforeEach(() => {
  vi.clearAllMocks()
  // The feed is remembered per account in the browser; each test starts fresh.
  localStorage.clear(); sessionStorage.clear()
  setIdentity((state) => { state.user.id = 'user-a'; state.activeOrg.id = 'org-a' }); flush()
  vi.mocked(loadDashboardInformationSnapshot).mockResolvedValue({ news, newsError: null, weather: null, weatherError: null, traffic: null, trafficError: null, usingLiveLocation: false })
  vi.mocked(loadNews).mockResolvedValue(news)
  vi.mocked(loadTraffic).mockResolvedValue({ data: { success: true, data: [], timestamp: '2026-09-28' }, usingLiveLocation: false })
  vi.mocked(loadAquatiqUpdates).mockResolvedValue({ pages: [], hasMore: false })
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', '') }
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open') }
})
afterEach(() => vi.restoreAllMocks())
describe('combined dashboard updates', () => {
  it('switches all three feeds without changing route or starting a chat, and shares selection with the modal', async () => {
    const onPrompt = vi.fn()
    render(() => <DashboardUpdatesCard onPrompt={onPrompt} />)
    await screen.findByText('Morning news')
    expect(loadAquatiqUpdates).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Traffic' })); flush()
    await screen.findByText('No traffic observations available.')
    fireEvent.click(screen.getByRole('button', { name: 'Open updates' })); flush()
    const modal = screen.getByRole('dialog')
    expect(within(modal).getByRole('button', { name: 'Traffic' }).getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(within(modal).getByRole('button', { name: 'Aquatiq' })); flush()
    await waitFor(() => expect(within(modal).getByText('No SharePoint pages here yet')).toBeTruthy())
    expect(loadAquatiqUpdates).toHaveBeenCalledWith('org-a', expect.any(AbortSignal))
    fireEvent.click(within(modal).getByRole('button', { name: 'Close updates' })); flush()
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(screen.getByRole('button', { name: 'Aquatiq' }).getAttribute('aria-pressed')).toBe('true')
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Open updates' }))
    expect(onPrompt).not.toHaveBeenCalled()
  })
  it('ignores a stale feed response after changing feed and distinguishes errors from empty pages', async () => {
    let resolveTraffic: (value: Awaited<ReturnType<typeof loadTraffic>>) => void = () => undefined
    vi.mocked(loadTraffic).mockImplementationOnce(() => new Promise((resolve) => { resolveTraffic = resolve }))
    vi.mocked(loadAquatiqUpdates).mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce({ pages: [], hasMore: false })
    render(() => <DashboardUpdatesCard onPrompt={vi.fn()} />)
    fireEvent.click(screen.getByRole('button', { name: 'Traffic' })); flush()
    fireEvent.click(screen.getByRole('button', { name: 'Aquatiq' })); flush()
    await screen.findByText('Couldn’t load updates')
    resolveTraffic({ data: { success: true, data: [], timestamp: '' }, usingLiveLocation: false }); await Promise.resolve(); flush()
    expect(screen.queryByText('No traffic observations available.')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Try again' })); flush()
    await screen.findByText('No SharePoint pages here yet')
  })
  it('discards old organization pages and retries with the current scope', async () => {
    let resolveOld: (value: Awaited<ReturnType<typeof loadAquatiqUpdates>>) => void = () => undefined
    vi.mocked(loadAquatiqUpdates).mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve }))
      .mockResolvedValueOnce({ pages: [{ id: 'new', title: 'Current company page', url: 'https://aquatiq.sharepoint.com/new', site: 'https://aquatiq.sharepoint.com' }], hasMore: false })
    // The feed is remembered per workspace, so org-b has to be remembered on Aquatiq
    // too for the switch to reload the same feed under the new scope.
    localStorage.setItem(homeLayoutStorageKey('user-a', 'org-b')!, JSON.stringify({ feed: 'aquatiq' }))
    render(() => <DashboardUpdatesCard onPrompt={vi.fn()} />)
    fireEvent.click(screen.getByRole('button', { name: 'Aquatiq' })); flush()
    setIdentity((state) => { state.activeOrg.id = 'org-b' }); flush()
    await screen.findByText('Current company page')
    resolveOld({ pages: [{ id: 'old', title: 'Old company page', url: 'https://aquatiq.sharepoint.com/old', site: 'https://aquatiq.sharepoint.com' }], hasMore: false }); await Promise.resolve(); flush()
    expect(screen.queryByText('Old company page')).toBeNull()
    expect(loadAquatiqUpdates).toHaveBeenLastCalledWith('org-b', expect.any(AbortSignal))
  })
  it('filters SharePoint sites in the modal and applies that selection to the compact card', async () => {
    vi.mocked(loadAquatiqUpdates).mockResolvedValue({ pages: [
      { id: 'one', title: 'Company announcement', url: 'https://aquatiq.sharepoint.com/sites/company/post', site: 'https://aquatiq.sharepoint.com/sites/company' },
      { id: 'two', title: 'Team announcement', url: 'https://aquatiq.sharepoint.com/sites/team/post', site: 'https://aquatiq.sharepoint.com/sites/team' },
    ], hasMore: false })
    render(() => <DashboardUpdatesCard onPrompt={vi.fn()} />)
    fireEvent.click(screen.getByRole('button', { name: 'Aquatiq' })); flush()
    await screen.findByText('Company announcement')
    fireEvent.click(screen.getByRole('button', { name: 'Open updates' })); flush()
    const modal = screen.getByRole('dialog')
    fireEvent.change(within(modal).getByRole('combobox', { name: 'SharePoint site' }), { target: { value: 'https://aquatiq.sharepoint.com/sites/team' } }); flush()
    expect(within(modal).queryByText('Company announcement')).toBeNull()
    expect(within(modal).getByRole('link', { name: /Team announcement/ }).getAttribute('href')).toBe('https://aquatiq.sharepoint.com/sites/team/post')
    fireEvent.click(within(modal).getByRole('button', { name: 'Close updates' })); flush()
    expect(screen.queryByText('Company announcement')).toBeNull()
    expect(screen.getByText('Team announcement')).toBeTruthy()
  })
})

describe('dashboard updates are remembered per browser and account', () => {
  const layoutKey = (user: string, org = 'org-a') => homeLayoutStorageKey(user, org)!
  const remembered = (key: string) => JSON.parse(localStorage.getItem(key) ?? 'null')
  const pressedFeed = () => (['News', 'Traffic', 'Aquatiq'] as const).filter((name) => screen.getByRole('button', { name }).getAttribute('aria-pressed') === 'true')
  const openTopic = () => {
    fireEvent.click(screen.getByRole('button', { name: 'Open updates' })); flush()
    return within(screen.getByRole('dialog')).getByRole('combobox', { name: 'Filter news' }) as HTMLSelectElement
  }

  it('remembers the feed when the card remounts and loads it straight away', async () => {
    const first = render(() => <DashboardUpdatesCard onPrompt={vi.fn()} />)
    expect(pressedFeed()).toEqual(['News'])
    expect(localStorage.getItem(layoutKey('user-a'))).toBeNull()
    await screen.findByText('Morning news')
    fireEvent.click(screen.getByRole('button', { name: 'Traffic' })); flush()
    await screen.findByText('No traffic observations available.')
    expect(remembered(layoutKey('user-a'))).toMatchObject({ feed: 'traffic' })
    first.unmount()

    vi.mocked(loadDashboardInformationSnapshot).mockClear()
    render(() => <DashboardUpdatesCard onPrompt={vi.fn()} />)
    // Restored on the first render, and the default feed is never fetched first.
    expect(pressedFeed()).toEqual(['Traffic'])
    await screen.findByText('No traffic observations available.')
    expect(loadDashboardInformationSnapshot).not.toHaveBeenCalled()
    expect(screen.queryByText('Morning news')).toBeNull()
  })

  it('remembers the news topic and uses it for the first fetch', async () => {
    const first = render(() => <DashboardUpdatesCard onPrompt={vi.fn()} />)
    await screen.findByText('Morning news')
    fireEvent.change(openTopic(), { target: { value: 'technology' } }); flush()
    await waitFor(() => expect(loadNews).toHaveBeenLastCalledWith('technology'))
    expect(remembered(layoutKey('user-a'))).toMatchObject({ feed: 'news', category: 'technology' })
    first.unmount()

    vi.mocked(loadDashboardInformationSnapshot).mockClear()
    vi.mocked(loadNews).mockClear()
    render(() => <DashboardUpdatesCard onPrompt={vi.fn()} />)
    await waitFor(() => expect(loadNews).toHaveBeenCalledWith('technology'))
    expect(loadNews).not.toHaveBeenCalledWith('all')
    expect(loadDashboardInformationSnapshot).not.toHaveBeenCalled()
    expect(openTopic().value).toBe('technology')
  })

  it('writes nothing for the refresh button or the open dialog', async () => {
    const first = render(() => <DashboardUpdatesCard onPrompt={vi.fn()} />)
    await screen.findByText('Morning news')
    fireEvent.click(screen.getByRole('button', { name: 'Refresh updates' })); flush()
    fireEvent.click(screen.getByRole('button', { name: 'Open updates' })); flush()
    expect(Object.keys(remembered(layoutKey('user-a')) ?? {})).toEqual([])
    first.unmount()
  })

  it('keeps each account apart and never writes under the previous one', async () => {
    render(() => <DashboardUpdatesCard onPrompt={vi.fn()} />)
    fireEvent.click(screen.getByRole('button', { name: 'Traffic' })); flush()
    setIdentity((state) => { state.user.id = 'user-b' }); flush()
    expect(pressedFeed()).toEqual(['News'])
    await screen.findByText('Morning news')
    expect(localStorage.getItem(layoutKey('user-b'))).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Aquatiq' })); flush()
    expect(remembered(layoutKey('user-b'))).toMatchObject({ feed: 'aquatiq' })
    expect(remembered(layoutKey('user-a'))).toMatchObject({ feed: 'traffic' })
    setIdentity((state) => { state.user.id = 'user-a' }); flush()
    expect(pressedFeed()).toEqual(['Traffic'])
    await screen.findByText('No traffic observations available.')
  })

  it('falls back to the defaults for an invalid remembered feed or topic', async () => {
    localStorage.setItem(layoutKey('user-a'), JSON.stringify({ feed: 'radio', category: 'evil&limit=9999' }))
    const first = render(() => <DashboardUpdatesCard onPrompt={vi.fn()} />)
    expect(pressedFeed()).toEqual(['News'])
    await screen.findByText('Morning news')
    expect(loadNews).not.toHaveBeenCalled()
    first.unmount()

    localStorage.setItem(layoutKey('user-a'), JSON.stringify({ feed: 'traffic', category: 'evil' }))
    const second = render(() => <DashboardUpdatesCard onPrompt={vi.fn()} />)
    expect(pressedFeed()).toEqual(['Traffic'])
    await screen.findByText('No traffic observations available.')
    second.unmount()

    for (const raw of ['{broken', '[]', '"aquatiq"']) {
      localStorage.setItem(layoutKey('user-a'), raw)
      const view = render(() => <DashboardUpdatesCard onPrompt={vi.fn()} />)
      expect(pressedFeed()).toEqual(['News'])
      view.unmount()
    }
    expect(loadNews).not.toHaveBeenCalled()
  })

  it('does not read or write the feed until the workspace is known', async () => {
    setIdentity((state) => { state.activeOrg.id = '' }); flush()
    const write = vi.spyOn(Storage.prototype, 'setItem')
    render(() => <DashboardUpdatesCard onPrompt={vi.fn()} />)
    fireEvent.click(screen.getByRole('button', { name: 'Traffic' })); flush()
    expect(pressedFeed()).toEqual(['Traffic'])
    await screen.findByText('No traffic observations available.')
    expect(write).not.toHaveBeenCalled()
  })
})
