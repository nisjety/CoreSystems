import { afterEach, describe, expect, it, vi } from 'vitest'
import { loadOutcomes, sanitizeOutcomes } from './outcomes-client'
const context = { websiteId: `web_${'a'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 8, quarter: 'Q4' as const, environment: 'development' as const }
const report = { ...context, source: 'leads_core', status: 'connected', period: { startDate: '2026-10-01', endDate: '2026-10-28' }, timeZone: 'Europe/Oslo', asOf: '2026-10-28T12:00:00Z', metrics: { inquiries: 1, leads: 1, bookings: null }, lastReceivedAt: '2026-10-28T11:00:00Z' }
describe('first-party outcome projection', () => {
  afterEach(() => vi.unstubAllGlobals())
  it('keeps confirmed counts, unknown bookings and strips person/credential data', () => {
    const result = sanitizeOutcomes({ ...report, email: 'private', token: 'private', metrics: { ...report.metrics, email: 'private' } }, context)
    expect(result.metrics).toEqual(report.metrics); expect(JSON.stringify(result)).not.toContain('private')
    const missing = sanitizeOutcomes({ ...report, status: 'unavailable', reason: 'not_connected', metrics: null, lastReceivedAt: null, asOf: undefined }, context)
    expect(missing.metrics).toBeNull()
  })
  it('rejects other sites, revisions, environments and fabricated or contradictory outcomes', () => {
    for (const delta of [{ websiteRevision: 9 }, { environment: 'production' }, { site: 'https://other.no/' }, { quarter: 'Q1' }, { metrics: { inquiries: -1, leads: -1, bookings: null } }, { metrics: { inquiries: 1, leads: 2, bookings: null } }, { metrics: { inquiries: 1, leads: 1, bookings: 1 } }, { lastReceivedAt: '2027-01-01T00:00:00Z' }]) expect(() => sanitizeOutcomes({ ...report, ...delta }, context)).toThrow()
  })
  it('reads through the signed-in same-origin gateway without organization or write authority', async () => {
    const fetcher = vi.fn().mockResolvedValue(Response.json({ data: report })); vi.stubGlobal('fetch', fetcher)
    await loadOutcomes(context)
    expect(fetcher.mock.calls[0]?.[0]).toContain('/api/v1/marketing/outcomes?websiteId=')
    expect(fetcher.mock.calls[0]?.[0]).toContain('environment=development')
    expect(fetcher.mock.calls[0]?.[1]?.method).toBe('GET'); expect(fetcher.mock.calls[0]?.[1]?.credentials).toBe('include')
    expect(fetcher.mock.calls[0]?.[1]?.body).toBeUndefined()
  })
})
