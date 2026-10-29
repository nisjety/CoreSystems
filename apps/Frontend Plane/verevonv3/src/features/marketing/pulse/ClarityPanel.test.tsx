// @vitest-environment jsdom
import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { ClarityPanel } from './ClarityPanel.tsx'
const site = 'https://example.org/'
const websiteId = 'web_' + '1'.repeat(32)
const snapshot = { site, source: 'microsoft_clarity', scope: 'clarity_project', status: 'connected', projectId: 'project123', window: { numOfDays: 3, timeZone: 'UTC', observedAt: '2026-10-21T12:00:00Z' }, metrics: { sessions: 12, botSessions: 0, users: null, pagesPerSession: 1.5 } }
const json = (data: unknown) => new Response(JSON.stringify({ data }))
describe('Clarity panel', () => {
  it('shows measured UX and authenticated project links without claiming embedded replays', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(json({ ...snapshot, metrics: { ...snapshot.metrics, ux: { averageScrollDepth: 30.27, activeSeconds: 37, totalSeconds: 59, deadClickPercent: 0 } } })))
    render(() => <I18nProvider><ClarityPanel site={site} websiteId={websiteId} revision={1} /></I18nProvider>)
    await waitFor(() => expect(screen.getByTestId('clarity-activeSeconds').textContent).toBe('37 s'))
    expect(screen.getByTestId('clarity-deadClickPercent').textContent).toBe('0 %')
    expect(screen.getByTestId('clarity-rageClickPercent').textContent).toBe('—')
    const recordings = screen.getByRole('link', { name: /recordings|øktopptak/i })
    expect(recordings.getAttribute('href')).toBe('https://clarity.microsoft.com/projects/view/project123/impressions?date=Last%203%20days')
    expect(recordings.getAttribute('rel')).toBe('noopener noreferrer')
    expect(screen.getByRole('link', { name: /heatmaps|varmekart/i }).getAttribute('href')).toContain('/project123/heatmaps?')
    expect(screen.getByText(/not embedded|ikke bygget inn/i)).toBeTruthy()
  })
  afterEach(() => { cleanup(); vi.unstubAllGlobals() })
  it('refreshes saved exports when collection settles without adding a quarter to Clarity', async () => {
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(json({ ...snapshot, status: 'unavailable', reason: 'snapshot_expired', metrics: null }))); vi.stubGlobal('fetch', fetcher)
    const [refresh, setRefresh] = createSignal(0)
    render(() => <I18nProvider><ClarityPanel site={site} websiteId={websiteId} revision={1} refreshKey={refresh()} /></I18nProvider>)
    await waitFor(() => expect(screen.getByRole('status').textContent).toMatch(/snapshot|innhenting/i))
    fetcher.mockImplementation(() => Promise.resolve(json(snapshot))); setRefresh(1)
    await waitFor(() => expect(screen.getByTestId('clarity-sessions').textContent).toBe('12'))
    expect(fetcher).toHaveBeenCalledTimes(2)
    expect(fetcher.mock.calls.every((call) => !String(call[0]).includes('quarter='))).toBe(true)
  })
  it('labels the fixed recent project-wide window, preserves zero and does not poll', async () => {
    const fetcher = vi.fn().mockResolvedValue(json(snapshot)); vi.stubGlobal('fetch', fetcher)
    render(() => <I18nProvider><ClarityPanel site={site} websiteId={websiteId} revision={1} /></I18nProvider>)
    await waitFor(() => expect(screen.getByTestId('clarity-sessions').textContent).toBe('12'))
    expect(screen.getByTestId('clarity-botSessions').textContent).toBe('0')
    expect(screen.getByTestId('clarity-users').textContent).toBe('—')
    expect(screen.getByTestId('clarity-window').textContent).toMatch(/72.*UTC/i)
    expect(screen.getByText(/whole.*project|hele.*prosjekt/i)).toBeTruthy()
    expect(screen.getByText(/not.*quarter|ikke.*kvartal/i)).toBeTruthy()
    await new Promise((resolve) => setTimeout(resolve, 20)); expect(fetcher).toHaveBeenCalledOnce()
    expect(fetcher.mock.calls[0]?.[0]).not.toContain('quarter=')
  })
  it('clears old project data and rejects late responses after the selected website changes', async () => {
    let finishOld: ((value: Response) => void) | undefined
    const fetcher = vi.fn().mockImplementationOnce(() => new Promise<Response>((resolve) => { finishOld = resolve })).mockResolvedValueOnce(json({ ...snapshot, site: 'https://other.example/', projectId: 'other123', metrics: { ...snapshot.metrics, sessions: 4 } }))
    vi.stubGlobal('fetch', fetcher)
    const [context, setContext] = createSignal({ site, websiteId, revision: 1 })
    render(() => <I18nProvider><ClarityPanel {...context()} /></I18nProvider>)
    await waitFor(() => expect(fetcher).toHaveBeenCalledOnce())
    setContext({ site: 'https://other.example/', websiteId: 'web_' + '2'.repeat(32), revision: 2 })
    await waitFor(() => expect(screen.getByTestId('clarity-sessions').textContent).toBe('4'))
    finishOld?.(json(snapshot)); await new Promise((resolve) => setTimeout(resolve, 0))
    expect(screen.getByTestId('clarity-sessions').textContent).toBe('4')
    expect(screen.queryByText('project123')).toBeNull()
  })
  it('shows a quota-limited unavailable state without made-up UX or recording links', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(json({ ...snapshot, status: 'unavailable', reason: 'quota_limited', metrics: null })))
    render(() => <I18nProvider><ClarityPanel site={site} websiteId={websiteId} revision={1} /></I18nProvider>)
    await waitFor(() => expect(screen.getByRole('status').textContent).toMatch(/quota|kvote/i))
    expect(screen.getByTestId('clarity-sessions').textContent).toBe('—')
    expect(screen.queryByRole('link')).toBeNull()
    expect(screen.queryByRole('textbox')).toBeNull()
  })
})
