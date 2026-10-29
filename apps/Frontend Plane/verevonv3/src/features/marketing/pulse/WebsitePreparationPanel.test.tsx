import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { WebsitePreparationPanel } from './WebsitePreparationPanel'
import { sourceKeys } from '@/shared/api/marketing/preparation-client'
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }), localeDateTime: () => 'en-GB' }))
const context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 3 }
const steps = ['discovery', 'pagespeed_mobile', 'pagespeed_desktop'].map((key) => ({ key, status: 'queued' }))
const idle = { ...context, source: 'marketing_preparation', status: 'idle', steps }
const queued = { ...idle, status: 'queued', jobId: `prep_${'2'.repeat(32)}`, queuedAt: '2026-10-23T10:00:00Z' }
const catalog = { ...context, source: 'marketing_source_catalog', items: sourceKeys.map((key) => ({ key, implemented: false, mode: 'oauth', status: 'not_implemented', refresh: 'not_available' })) }
const response = (data: unknown, status = 200) => new Response(JSON.stringify({ data }), { status })
const mount = (allowed = true) => render(() => <WebsitePreparationPanel {...context} revision={3} canManageIntegrations={allowed} />)
describe('website preparation UI', () => {
  afterEach(() => { cleanup(); vi.unstubAllGlobals(); vi.useRealTimers() })
  it('reads receipts and sources without starting work and labels planned sources truthfully', async () => {
    const fetcher = vi.fn().mockImplementation((url) => Promise.resolve(response(String(url).includes('/sources?') ? catalog : idle)))
    vi.stubGlobal('fetch', fetcher); mount()
    await screen.findByText('Not prepared')
    expect(fetcher).toHaveBeenCalledTimes(2)
    expect(fetcher.mock.calls.every((call) => call[1].method === 'GET')).toBe(true)
    expect(screen.getAllByText('Planned—not collecting yet')).toHaveLength(sourceKeys.length)
    expect(screen.getByText(/does not mean private account data is ready/)).toBeTruthy()
  })
  it('only prepares after explicit administrator action and sends selected revision', async () => {
    const fetcher = vi.fn().mockImplementation((url, init) => Promise.resolve(response(String(url).includes('/sources?') ? catalog : init.method === 'POST' ? queued : idle, init.method === 'POST' ? 202 : 200)))
    vi.stubGlobal('fetch', fetcher); mount()
    const button = await screen.findByRole('button', { name: 'Prepare public checks' })
    await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(button)
    await screen.findByText(queued.jobId)
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Starting…' })).toBeNull())
    const writes = fetcher.mock.calls.filter((call) => call[1].method === 'POST')
    expect(writes).toHaveLength(1)
    expect(JSON.parse(writes[0]?.[1]?.body ?? '{}')).toEqual(context)
    expect((screen.getByRole('button', { name: 'Prepare public checks' }) as HTMLButtonElement).disabled).toBe(true)
  })
  it('does not expose writes to read-only actors', async () => {
    vi.stubGlobal('fetch', vi.fn().mockImplementation((url) => Promise.resolve(response(String(url).includes('/sources?') ? catalog : idle))))
    mount(false); await screen.findByText('Not prepared')
    expect(screen.queryByRole('button', { name: 'Prepare public checks' })).toBeNull()
    expect(screen.getByText(/An administrator can prepare/)).toBeTruthy()
  })
  it('clears and aborts old context requests when revision changes', async () => {
    let resolveOld: ((v: Response) => void) | undefined
    const fetcher = vi.fn().mockImplementation((url) => String(url).includes('/sources?') ? Promise.resolve(response(catalog)) : new Promise<Response>((resolve) => { resolveOld = resolve }))
    vi.stubGlobal('fetch', fetcher)
    const [revision, setRevision] = createSignal(3)
    const view = render(() => <WebsitePreparationPanel {...context} revision={revision()} />)
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(2))
    const signal = fetcher.mock.calls[0]?.[1]?.signal as AbortSignal
    fetcher.mockImplementation((url) => Promise.resolve(response(String(url).includes('/sources?') ? { ...catalog, websiteRevision: 4 } : { ...idle, websiteRevision: 4 })))
    setRevision(4)
    await screen.findByText('Not prepared')
    expect(signal.aborted).toBe(true)
    resolveOld?.(response(queued)); await Promise.resolve(); await Promise.resolve()
    expect(view.container.textContent).not.toContain(queued.jobId)
  })
  it('fails closed without displaying raw server errors or claiming completed setup', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ token: 'private' }), { status: 500 })))
    mount(); await screen.findAllByRole('alert')
    expect(screen.queryByText('private')).toBeNull()
    expect((screen.getByRole('button', { name: 'Prepare public checks' }) as HTMLButtonElement).disabled).toBe(true)
  })
  it('polls active receipts using GET only and stops at a terminal result', async () => {
    vi.useFakeTimers()
    const complete = { ...queued, status: 'complete', startedAt: '2026-10-23T10:00:01Z', completedAt: '2026-10-23T10:01:00Z', steps: steps.map((item) => ({ ...item, status: 'complete', observedAt: '2026-10-23T10:01:00Z' })) }
    let reads = 0
    const fetcher = vi.fn().mockImplementation((url) => Promise.resolve(response(String(url).includes('/sources?') ? catalog : ++reads === 1 ? queued : complete)))
    vi.stubGlobal('fetch', fetcher); mount()
    await vi.advanceTimersByTimeAsync(0)
    expect(reads).toBe(1)
    await vi.advanceTimersByTimeAsync(2000)
    expect(reads).toBe(2)
    expect(screen.getAllByText('Complete').length).toBeGreaterThan(0)
    await vi.advanceTimersByTimeAsync(10000)
    expect(reads).toBe(2)
    expect(fetcher.mock.calls.every((call) => call[1].method === 'GET')).toBe(true)
  })
  it('bounds polling at ninety reads then supports manual read refresh without a retry POST', async () => {
    vi.useFakeTimers()
    const fetcher = vi.fn().mockImplementation((url) => Promise.resolve(response(String(url).includes('/sources?') ? catalog : queued)))
    vi.stubGlobal('fetch', fetcher); mount()
    await vi.advanceTimersByTimeAsync(180000)
    expect(fetcher).toHaveBeenCalledTimes(92)
    expect(screen.getByText(/Automatic status refresh stopped/)).toBeTruthy()
    await vi.advanceTimersByTimeAsync(10000)
    expect(fetcher).toHaveBeenCalledTimes(92)
    fireEvent.click(screen.getByRole('button', { name: 'Refresh preparation status' }))
    await vi.advanceTimersByTimeAsync(0)
    expect(fetcher).toHaveBeenCalledTimes(94)
    expect(fetcher.mock.calls.every((call) => call[1].method === 'GET')).toBe(true)
  })
  it('aborts an in-flight prepare when admin access is revoked and ignores its late result', async () => {
    let resolveWrite: ((value: Response) => void) | undefined
    const fetcher = vi.fn().mockImplementation((url, init) => init.method === 'POST' ? new Promise<Response>((resolve) => { resolveWrite = resolve }) : Promise.resolve(response(String(url).includes('/sources?') ? catalog : idle)))
    vi.stubGlobal('fetch', fetcher)
    const [allowed, setAllowed] = createSignal(true)
    render(() => <WebsitePreparationPanel {...context} revision={3} canManageIntegrations={allowed()} />)
    await screen.findByText('Not prepared')
    fireEvent.click(screen.getByRole('button', { name: 'Prepare public checks' }))
    await waitFor(() => expect(resolveWrite).toBeDefined())
    const write = fetcher.mock.calls.find((call) => call[1].method === 'POST')
    setAllowed(false)
    await screen.findByText(/An administrator can prepare/)
    expect(write?.[1]?.signal.aborted).toBe(true)
    resolveWrite?.(response(queued, 202)); await Promise.resolve(); await Promise.resolve()
    expect(screen.queryByText(queued.jobId)).toBeNull()
    expect(screen.queryByRole('button', { name: 'Prepare public checks' })).toBeNull()
  })
})
