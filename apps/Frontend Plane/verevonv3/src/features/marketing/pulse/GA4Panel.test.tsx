// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { GA4Panel } from './GA4Panel.tsx'
const site = 'https://example.org/'
const websiteId = 'web_' + '1'.repeat(32)
const metrics = { users: 8, sessions: 12, engagedSessions: 7, pageViews: 20, keyEvents: 1.5, engagementSeconds: 46.25 }
const snapshot = { site, source: 'google_analytics', status: 'connected', period: { startDate: '2026-10-01', endDate: '2026-10-03' }, timeZone: 'Europe/Oslo', totals: metrics, days: [{ date: '2026-10-01', ...metrics }, { date: '2026-10-03', ...metrics }] }
const json = (data: unknown) => new Response(JSON.stringify({ data }))
describe('GA4 panel', () => {
  afterEach(() => { cleanup(); vi.unstubAllGlobals() })
  it('refreshes the saved projection after collection settles and labels absent snapshots separately', async () => {
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(json({ ...snapshot, status: 'unavailable', reason: 'snapshot_pending', totals: null, days: [] }))); vi.stubGlobal('fetch', fetcher)
    const [refresh, setRefresh] = createSignal(0)
    render(() => <I18nProvider><GA4Panel site={site} websiteId={websiteId} revision={1} quarter="Q4" refreshKey={refresh()} /></I18nProvider>)
    await waitFor(() => expect(screen.getByRole('status').textContent).toMatch(/snapshot|innhenting/i))
    expect(fetcher).toHaveBeenCalledTimes(1)
    fetcher.mockImplementation(() => Promise.resolve(json(snapshot))); setRefresh(1)
    await waitFor(() => expect(screen.getByTestId('ga4-users').textContent).toBe('8'))
    expect(fetcher).toHaveBeenCalledTimes(2)
  })
  it('renders independent provider totals, actual property dates and data-quality warnings', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(json({ ...snapshot, reportingSite: 'https://www.example.org/', quality: { thresholded: true, sampled: false, dataLoss: false } })))
    render(() => <I18nProvider><GA4Panel site={site} websiteId={websiteId} revision={1} quarter="Q4" /></I18nProvider>)
    await waitFor(() => expect(screen.getByTestId('ga4-users').textContent).toBe('8'))
    expect(screen.getByTestId('ga4-period').textContent).toContain('Europe/Oslo')
    expect(screen.getByTestId('ga4-reporting-host').textContent).toMatch(/(?:GA4 reporting host|GA4-rapporteringsvert).*www\.example\.org/i)
    expect(screen.getByTestId('ga4-days').querySelectorAll('tbody tr')).toHaveLength(2)
    expect(screen.getByText(/not added|ikke summert/i)).toBeTruthy()
    expect(screen.getByRole('note').textContent).toMatch(/threshold|tersk/i)
  })
  it('distinguishes an empty provider report from measured zero and connection failure', async () => {
    const fetcher = vi.fn().mockResolvedValue(json({ ...snapshot, totals: null, days: [] })); vi.stubGlobal('fetch', fetcher)
    render(() => <I18nProvider><GA4Panel site={site} websiteId={websiteId} revision={1} quarter="Q4" /></I18nProvider>)
    await waitFor(() => expect(screen.getByText(/no report data|ingen rapportdata/i)).toBeTruthy())
    expect(screen.getByTestId('ga4-users').textContent).toBe('—')
    expect(screen.queryByRole('alert')).toBeNull()
  })
  it('clears old data when quarter or website revision changes and ignores late responses', async () => {
    let finishOld: ((value: Response) => void) | undefined
    const fetcher = vi.fn().mockResolvedValueOnce(json(snapshot)).mockImplementationOnce(() => new Promise<Response>((resolve) => { finishOld = resolve })).mockResolvedValueOnce(json({ ...snapshot, totals: { ...metrics, users: 3 } }))
    vi.stubGlobal('fetch', fetcher)
    const [quarter, setQuarter] = createSignal<'Q3' | 'Q4'>('Q4')
    const [revision, setRevision] = createSignal(1)
    render(() => <I18nProvider><GA4Panel site={site} websiteId={websiteId} revision={revision()} quarter={quarter()} /></I18nProvider>)
    await waitFor(() => expect(screen.getByTestId('ga4-users').textContent).toBe('8'))
    setQuarter('Q3')
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(2))
    expect(screen.getByTestId('ga4-users').textContent).toBe('—')
    setRevision(2)
    await waitFor(() => expect(screen.getByTestId('ga4-users').textContent).toBe('3'))
    finishOld?.(json({ ...snapshot, totals: { ...metrics, users: 999 } }))
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(screen.getByTestId('ga4-users').textContent).toBe('3')
  })
  it('offers website connection management on a consent requirement without asking for credentials', async () => {
    const manage = vi.fn(); vi.stubGlobal('fetch', vi.fn().mockResolvedValue(json({ ...snapshot, status: 'unavailable', reason: 'requires_consent', totals: null, days: [] })))
    render(() => <I18nProvider><GA4Panel site={site} websiteId={websiteId} revision={1} quarter="Q4" onManageWebsite={manage} /></I18nProvider>)
    const button = await screen.findByRole('button', { name: /manage website|administrer nettsted/i })
    fireEvent.click(button); expect(manage).toHaveBeenCalledOnce()
    expect(screen.queryByTestId('ga4-reporting-host')).toBeNull()
    expect(screen.queryByRole('textbox')).toBeNull()
  })
})
