// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { PageSpeedPanel } from './PageSpeedPanel.tsx'

const site = 'https://aquatiq.com/'
const data = (status = 'idle') => ({ site, source: 'google_pagespeed', strategy: 'mobile', status })
const completed = { ...data('complete'), report: { performance: 89, accessibility: null, bestPractices: 100, seo: 90, lcpMs: 2500, cls: 0, tbtMs: 18, fcpMs: 1000, speedIndexMs: 2000, fetchTime: '2026-10-21T00:00:00Z', lighthouseVersion: '12.8.2', checks: [{ id: 'canonical', status: 'failed', score: 0 }] } }
const reply = (value: unknown) => new Response(JSON.stringify({ data: value }))
const mount = () => render(() => <I18nProvider><PageSpeedPanel site={site} /></I18nProvider>)
describe('PageSpeed panel', () => {
  it('updates an open dashboard when its daily saved report is due, without another provider command', async () => {
    vi.useFakeTimers();vi.setSystemTime(new Date('2026-10-21T00:00:00Z'))
    const cache = { stale: false, refreshStatus: 'complete', lastSuccessAt: completed.report.fetchTime, nextRefreshAt: '2026-10-22T00:00:00Z' }
    const newer = { ...completed, report: { ...completed.report, performance: 92, fetchTime: '2026-10-22T00:00:00Z' }, cache: { ...cache, lastSuccessAt: '2026-10-22T00:00:00Z', nextRefreshAt: '2026-10-23T00:00:00Z' } }
    const fetcher = vi.fn().mockResolvedValueOnce(reply({ ...completed, cache })).mockResolvedValueOnce(reply(newer))
    vi.stubGlobal('fetch', fetcher);mount();await vi.advanceTimersByTimeAsync(0)
    expect(screen.getByText('89')).toBeTruthy()
    await vi.advanceTimersByTimeAsync(86_399_999);expect(fetcher).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(5001);expect(screen.getByText('92')).toBeTruthy()
    expect(fetcher).toHaveBeenCalledTimes(2)
    expect(fetcher.mock.calls.every(call => call[1].method === 'GET')).toBe(true)
    cleanup();await vi.advanceTimersByTimeAsync(86_400_000);expect(fetcher).toHaveBeenCalledTimes(2)
  })
  it('shows technical review counts and opens their existing diagnostics without another request', async () => {
    const fetcher = vi.fn().mockResolvedValue(reply(completed))
    vi.stubGlobal('fetch', fetcher)
    const scroll = vi.fn()
    const old = HTMLElement.prototype.scrollIntoView
    HTMLElement.prototype.scrollIntoView = scroll
    try {
      const view = mount();await screen.findByText('89')
      const summary = screen.getByRole('group', { name: /Oppsummering av tekniske kontroller|Technical check summary/ })
      expect(summary.textContent).toMatch(/1/)
      const details = view.container.querySelector('details')!
      expect(details.open).toBe(false)
      fireEvent.click(screen.getByRole('button', { name: /Trenger vurdering.*Se kontroller|Needs review.*View checks/ }))
      expect(details.open).toBe(true)
      expect(scroll).toHaveBeenCalledTimes(1)
      expect(fetcher).toHaveBeenCalledTimes(1)
    } finally { HTMLElement.prototype.scrollIntoView = old }
  })
  it('serves saved scores while a daily refresh runs and polls only stored status', async () => {
    vi.useFakeTimers()
    const cache = { stale: true, refreshStatus: 'running', lastSuccessAt: completed.report.fetchTime, lastAttemptAt: '2026-10-22T00:00:00Z' }
    const fetcher = vi.fn().mockResolvedValueOnce(reply({ ...completed, cache })).mockResolvedValueOnce(reply({ ...completed, cache: { ...cache, refreshStatus: 'complete', stale: false } }))
    vi.stubGlobal('fetch', fetcher); mount()
    await vi.advanceTimersByTimeAsync(0)
    expect(screen.getByText('89')).toBeTruthy()
    expect(screen.getByTestId('pagespeed-cache-state').textContent).toMatch(/background|bakgrunnen/)
    expect(screen.getByRole('button', { name: /Kjør måling|Run audit/ }).hasAttribute('disabled')).toBe(true)
    await vi.advanceTimersByTimeAsync(5000)
    expect(screen.getByTestId('pagespeed-cache-state').textContent).toMatch(/hvert døgn|every day/)
    expect(fetcher.mock.calls.every(call => call[1].method === 'GET')).toBe(true)
  })
  it('keeps the saved report visible after a failed refresh or a transient status-read failure', async () => {
    const cache = { stale: true, refreshStatus: 'failed', refreshReason: 'quota_limited', lastSuccessAt: completed.report.fetchTime, nextRefreshAt: '2026-10-22T01:00:00Z' }
    const fetcher = vi.fn().mockResolvedValueOnce(reply({ ...completed, cache })).mockRejectedValueOnce(new Error('offline'))
    vi.stubGlobal('fetch', fetcher);mount()
    await screen.findByText('89')
    expect(screen.getByTestId('pagespeed-cache-state').textContent).toMatch(/mislyktes|failed/)
    fireEvent.click(screen.getByRole('button', { name: /Kjør måling|Run audit/ }))
    await screen.findByRole('alert')
    expect(screen.getByText('89')).toBeTruthy()
    expect(screen.getByRole('heading', { name: /Forsidens SEO-grunnlag|Homepage SEO foundations/ })).toBeTruthy()
  })
  it('waits for automatic preparation and reads the completed measurement without starting an audit', async () => {
    vi.useFakeTimers()
    const websiteId=`web_${'1'.repeat(32)}`;let reads=0
    const fetcher=vi.fn().mockImplementation((url:string)=>{
      if(url.includes('/preparation?'))return Promise.resolve(reply({websiteId,site,websiteRevision:2,source:'marketing_preparation',status:'queued',jobId:`prep_${'2'.repeat(32)}`,queuedAt:'2026-10-23T10:00:00Z',steps:['discovery','pagespeed_mobile','pagespeed_desktop'].map(key=>({key,status:'queued'}))}))
      reads++;return Promise.resolve(reply(reads===1?data():completed))
    });vi.stubGlobal('fetch',fetcher)
    render(()=><I18nProvider><PageSpeedPanel site={site} websiteId={websiteId} revision={2}/></I18nProvider>)
    await vi.advanceTimersByTimeAsync(0)
    expect(screen.getByRole('status').textContent).toMatch(/background|bakgrunnen/)
    await vi.advanceTimersByTimeAsync(5000)
    expect(screen.getByText('89')).toBeTruthy()
    expect(fetcher.mock.calls.every(call=>call[1].method==='GET')).toBe(true)
  })
  afterEach(() => { cleanup(); vi.unstubAllGlobals(); vi.useRealTimers(); window.localStorage.clear() })
  it('refreshes cached diagnostics after a task recheck without starting another audit', async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(reply(completed)).mockResolvedValueOnce(reply({ ...completed, report: { ...completed.report, performance: 92 } }))
    vi.stubGlobal('fetch', fetchMock)
    const [refreshKey, setRefreshKey] = createSignal(0)
    render(() => <I18nProvider><PageSpeedPanel site={site} refreshKey={refreshKey()} /></I18nProvider>)
    await screen.findByText('89')
    setRefreshKey(1)
    await screen.findByText('92')
    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(fetchMock.mock.calls.every((call) => call[1].method === 'GET')).toBe(true)
  })
  it('loads cached status only and never starts an audit on mount or device change', async () => {
    const fetchMock = vi.fn().mockImplementation(async (url: string) => reply({ ...data(), strategy: url.includes('desktop') ? 'desktop' : 'mobile' }))
    vi.stubGlobal('fetch', fetchMock)
    mount()
    await screen.findByText(/Ingen.*måling|No.*measurement/i)
    expect(fetchMock).toHaveBeenCalledTimes(1)
    fireEvent.change(screen.getByLabelText(/Enhet|Device/), { target: { value: 'desktop' } })
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2))
    expect(fetchMock.mock.calls.every((call) => call[1].method === 'GET')).toBe(true)
    expect(screen.queryByText('0')).toBeNull()
  })
  it('runs only after manual click and displays lab measurements separately from field CWV', async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(reply(data())).mockResolvedValueOnce(reply({ ...data('complete'), report: {
      performance: 89, accessibility: null, bestPractices: 100, seo: 90, lcpMs: 2500, cls: 0, tbtMs: 18, fcpMs: 1000, speedIndexMs: 2000, fetchTime: '2026-10-21T00:00:00Z', lighthouseVersion: '12.8.2',
    } }))
    vi.stubGlobal('fetch', fetchMock)
    mount()
    await screen.findByText(/Ingen.*måling|No.*measurement/i)
    fireEvent.click(screen.getByRole('button', { name: /Kjør måling|Run audit/ }))
    await screen.findByText('89')
    expect(fetchMock.mock.calls[1]?.[1].method).toBe('POST')
    expect(screen.getByText(/^TBT/)).toBeTruthy()
    expect(screen.getByText(/ikke.*INP|not.*INP/i)).toBeTruthy()
    expect(screen.getByText(/12.8.2/)).toBeTruthy()
    expect(screen.getByTestId('pagespeed-accessibility').textContent).toContain('—')
  })
  it('reads completed diagnostics and filters locally without consuming another audit', async () => {
    const fetchMock = vi.fn().mockResolvedValue(reply(completed))
    vi.stubGlobal('fetch', fetchMock)
    mount()
    await screen.findByRole('heading', { name: /Forsidens SEO-grunnlag|Homepage SEO foundations/ })
    expect(screen.getAllByRole('listitem')).toHaveLength(11)
    fireEvent.click(screen.getByRole('button', { name: /Trenger vurdering \(1\)|Needs review \(1\)/ }))
    expect(screen.getAllByRole('listitem')).toHaveLength(1)
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock.mock.calls[0]?.[1].method).toBe('GET')
  })
  it('saves only an actionable completed observation after explicit click', async () => {
    const websiteId = `web_${'1'.repeat(32)}`
    const saved = vi.fn()
    const fetchMock = vi.fn().mockResolvedValueOnce(reply(completed)).mockResolvedValueOnce(reply({ websiteId, site, tasks: [], activity: [], summary: { open: 0, inProgress: 0, done: 0, dismissed: 0, total: 0, completionPercent: null } }))
    vi.stubGlobal('fetch', fetchMock)
    render(() => <I18nProvider><PageSpeedPanel site={site} websiteId={websiteId} revision={2} onTaskSaved={saved} /></I18nProvider>)
    const save = await screen.findByRole('button', { name: /Lagre i oppgavelisten|Save to backlog/ })
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(screen.getAllByRole('button', { name: /Lagre i oppgavelisten|Save to backlog/ })).toHaveLength(1)
    fireEvent.click(save)
    await waitFor(() => expect(saved).toHaveBeenCalledTimes(1))
    expect(fetchMock.mock.calls[1]?.[0]).toBe('/api/v1/visibility/work/technical')
    expect(JSON.parse(String(fetchMock.mock.calls[1]?.[1].body))).toEqual({ websiteId, site, websiteRevision: 2, strategy: 'mobile', checkId: 'canonical', fetchTime: completed.report.fetchTime })
    expect(screen.getByRole('button', { name: /Lagret|Saved/ }).hasAttribute('disabled')).toBe(true)
  })
  it('explains quota errors without resetting Search Console or retrying automatically', async () => {
    const fetchMock = vi.fn().mockImplementation(async () => reply({ ...data('failed'), reason: 'quota_limited' }))
    vi.stubGlobal('fetch', fetchMock)
    mount()
    await screen.findByText(/kvote|quota/i)
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(screen.queryByText('0')).toBeNull()
    expect(screen.queryByRole('heading', { name: /Forsidens SEO-grunnlag|Homepage SEO foundations/ })).toBeNull()
  })

  it.each(['site', 'device', 'revision'])('clears diagnostics on %s changes and ignores superseded results', async (change) => {
    let resolveStale: ((value: Response) => void) | undefined
    const fetchMock = vi.fn().mockResolvedValueOnce(reply(completed)).mockReturnValueOnce(new Promise<Response>((resolve) => { resolveStale = resolve }))
      .mockImplementation(async (url: string) => { const query = new URL(url, 'http://localhost').searchParams; return reply({ ...data(), site: query.get('site'), strategy: query.get('strategy') }) })
    vi.stubGlobal('fetch', fetchMock)
    const [website, setWebsite] = createSignal(site)
    const [revision, setRevision] = createSignal(0)
    render(() => <I18nProvider><PageSpeedPanel site={website()} revision={revision()} /></I18nProvider>)
    await screen.findByRole('heading', { name: /Forsidens SEO-grunnlag|Homepage SEO foundations/ })
    if (change === 'site') setWebsite('https://example.com/')
    else if (change === 'device') fireEvent.change(screen.getByLabelText(/Enhet|Device/), { target: { value: 'desktop' } })
    else setRevision(1)
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2))
    expect(screen.queryByRole('heading', { name: /Forsidens SEO-grunnlag|Homepage SEO foundations/ })).toBeNull()
    setRevision(2)
    await screen.findByText(/Ingen.*måling|No.*measurement/i)
    expect(fetchMock.mock.calls[1]?.[1].signal.aborted).toBe(true)
    resolveStale?.(reply(completed))
    await waitFor(() => expect(screen.queryByRole('heading', { name: /Forsidens SEO-grunnlag|Homepage SEO foundations/ })).toBeNull())
    expect(fetchMock.mock.calls.every((call) => call[1].method === 'GET')).toBe(true)
  })
  it('clears stale site results and aborts outstanding requests when switching websites', async () => {
    let resolveOld: ((value: Response) => void) | undefined
    const fetchMock = vi.fn().mockReturnValueOnce(new Promise<Response>((resolve) => { resolveOld = resolve })).mockResolvedValueOnce(reply({ ...data(), site: 'https://example.com/' }))
    vi.stubGlobal('fetch', fetchMock)
    const [website, setWebsite] = createSignal(site)
    render(() => <I18nProvider><PageSpeedPanel site={website()} /></I18nProvider>)
    setWebsite('https://example.com/')
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2))
    expect(fetchMock.mock.calls[0]?.[1].signal.aborted).toBe(true)
    resolveOld?.(reply({ ...data('failed'), reason: 'quota_limited' }))
    await screen.findByText(/Ingen.*måling|No.*measurement/i)
    expect(screen.queryByText(/kvote|quota/i)).toBeNull()
  })

  it('bounds status polling to two minutes without automatic provider commands', async () => {
    vi.useFakeTimers()
    const fetchMock = vi.fn().mockImplementation(async () => reply(data('running')))
    vi.stubGlobal('fetch', fetchMock)
    mount()
    await vi.advanceTimersByTimeAsync(125_000)
    expect(screen.getByText(/fortsatt tid|still taking time/i)).toBeTruthy()
    const count = fetchMock.mock.calls.length
    expect(count).toBeGreaterThan(2)
    expect(count).toBeLessThanOrEqual(25)
    expect(fetchMock.mock.calls.every((call) => call[1].method === 'GET')).toBe(true)
    await vi.advanceTimersByTimeAsync(120_000)
    expect(fetchMock).toHaveBeenCalledTimes(count)
    cleanup()
    await vi.advanceTimersByTimeAsync(120_000)
    expect(fetchMock).toHaveBeenCalledTimes(count)
  })

  it('keeps network failures isolated and only refreshes cached status on retry', async () => {
    const fetchMock = vi.fn().mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce(reply(data()))
    vi.stubGlobal('fetch', fetchMock)
    mount()
    await screen.findByRole('alert')
    fireEvent.click(screen.getByRole('button', { name: /Oppdater status|Refresh status/ }))
    await screen.findByText(/Ingen.*måling|No.*measurement/i)
    expect(fetchMock.mock.calls.every((call) => call[1].method === 'GET')).toBe(true)
  })
})
