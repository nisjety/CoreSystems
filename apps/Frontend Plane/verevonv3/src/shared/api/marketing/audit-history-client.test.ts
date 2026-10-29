import { afterEach, describe, expect, it, vi } from 'vitest'
import { loadAuditInsights, sanitizeAuditInsights } from './audit-insights-client'
import { loadWeeklyReport, sanitizeWeeklyReport } from './weekly-report-client'

const context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 7 }
const counts = { new: 1, unchanged: 0, resolved: 0, regressed: 0, unknown: 0 }
const item = { pageUrl: context.site, ruleId: 'missing_title_signal', status: 'new', checkStatus: 'failed', severity: 'warning', proofAuditId: `audit_${'2'.repeat(32)}`, observedAt: '2026-10-22T12:00:00Z', runId: `run_0${'1'.repeat(25)}`, fingerprint: `blake3:${'a'.repeat(64)}`, relatedUrls: [] }
const insights = { ...context, source: 'marketing_audit_history', asOf: '2026-10-22T12:01:00Z', trackingSince: '2026-10-21T00:00:00Z', partial: true, historyLimited: false, truncated: false, comparisonAvailable: false, latestAuditId: item.proofAuditId, latestStatus: 'complete', summary: counts, items: [item], alerts: [{ kind: 'new_finding', auditId: item.proofAuditId, pageUrl: item.pageUrl, ruleId: item.ruleId, reason: '', observedAt: item.observedAt }], availableWeeks: ['2026-10-19'] }
const receipt = { auditId: item.proofAuditId, status: 'complete', startedAt: '2026-10-22T11:59:00Z', completedAt: '2026-10-22T12:01:00Z', checkedPages: 2, failedPages: 0, findingCount: 1 }
const weekly = { ...context, source: 'marketing_audit_history', asOf: insights.asOf, trackingSince: insights.trackingSince, partial: true, historyLimited: false, truncated: false, weekStart: '2026-10-19', endExclusive: '2026-10-26', status: 'open', observed: true, audits: { total: 1, complete: 1, failed: 0 }, pages: { attempted: 2, checked: 2, failed: 0 }, failedChecks: 1, changes: counts, receipts: [receipt], availableWeeks: ['2026-10-19'], tasksAsOf: insights.asOf, work: { open: 1, inProgress: 0, done: 0, dismissed: 0, total: 1, completionPercent: 0 } }
const response = (data: unknown) => new Response(JSON.stringify({ data }))
describe('read-only audit insights and weekly report contracts', () => {
  afterEach(() => vi.unstubAllGlobals())
  it('preserves baseline and original proof while stripping unknown provider data', () => {
    const safe = sanitizeAuditInsights({ ...insights, token: 'private', items: [{ ...item, html: 'private' }] }, context)
    expect(safe).toEqual(insights)
    const unknown = { ...item, status: 'unknown', checkStatus: 'unavailable', observedAt: '2026-10-21T12:00:00Z' }
    expect(sanitizeAuditInsights({ ...insights, comparisonAvailable: true, summary: { ...counts, new: 0, unknown: 1 }, items: [unknown], alerts: [] }, context).items[0]?.observedAt).toBe(unknown.observedAt)
  })
  it('rejects foreign contexts, forged proof, fabricated counts and unsafe URLs', () => {
    for (const patch of [{ site: 'https://agenci.no/' }, { websiteRevision: 8 }, { partial: false }, { latestStatus: ['complete'] }, { summary: { ...counts, new: 2 } }, { items: [{ ...item, pageUrl: `${context.site}login` }] }, { items: [{ ...item, fingerprint: 'forged' }] }, { items: [{ ...item, proofAuditId: '' }] }, { items: [{ ...item, observedAt: '2026-10-23T12:00:00Z' }] }, { alerts: [{ ...insights.alerts[0], kind: 'email_sent' }] }, { alerts: [{ ...insights.alerts[0], auditId: `audit_${'3'.repeat(32)}` }] }, { alerts: [{ ...insights.alerts[0], kind: 'regressed_finding' }] }, { availableWeeks: ['2026-10-20'] }]) expect(() => sanitizeAuditInsights({ ...insights, ...patch }, context)).toThrow()
  })
  it('projects actual observation rollups and current work separately from the selected report week', () => {
    expect(sanitizeWeeklyReport({ ...weekly, token: 'private', receipts: [{ ...receipt, raw: 'private' }] }, context, '2026-10-19')).toEqual(weekly)
    expect(sanitizeWeeklyReport({ ...weekly, weekStart: '2026-10-12', endExclusive: '2026-10-19', status: 'closed', observed: false, audits: { total: 0, complete: 0, failed: 0 }, pages: { attempted: 0, checked: 0, failed: 0 }, failedChecks: 0, changes: { ...counts, new: 0 }, receipts: [] }, context, '2026-10-12').observed).toBe(false)
  })
  it('rejects impossible weeks, rollups, receipts and stale work summaries', () => {
    for (const patch of [{ weekStart: '2026-10-20' }, { endExclusive: '2026-10-27' }, { status: 'closed' }, { observed: false }, { audits: { total: 1, complete: 0, failed: 0 } }, { pages: { attempted: 2, checked: 1, failed: 0 } }, { receipts: [{ ...receipt, auditId: 'forged' }] }, { tasksAsOf: '' }, { work: { ...weekly.work, total: 3 } }]) expect(() => sanitizeWeeklyReport({ ...weekly, ...patch }, context, '2026-10-19')).toThrow()
    expect(() => sanitizeWeeklyReport({ ...weekly, truncated: true, receipts: [{ ...receipt, status: 'failed', reason: 'source_unavailable' }] }, context, '2026-10-19')).toThrow()
  })
  it('only sends scoped GET reads, never starts captures, writes tasks or sends email', async () => {
    const fetcher = vi.fn().mockResolvedValueOnce(response(insights)).mockResolvedValueOnce(response(weekly))
    vi.stubGlobal('fetch', fetcher)
    await loadAuditInsights(context)
    await loadWeeklyReport(context, '2026-10-19')
    expect(fetcher.mock.calls[0]?.[0]).toContain('/api/v1/visibility/audit-insights?')
    expect(fetcher.mock.calls[1]?.[0]).toContain('weekStart=2026-10-19')
    for (const call of fetcher.mock.calls) expect(call[1]).toMatchObject({ method: 'GET', credentials: 'include', redirect: 'error' })
    await expect(loadWeeklyReport(context, '2026-10-20')).rejects.toThrow()
    expect(fetcher).toHaveBeenCalledTimes(2)
  })
})
