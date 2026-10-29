import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { AnalyticsSyncPanel } from './AnalyticsSyncPanel'
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }), localeDateTime: () => 'en-GB' }))
const context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 3, quarter: 'Q4' as const }
const idle = { ...context, source: 'marketing_analytics_sync', enabled: false, status: 'idle', steps: ['gsc', 'ga4', 'clarity'].map((key) => ({ key, status: 'queued' })) }
const queued = { ...idle, enabled: true, status: 'queued', jobId: `sync_${'2'.repeat(32)}`, queuedAt: '2026-10-23T10:00:00Z' }
const complete = { ...queued, status: 'complete', startedAt: '2026-10-23T10:00:01Z', completedAt: '2026-10-23T10:01:00Z', nextSyncAt: '2026-10-23T16:01:00Z', expiresAt: '2026-10-23T16:01:00Z', steps: idle.steps.map((step) => ({ ...step, status: 'connected', observedAt: '2026-10-23T10:01:00Z' })) }
const response = (data: unknown, status = 200) => new Response(JSON.stringify({ data }), { status })
const mount = (allowed = true, onSettled = vi.fn()) => render(() => <AnalyticsSyncPanel {...context} revision={3} canManageIntegrations={allowed} onSettled={onSettled} />)
describe('analytics sync UI', () => {
  afterEach(() => { cleanup(); vi.unstubAllGlobals(); vi.useRealTimers() })
  it('loads metadata without writes and explains source windows and bounded freshness', async () => {
    const fetcher = vi.fn().mockResolvedValue(response(idle)); vi.stubGlobal('fetch', fetcher); mount()
    await screen.findByText('Not collected')
    expect(fetcher).toHaveBeenCalledTimes(1)
    expect(fetcher.mock.calls[0]?.[1]?.method).toBe('GET')
    expect(screen.getByText(/Clarity keeps its own rolling 72-hour/)).toBeTruthy()
    expect(screen.getByText(/up to six hours/)).toBeTruthy()
  })
  it('starts collection only after explicit administrator action with revision and quarter', async () => {
    const fetcher = vi.fn().mockImplementation((_url, init) => Promise.resolve(response(init.method === 'POST' ? queued : idle, init.method === 'POST' ? 202 : 200)))
    vi.stubGlobal('fetch', fetcher); mount()
    const button = await screen.findByRole('button', { name: 'Collect now and enable background sync' })
    await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(button); await screen.findByText(queued.jobId)
    const writes = fetcher.mock.calls.filter((call) => call[1].method === 'POST')
    expect(writes).toHaveLength(1); expect(JSON.parse(writes[0]?.[1]?.body)).toEqual(context)
  })
  it('pauses future collection without claiming saved metrics or provider access were deleted', async () => {
    const fetcher = vi.fn().mockImplementation((_url, init) => Promise.resolve(response(init.method === 'PATCH' ? { ...complete, enabled: false, nextSyncAt: undefined } : complete)))
    vi.stubGlobal('fetch', fetcher); const settled = vi.fn(); mount(true, settled)
    fireEvent.click(await screen.findByRole('button', { name: 'Pause background sync' }))
    await screen.findByText(/Background sync paused/)
    const write = fetcher.mock.calls.find((call) => call[1].method === 'PATCH')
    expect(JSON.parse(write?.[1]?.body)).toEqual({ ...context, enabled: false })
    expect(screen.getByText(/Pausing does not delete saved snapshots/)).toBeTruthy()
    expect(settled).not.toHaveBeenCalled()
  })
  it('shows no mutation controls to read-only actors and no raw server errors', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('private token', { status: 403 }))); mount(false)
    await screen.findByRole('alert')
    expect(screen.queryByRole('button', { name: /Collect now|Pause background|Resume background/ })).toBeNull()
    expect(screen.queryByText('private token')).toBeNull()
  })
  it('polls active jobs with GET then notifies once; initial saved terminal never notifies', async () => {
    vi.useFakeTimers(); const settled = vi.fn(); let reads = 0
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(response(++reads === 1 ? queued : complete)))
    vi.stubGlobal('fetch', fetcher); mount(true, settled)
    await vi.advanceTimersByTimeAsync(0); expect(reads).toBe(1)
    await vi.advanceTimersByTimeAsync(2000); expect(reads).toBe(2); expect(settled).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(10000); expect(reads).toBe(2)
    fireEvent.click(screen.getByRole('button', { name: 'Refresh collection status' })); await vi.advanceTimersByTimeAsync(0)
    expect(settled).toHaveBeenCalledTimes(1)
    expect(fetcher.mock.calls.every((call) => call[1].method === 'GET')).toBe(true)
  })
  it('observes a scheduled completion with metadata-only polling and refreshes reports once', async () => {
    vi.useFakeTimers(); const settled = vi.fn(); let reads = 0
    const next = { ...complete, jobId: `sync_${'3'.repeat(32)}` }
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(response(++reads === 1 ? complete : next)))
    vi.stubGlobal('fetch', fetcher); mount(true, settled)
    await vi.advanceTimersByTimeAsync(0); expect(settled).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(60000); expect(settled).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(60000); expect(settled).toHaveBeenCalledTimes(1)
    expect(fetcher.mock.calls.every((call) => call[1].method === 'GET')).toBe(true)
  })
  it('bounds active polling and offers a GET-only status refresh', async () => {
    vi.useFakeTimers(); const fetcher = vi.fn().mockImplementation(() => Promise.resolve(response(queued))); vi.stubGlobal('fetch', fetcher); mount()
    await vi.advanceTimersByTimeAsync(180000)
    expect(fetcher).toHaveBeenCalledTimes(91)
    expect(screen.getByText(/Automatic status refresh stopped/)).toBeTruthy()
    await vi.advanceTimersByTimeAsync(10000); expect(fetcher).toHaveBeenCalledTimes(91)
  })
  it('shows last success, stale state and reconnect alongside failure without any provider write', async () => {
    const failed = { ...complete, status: 'failed', expiresAt: undefined, steps: idle.steps.map((s) => ({ ...s, status: 'unavailable', reason: 'authorization_required', observedAt: complete.completedAt, lastSuccessAt: '2026-10-22T10:00:00Z' })) }
    const fetcher = vi.fn().mockResolvedValue(response(failed)); vi.stubGlobal('fetch', fetcher); mount()
    await screen.findByText('No sources available')
    expect(screen.getAllByText(/Last successful sync:.*22 Oct 2026/)).toHaveLength(3)
    expect(screen.getAllByText('Stale data · collection needed')).toHaveLength(3)
    expect(screen.getAllByRole('link', { name: /Manage connection/ }).every((link) => link.getAttribute('href') === '/settings/integrations')).toBe(true)
    expect(fetcher).toHaveBeenCalledTimes(1)
  })
  it('aborts and rejects late writes after role loss or website context change', async () => {
    let resolveWrite: ((value: Response) => void) | undefined
    const fetcher = vi.fn().mockImplementation((_url, init) => init.method === 'POST' ? new Promise<Response>((resolve) => { resolveWrite = resolve }) : Promise.resolve(response(idle)))
    vi.stubGlobal('fetch', fetcher); const [allowed, setAllowed] = createSignal(true)
    render(() => <AnalyticsSyncPanel {...context} revision={3} canManageIntegrations={allowed()} />)
    await screen.findByText('Not collected')
    fireEvent.click(screen.getByRole('button', { name: 'Collect now and enable background sync' }))
    await waitFor(() => expect(fetcher.mock.calls.some((call) => call[1].method === 'POST')).toBe(true))
    const writeSignal = fetcher.mock.calls.find((call) => call[1].method === 'POST')?.[1]?.signal as AbortSignal
    setAllowed(false); await screen.findByText(/An administrator can configure/)
    expect(writeSignal.aborted).toBe(true)
    resolveWrite?.(response(queued, 202)); await Promise.resolve(); await Promise.resolve()
    expect(screen.queryByText(queued.jobId)).toBeNull()
  })
})
