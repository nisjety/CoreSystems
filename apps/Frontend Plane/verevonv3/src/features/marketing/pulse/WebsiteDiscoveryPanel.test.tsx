import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { WebsiteDiscoveryPanel } from './WebsiteDiscoveryPanel.tsx'
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }), localeDateTime: () => 'en-GB' }))
const context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 7 }
const snapshot = (status: string) => ({ ...context, source: 'quarry_map', status, limit: 25, partial: true, pages: [] })
describe('discovery panel', () => {
  afterEach(() => { cleanup(); vi.unstubAllGlobals(); vi.useRealTimers() })
  it('only reads on mount, then starts once by explicit user action', async () => {
    const fetcher = vi.fn<(url: string, init?: RequestInit) => Promise<Response>>(async () => new Response(JSON.stringify({ data: snapshot('idle') }), { status: 200 })); vi.stubGlobal('fetch', fetcher)
    render(() => <WebsiteDiscoveryPanel site={context.site} websiteId={context.websiteId} revision={7} />)
    await waitFor(() => expect((screen.getByRole('button', { name: 'Discover pages' }) as HTMLButtonElement).disabled).toBe(false))
    expect(fetcher).toHaveBeenCalledTimes(1); expect(fetcher.mock.calls[0]?.[1]).toMatchObject({ method: 'GET' })
    await fireEvent.click(screen.getByRole('button', { name: 'Discover pages' }))
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(2))
    expect(fetcher.mock.calls[1]?.[1]).toMatchObject({ method: 'POST' })
    expect(screen.getByText(/not audited pages/i)).toBeTruthy()
  })
  it('aborts the old context and never displays its late result', async () => {
    let resolveOld: ((value: Response) => void) | undefined
    const fetcher = vi.fn<(url: string, init?: RequestInit) => Promise<Response>>(() => new Promise<Response>((resolve) => { resolveOld = resolve })); vi.stubGlobal('fetch', fetcher)
    const [site, setSite] = createSignal(context.site)
    render(() => <WebsiteDiscoveryPanel site={site()} websiteId={context.websiteId} revision={7} />)
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(1))
    const old = resolveOld; const signal = fetcher.mock.calls[0]?.[1]?.signal
    setSite('https://agenci.no/')
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(2))
    expect(signal?.aborted).toBe(true)
    old?.(new Response(JSON.stringify({ data: { ...snapshot('complete'), pages: [{ url: 'https://triodelab.no/private-old-page' }], discoveredAt: '2026-10-22T12:00:00Z' } })))
    await Promise.resolve(); await Promise.resolve()
    expect(screen.queryByText('https://triodelab.no/private-old-page')).toBeNull()
  })
  it('explains missing Quarry configuration without claiming an audit happened', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ data: { ...snapshot('failed'), reason: 'not_configured' } }))))
    render(() => <WebsiteDiscoveryPanel site={context.site} websiteId={context.websiteId} revision={7} />)
    expect(await screen.findByText(/Quarry is not configured/i)).toBeTruthy()
    expect(screen.queryByText(/health score/i)).toBeNull()
  })
  it('polls only GET at bounded intervals and stops without another start', async () => {
    vi.useFakeTimers()
    const fetcher = vi.fn<(url: string, init?: RequestInit) => Promise<Response>>(async () => new Response(JSON.stringify({ data: snapshot('running') }))); vi.stubGlobal('fetch', fetcher)
    render(() => <WebsiteDiscoveryPanel site={context.site} websiteId={context.websiteId} revision={7} />)
    await vi.advanceTimersByTimeAsync(0)
    expect(fetcher).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(3000)
    expect(fetcher).toHaveBeenCalledTimes(2)
    await vi.advanceTimersByTimeAsync(60_000)
    expect(fetcher).toHaveBeenCalledTimes(15)
    expect(fetcher.mock.calls.every((call) => call[1]?.method === 'GET')).toBe(true)
    expect(screen.getByText(/Automatic status reading stopped/i)).toBeTruthy()
  })
  it('separates transport policy blocking from account connection failures', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ data: { ...snapshot('failed'), reason: 'policy_blocked' } }))))
    render(() => <WebsiteDiscoveryPanel site={context.site} websiteId={context.websiteId} revision={7} />)
    expect(await screen.findByText(/not a Google connection failure/i)).toBeTruthy()
    expect(screen.queryByText(/Check website permissions and service access/i)).toBeNull()
  })
})
