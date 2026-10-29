import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AuditInsightsPanel } from './AuditInsightsPanel'
import { WeeklyReportPanel } from './WeeklyReportPanel'
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }), localeDateTime: () => 'en-GB' }))

const context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 7 }
const counts = { new: 0, unchanged: 0, resolved: 0, regressed: 0, unknown: 1 }
const item = { pageUrl: context.site, ruleId: 'missing_title_signal', status: 'unknown', checkStatus: 'unavailable', severity: 'warning', proofAuditId: `audit_${'2'.repeat(32)}`, observedAt: '2026-10-21T12:00:00Z', runId: `run_0${'1'.repeat(25)}`, fingerprint: `blake3:${'a'.repeat(64)}`, relatedUrls: [] }
const base = { ...context, source: 'marketing_audit_history', asOf: '2026-10-22T12:01:00Z', trackingSince: '2026-10-21T00:00:00Z', partial: true, historyLimited: true, truncated: true, availableWeeks: ['2026-10-19'] }
const insights = { ...base, comparisonAvailable: false, latestAuditId: `audit_${'3'.repeat(32)}`, latestStatus: 'failed', summary: counts, items: [item], alerts: [{ kind: 'audit_failed', auditId: `audit_${'3'.repeat(32)}`, pageUrl: '', ruleId: '', reason: 'source_unavailable', observedAt: base.asOf }] }
const weekly = { ...base, historyLimited: false, truncated: false, weekStart: '2026-10-19', endExclusive: '2026-10-26', status: 'open', observed: true, audits: { total: 1, complete: 1, failed: 0 }, pages: { attempted: 2, checked: 2, failed: 0 }, failedChecks: 1, changes: { ...counts, unknown: 0, new: 1 }, receipts: [{ auditId: item.proofAuditId, status: 'complete', startedAt: '2026-10-22T11:59:00Z', completedAt: '2026-10-22T12:01:00Z', checkedPages: 2, failedPages: 0, findingCount: 1 }], tasksAsOf: base.asOf, work: { open: 1, inProgress: 0, done: 0, dismissed: 0, total: 1, completionPercent: 0 } }
const response = (data: unknown) => new Response(JSON.stringify({ data }))
describe('read-only audit history panels', () => {
  beforeEach(() => { vi.useFakeTimers({ toFake: ['Date'] }); vi.setSystemTime(new Date('2026-10-22T12:02:00Z')) })
  afterEach(() => { cleanup(); vi.unstubAllGlobals(); vi.useRealTimers() })
  it('shows baseline, coverage/retention gaps and old proof without treating unknown as resolved', async () => {
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(response(insights)))
    vi.stubGlobal('fetch', fetcher)
    render(() => <AuditInsightsPanel {...context} revision={7} />)
    await screen.findByText('Missing page title')
    expect(screen.getByText(/No prior comparable baseline/)).toBeTruthy()
    expect(screen.getByText(/Recording or retention gaps/)).toBeTruthy()
    expect(screen.getByText(/Display is truncated/)).toBeTruthy()
    expect(screen.getByText(/Proof may predate the latest audit/)).toBeTruthy()
    expect(screen.getByText(/Audit failed/)).toBeTruthy()
    expect(screen.getByRole('link', { name: item.pageUrl }).getAttribute('href')).toBe(item.pageUrl)
    expect(screen.getByText(item.runId)).toBeTruthy()
    expect(screen.queryByRole('button', { name: /dismiss|send|run audit/i })).toBeNull()
    await waitFor(() => expect((screen.getByRole('button', { name: 'Refresh audit insights' }) as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(screen.getByRole('button', { name: 'Refresh audit insights' }))
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(2))
    expect(fetcher.mock.calls.every((call) => call[1]?.method === 'GET')).toBe(true)
  })
  it('shows an honest unavailable state and never substitutes zero insights after a failed read', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('offline')))
    const view = render(() => <AuditInsightsPanel {...context} revision={7} />)
    expect((await screen.findByRole('alert')).textContent).toContain('could not be loaded')
    expect(view.container.textContent).not.toContain('No issues')
    expect(view.container.querySelector('[data-testid="insight-counts"]')).toBeNull()
  })
  it('labels weekly data provisional and observation-based, not unique pages or historical work status', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response(weekly)))
    render(() => <WeeklyReportPanel {...context} revision={7} />)
    await screen.findByText(/Open week · provisional/)
    expect(screen.getByText(/Capture observations, not unique pages/)).toBeTruthy()
    expect(screen.getByText(/Current work queue as of/)).toBeTruthy()
    expect(screen.getByText(/not a week-end snapshot/)).toBeTruthy()
    expect(screen.getByText(item.proofAuditId)).toBeTruthy()
    expect((screen.getByRole('button', { name: 'Next week' }) as HTMLButtonElement).disabled).toBe(true)
  })
  it('navigates UTC weeks through controlled GET reads and explains empty historical coverage', async () => {
    const older = { ...weekly, weekStart: '2026-10-12', endExclusive: '2026-10-19', status: 'closed', observed: false, historyLimited: true, audits: { total: 0, complete: 0, failed: 0 }, pages: { attempted: 0, checked: 0, failed: 0 }, failedChecks: 0, changes: { ...counts, unknown: 0 }, receipts: [] }
    const fetcher = vi.fn().mockResolvedValueOnce(response(weekly)).mockResolvedValueOnce(response(older))
    vi.stubGlobal('fetch', fetcher)
    render(() => <WeeklyReportPanel {...context} revision={7} />)
    await screen.findByText(/Open week · provisional/)
    await waitFor(() => expect((screen.getByRole('button', { name: 'Previous week' }) as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(screen.getByRole('button', { name: 'Previous week' }))
    await screen.findByText(/No recorded audit observations/)
    expect(fetcher.mock.calls[1]?.[0]).toContain('weekStart=2026-10-12')
    expect(fetcher.mock.calls.every((call) => call[1]?.method === 'GET')).toBe(true)
    fireEvent.change(screen.getByLabelText('Week starting Monday · UTC'), { target: { value: '2026-10-20' } })
    expect((await screen.findByRole('alert')).textContent).toContain('Monday')
    expect((screen.getByLabelText('Week starting Monday · UTC') as HTMLInputElement).value).toBe('2026-10-12')
    expect(fetcher).toHaveBeenCalledTimes(2)
  })
  it('cancels old-site reads and ignores their late evidence', async () => {
    let resolveOld: ((value: Response) => void) | undefined
    const otherSite = 'https://agenci.no/'
    const fetcher = vi.fn().mockImplementationOnce(() => new Promise<Response>((resolve) => { resolveOld = resolve })).mockResolvedValueOnce(response({ ...insights, site: otherSite, latestStatus: 'idle', latestAuditId: '', comparisonAvailable: false, summary: { ...counts, unknown: 0 }, items: [], alerts: [] }))
    vi.stubGlobal('fetch', fetcher)
    const [site, setSite] = createSignal(context.site)
    const view = render(() => <AuditInsightsPanel site={site()} websiteId={context.websiteId} revision={7} />)
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(1))
    const signal = fetcher.mock.calls[0]?.[1]?.signal
    setSite(otherSite)
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(2))
    expect(signal.aborted).toBe(true)
    resolveOld?.(response(insights)); await new Promise((resolve) => setTimeout(resolve, 0))
    expect(view.container.textContent).not.toContain('Missing page title')
  })
  it('refreshes insights and reports through scoped GET only when the shared completion key changes', async () => {
    const fetcher = vi.fn().mockImplementation((path: string) => Promise.resolve(response(path.includes('/audit-insights?') ? insights : weekly)))
    vi.stubGlobal('fetch', fetcher)
    const [refreshKey, setRefreshKey] = createSignal(0)
    render(() => <><AuditInsightsPanel {...context} revision={7} refreshKey={refreshKey()} /><WeeklyReportPanel {...context} revision={7} refreshKey={refreshKey()} /></>)
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(2))
    await screen.findByText('Missing page title')
    setRefreshKey(1)
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(4))
    expect(fetcher.mock.calls.filter((call) => String(call[0]).includes('/audit-insights?'))).toHaveLength(2)
    expect(fetcher.mock.calls.filter((call) => String(call[0]).includes('/reports/weekly?'))).toHaveLength(2)
    expect(fetcher.mock.calls.every((call) => call[1]?.method === 'GET')).toBe(true)
  })
})
