import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { WebVitalsPanel } from './WebVitalsPanel'
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }), localeDateTime: () => 'en-GB' }))
const context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 7 }
const empty = { count: 0, p75: null }
const group = (viewport: string) => ({ viewport, metrics: { lcp: empty, inp: empty, cls: empty } })
const base = { ...context, source: 'first_party_web_vitals', environment: 'production', status: 'awaiting_samples', reportingOrigin: 'https://www.triodelab.no', window: { startDate: '2026-10-17', endDate: '2026-10-23' }, asOf: '2026-10-23T10:00:00Z', retentionDays: 7, sampleCount: 0, groups: [group('narrow'), group('wide')] }
const response = (data: unknown) => new Response(JSON.stringify({ data }))
const mount = () => render(() => <WebVitalsPanel websiteId={context.websiteId} site={context.site} revision={7} />)
describe('first-party vitals panel', () => {
  afterEach(() => { cleanup(); vi.unstubAllGlobals() })
  it('shows measured p75 and sample limits without implying CrUX coverage or visitor counts', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response({ ...base, status: 'connected', sampleCount: 2, groups: [{ viewport: 'narrow', metrics: { lcp: { count: 1, p75: 1200 }, inp: empty, cls: { count: 1, p75: 0 } } }, group('wide')] }))); mount()
    await screen.findByText('Browser measurements available')
    expect(screen.getByTestId('vitals-narrow-cls').textContent).toBe('0')
    expect(screen.getByTestId('vitals-narrow-inp').textContent).toBe('—')
    expect(screen.getAllByText('Limited sample: fewer than 20 measurements')).toHaveLength(2)
    expect(screen.getByText(/not visitors or pageviews/)).toBeTruthy(); expect(screen.getByText(/not CrUX or Lighthouse/)).toBeTruthy()
    expect(screen.getByText(/viewport width.*not physical device/)).toBeTruthy()
  })
  it('defaults to production, explains awaiting observations and refreshes using GET only', async () => {
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(response(base))); vi.stubGlobal('fetch', fetcher); mount()
    await screen.findByText('Waiting for consented browser measurements')
    expect(screen.getByLabelText('Measurement environment').getAttribute('value')).not.toBe('development')
    expect(screen.getByText(/0 metric measurements$/)).toBeTruthy(); fireEvent.click(screen.getByRole('button', { name: 'Refresh browser measurements' }))
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(2)); expect(fetcher.mock.calls.every((call) => call[1].method === 'GET')).toBe(true)
  })
  it('keeps development separate and labelled as localhost tests', async () => {
    const fetcher = vi.fn().mockImplementation((url: string) => Promise.resolve(response(url.includes('development') ? { ...base, environment: 'development', reportingOrigin: 'http://localhost:3000' } : base))); vi.stubGlobal('fetch', fetcher); mount()
    await screen.findByText('Waiting for consented browser measurements')
    fireEvent.change(screen.getByLabelText('Measurement environment'), { target: { value: 'development' } })
    await screen.findByText('http://localhost:3000'); expect(screen.getByText(/Development.*localhost.*not production traffic/)).toBeTruthy()
  })
  it('shows configuration and transport failures as unknown, not zero performance', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response({ ...base, status: 'not_configured', reportingOrigin: undefined }))); mount()
    await screen.findByText(/server-side measurement export is not configured/)
    cleanup(); vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('secret', { status: 503 }))); mount()
    await screen.findByRole('alert'); expect(screen.queryByText('secret')).toBeNull()
  })
  it('aborts old revisions and discards late results', async () => {
    let finish: ((value: Response) => void) | undefined
    const fetcher = vi.fn().mockImplementationOnce(() => new Promise<Response>((resolve) => { finish = resolve })).mockImplementation(() => Promise.resolve(response({ ...base, websiteRevision: 8 }))); vi.stubGlobal('fetch', fetcher)
    const [revision, setRevision] = createSignal(7)
    render(() => <WebVitalsPanel websiteId={context.websiteId} site={context.site} revision={revision()} />)
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(1)); const signal = fetcher.mock.calls[0]![1].signal as AbortSignal
    setRevision(8); await screen.findByText('Waiting for consented browser measurements'); expect(signal.aborted).toBe(true)
    finish?.(response({ ...base, status: 'connected', sampleCount: 1, groups: [{ viewport: 'narrow', metrics: { lcp: { count: 1, p75: 9876 }, inp: empty, cls: empty } }, group('wide')] }))
    await Promise.resolve(); await Promise.resolve(); expect(screen.queryByText('9,876')).toBeNull()
  })
  it('imports only after administrator action and aborts import on role loss', async () => {
    let finish: ((value: Response) => void) | undefined
    const fetcher = vi.fn().mockImplementation((_url: string, init: RequestInit) => init.method === 'POST' ? new Promise<Response>((resolve) => { finish = resolve }) : Promise.resolve(response(base))); vi.stubGlobal('fetch', fetcher)
    const [allowed, setAllowed] = createSignal(true)
    render(() => <WebVitalsPanel websiteId={context.websiteId} site={context.site} revision={7} canManageIntegrations={allowed()} />)
    await screen.findByText('Waiting for consented browser measurements'); expect(fetcher).toHaveBeenCalledTimes(1)
    fireEvent.click(screen.getByRole('button', { name: 'Import from website' }))
    await screen.findByText('Importing website measurements…')
    const post = fetcher.mock.calls.find((call) => call[1].method === 'POST')!
    expect(post[0]).toBe('/api/v1/visibility/web-vitals/sync'); expect(JSON.parse(post[1].body as string)).toEqual({ ...context, environment: 'production' })
    setAllowed(false); await waitFor(() => expect((post[1].signal as AbortSignal).aborted).toBe(true))
    expect(screen.queryByRole('button', { name: 'Import from website' })).toBeNull()
    finish?.(response({ ...base, status: 'connected', sampleCount: 1, groups: [{ viewport: 'narrow', metrics: { lcp: { count: 1, p75: 9876 }, inp: empty, cls: empty } }, group('wide')] }))
    await Promise.resolve(); await Promise.resolve(); expect(screen.queryByText('9,876')).toBeNull()
  })
})
