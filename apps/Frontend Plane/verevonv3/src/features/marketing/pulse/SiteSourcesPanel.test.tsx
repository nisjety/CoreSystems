import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AlgoliaPanel, CookiebotPanel } from './SiteSourcesPanel'
vi.mock('./AlgoliaCollectionPanel',()=>({AlgoliaCollectionPanel:()=>null}))
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }), localeDateTime: () => 'en-GB' }))
const context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 7 }
const common = { ...context, provider: 'cookiebot', status: 'not_loaded', observedAt: '2026-10-23T10:00:00Z', window: { startDate: '2026-10-16', endDate: '2026-10-22' } }
const cookie = { ...common, status: 'connected', cookiebot: { reportingDomain: 'www.triodelab.no', consents: { status: 'connected', reportedDays: 7, totals: { optIn: 12, optOut: 3, preferences: 8, statistics: 10, marketing: 5 } }, inventory: { status: 'connected', scannedAt: '2026-10-20T09:00:00Z', total: 0, categories: { necessary: 0, preferences: 0, statistics: 0, marketing: 0, unclassified: 0 }, thirdParty: 0, secure: 0, httpOnly: 0 } } }
const response = (data: unknown) => new Response(JSON.stringify({ data }))
const mount = (admin = false) => render(() => <CookiebotPanel websiteId={context.websiteId} site={context.site} revision={7} canManageIntegrations={admin} />)
describe('site-source panels', () => {
  beforeEach(() => { vi.useFakeTimers({ toFake: ['Date'] }); vi.setSystemTime(new Date('2026-10-23T11:00:00Z')) })
  afterEach(() => { cleanup(); vi.unstubAllGlobals(); vi.useRealTimers() })
  it('reads only; not-loaded is not treated as missing credentials or zero', async () => {
    const fetcher = vi.fn().mockResolvedValue(response(common)); vi.stubGlobal('fetch', fetcher); mount()
    await screen.findByText('No saved provider snapshot')
    expect(fetcher).toHaveBeenCalledTimes(1); expect(fetcher.mock.calls[0]![1].method).toBe('GET')
    expect(screen.queryByText(/protected server configuration/)).toBeNull(); expect(screen.queryByRole('button', { name: 'Fetch Cookiebot report' })).toBeNull()
  })
  it('imports explicitly for admins and explains protected configuration without browser keys', async () => {
    const fetcher = vi.fn().mockImplementation((_url: string, init: RequestInit) => Promise.resolve(response(init.method === 'POST' ? { ...common, status: 'not_configured' } : common))); vi.stubGlobal('fetch', fetcher); mount(true)
    await screen.findByText('No saved provider snapshot'); fireEvent.click(screen.getByRole('button', { name: 'Fetch Cookiebot report' }))
    await screen.findByText(/protected server configuration/)
    expect(JSON.parse(fetcher.mock.calls[1]![1].body as string)).toEqual({ ...context, provider: 'cookiebot' })
    expect(screen.queryByRole('textbox')).toBeNull()
  })
  it('labels consent event totals, actual coverage and scan provenance without compliance claims', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response(cookie))); mount()
    await screen.findByText('12')
    expect(screen.getByText(/events, not unique visitors/)).toBeTruthy(); expect(screen.getByText(/7 of 7 completed UTC dates/)).toBeTruthy()
    expect(screen.getByText(/not proof of current compliance/)).toBeTruthy(); expect(screen.getByText('www.triodelab.no')).toBeTruthy()
    expect(screen.queryByText('Compliant')).toBeNull()
  })
  it('renders existing Algolia aggregates, zero-search null rates and source scope', async () => {
    const algolia = { ...common, provider: 'algolia', status: 'connected', algolia: { indexName: 'triodelab_public', region: 'de', analytics: { status: 'connected', reportedDays: 0, searches: 0, noResultSearches: 0, noResultRate: null } } }
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response(algolia)))
    render(() => <AlgoliaPanel websiteId={context.websiteId} site={context.site} revision={7} />)
    await screen.findByText('triodelab_public'); expect(screen.getByTestId('algolia-no-result-rate').textContent).toBe('—')
    expect(screen.getByText(/Existing index.*DE/)).toBeTruthy(); expect(screen.getByText(/No user IDs, individual search logs/)).toBeTruthy()
  })
  it('labels OAuth analytics region as provider-managed without guessing geography', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response({ ...common, provider: 'algolia', status: 'connected', algolia: { indexName: 'public_index', region: 'provider', analytics: { status: 'connected', reportedDays: 1, searches: 2, noResultSearches: 1, noResultRate: 0.5 } } })))
    render(() => <AlgoliaPanel websiteId={context.websiteId} site={context.site} revision={7} />)
    await screen.findByText(/Existing index.*Provider-managed/)
    expect(screen.queryByText(/Existing index.*DE/)).toBeNull()
  })
  it('discards stale revision and role-loss sync responses', async () => {
    let finish: ((value: Response) => void) | undefined
    const fetcher = vi.fn().mockImplementation((_url: string, init: RequestInit) => init.method === 'POST' ? new Promise<Response>((resolve) => { finish = resolve }) : Promise.resolve(response(common))); vi.stubGlobal('fetch', fetcher)
    const [admin, setAdmin] = createSignal(true)
    render(() => <CookiebotPanel websiteId={context.websiteId} site={context.site} revision={7} canManageIntegrations={admin()} />)
    await screen.findByText('No saved provider snapshot'); fireEvent.click(screen.getByRole('button', { name: 'Fetch Cookiebot report' }))
    const post = fetcher.mock.calls.find((call) => call[1].method === 'POST')!
    setAdmin(false); await waitFor(() => expect((post[1].signal as AbortSignal).aborted).toBe(true))
    finish?.(response(cookie)); await Promise.resolve(); await Promise.resolve(); expect(screen.queryByText('12')).toBeNull()
  })
  it('aborts previous website revision reads and cannot render a late old report', async () => {
    let finish: ((value: Response) => void) | undefined
    const fetcher = vi.fn().mockImplementationOnce(() => new Promise<Response>((resolve) => { finish = resolve })).mockImplementation(() => Promise.resolve(response({ ...common, websiteRevision: 8 }))); vi.stubGlobal('fetch', fetcher)
    const [revision, setRevision] = createSignal(7)
    render(() => <CookiebotPanel websiteId={context.websiteId} site={context.site} revision={revision()} />)
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(1)); const signal = fetcher.mock.calls[0]![1].signal as AbortSignal
    setRevision(8); await screen.findByText('No saved provider snapshot'); expect(signal.aborted).toBe(true)
    finish?.(response(cookie)); await Promise.resolve(); await Promise.resolve(); expect(screen.queryByText('12')).toBeNull()
  })
  it('never exposes transport errors or invents successful empty data', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('secret', { status: 503 }))); mount()
    await screen.findByRole('alert'); expect(screen.queryByText('secret')).toBeNull(); expect(screen.queryByText('0')).toBeNull()
  })
  it.each([
    ['six hours', '2026-10-23T15:59:59Z', '2026-10-23T10:00:00Z'],
    ['UTC window rollover', '2026-10-23T23:59:59Z', '2026-10-23T23:59:00Z'],
  ])('hides evidence at %s until saved status is refreshed', async (_label, current, observedAt) => {
    vi.useFakeTimers(); vi.setSystemTime(new Date(current))
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response({ ...cookie, observedAt }))); mount()
    await vi.advanceTimersByTimeAsync(0); expect(screen.getByText('12')).toBeTruthy()
    await vi.advanceTimersByTimeAsync(1002)
    expect(screen.queryByText('12')).toBeNull(); expect(screen.getByText(/saved snapshot has expired/)).toBeTruthy()
  })
})
