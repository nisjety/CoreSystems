import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { TrendsPanel } from './TrendsPanel'
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }), localeDateTime: () => 'en-GB' }))
const context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 3 }
const fixture = (status = 'available') => ({ ...context, report: { terms: ['SEO'], geo: 'NO', timeframe: 'today 12-m', source: 'pytrends_dev', scale: 'relative_0_100_per_request', status, observedAt: new Date().toISOString(), cached: false, rows: status === 'available' ? [{ date: '2026-01-01', values: [0], partial: false }, { date: '2026-01-08', values: [100], partial: true }] : [] } })
const response = (data: unknown) => new Response(JSON.stringify({ data }), { status: 200 })
const waiting = { ...context, source: 'marketing_trends_auto', status: 'waiting_for_search_console' }
const automatic = () => ({ ...context, source: 'marketing_trends_auto', status: 'complete', expiresAt: new Date(Date.now() + 3600000).toISOString(), seed: { source: 'google_search_console', selection: 'impressions_then_clicks', terms: ['SEO'], observedAt: new Date().toISOString(), period: { startDate: '2026-10-01', endDate: '2026-10-21' } }, report: { ...fixture().report, geo: '' } })
const manualFetcher = (status = 'available') => vi.fn((_url: unknown, init: RequestInit) => Promise.resolve(response(init.method === 'GET' ? waiting : fixture(status))))
const mount = (allowed = true) => render(() => <TrendsPanel {...context} revision={3} canManageIntegrations={allowed} />)
const advanced = () => fireEvent.click(screen.getByText('Advanced: research other terms'))
const submit = () => { advanced(); fireEvent.input(screen.getByRole('textbox'), { target: { value: 'SEO' } }); fireEvent.click(screen.getByRole('button', { name: 'Research search interest' })) }
describe('development Trends research', () => {
  afterEach(() => { cleanup(); vi.unstubAllGlobals() })
  it('reads saved state on render, then runs manual research only after an explicit request', async () => {
    const fetcher = manualFetcher(); vi.stubGlobal('fetch', fetcher); mount()
    expect(fetcher.mock.calls.every((call) => call[1].method === 'GET')).toBe(true); submit()
    await screen.findByText('Measured relative search interest')
    expect(fetcher.mock.calls.filter((call) => call[1].method === 'POST')).toHaveLength(1)
    expect(JSON.parse(String(fetcher.mock.calls.find((call) => call[1].method === 'POST')![1].body))).toEqual({ ...context, terms: ['SEO'], geo: 'NO', timeframe: 'today 12-m' })
    expect(screen.getAllByText('100')).toHaveLength(2); expect(screen.getByText('Yes')).toBeTruthy()
    expect(screen.getByText(/not.*absolute search volumes/)).toBeTruthy()
  })
  it('shows provider refusal without a fabricated chart or retry loop', async () => {
    const fetcher = manualFetcher('rate_limited'); vi.stubGlobal('fetch', fetcher); mount(); submit()
    await screen.findByText('Google is rate-limiting requests. The adapter is paused.')
    expect(screen.queryByRole('img')).toBeNull(); expect(fetcher.mock.calls.filter((call) => call[1].method === 'POST')).toHaveLength(1)
  })
  it('does not let viewers start provider requests', () => {
    const fetcher = manualFetcher(); vi.stubGlobal('fetch', fetcher); mount(false); advanced()
    expect((screen.getByRole('button', { name: 'Research search interest' }) as HTMLButtonElement).disabled).toBe(true)
    expect(fetcher.mock.calls.every((call) => call[1].method === 'GET')).toBe(true)
  })
  it('drops responses from the previous site after switching context', async () => {
    let finish!: (v: Response) => void
    const fetcher = vi.fn((url: unknown, init: RequestInit) => init.method === 'GET' ? Promise.resolve(response({ ...waiting, site: new URL(String(url), 'http://localhost').searchParams.get('site') })) : new Promise<Response>((resolve) => { finish = resolve })); vi.stubGlobal('fetch', fetcher)
    const [site, setSite] = createSignal(context.site)
    render(() => <TrendsPanel websiteId={context.websiteId} site={site()} revision={3} canManageIntegrations />)
    submit(); setSite('https://agenci.no/'); finish(response(fixture()))
    await Promise.resolve(); await Promise.resolve()
    expect(screen.queryByText('Measured relative search interest')).toBeNull()
    expect((fetcher.mock.calls.find((call) => call[1].method === 'POST')![1].signal as AbortSignal).aborted).toBe(true)
  })
  it('shows the saved automatic report without keyword entry or a provider POST', async () => {
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(response(automatic()))); vi.stubGlobal('fetch', fetcher); mount()
    await screen.findByText('Automatic report from Search Console keywords')
    expect(screen.getByRole('img')).toBeTruthy()
    expect(screen.getByText('Advanced: research other terms').closest('details')?.hasAttribute('open')).toBe(false)
    expect((screen.getByRole('textbox') as HTMLInputElement).value).toBe('')
    expect(fetcher).toHaveBeenCalledTimes(1); expect(fetcher.mock.calls[0]![1].method).toBe('GET')
    fireEvent.click(screen.getByRole('button', { name: 'Refresh saved report' }))
    await screen.findByText('Automatic report from Search Console keywords')
    expect(fetcher.mock.calls.every((call) => call[1].method === 'GET')).toBe(true)
  })
  it('shows a missing Search Console snapshot honestly without submitting sample terms', async () => {
    const fetcher = manualFetcher(); vi.stubGlobal('fetch', fetcher); mount()
    await screen.findByText(/Waiting for fresh Search Console keywords/)
    expect(screen.queryByRole('img')).toBeNull(); expect(screen.getByRole('link', { name: 'Manage account connection' }).getAttribute('href')).toBe('/settings/integrations')
    expect(fetcher.mock.calls.every((call) => call[1].method === 'GET')).toBe(true)
  })
  it('rereads saved Trends when Search Console collection settles without starting provider work', async () => {
    let reads = 0
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(response(++reads === 1 ? waiting : automatic()))); vi.stubGlobal('fetch', fetcher)
    const [key, setKey] = createSignal(0)
    render(() => <TrendsPanel {...context} revision={3} refreshKey={key()} canManageIntegrations />)
    await screen.findByText(/Waiting for fresh Search Console keywords/)
    setKey(1)
    await screen.findByText('Automatic report from Search Console keywords')
    expect(fetcher).toHaveBeenCalledTimes(2)
    expect(fetcher.mock.calls.every((call) => call[1].method === 'GET')).toBe(true)
  })
})
