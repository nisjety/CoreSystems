import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { AuditMonitoringPanel } from './AuditMonitoringPanel'
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }), localeDateTime: () => 'en-GB' }))

const context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 7 }
const empty = { ...context, source: 'marketing_scheduler', revision: 0, configured: false, enabled: false, cadence: 'daily', status: 'not_configured', nextRunAt: '', updatedAt: '', lastAuditId: '', lastOutcome: 'none', failureCount: 0, history: [] }
const active = { ...empty, revision: 1, configured: true, enabled: true, status: 'active', updatedAt: '2026-10-22T12:00:00Z', nextRunAt: '2026-10-29T12:00:00Z', cadence: 'weekly' }
const response = (data: unknown, status = 200) => new Response(JSON.stringify({ data }), { status })
const mount = (canConfigure = true) => render(() => <AuditMonitoringPanel {...context} revision={7} canConfigure={canConfigure} />)

describe('audit monitoring panel', () => {
  afterEach(() => { cleanup(); vi.unstubAllGlobals() })
  it('loads status only and enables the selected elapsed-UTC cadence only after an explicit save', async () => {
    const fetcher = vi.fn().mockResolvedValueOnce(response(empty)).mockResolvedValueOnce(response(active))
    vi.stubGlobal('fetch', fetcher); mount()
    expect((screen.getByRole('button', { name: 'Enable monitoring' }) as HTMLButtonElement).disabled).toBe(true)
    await waitFor(() => expect((screen.getByRole('button', { name: 'Enable monitoring' }) as HTMLButtonElement).disabled).toBe(false))
    expect(fetcher).toHaveBeenCalledTimes(1)
    expect(fetcher.mock.calls[0]?.[1]).toMatchObject({ method: 'GET' })
    fireEvent.change(screen.getByLabelText('Cadence'), { target: { value: 'weekly' } })
    expect(fetcher).toHaveBeenCalledTimes(1)
    fireEvent.click(screen.getByRole('button', { name: 'Enable monitoring' }))
    await screen.findByText('Active')
    expect(JSON.parse(fetcher.mock.calls[1]?.[1]?.body)).toEqual({ ...context, scheduleRevision: 0, enabled: true, cadence: 'weekly' })
    expect(fetcher.mock.calls[1]?.[1]).toMatchObject({ method: 'PATCH' })
    expect(screen.getByText(/29 Oct 2026, 12:00.*UTC/)).toBeTruthy()
    expect(screen.getAllByText(/No immediate capture/).length).toBeGreaterThan(0)
    expect(fetcher.mock.calls.some((call) => String(call[0]).endsWith('/audits'))).toBe(false)
  })
  it('pauses with the current schedule revision and refreshes status without starting collection', async () => {
    const paused = { ...active, revision: 2, enabled: false, status: 'paused', nextRunAt: '' }
    const fetcher = vi.fn().mockResolvedValueOnce(response(active)).mockResolvedValueOnce(response(paused)).mockResolvedValueOnce(response(paused))
    vi.stubGlobal('fetch', fetcher); mount()
    await waitFor(() => expect((screen.getByRole('button', { name: 'Pause monitoring' }) as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(screen.getByRole('button', { name: 'Pause monitoring' }))
    await screen.findByText('Paused')
    expect(JSON.parse(fetcher.mock.calls[1]?.[1]?.body)).toEqual({ ...context, scheduleRevision: 1, enabled: false, cadence: 'weekly' })
    fireEvent.click(screen.getByRole('button', { name: 'Refresh monitoring status' }))
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(3))
    expect(fetcher.mock.calls[2]?.[1]).toMatchObject({ method: 'GET' })
  })
  it('shows actual scheduled receipt outcomes, attempts and source failures without claiming resolution', async () => {
    const receipt = { auditId: `audit_${'2'.repeat(32)}`, scheduledFor: '2026-10-22T12:00:00Z', startedAt: '2026-10-22T12:00:01Z', completedAt: '2026-10-22T12:01:00Z', status: 'failed', attempt: 2, checkedPages: 0, findingCount: 0, reason: 'source_unavailable' }
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response({ ...active, lastAuditId: receipt.auditId, lastOutcome: 'failed', failureCount: 2, history: [receipt], reason: 'source_unavailable' })))
    mount()
    await screen.findAllByText(receipt.auditId)
    expect(screen.getByText(/Attempt 2/)).toBeTruthy()
    expect(screen.getAllByText(/Source unavailable/).length).toBeGreaterThan(0)
    expect(screen.getByText(/does not create or close work tasks/)).toBeTruthy()
  })
  it('reports stale saves and does not optimistically claim monitoring was enabled', async () => {
    const fetcher = vi.fn().mockResolvedValueOnce(response(empty)).mockResolvedValueOnce(response({}, 409))
    vi.stubGlobal('fetch', fetcher); mount()
    await waitFor(() => expect((screen.getByRole('button', { name: 'Enable monitoring' }) as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(screen.getByRole('button', { name: 'Enable monitoring' }))
    expect((await screen.findByRole('alert')).textContent).toMatch(/changed.*Refresh/i)
    expect(screen.queryByText('Active')).toBeNull()
  })
  it('keeps configuration disabled after a failed read and for read-only actors', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('offline')))
    const view = mount()
    await screen.findByRole('alert')
    expect((screen.getByRole('button', { name: 'Enable monitoring' }) as HTMLButtonElement).disabled).toBe(true)
    view.unmount()
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response(active)))
    mount(false)
    await screen.findByText('Active')
    expect(screen.queryByRole('button', { name: 'Save schedule' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Pause monitoring' })).toBeNull()
    expect(screen.getByText(/Read-only access/)).toBeTruthy()
  })
  it('aborts old requests and ignores stale save results after a website change', async () => {
    let resolveSave: ((value: Response) => void) | undefined
    const otherSite = 'https://agenci.no/'
    const fetcher = vi.fn().mockResolvedValueOnce(response(empty)).mockImplementationOnce(() => new Promise<Response>((resolve) => { resolveSave = resolve })).mockResolvedValueOnce(response({ ...empty, site: otherSite }))
    vi.stubGlobal('fetch', fetcher)
    const [site, setSite] = createSignal(context.site)
    const view = render(() => <AuditMonitoringPanel site={site()} websiteId={context.websiteId} revision={7} />)
    await waitFor(() => expect((screen.getByRole('button', { name: 'Enable monitoring' }) as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(screen.getByRole('button', { name: 'Enable monitoring' }))
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(2))
    const signal = fetcher.mock.calls[1]?.[1]?.signal
    setSite(otherSite)
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(3))
    expect(signal.aborted).toBe(true)
    resolveSave?.(response(active)); await new Promise((resolve) => setTimeout(resolve, 0))
    expect(view.container.textContent).not.toContain('Active')
    expect(view.container.textContent).not.toContain('29 Oct 2026')
  })
})
