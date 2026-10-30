import { afterEach, describe, expect, it, vi } from 'vitest'
import { configureAuditMonitoring, loadAuditMonitoring, sanitizeAuditMonitoring } from './monitoring-client'

const context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 7 }
const empty = { ...context, source: 'marketing_scheduler', revision: 0, configured: false, enabled: false, cadence: 'daily', status: 'not_configured', nextRunAt: '', updatedAt: '', lastAuditId: '', lastOutcome: 'none', failureCount: 0, history: [] }
const active = { ...empty, revision: 1, configured: true, enabled: true, status: 'active', updatedAt: '2026-10-22T12:00:00Z', nextRunAt: '2026-10-23T12:00:00Z' }
const receipt = { auditId: `audit_${'2'.repeat(32)}`, scheduledFor: '2026-10-23T12:00:00Z', startedAt: '2026-10-23T12:00:01Z', completedAt: '2026-10-23T12:01:00Z', status: 'complete', attempt: 1, checkedPages: 10, findingCount: 0 }
const response = (data: unknown, status = 200) => new Response(JSON.stringify({ data }), { status })

describe('audit monitoring client', () => {
  afterEach(() => vi.unstubAllGlobals())
  it('projects unconfigured and active schedules without leaking unknown fields', () => {
    expect(sanitizeAuditMonitoring(empty, context)).toEqual(empty)
    const value = { ...active, lastAuditId: receipt.auditId, lastOutcome: 'complete', history: [{ ...receipt, token: 'private' }], token: 'private' }
    expect(sanitizeAuditMonitoring(value, context)).toEqual({ ...active, lastAuditId: receipt.auditId, lastOutcome: 'complete', history: [receipt] })
    expect(sanitizeAuditMonitoring({ ...active, status: 'paused', enabled: false, nextRunAt: '' }, context).enabled).toBe(false)
  })
  it('rejects stale contexts and malformed lifecycle, timestamps, counts or enum coercions', () => {
    for (const patch of [
      { websiteRevision: 8 }, { site: 'https://agenci.no/' }, { source: 'other' }, { revision: -1 }, { status: ['active'] },
      { status: 'paused' }, { enabled: false }, { nextRunAt: '' }, { nextRunAt: '2026-02-30T12:00:00Z' },
      { cadence: 'hourly' }, { failureCount: -1 }, { failureCount: 4 }, { configured: false },
      { history: Array(11).fill(receipt) }, { history: [{ ...receipt, attempt: 4 }] },
      { history: [{ ...receipt, auditId: 'forged' }] }, { history: [{ ...receipt, checkedPages: 11 }] },
      { history: [{ ...receipt, findingCount: 61 }] }, { reason: 'unknown' },
    ]) expect(() => sanitizeAuditMonitoring({ ...active, ...patch }, context)).toThrow()
    expect(() => sanitizeAuditMonitoring({ ...empty, cadence: 'weekly' }, context)).toThrow()
    expect(() => sanitizeAuditMonitoring({ ...active, lastAuditId: receipt.auditId, lastOutcome: 'failed', history: [receipt] }, context)).toThrow()
    expect(() => sanitizeAuditMonitoring({ ...active, lastAuditId: receipt.auditId, lastOutcome: 'complete', history: [{ ...receipt, reason: 'source_unavailable' }] }, context)).toThrow()
    expect(() => sanitizeAuditMonitoring({ ...active, lastAuditId: receipt.auditId, lastOutcome: 'failed', history: [{ ...receipt, status: 'failed' }] }, context)).toThrow()
    expect(() => sanitizeAuditMonitoring({ ...active, lastAuditId: receipt.auditId, lastOutcome: 'complete', history: [{ ...receipt, auditId: `audit_${'3'.repeat(32)}` }, receipt] }, context)).toThrow()
  })
  it('accepts explicit failures and running receipts, never interprets missing collection as success', () => {
    const failed = { ...receipt, status: 'failed', reason: 'source_unavailable' }
    const value = { ...active, lastAuditId: receipt.auditId, lastOutcome: 'failed', failureCount: 1, history: [failed], reason: 'source_unavailable' }
    expect(sanitizeAuditMonitoring(value, context).lastOutcome).toBe('failed')
    expect(sanitizeAuditMonitoring({ ...value, status: 'blocked', enabled: false, nextRunAt: '', reason: 'access_denied' }, context).status).toBe('blocked')
    expect(sanitizeAuditMonitoring({ ...active, lastAuditId: receipt.auditId, lastOutcome: 'running', history: [{ ...receipt, status: 'running', completedAt: '', checkedPages: 0, findingCount: 0 }] }, context).lastOutcome).toBe('running')
  })
  it('reads status only with GET and configures only the exact explicit optimistic command', async () => {
    const fetcher = vi.fn().mockResolvedValueOnce(response(empty)).mockResolvedValueOnce(response(active))
    vi.stubGlobal('fetch', fetcher)
    await loadAuditMonitoring(context)
    const input = { ...context, scheduleRevision: 0, enabled: true, cadence: 'daily' as const }
    await configureAuditMonitoring({ ...input, token: 'never-forward' } as typeof input)
    expect(fetcher.mock.calls[0]?.[0]).toBe(`/api/v1/visibility/monitoring?websiteId=${context.websiteId}&site=${encodeURIComponent(context.site)}`)
    expect(fetcher.mock.calls[0]?.[1]).toMatchObject({ method: 'GET', credentials: 'include', redirect: 'error' })
    expect(fetcher.mock.calls[1]?.[1]).toMatchObject({ method: 'PATCH', body: JSON.stringify(input) })
    await expect(configureAuditMonitoring({ ...input, scheduleRevision: -1 })).rejects.toThrow()
    expect(fetcher).toHaveBeenCalledTimes(2)
  })
  it('reports optimistic conflicts and invalid responses without returning stale projections', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(response({}, 409)).mockResolvedValueOnce(response({ ...active, site: 'https://agenci.no/' })))
    await expect(configureAuditMonitoring({ ...context, scheduleRevision: 1, enabled: false, cadence: 'daily' })).rejects.toThrow('monitoring_conflict')
    await expect(loadAuditMonitoring(context)).rejects.toThrow()
  })
})
