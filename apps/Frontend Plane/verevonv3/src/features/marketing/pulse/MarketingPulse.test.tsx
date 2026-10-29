// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { MarketingPulse } from './MarketingPulse.tsx'
import type { Website } from '../../../shared/api/marketing/website-client.ts'
import { loadMarketingWork } from '../../../shared/api/marketing/marketing-work-client.ts'
vi.mock('@/shared/api/marketing/preparation-client', async original => {
  const actual = await original<typeof import('@/shared/api/marketing/preparation-client')>()
  return { ...actual, loadSourceCatalog: async (context: object) => ({ ...context, source: 'marketing_source_catalog', items: actual.sourceKeys.map(key => ({ key, implemented: true, mode: 'public_check', status: key === 'search_console' ? 'connected' : key === 'quarry' || key === 'pagespeed' ? 'available' : 'not_connected', refresh: 'manual' })) }) }
})

const registeredWebsite: Website = {
  id: 'web_' + '1'.repeat(32), site: 'https://aquatiq.com/', revision: 1,
  createdAt: '2026-10-21T00:00:00Z', updatedAt: '2026-10-21T00:00:00Z', ownership: { status: 'google_owner' },
  integrations: { searchConsole: { status: 'connected', property: 'sc-domain:aquatiq.com' }, ga4: { status: 'requires_consent' }, clarity: { status: 'not_connected' }, pageSpeed: { status: 'available' } },
}
vi.mock('../../../shared/api/marketing/website-client.ts', async (original) => ({
  ...await original<typeof import('../../../shared/api/marketing/website-client.ts')>(),
  loadWebsites: async () => [registeredWebsite],
  addWebsite: async () => ({ ...registeredWebsite, id: 'web_' + '2'.repeat(32), site: 'https://example.com/', ownership: { status: 'unverified' }, integrations: { ...registeredWebsite.integrations, searchConsole: { status: 'not_found' } } }),
}))

vi.mock('./SearchIntelligence.tsx', () => ({ SearchIntelligence: () => null }))
vi.mock('./PageSpeedPanel.tsx', () => ({ PageSpeedPanel: () => null }))
vi.mock('./PublicSignalsPanel', () => ({ PublicSignalsPanel: () => null }))
vi.mock('./WebVitalsPanel', () => ({ WebVitalsPanel: () => null }))
vi.mock('./SiteSourcesPanel', () => ({ CookiebotPanel: () => null, AlgoliaPanel: () => null }))
vi.mock('./GA4Panel.tsx', () => ({ GA4Panel: () => null }))
vi.mock('./ClarityPanel.tsx', () => ({ ClarityPanel: () => null }))
vi.mock('@/features/visibility/components/VisibilitySummary', () => ({ VisibilitySummary: () => null }))
vi.mock('./AnalyticsSyncPanel', () => ({ AnalyticsSyncPanel: (props: { onSettled?: () => void; canManageIntegrations?: boolean }) => <button type="button" data-manage-sync={String(props.canManageIntegrations)} onClick={() => props.onSettled?.()}>Notify saved analytics settled</button> }))
vi.mock('../../../shared/api/marketing/marketing-work-client.ts', async (original) => ({
  ...await original<typeof import('../../../shared/api/marketing/marketing-work-client.ts')>(),
  loadMarketingWork: vi.fn(),
}))

/** Route fixtures by owner endpoint: independent panels must not consume the
 * next Search Console fixture or alter assertions about Visibility requests. */
function stubPulseFetch(pulseFetch: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>) {
  vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const url = new URL(input instanceof Request ? input.url : String(input), 'http://localhost')
    if (url.pathname === '/api/v1/visibility/pulse') return pulseFetch(input, init)
    if (url.pathname === '/api/v1/visibility/discovery' && init?.method === 'GET') {
      return Promise.resolve(new Response(JSON.stringify({ data: {
        websiteId: url.searchParams.get('websiteId'), site: url.searchParams.get('site'), websiteRevision: 1,
        source: 'quarry_map', status: 'idle', limit: 25, partial: true, pages: [],
      } }), { status: 200 }))
    }
    throw new Error(`Unexpected fixture endpoint: ${url.pathname}`)
  }))
}

