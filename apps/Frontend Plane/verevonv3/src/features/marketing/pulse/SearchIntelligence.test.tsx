// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { SearchIntelligence } from './SearchIntelligence.tsx'
import type { PulseQuarter } from '../../../shared/api/marketing/pulse-client.ts'

const query = { key: 'water hygiene', clicks: 2, impressions: 200, ctr: 0.01, position: 5.25 }
const data = (site = 'https://aquatiq.com/') => ({
  site, source: 'google_search_console', status: 'connected',
  period: { startDate: '2026-10-01', endDate: '2026-10-20' },
  queries: [query], pages: [{ ...query, key: 'https://aquatiq.com/services/' }],
  opportunities: [{ ...query, type: 'title_meta' }], rowLimit: 100, queriesAtLimit: false, pagesAtLimit: false,
})
const response = (value = data()) => new Response(JSON.stringify({ data: value }))
const websiteId = `web_${'1'.repeat(32)}`
const mount = (props: Partial<Parameters<typeof SearchIntelligence>[0]> = {}) => render(() => <I18nProvider><SearchIntelligence site="https://aquatiq.com/" quarter="Q4" websiteId={websiteId} revision={2} {...props} /></I18nProvider>)

describe('SearchIntelligence', () => {
  afterEach(() => { cleanup(); vi.unstubAllGlobals(); window.localStorage.clear() })

  it('refreshes the saved search projection after collection without starting provider work', async () => {
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(response())); vi.stubGlobal('fetch', fetcher)
    const [refresh, setRefresh] = createSignal(0)
    render(() => <I18nProvider><SearchIntelligence site="https://aquatiq.com/" quarter="Q4" websiteId={websiteId} revision={2} refreshKey={refresh()} /></I18nProvider>)
    await screen.findByRole('cell', { name: 'water hygiene' }); expect(fetcher).toHaveBeenCalledTimes(1)
    setRefresh(1); await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(2))
    expect(fetcher.mock.calls.every((call) => !call[1]?.method || call[1]?.method === 'GET')).toBe(true)
  })

  it('renders real query metrics, dates, ratio CTR and provider caveats', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response()))
    mount()
    await screen.findByRole('cell', { name: 'water hygiene' })
    expect(screen.getByRole('heading', { name: /Søkeinnsikt|Search intelligence/ })).toBeTruthy()
    expect(screen.getByRole('cell', { name: /1\s%/ })).toBeTruthy()
    expect(screen.getByRole('cell', { name: '5,3' })).toBeTruthy()
    expect(screen.getByTestId('search-period').textContent).toMatch(/1.*okt.*2026.*20.*okt.*2026/)
    expect(screen.getByText(/anonyme søk|anonymized queries/i)).toBeTruthy()
    expect(screen.getByRole('table')).toBeTruthy()
    expect(screen.getAllByRole('tab')).toHaveLength(3)
    expect(screen.queryByRole('link')).toBeNull()
  })

  it('switches queries, pages and rule-based opportunities without following URLs', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response()))
    mount()
    await screen.findByRole('cell', { name: 'water hygiene' })
    fireEvent.click(screen.getByRole('tab', { name: /Sider|Pages/ }))
    expect(screen.getByRole('cell', { name: 'https://aquatiq.com/services/' })).toBeTruthy()
    expect(screen.queryByRole('link')).toBeNull()
    fireEvent.click(screen.getByRole('tab', { name: /Muligheter|Opportunities/ }))
    expect(screen.getByRole('cell', { name: /tittel og metabeskrivelse|title and meta description/i })).toBeTruthy()
    expect(screen.getByText(/ikke lagrede oppgaver|not saved tasks/i)).toBeTruthy()
  })

  it('saves only a user-selected GSC opportunity and refreshes the saved work view', async () => {
    const task = { id: `task_${'2'.repeat(32)}`, websiteId, site: 'https://aquatiq.com/', revision: 1, status: 'todo', createdAt: '2026-10-21T10:00:00Z', updatedAt: '2026-10-21T10:00:00Z', evidence: { kind: 'gsc_opportunity', key: query.key, opportunityType: 'title_meta', period: { startDate: '2026-10-01', endDate: '2026-10-20' }, clicks: query.clicks, impressions: query.impressions, ctr: query.ctr, position: query.position, observedAt: '2026-10-21T10:00:00Z' } }
    const work = { websiteId, site: 'https://aquatiq.com/', tasks: [task], activity: [{ id: `evt_${'3'.repeat(32)}`, taskId: task.id, kind: 'created', status: 'todo', occurredAt: task.createdAt }], summary: { open: 1, inProgress: 0, done: 0, dismissed: 0, total: 1, completionPercent: 0 } }
    const onTaskSaved = vi.fn()
    const fetchMock = vi.fn().mockResolvedValueOnce(response()).mockResolvedValueOnce(new Response(JSON.stringify({ data: work }), { status: 201 }))
    vi.stubGlobal('fetch', fetchMock)
    mount({ onTaskSaved })
    fireEvent.click(await screen.findByRole('tab', { name: /Muligheter|Opportunities/ }))
    fireEvent.click(await screen.findByRole('button', { name: /Legg i oppgavelisten|Add to backlog/ }))
    await waitFor(() => expect(onTaskSaved).toHaveBeenCalledOnce())
    expect(fetchMock.mock.calls[1]?.[0]).toBe('/api/v1/visibility/work')
    expect(JSON.parse(String(fetchMock.mock.calls[1]?.[1]?.body))).toEqual({ websiteId, site: 'https://aquatiq.com/', websiteRevision: 2, quarter: 'Q4', query: query.key })
    expect(screen.getByRole('button', { name: /Lagret|Saved/ })).toBeTruthy()
  })

  it('paginates and filters locally, resetting the page when the filter changes', async () => {
    const queries = Array.from({ length: 23 }, (_, index) => ({ ...query, key: `query ${String(index + 1).padStart(2, '0')}` }))
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response({ ...data(), queries, opportunities: [] })))
    mount()
    await screen.findByRole('cell', { name: 'query 01' })
    expect(screen.getAllByRole('row')).toHaveLength(11)
    fireEvent.click(screen.getByRole('button', { name: /Neste|Next/ }))
    expect(screen.getByRole('cell', { name: 'query 11' })).toBeTruthy()
    expect(screen.queryByRole('cell', { name: 'query 01' })).toBeNull()
    fireEvent.input(screen.getByRole('searchbox'), { target: { value: '23' } })
    expect(screen.getByRole('cell', { name: 'query 23' })).toBeTruthy()
    expect(screen.getByRole('button', { name: /Neste|Next/ }).hasAttribute('disabled')).toBe(true)
    fireEvent.input(screen.getByRole('searchbox'), { target: { value: 'missing' } })
    expect(screen.getByText(/Ingen treff|No matching rows/)).toBeTruthy()
  })

  it('shows honest loading, empty, unavailable and retry states', async () => {
    const fetchMock = vi.fn().mockRejectedValue(new Error('offline'))
    vi.stubGlobal('fetch', fetchMock)
    mount()
    expect(screen.getByRole('status').textContent).toMatch(/Laster|Loading/)
    await screen.findByRole('alert')
    expect(screen.queryByRole('table')).toBeNull()
    fetchMock.mockResolvedValue(response({ ...data(), queries: [], pages: [], opportunities: [] }))
    fireEvent.click(screen.getByRole('button', { name: /Prøv igjen|Try again/ }))
    await screen.findByText(/Ingen søkeord.*perioden|No query rows.*period/i)
    expect(screen.queryByRole('cell')).toBeNull()
  })

  it('explains a property verification failure without fabricated zeros', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response({ ...data(), status: 'unavailable', reason: 'property_unverified', queries: [], pages: [], opportunities: [] } as ReturnType<typeof data>)))
    mount()
    await screen.findByText(/ikke verifisert|not verified/i)
    expect(screen.queryByRole('table')).toBeNull()
    expect(screen.queryByRole('cell', { name: '0' })).toBeNull()
  })

  it('clears old rows immediately on site changes and ignores aborted late responses', async () => {
    let completeOld: ((value: Response) => void) | undefined
    let completeNew: ((value: Response) => void) | undefined
    const fetchMock = vi.fn().mockResolvedValueOnce(response())
      .mockReturnValueOnce(new Promise<Response>((resolve) => { completeOld = resolve }))
      .mockReturnValueOnce(new Promise<Response>((resolve) => { completeNew = resolve }))
    vi.stubGlobal('fetch', fetchMock)
    const [site, setSite] = createSignal('https://aquatiq.com/')
    const [quarter, setQuarter] = createSignal<PulseQuarter>('Q4')
    render(() => <I18nProvider><SearchIntelligence site={site()} quarter={quarter()} /></I18nProvider>)
    await screen.findByRole('cell', { name: 'water hygiene' })
    setQuarter('Q3')
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2))
    expect(screen.queryByRole('cell')).toBeNull()
    expect(screen.getByTestId('search-period').textContent).not.toContain('2026')
    setSite('https://example.com/')
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(3))
    expect(fetchMock.mock.calls[1]?.[1]?.signal.aborted).toBe(true)
    completeOld?.(response({ ...data(), queries: [{ ...query, key: 'stale' }], opportunities: [] }))
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(screen.queryByRole('cell')).toBeNull()
    completeNew?.(response({ ...data('https://example.com/'), period: { startDate: '2026-07-01', endDate: '2026-09-30' }, queries: [{ ...query, key: 'current' }], opportunities: [] }))
    await screen.findByRole('cell', { name: 'current' })
    expect(screen.queryByText('stale')).toBeNull()
  })

  it('renders untrusted query and page names as text, never HTML or links', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response({ ...data(), queries: [{ ...query, key: '<img src=x onerror=alert(1)>' }], opportunities: [] })))
    const view = mount()
    await screen.findByRole('cell', { name: '<img src=x onerror=alert(1)>' })
    expect(view.container.querySelector('img')).toBeNull()
    expect(view.container.querySelector('a')).toBeNull()
  })

  it('supports accessible tab keyboard movement and English number/date formatting', async () => {
    window.localStorage.setItem('verevon.locale', 'en')
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response()))
    mount()
    await screen.findByRole('cell', { name: 'water hygiene' })
    expect(screen.getByRole('cell', { name: '1%' })).toBeTruthy()
    expect(screen.getByRole('cell', { name: '5.3' })).toBeTruthy()
    expect(screen.getByTestId('search-period').textContent).toContain('Oct')
    const queries = screen.getByRole('tab', { name: /Queries/ })
    fireEvent.keyDown(queries, { key: 'ArrowRight' })
    expect(screen.getByRole('tab', { name: /Pages/ }).getAttribute('aria-selected')).toBe('true')
    expect(document.activeElement).toBe(screen.getByRole('tab', { name: /Pages/ }))
    fireEvent.keyDown(document.activeElement!, { key: 'End' })
    expect(screen.getByRole('tab', { name: /Opportunities/ }).getAttribute('aria-selected')).toBe('true')
    fireEvent.keyDown(document.activeElement!, { key: 'Home' })
    expect(queries.getAttribute('aria-selected')).toBe('true')
    fireEvent.keyDown(queries, { key: 'ArrowLeft' })
    expect(screen.getByRole('tab', { name: /Opportunities/ }).getAttribute('aria-selected')).toBe('true')
    fireEvent.keyDown(document.activeElement!, { key: 'Escape' })
    expect(screen.getByRole('tab', { name: /Opportunities/ }).getAttribute('aria-selected')).toBe('true')
  })

  it.each(['not_configured', 'source_unavailable'])('explains unavailable source %s without affecting dashboard data', async (reason) => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response({ ...data(), status: 'unavailable', reason, queries: [], pages: [], opportunities: [] } as ReturnType<typeof data>)))
    mount()
    await screen.findByText(reason === 'not_configured' ? /ikke koblet|not connected/i : /midlertidig utilgjengelig|temporarily unavailable/i)
    expect(screen.queryByRole('table')).toBeNull()
  })

  it('shows empty pages and suggestions without inventing rows', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response({ ...data(), queries: [], pages: [], opportunities: [] })))
    mount()
    await screen.findByText(/Ingen søkeord|No query rows/)
    fireEvent.click(screen.getByRole('tab', { name: /Sider|Pages/ }))
    expect(screen.getByText(/Ingen sider|No page rows/)).toBeTruthy()
    fireEvent.click(screen.getByRole('tab', { name: /Muligheter|Opportunities/ }))
    expect(screen.getByText(/Ingen søkeord oppfyller|No queries meet/)).toBeTruthy()
    expect(screen.queryByRole('table')).toBeNull()
  })

  it('renders every supported suggestion category and warns about a limited sample', async () => {
    const queries = Array.from({ length: 100 }, (_, i) => ({ ...query, key: `query ${i}`, position: i === 0 ? 2 : i === 1 ? 8 : i === 2 ? 15 : 5, clicks: i === 1 ? 8 : 2, ctr: i === 1 ? 0.04 : 0.01 }))
    const opportunities = queries.slice(0, 3).map((row, i) => ({ ...row, type: ['snippet_review', 'title_test', 'content_expansion'][i]! }))
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response({ ...data(), queries, opportunities, queriesAtLimit: true })))
    mount()
    await screen.findByRole('cell', { name: 'query 0' })
    expect(screen.getByText(/Radgrensen er nådd|row limit was reached/i)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /Neste|Next/ }))
    fireEvent.click(screen.getByRole('button', { name: /Forrige|Previous/ }))
    expect(screen.getByRole('cell', { name: 'query 0' })).toBeTruthy()
    fireEvent.click(screen.getByRole('tab', { name: /Muligheter|Opportunities/ }))
    expect(screen.getByRole('cell', { name: /Vurder søkeutdraget|Review search snippet/ })).toBeTruthy()
    expect(screen.getByRole('cell', { name: /Test en tydeligere tittel|Test a clearer title/ })).toBeTruthy()
    expect(screen.getByRole('cell', { name: /Vurder å utvide innholdet|Consider expanding content/ })).toBeTruthy()
  })
})
