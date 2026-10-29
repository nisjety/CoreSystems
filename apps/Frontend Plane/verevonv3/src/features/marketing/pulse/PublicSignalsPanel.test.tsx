import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { PublicSignalsPanel } from './PublicSignalsPanel'
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }), localeDateTime: () => 'en-GB' }))
const context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 3 }
const idle = { ...context, source: 'marketing_public_signals', enabled: false, status: 'idle' }
const queued = { ...idle, enabled: true, status: 'queued', jobId: `public_${'2'.repeat(32)}`, queuedAt: '2026-10-23T10:00:00Z' }
const report = { site: context.site, source: 'public_web_signals', observedAt: '2026-10-23T10:01:00Z', crux: [{ formFactor: 'PHONE', status: 'connected', period: { startDate: '2026-09-24', endDate: '2026-10-21' }, metrics: { lcpMs: 2200, inpMs: null, cls: 0 } }, { formFactor: 'DESKTOP', status: 'unavailable', reason: 'not_configured' }], commonCrawl: { status: 'connected', index: 'CC-MAIN-2026-43', captures: [{ url: 'https://triodelab.no/innsikt', capturedAt: '2026-10-20T10:01:00Z', httpStatus: 200 }], limit: 20, truncated: true } }
const complete = { ...queued, status: 'complete', startedAt: '2026-10-23T10:00:01Z', completedAt: '2026-10-23T10:01:00Z', expiresAt: '2026-10-24T10:01:00Z', nextSyncAt: '2026-10-24T10:01:00Z', report }
const response = (data: unknown, status = 200) => new Response(JSON.stringify({ data }), { status })
const mount = (allowed = true) => render(() => <PublicSignalsPanel {...context} revision={3} canManageIntegrations={allowed} />)
describe('public signals UI', () => {
  it('shows the source host for automatically discovered archive records', async () => {
    vi.useFakeTimers({toFake:['Date']});vi.setSystemTime(new Date('2026-10-23T11:00:00Z'))
    const archive={...report.commonCrawl,reportingHost:'www.triodelab.no',captures:report.commonCrawl.captures.map(v=>({...v,url:v.url.replace('triodelab.no','www.triodelab.no')}))}
    vi.stubGlobal('fetch',vi.fn().mockResolvedValue(response({...complete,report:{...report,commonCrawl:archive}})));mount()
    await screen.findByText('Public checks finished')
    expect(screen.getByTestId('commoncrawl-reporting-host').textContent).toContain('www.triodelab.no')
    expect(screen.getByTestId('commoncrawl-reporting-host').textContent).toContain('found automatically')
  })
  it('labels the automatically discovered origin without replacing the selected website', async () => {
    vi.useFakeTimers({ toFake: ['Date'] }); vi.setSystemTime(new Date('2026-10-23T11:00:00Z'))
    const discovered = { ...report, crux: [{ ...report.crux[0], reportingOrigin: 'https://www.triodelab.no' }, report.crux[1]] }
    const fetcher = vi.fn().mockResolvedValue(response({ ...complete, report: discovered })); vi.stubGlobal('fetch', fetcher); mount()
    await screen.findByText('Public checks finished')
    expect(screen.getByTestId('crux-PHONE-origin').textContent).toContain('https://www.triodelab.no')
    expect(screen.getByTestId('crux-PHONE-origin').textContent).toContain('found automatically')
    expect(screen.getByText(/exact www or non-www counterpart is checked automatically/)).toBeTruthy()
    expect(String(fetcher.mock.calls[0]?.[0])).toContain(encodeURIComponent(context.site))
    expect(fetcher).toHaveBeenCalledTimes(1)
  })
  afterEach(() => { cleanup(); vi.unstubAllGlobals(); vi.useRealTimers() })
  it('distinguishes CrUX missing coverage from an upstream archive timeout', async () => {
    vi.useFakeTimers({ toFake: ['Date'] }); vi.setSystemTime(new Date('2026-10-23T11:00:00Z'))
    const coverageReport = { ...report, crux: report.crux.map((device) => ({ formFactor: device.formFactor, status: 'unavailable', reason: 'no_data' })), commonCrawl: { status: 'unavailable', reason: 'timeout', index: 'CC-MAIN-2026-43', captures: [], limit: 20, truncated: false } }
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response({ ...complete, report: coverageReport }))); mount()
    await screen.findByText('Public checks finished')
    expect(screen.getAllByText(/Google has no public CrUX report for this origin and device/)).toHaveLength(2)
    expect(screen.getByText(/Connecting Analytics or Clarity does not create CrUX coverage/)).toBeTruthy()
    expect(screen.getByText(/The provider timed out.*not evidence of no data/)).toBeTruthy()
    expect(screen.getByText('CC-MAIN-2026-43')).toBeTruthy()
    expect(screen.queryByText(/deployment API key/)).toBeNull()
    expect(screen.queryByTestId('crux-PHONE-cls')).toBeNull()
  })
  it('reads saved public signals without collection and labels real field evidence and archive limits', async () => {
    vi.useFakeTimers({ toFake: ['Date'] }); vi.setSystemTime(new Date('2026-10-23T11:00:00Z'))
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(response(complete))); vi.stubGlobal('fetch', fetcher); mount()
    await screen.findByText('Public checks finished')
    expect(fetcher).toHaveBeenCalledTimes(1); expect(fetcher.mock.calls[0]?.[1].method).toBe('GET')
    expect(screen.getByTestId('crux-PHONE-inpMs').textContent).toBe('—')
    expect(screen.getByTestId('crux-PHONE-cls').textContent).toBe('0')
    expect(screen.getByText(/28-day.*75th percentile/)).toBeTruthy()
    expect(screen.getByText(/deployment API key.*not.*sign-in/i)).toBeTruthy()
    expect(screen.getByText(/not.*backlink.*current.*404/i)).toBeTruthy()
    expect(screen.getByText(/does not prove.*AI training|does not prove.*AI.*visibility/i)).toBeTruthy()
    const link = screen.getByRole('link', { name: 'https://triodelab.no/innsikt' })
    expect(link.getAttribute('rel')).toBe('noopener noreferrer'); expect(link.getAttribute('target')).toBe('_blank')
    expect(screen.queryByText(/Core Web Vitals passed/i)).toBeNull()
  })
  it('keeps labelled prior evidence visible during a queued refresh', async () => {
    vi.useFakeTimers({ toFake: ['Date'] }); vi.setSystemTime(new Date('2026-10-24T11:00:00Z'))
    const previous = { ...report, observedAt: '2026-10-23T10:01:00Z' }
    const pending = { ...queued, queuedAt: '2026-10-24T11:00:00Z', cachedReport: previous }
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response(pending))); mount()
    await screen.findByTestId('public-signals-cached')
    expect(screen.getByTestId('crux-PHONE-cls').textContent).toBe('0')
    expect(screen.getByText(/previous evidence, not a new successful check/)).toBeTruthy()
  })
  it('starts collection and enrollment only after an administrator click', async () => {
    const fetcher = vi.fn().mockImplementation((_url, init) => Promise.resolve(response(init.method === 'POST' ? queued : idle, init.method === 'POST' ? 202 : 200))); vi.stubGlobal('fetch', fetcher); mount()
    await screen.findByText('No public report yet')
    fireEvent.click(screen.getByRole('button', { name: 'Collect public signals and enable daily sync' }))
    await screen.findByText(queued.jobId)
    expect(JSON.parse(fetcher.mock.calls.find((call) => call[1].method === 'POST')?.[1].body)).toEqual(context)
  })
  it('allows viewers to read but not configure or collect', async () => {
    vi.stubGlobal('fetch', vi.fn().mockImplementation(() => Promise.resolve(response(idle)))); mount(false)
    await screen.findByText('No public report yet')
    expect(screen.queryByRole('button', { name: /Collect public|Pause daily|Resume daily/ })).toBeNull()
    expect(screen.getByText(/An administrator can configure/)).toBeTruthy()
  })
  it('pauses future jobs explicitly without claiming reports were deleted', async () => {
    vi.useFakeTimers({ toFake: ['Date'] }); vi.setSystemTime(new Date('2026-10-23T11:00:00Z'))
    const fetcher = vi.fn().mockImplementation((_url, init) => Promise.resolve(response(init.method === 'PATCH' ? { ...complete, enabled: false, nextSyncAt: undefined } : complete))); vi.stubGlobal('fetch', fetcher); mount()
    fireEvent.click(await screen.findByRole('button', { name: 'Pause daily sync' }))
    await screen.findByRole('button', { name: 'Resume daily sync' })
    expect(JSON.parse(fetcher.mock.calls.find((call) => call[1].method === 'PATCH')?.[1].body)).toEqual({ ...context, enabled: false })
    expect(screen.getByTestId('crux-PHONE-cls').textContent).toBe('0')
    expect(screen.getByText(/does not delete saved reports/)).toBeTruthy()
  })
  it('polls active jobs with GET only and stops when checks finish', async () => {
    vi.useFakeTimers(); vi.setSystemTime(new Date('2026-10-23T11:00:00Z')); let reads = 0
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(response(++reads === 1 ? queued : complete))); vi.stubGlobal('fetch', fetcher); mount()
    await vi.advanceTimersByTimeAsync(2000)
    expect(reads).toBe(2); expect(screen.getByText('Public checks finished')).toBeTruthy()
    await vi.advanceTimersByTimeAsync(10000); expect(reads).toBe(2)
    expect(fetcher.mock.calls.every((call) => call[1].method === 'GET')).toBe(true)
  })
  it('bounds active polling and can refresh status without a collection write', async () => {
    vi.useFakeTimers(); const fetcher = vi.fn().mockImplementation(() => Promise.resolve(response(queued))); vi.stubGlobal('fetch', fetcher); mount()
    await vi.advanceTimersByTimeAsync(180000); expect(fetcher).toHaveBeenCalledTimes(91)
    expect(screen.getByText(/Automatic status refresh stopped/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Refresh public signal status' })); await vi.advanceTimersByTimeAsync(0)
    expect(fetcher).toHaveBeenCalledTimes(92); expect(fetcher.mock.calls.every((call) => call[1].method === 'GET')).toBe(true)
  })
  it('aborts in-flight work on role loss and discards late responses', async () => {
    let finish: ((value: Response) => void) | undefined
    const fetcher = vi.fn().mockImplementation((_url, init) => init.method === 'POST' ? new Promise<Response>((resolve) => { finish = resolve }) : Promise.resolve(response(idle))); vi.stubGlobal('fetch', fetcher)
    const [allowed, setAllowed] = createSignal(true)
    render(() => <PublicSignalsPanel {...context} revision={3} canManageIntegrations={allowed()} />)
    await screen.findByText('No public report yet'); fireEvent.click(screen.getByRole('button', { name: 'Collect public signals and enable daily sync' }))
    await waitFor(() => expect(fetcher.mock.calls.some((call) => call[1].method === 'POST')).toBe(true))
    const signal = fetcher.mock.calls.find((call) => call[1].method === 'POST')?.[1].signal as AbortSignal
    setAllowed(false); await screen.findByText(/An administrator can configure/); expect(signal.aborted).toBe(true)
    finish?.(response(queued, 202)); await Promise.resolve(); await Promise.resolve(); expect(screen.queryByText(queued.jobId)).toBeNull()
  })
  it('aborts old revision reads and never shows a late report for changed website bindings', async () => {
    let finish: ((value: Response) => void) | undefined
    const fetcher = vi.fn().mockImplementationOnce(() => new Promise<Response>((resolve) => { finish = resolve })).mockImplementation(() => Promise.resolve(response({ ...idle, websiteRevision: 4 }))); vi.stubGlobal('fetch', fetcher)
    const [revision, setRevision] = createSignal(3)
    render(() => <PublicSignalsPanel {...context} revision={revision()} canManageIntegrations />)
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(1))
    const signal = fetcher.mock.calls[0]?.[1].signal as AbortSignal
    setRevision(4); await screen.findByText('No public report yet'); expect(signal.aborted).toBe(true)
    finish?.(response(complete)); await Promise.resolve(); await Promise.resolve()
    expect(screen.queryByTestId('crux-PHONE-cls')).toBeNull()
  })
  it('hides a report at its expiry while preserving the historical receipt', async () => {
    vi.useFakeTimers(); vi.setSystemTime(new Date('2026-10-24T10:00:59Z'))
    vi.stubGlobal('fetch', vi.fn().mockImplementation(() => Promise.resolve(response(complete)))); mount()
    await vi.advanceTimersByTimeAsync(0); expect(screen.getByTestId('crux-PHONE-cls').textContent).toBe('0')
    await vi.advanceTimersByTimeAsync(1002)
    expect(screen.queryByTestId('crux-PHONE-cls')).toBeNull()
    expect(screen.getByText(complete.jobId)).toBeTruthy()
    expect(screen.getByText(/saved public report is unavailable or expired/)).toBeTruthy()
  })
  it('treats missing expired reports and transport failures as unknown, never healthy', async () => {
    vi.stubGlobal('fetch', vi.fn().mockImplementation(() => Promise.resolve(response({ ...complete, report: undefined })))); mount()
    await screen.findByText(/saved public report is unavailable or expired/)
    expect(screen.queryByTestId('crux-PHONE-cls')).toBeNull()
    cleanup(); vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('secret token', { status: 503 }))); mount()
    await screen.findByRole('alert'); expect(screen.queryByText('secret token')).toBeNull()
  })
})
