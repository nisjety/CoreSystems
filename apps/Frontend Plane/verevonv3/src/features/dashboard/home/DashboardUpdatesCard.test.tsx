// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { createStore, flush } from 'solid-js'
import { beforeEach, describe, expect, it, vi } from 'vitest'
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
const [identity, setIdentity] = createStore({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } })
const news: InformationNewsPayload = { articles: [{ id: 'a', title: 'Morning news', source: 'NRK', link: 'https://nrk.no/story', description: 'A longer story', categories: [], publishDate: '2026-09-28' }], totalCount: 1, hasMore: false, lastUpdated: '2026-09-28' }
beforeEach(() => {
  vi.clearAllMocks()
  setIdentity((state) => { state.activeOrg.id = 'org-a' }); flush()
  vi.mocked(loadDashboardInformationSnapshot).mockResolvedValue({ news, newsError: null, weather: null, weatherError: null, traffic: null, trafficError: null, usingLiveLocation: false })
  vi.mocked(loadNews).mockResolvedValue(news)
  vi.mocked(loadTraffic).mockResolvedValue({ data: { success: true, data: [], timestamp: '2026-09-28' }, usingLiveLocation: false })
  vi.mocked(loadAquatiqUpdates).mockResolvedValue({ pages: [], hasMore: false })
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', '') }
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open') }
})
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
