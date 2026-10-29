import { afterEach, describe, expect, it, vi } from 'vitest'
import { loadSiteAudit, sanitizeSiteAudit, startSiteAudit } from './audit-client'

export const auditContext = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 7 }
export const idleAudit = { ...auditContext, id: '', source: 'quarry_capture', ruleVersion: 'bounded_rules_v1', status: 'idle', limit: 10, partial: true, startedAt: '', completedAt: '', pages: [], findings: [], history: [] }
export const auditPage = { url: 'https://www.triodelab.no/', finalUrl: 'https://www.triodelab.no/', status: 'checked', httpStatus: 200, title: '', description: '', canonical: '', observedAt: '2026-10-22T12:00:00Z', runId: `run_0${'1'.repeat(25)}`, fingerprint: `blake3:${'a'.repeat(64)}`, links: [], checks: [{ ruleId: 'missing_title_signal', status: 'failed' }] }
export const auditFinding = { id: `finding_${'2'.repeat(32)}`, ruleId: 'missing_title_signal', severity: 'warning', pageUrl: auditPage.url, observedAt: auditPage.observedAt, runId: auditPage.runId, fingerprint: auditPage.fingerprint, relatedUrls: [] }
export const completeAudit = { ...idleAudit, id: `audit_${'3'.repeat(32)}`, status: 'complete', startedAt: '2026-10-22T11:59:00Z', completedAt: '2026-10-22T12:01:00Z', pages: [auditPage], findings: [auditFinding] }
describe('bounded site audit client', () => {
 afterEach(() => vi.unstubAllGlobals())
 it('preserves compact evidence, never provider HTML, keys or unknown fields', () => {
  expect(sanitizeSiteAudit({ ...completeAudit, token: 'secret', pages: [{ ...auditPage, html: '<form>private</form>' }] }, auditContext)).toEqual(completeAudit)
 })
 it('rejects changed bindings, oversized runs, forged findings and private page URLs', () => {
  for (const value of [ { ...completeAudit, websiteRevision: 8 }, { ...completeAudit, site: 'https://agenci.no/' }, { ...completeAudit, partial: false }, { ...completeAudit, limit: 101 }, { ...completeAudit, pages: Array(11).fill(auditPage) }, { ...completeAudit, findings: [{ ...auditFinding, pageUrl: 'https://triodelab.no/not-checked' }] }, { ...completeAudit, pages: [{ ...auditPage, url: 'https://www.triodelab.no/admin' }] }, { ...completeAudit, pages: [{ ...auditPage, fingerprint: 'invented' }] } ]) expect(() => sanitizeSiteAudit(value, auditContext)).toThrow()
 })
 it('keeps unconfigured state diagnostic and rejects coerced enums or impossible timestamps', () => {
  expect(sanitizeSiteAudit({ ...idleAudit, status: 'failed', reason: 'not_configured' }, auditContext).reason).toBe('not_configured')
  for (const value of [{ ...completeAudit, status: ['complete'] }, { ...completeAudit, startedAt: '2026-02-30T12:00:00Z' }, { ...completeAudit, pages: [{ ...auditPage, status: ['checked'] }] }, { ...completeAudit, findings: [{ ...auditFinding, severity: ['warning'] }] }]) expect(() => sanitizeSiteAudit(value, auditContext)).toThrow()
 })
 it('only starts network capture through an explicit POST with revision, not status reads', async () => {
  const fetcher = vi.fn<(input: RequestInfo | URL, init?: RequestInit) => Promise<Response>>(async () => new Response(JSON.stringify({ data: idleAudit }))); vi.stubGlobal('fetch', fetcher)
  await loadSiteAudit(auditContext)
  await startSiteAudit(auditContext)
  expect(fetcher.mock.calls[0]).toMatchObject([expect.stringContaining('/api/v1/visibility/audits?'), { credentials: 'include', method: 'GET' }])
  expect(fetcher.mock.calls[1]).toMatchObject(['/api/v1/visibility/audits', { credentials: 'include', method: 'POST', body: JSON.stringify(auditContext) }])
 })
})