describe('MarketingPulse', () => {
  beforeEach(() => {
    // Work is an independent persisted projection, not a second Visibility response.
    vi.mocked(loadMarketingWork).mockImplementation(async (websiteId, site) => ({
      websiteId, site, tasks: [], activity: [],
      summary: { open: 0, inProgress: 0, done: 0, dismissed: 0, total: 0, completionPercent: null },
    }))
    vi.useFakeTimers({ toFake: ['Date'] })
    vi.setSystemTime(new Date('2026-10-21T12:00:00Z'))
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
    vi.useRealTimers()
    window.localStorage.clear()
  })

  it('refreshes saved primary metrics after analytics collection settles with no automatic write', async () => {
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(new Response(JSON.stringify({ data: { site: registeredWebsite.site, source: 'google_search_console', status: 'unavailable', reason: 'snapshot_pending', period: { startDate: '2026-10-01', endDate: '2026-10-20' }, totals: null, days: [] } }))))
    stubPulseFetch(fetcher)
    render(() => <I18nProvider><MarketingPulse canManageIntegrations={false} /></I18nProvider>)
    const notify = await screen.findByRole('button', { name: 'Notify saved analytics settled' })
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(1))
    expect(notify.getAttribute('data-manage-sync')).toBe('false')
    expect(await screen.findByText(/(?:snapshot|rapport).*(?:mangler|missing)|(?:mangler|missing).*(?:snapshot|rapport)/i)).toBeTruthy()
    fireEvent.click(notify)
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(2))
    expect(vi.mocked(fetch).mock.calls.every((call) => !call[1]?.method || call[1]?.method === 'GET')).toBe(true)
  })

  it('renders an honest unavailable state when the backend cannot be reached', async () => {
    const fetchMock = vi.fn().mockRejectedValue(new Error('offline'))
    stubPulseFetch(fetchMock)

    render(() => (
      <I18nProvider>
        <MarketingPulse />
      </I18nProvider>
    ))

    expect(screen.getByRole('heading', { name: /visibility|synlighet/i })).toBeTruthy()
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1))
    const calls = vi.mocked(fetch).mock.calls
    expect(calls.filter(([input]) => String(input).startsWith('/api/v1/visibility/discovery?'))).toHaveLength(0)
    expect(calls.every(([, init]) => !init?.method || init.method === 'GET')).toBe(true)
    expect(fetchMock.mock.calls[0]?.[0]).toContain('/api/v1/visibility/pulse?quarter=Q4')
    expect(screen.getByTestId('pulse-primary-value').textContent).toBe('—')
    expect(screen.getByTestId('pulse-activity-chart')).toBeTruthy()
    expect(screen.getByTestId('pulse-timeline-scrubber')).toBeTruthy()
    expect(screen.getAllByTestId('pulse-summary-card')).toHaveLength(3)
    expect(screen.getByTestId('pulse-project-momentum-card')).toBeTruthy()
    expect(screen.getAllByTestId('pulse-metric-value').every((value) => value.textContent === '—')).toBe(true)
    expect(screen.getByTestId('pulse-momentum-value').textContent).toBe('—')
    expect(screen.getByTestId('pulse-backlog-count').textContent).toBe('0')
    expect(screen.getByTestId('pulse-log-count').textContent).toBe('0')
    const breakdown = screen.getByTestId('pulse-workspace-breakdown')
    expect(breakdown.classList.contains('marketing-pulse__breakdown')).toBe(true)
    expect(breakdown.querySelectorAll('.marketing-pulse__ledger')).toHaveLength(2)
    expect(screen.getByTestId('pulse-connected-services')).toBeTruthy()
    await waitFor(() => expect(screen.getByText(/No connection to marketing data|Ingen tilkobling til markedsføringsdata/i)).toBeTruthy())
  })

  it('renders the empty backlog as a flat breakdown list and supports the workspace views', async () => {
    stubPulseFetch(vi.fn().mockRejectedValue(new Error('offline')))

    render(() => (
      <I18nProvider>
        <MarketingPulse />
      </I18nProvider>
    ))

    const viewSwitch = await screen.findByTestId('pulse-workspace-view-switch')
    const calendar = screen.getByRole('button', { name: /kalender|calendar/i })
    const board = screen.getByRole('button', { name: /tavle|board/i })
    const backlogRow = await screen.findByTestId('pulse-backlog-empty-row')

    expect(viewSwitch).toBeTruthy()
    expect(board.getAttribute('aria-pressed')).toBe('true')
    expect(backlogRow.classList.contains('marketing-pulse__ledger-row')).toBe(true)
    expect(backlogRow.textContent).toMatch(/Ingen prioriterte oppgaver ennå|No prioritized work yet/i)
    expect(backlogRow.textContent).toMatch(/Lagre et konkret forslag|Save a specific suggestion/i)
    expect(screen.queryByTestId('pulse-backlog-empty-card')).toBeNull()

    fireEvent.click(calendar)
    expect(calendar.getAttribute('aria-pressed')).toBe('true')
    expect(screen.getByTestId('pulse-calendar-empty')).toBeTruthy()

    fireEvent.click(board)
    expect(screen.getByTestId('pulse-backlog-empty-row')).toBeTruthy()
  })

  it('renders only authenticated backend totals and actual chart bars', async () => {
    stubPulseFetch(vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: {
      site: 'https://aquatiq.com/', source: 'google_search_console', status: 'connected',
      period: { startDate: '2026-10-01', endDate: '2026-10-20' },
      totals: { clicks: 17, impressions: 440 },
      days: [{ date: '2026-10-01', clicks: 7, impressions: 140 }, { date: '2026-10-02', clicks: 10, impressions: 300 }],
    } }), { status: 200 })))

    render(() => <I18nProvider><MarketingPulse /></I18nProvider>)

    await waitFor(() => expect(screen.getByTestId('pulse-primary-value').textContent).toBe('17'))
    expect(screen.getByRole('img', { name: /daglige søkesignaler|daily search signals/i })).toBeTruthy()
    expect(screen.getByTestId('pulse-activity-chart').querySelectorAll('[data-pulse-bar]')).toHaveLength(2)
    expect(screen.getByText('aquatiq.com')).toBeTruthy()
    expect(screen.getByRole('button', { name: /Search Console · (Tilkoblet|Connected)/ })).toBeTruthy()

    fireEvent.change(screen.getByLabelText(/Diagrammåling|Chart metric/i), { target: { value: 'impressions' } })
    expect(screen.getByTestId('pulse-primary-value').textContent).toBe('440')
  })

  it('withholds carried totals when the backend marks the source unavailable', async () => {
    stubPulseFetch(vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: {
      site: 'https://aquatiq.com/', source: 'google_search_console', status: 'unavailable', reason: 'source_unavailable',
      period: { startDate: '2026-10-01', endDate: '2026-10-20' },
      totals: { clicks: 17, impressions: 440 }, days: [],
    } }), { status: 200 })))

    render(() => <I18nProvider><MarketingPulse /></I18nProvider>)

    await waitFor(() => expect(screen.getByTestId('pulse-primary-value').textContent).toBe('—'))
    expect(screen.getAllByTestId('pulse-metric-value').map((value) => value.textContent)).toEqual(['—', '—'])
  })

  it('highlights only persisted signal days while the initial filter covers the response period', async () => {
    stubPulseFetch(vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: {
      site: 'https://aquatiq.com/', source: 'google_search_console', status: 'connected',
      period: { startDate: '2026-10-01', endDate: '2026-10-20' },
      totals: { clicks: 17, impressions: 440 },
      days: [{ date: '2026-10-01', clicks: 7, impressions: 140 }, { date: '2026-10-18', clicks: 10, impressions: 300 }],
    } }), { status: 200 })))

    render(() => <I18nProvider><MarketingPulse /></I18nProvider>)

    await waitFor(() => expect(screen.getByTestId('pulse-primary-value').textContent).toBe('17'))
    const chart = screen.getByTestId('pulse-activity-chart')
    expect(chart.getAttribute('data-selection-start-date')).toBe('2026-10-01')
    expect(chart.getAttribute('data-selection-end-date')).toBe('2026-10-20')
    expect([...chart.querySelectorAll('[data-pulse-selected]')].map((bar) => bar.getAttribute('data-pulse-date'))).toEqual(['2026-10-01', '2026-10-18'])

    const context = screen.getByTestId('pulse-chart-context')
    expect(context.getAttribute('data-start-date')).toBe('2026-10-01')
    expect(context.getAttribute('data-end-date')).toBe('2026-10-20')
    expect(context.textContent).toContain('17')
    expect(context.textContent).toMatch(/Google Search Console/)
    expect(screen.getByTestId('pulse-timeline-scrubber').querySelectorAll('[data-pulse-range-handle]')).toHaveLength(2)
  })

  it('aligns bars and timeline to the actual response period and distinguishes missing days', async () => {
    stubPulseFetch(vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: {
      site: 'https://aquatiq.com/', source: 'google_search_console', status: 'connected',
      period: { startDate: '2026-10-01', endDate: '2026-10-20' },
      updatedAt: '2026-10-21T10:00:00Z',
      totals: { clicks: 17, impressions: 440 },
      days: [{ date: '2026-10-01', clicks: 7, impressions: 140 }, { date: '2026-10-18', clicks: 10, impressions: 300 }],
    } }), { status: 200 })))
    render(() => <I18nProvider><MarketingPulse /></I18nProvider>)

    await waitFor(() => expect(screen.getByTestId('pulse-primary-value').textContent).toBe('17'))
    const chart = screen.getByTestId('pulse-activity-chart')
    expect(chart.getAttribute('data-start-date')).toBe('2026-10-01')
    expect(chart.getAttribute('data-end-date')).toBe('2026-10-20')
    const slots = [...chart.querySelectorAll<HTMLSpanElement>('span[data-pulse-date]')]
    expect(slots.map((slot) => slot.getAttribute('data-pulse-date'))).toEqual(
      Array.from({ length: 20 }, (_, index) => `2026-10-${String(index + 1).padStart(2, '0')}`),
    )
    expect(slots.filter((slot) => slot.hasAttribute('data-pulse-bar')).map((slot) => slot.getAttribute('data-pulse-date'))).toEqual(['2026-10-01', '2026-10-18'])
    expect(slots.filter((slot) => slot.classList.contains('is-empty'))).toHaveLength(18)
    expect(slots[1]?.hasAttribute('data-pulse-bar')).toBe(false)

    const dateLabel = (date: string) => new Intl.DateTimeFormat('nb-NO', { day: 'numeric', month: 'short', year: 'numeric', timeZone: 'UTC' }).format(new Date(`${date}T00:00:00Z`))
    const dateRange = screen.getByTestId('pulse-date-range').textContent
    expect(dateRange).toContain(dateLabel('2026-10-01'))
    expect(dateRange).toContain(dateLabel('2026-10-20'))
    const availability = screen.getByTestId('pulse-data-availability').textContent
    expect(availability).toContain(dateLabel('2026-10-18'))
    expect(availability).not.toContain(dateLabel('2026-10-20'))
    expect(availability).not.toContain(dateLabel('2026-10-21'))

    const timeline = screen.getByTestId('pulse-timeline-scrubber')
    expect(timeline.getAttribute('data-start-date')).toBe('2026-10-01')
    expect(timeline.getAttribute('data-end-date')).toBe('2026-10-20')
    const dates = [...timeline.querySelectorAll('.marketing-pulse__timeline-labels span[data-pulse-date]')].map((label) => label.getAttribute('data-pulse-date') ?? '')
    expect(dates[0]).toBe('2026-10-01')
    expect(dates.at(-1)).toBe('2026-10-20')
    expect(dates).toEqual([...new Set(dates)].sort())
    expect(dates.every((date) => date >= '2026-10-01' && date <= '2026-10-20')).toBe(true)
    expect(timeline.textContent).not.toMatch(/Jan|Apr|Jul/)
  })

  it('keeps an unobserved date range unknown while retaining genuinely measured zero', async () => {
    stubPulseFetch(() => Promise.resolve(Response.json({ data: {
      site: registeredWebsite.site, source: 'google_search_console', status: 'connected', propertyScope: 'domain',
      period: { startDate: '2026-10-01', endDate: '2026-10-20' }, totals: { clicks: 7, impressions: 23 },
      days: [{ date: '2026-10-02', clicks: 0, impressions: 3 }, { date: '2026-10-18', clicks: 7, impressions: 20 }],
    } })))
    render(() => <I18nProvider><MarketingPulse /></I18nProvider>)
    await waitFor(() => expect(screen.getByTestId('pulse-primary-value').textContent).toBe('7'))
    const end = screen.getByRole('slider', { name: /til dato|to date/i })
    fireEvent.keyDown(end, { key: 'Home' })
    await waitFor(() => expect(screen.getByTestId('pulse-primary-value').textContent).toBe('—'))
    expect(end.getAttribute('aria-valuenow')).toBe('1')
    fireEvent.keyDown(end, { key: 'ArrowRight' })
    await waitFor(() => expect(screen.getByTestId('pulse-primary-value').textContent).toBe('0'))
    fireEvent.keyDown(end, { key: 'End' })
    await waitFor(() => expect(screen.getByTestId('pulse-primary-value').textContent).toBe('7'))
  })

  it('uses the requested quarter calendar without making up a latest data date for empty rows', async () => {
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({ data: {
        site: 'https://aquatiq.com/', source: 'google_search_console', status: 'connected',
        period: { startDate: '2026-10-01', endDate: '2026-10-20' },
        totals: { clicks: 0, impressions: 0 }, days: [],
      } }), { status: 200 }))
      .mockResolvedValueOnce(new Response(JSON.stringify({ data: {
        site: 'https://aquatiq.com/', source: 'google_search_console', status: 'connected',
        period: { startDate: '2026-07-01', endDate: '2026-09-30' },
        updatedAt: '2026-10-21T10:00:00Z',
        totals: { clicks: 0, impressions: 0 }, days: [],
      } }), { status: 200 }))
    stubPulseFetch(fetchMock)
    render(() => <I18nProvider><MarketingPulse /></I18nProvider>)
    await waitFor(() => expect(screen.getByTestId('pulse-primary-value').textContent).toBe('0'))
    fireEvent.click(screen.getByRole('button', { name: 'Q3' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2))
    expect(fetchMock.mock.calls[1]?.[0]).toContain('/api/v1/visibility/pulse?quarter=Q3')
    await waitFor(() => expect(screen.getByTestId('pulse-activity-chart').getAttribute('data-start-date')).toBe('2026-07-01'))

    const chart = screen.getByTestId('pulse-activity-chart')
    expect(chart.getAttribute('data-end-date')).toBe('2026-09-30')
    expect(chart.querySelectorAll('[data-pulse-bar]')).toHaveLength(0)
    const timeline = screen.getByTestId('pulse-timeline-scrubber')
    const dates = [...timeline.querySelectorAll('.marketing-pulse__timeline-labels span[data-pulse-date]')].map((label) => label.getAttribute('data-pulse-date') ?? '')
    expect(dates[0]).toBe('2026-07-01')
    expect(dates.at(-1)).toBe('2026-09-30')
    expect(dates.every((date) => date >= '2026-07-01' && date <= '2026-09-30')).toBe(true)
    expect(screen.getByTestId('pulse-data-availability').textContent).not.toMatch(/2026|September|september|October|oktober|through|til og med/i)
  })

  it('loads the current full-year projection from the period switch', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const quarter = new URL(String(input), 'http://localhost').searchParams.get('quarter')
      const data = quarter === 'FY'
        ? { site: 'https://aquatiq.com/', source: 'google_search_console', status: 'connected', period: { startDate: '2026-01-01', endDate: '2026-10-20' }, totals: { clicks: 42, impressions: 840 }, days: [{ date: '2026-01-01', clicks: 12, impressions: 240 }, { date: '2026-10-20', clicks: 30, impressions: 600 }] }
        : { site: 'https://aquatiq.com/', source: 'google_search_console', status: 'connected', period: { startDate: '2026-10-01', endDate: '2026-10-20' }, totals: { clicks: 17, impressions: 440 }, days: [{ date: '2026-10-01', clicks: 7, impressions: 140 }, { date: '2026-10-18', clicks: 10, impressions: 300 }] }
      return Promise.resolve(new Response(JSON.stringify({ data }), { status: 200 }))
    })
    stubPulseFetch(fetchMock)
    render(() => <I18nProvider><MarketingPulse /></I18nProvider>)

    await waitFor(() => expect(screen.getByTestId('pulse-primary-value').textContent).toBe('17'))
    const fullYear = screen.getByRole('button', { name: /hele året|full year/i })
    fireEvent.click(fullYear)

    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2))
    expect(fetchMock.mock.calls[1]?.[0]).toContain('/api/v1/visibility/pulse?quarter=FY')
    expect(fullYear.getAttribute('aria-pressed')).toBe('true')
    await waitFor(() => expect(screen.getByTestId('pulse-primary-value').textContent).toBe('42'))
  })

  it('keeps the full-year option unavailable until the first completed UTC day', async () => {
    vi.setSystemTime(new Date('2026-01-01T12:00:00Z'))
    stubPulseFetch(vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: {
      site: 'https://aquatiq.com/', source: 'google_search_console', status: 'unavailable', reason: 'snapshot_pending',
      period: { startDate: '2026-01-01', endDate: '2026-01-01' }, totals: null, days: [],
    } }), { status: 200 })))
    render(() => <I18nProvider><MarketingPulse /></I18nProvider>)

    expect((await screen.findByRole('button', { name: /hele året|full year/i }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('filters the selected chart range with its draggable start handle', async () => {
    stubPulseFetch(vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: {
      site: 'https://aquatiq.com/', source: 'google_search_console', status: 'connected',
      period: { startDate: '2026-10-01', endDate: '2026-10-20' }, totals: { clicks: 17, impressions: 440 },
      days: [{ date: '2026-10-01', clicks: 7, impressions: 140 }, { date: '2026-10-18', clicks: 10, impressions: 300 }],
    } }), { status: 200 })))
    render(() => <I18nProvider><MarketingPulse /></I18nProvider>)

    await waitFor(() => expect(screen.getByTestId('pulse-primary-value').textContent).toBe('17'))
    const timeline = screen.getByTestId('pulse-timeline-scrubber')
    const track = timeline.querySelector('.marketing-pulse__timeline-track') as HTMLDivElement
    Object.defineProperty(track, 'getBoundingClientRect', { configurable: true, value: () => ({ left: 0, width: 200 }) })
    const start = screen.getByRole('slider', { name: /fra dato|start date/i })
    fireEvent.pointerDown(start, { clientX: 0, pointerId: 1 })
    fireEvent.pointerMove(start, { clientX: 20, pointerId: 1 })
    fireEvent.pointerUp(start, { clientX: 20, pointerId: 1 })

    await waitFor(() => expect(screen.getByTestId('pulse-primary-value').textContent).toBe('10'))
    expect(timeline.getAttribute('data-selection-start-date')).toBe('2026-10-03')
    expect(timeline.getAttribute('data-selection-end-date')).toBe('2026-10-20')
  })

  it('keeps both drag handles reachable after collapsing the range to one day', async () => {
    stubPulseFetch(vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: {
      site: 'https://aquatiq.com/', source: 'google_search_console', status: 'connected',
      period: { startDate: '2026-10-01', endDate: '2026-10-20' }, totals: { clicks: 17, impressions: 440 },
      days: [{ date: '2026-10-01', clicks: 7, impressions: 140 }, { date: '2026-10-18', clicks: 10, impressions: 300 }],
    } }), { status: 200 })))
    render(() => <I18nProvider><MarketingPulse /></I18nProvider>)

    await waitFor(() => expect(screen.getByTestId('pulse-primary-value').textContent).toBe('17'))
    const timeline = screen.getByTestId('pulse-timeline-scrubber')
    const track = timeline.querySelector('.marketing-pulse__timeline-track') as HTMLDivElement
    Object.defineProperty(track, 'getBoundingClientRect', { configurable: true, value: () => ({ left: 0, width: 190 }) })
    const start = screen.getByRole('slider', { name: /fra dato|start date/i })
    const end = screen.getByRole('slider', { name: /til dato|end date/i })
    const dayFour = 30
    fireEvent.pointerDown(start, { clientX: dayFour, pointerId: 1 })
    fireEvent.pointerMove(start, { clientX: dayFour, pointerId: 1 })
    fireEvent.pointerUp(start, { clientX: dayFour, pointerId: 1 })
    fireEvent.pointerDown(end, { clientX: dayFour, pointerId: 2 })
    fireEvent.pointerMove(end, { clientX: dayFour, pointerId: 2 })
    fireEvent.pointerUp(end, { clientX: dayFour, pointerId: 2 })
    expect(timeline.getAttribute('data-selection-start-date')).toBe('2026-10-04')
    expect(timeline.getAttribute('data-selection-end-date')).toBe('2026-10-04')
    expect((start as HTMLButtonElement).style.left).toContain('calc')
    expect((end as HTMLButtonElement).style.left).toContain('calc')

    fireEvent.pointerDown(start, { clientX: 0, pointerId: 3 })
    fireEvent.pointerMove(start, { clientX: 0, pointerId: 3 })
    fireEvent.pointerUp(start, { clientX: 0, pointerId: 3 })
    expect(timeline.getAttribute('data-selection-start-date')).toBe('2026-10-01')
    expect(timeline.getAttribute('data-selection-end-date')).toBe('2026-10-04')
  })

  it('clears old chart dates while a new quarter loads and ignores a late quarter response', async () => {
    let completeQ3: ((response: Response) => void) | undefined
    let completeQ4: ((response: Response) => void) | undefined
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({ data: {
        site: 'https://aquatiq.com/', source: 'google_search_console', status: 'connected',
        period: { startDate: '2026-10-01', endDate: '2026-10-20' },
        totals: { clicks: 17, impressions: 440 }, days: [{ date: '2026-10-18', clicks: 17, impressions: 440 }],
      } }), { status: 200 }))
      .mockReturnValueOnce(new Promise<Response>((resolve) => { completeQ3 = resolve }))
      .mockReturnValueOnce(new Promise<Response>((resolve) => { completeQ4 = resolve }))
    stubPulseFetch(fetchMock)
    render(() => <I18nProvider><MarketingPulse /></I18nProvider>)
    await waitFor(() => expect(screen.getByTestId('pulse-primary-value').textContent).toBe('17'))

    fireEvent.click(screen.getByRole('button', { name: 'Q3' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2))
    expect(screen.getByTestId('pulse-activity-chart').getAttribute('data-start-date')).toBeNull()
    expect(screen.getByTestId('pulse-timeline-scrubber').querySelectorAll('[data-pulse-date]')).toHaveLength(0)
    expect(screen.getByTestId('pulse-date-range').textContent).not.toContain('2026')
    expect(screen.getByTestId('pulse-data-availability').textContent).not.toContain('2026')

    fireEvent.click(screen.getByRole('button', { name: 'Q4' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(3))
    completeQ3?.(new Response(JSON.stringify({ data: {
      site: 'https://aquatiq.com/', source: 'google_search_console', status: 'connected',
      period: { startDate: '2026-07-01', endDate: '2026-09-30' },
      totals: { clicks: 999, impressions: 1000 }, days: [{ date: '2026-07-01', clicks: 499, impressions: 500 }, { date: '2026-09-30', clicks: 500, impressions: 500 }],
    } }), { status: 200 }))
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(screen.getByTestId('pulse-primary-value').textContent).toBe('—')
    expect(screen.getByTestId('pulse-activity-chart').getAttribute('data-start-date')).toBeNull()
    expect(screen.getByTestId('pulse-timeline-scrubber').querySelectorAll('[data-pulse-date]')).toHaveLength(0)

    completeQ4?.(new Response(JSON.stringify({ data: {
      site: 'https://aquatiq.com/', source: 'google_search_console', status: 'connected',
      period: { startDate: '2026-10-01', endDate: '2026-10-20' },
      totals: { clicks: 18, impressions: 450 }, days: [{ date: '2026-10-18', clicks: 18, impressions: 450 }],
    } }), { status: 200 }))
    await waitFor(() => expect(screen.getByTestId('pulse-primary-value').textContent).toBe('18'))
    expect(screen.getByTestId('pulse-activity-chart').getAttribute('data-start-date')).toBe('2026-10-01')
    expect(screen.getByTestId('pulse-timeline-scrubber').getAttribute('data-end-date')).toBe('2026-10-20')
  })

  it('shows the unconfigured source without a local unlock form or invented metrics', async () => {
    stubPulseFetch(vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: {
      site: 'https://aquatiq.com/', source: 'google_search_console', status: 'unavailable', reason: 'not_configured',
      period: { startDate: '2026-10-01', endDate: '2026-10-20' }, totals: null, days: [],
    } }), { status: 200 })))
    render(() => <I18nProvider><MarketingPulse /></I18nProvider>)
    await waitFor(() => expect(screen.getAllByText(/Search Console.*not connected|Search Console.*ikke koblet/i).length).toBeGreaterThan(0))
    expect(screen.queryByLabelText(/local access code|lokal tilgangskode/i)).toBeNull()
    expect(screen.getByTestId('pulse-primary-value').textContent).toBe('—')
    expect(screen.getAllByTestId('pulse-summary-card').slice(0, 2).every((card) => /Search Console.*ikke koblet|Search Console.*not connected/i.test(card.textContent ?? ''))).toBe(true)
  })

  it('shows an unavailable state on a backend 401 without asking for a local code', async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response('{}', { status: 401 }))
    stubPulseFetch(fetchMock)
    render(() => <I18nProvider><MarketingPulse /></I18nProvider>)
    await waitFor(() => expect(screen.getByText(/Ingen tilkobling til markedsføringsdata|No connection to marketing data/i)).toBeTruthy())
    expect(screen.queryByLabelText(/local access code|lokal tilgangskode/i)).toBeNull()
    expect(screen.getByTestId('pulse-primary-value').textContent).toBe('—')
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it('adds a site from the toolbar and never keeps the prior site metrics while switching', async () => {
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({ data: {
        site: 'https://aquatiq.com/', source: 'google_search_console', status: 'connected',
        period: { startDate: '2026-10-01', endDate: '2026-10-20' }, totals: { clicks: 9, impressions: 20 }, days: [],
      } }), { status: 200 }))
      .mockResolvedValueOnce(new Response(JSON.stringify({ data: {
        site: 'https://example.com/', source: 'google_search_console', status: 'unavailable', reason: 'property_unverified',
        period: { startDate: '2026-10-01', endDate: '2026-10-20' }, totals: null, days: [],
      } }), { status: 200 }))
    stubPulseFetch(fetchMock)
    render(() => <I18nProvider><MarketingPulse /></I18nProvider>)
    await waitFor(() => expect(screen.getByTestId('pulse-primary-value').textContent).toBe('9'))
    fireEvent.click(screen.getByRole('button', { name: /nettsider|websites/i }))
    fireEvent.input(screen.getByLabelText(/nettadresse|website url/i), { target: { value: 'example.com' } })
    fireEvent.click(screen.getByRole('button', { name: /legg til nettsted|add website/i }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2))
    expect(fetchMock.mock.calls[1]?.[0]).toContain('site=https%3A%2F%2Fexample.com%2F')
    await waitFor(() => expect(screen.getByTestId('pulse-primary-value').textContent).toBe('—'))
    expect(screen.getAllByText('example.com').length).toBeGreaterThan(0)
    expect(window.localStorage.getItem('studio-pulse-site-stack-v1')).toBeNull()
    expect(window.localStorage.getItem('studio-pulse-active-website-id-v1')).toBe('web_' + '2'.repeat(32))
  })

  it('ignores a late response for the previously selected website', async () => {
    let completeOld: ((response: Response) => void) | undefined
    const oldResponse = new Promise<Response>((resolve) => { completeOld = resolve })
    const fetchMock = vi.fn()
      .mockReturnValueOnce(oldResponse)
      .mockResolvedValueOnce(new Response(JSON.stringify({ data: {
        site: 'https://example.com/', source: 'google_search_console', status: 'unavailable', reason: 'property_unverified',
        period: { startDate: '2026-10-01', endDate: '2026-10-20' }, totals: null, days: [],
      } }), { status: 200 }))
    stubPulseFetch(fetchMock)
    render(() => <I18nProvider><MarketingPulse /></I18nProvider>)
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1))
    fireEvent.click(screen.getByRole('button', { name: /nettsider|websites/i }))
    fireEvent.input(screen.getByLabelText(/nettadresse|website url/i), { target: { value: 'example.com' } })
    fireEvent.click(screen.getByRole('button', { name: /legg til nettsted|add website/i }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2))
    completeOld?.(new Response(JSON.stringify({ data: {
      site: 'https://aquatiq.com/', source: 'google_search_console', status: 'connected',
      period: { startDate: '2026-10-01', endDate: '2026-10-20' }, totals: { clicks: 999, impressions: 1000 }, days: [],
    } }), { status: 200 }))
    await new Promise((resolve) => setTimeout(resolve, 0))
    await waitFor(() => expect(screen.getAllByText('example.com').length).toBeGreaterThan(0))
    expect(screen.getByTestId('pulse-primary-value').textContent).toBe('—')
  })
})
