import { afterEach, describe, expect, it, vi } from 'vitest'
import { loadGA4, sanitizeGA4Snapshot } from './ga4-client.ts'
const site = 'https://example.org/'
const websiteId = 'web_' + '1'.repeat(32)
const metrics = { users: 8, sessions: 12, engagedSessions: 7, pageViews: 20, keyEvents: 1.5, engagementSeconds: 46.25 }
const snapshot = { site, source: 'google_analytics', status: 'connected', period: { startDate: '2026-10-01', endDate: '2026-10-03' }, timeZone: 'Europe/Oslo', totals: metrics, days: [{ date: '2026-10-01', ...metrics }, { date: '2026-10-03', ...metrics }], updatedAt: '2026-10-21T12:00:00Z' }
describe('GA4 projection boundary', () => {
  afterEach(() => vi.unstubAllGlobals())
  it('preserves independent period totals and fractional metrics, strips all provider metadata', () => {
    const value = sanitizeGA4Snapshot({ ...snapshot, token: 'private', propertyId: 'private', totals: { ...metrics, secret: 'private' }, days: snapshot.days.map((day) => ({ ...day, userId: 'private' })) })
    expect(value).toEqual(snapshot)
    expect(value.totals?.users).toBe(8)
    expect(value.days.reduce((sum, day) => sum + day.users, 0)).toBe(16)
    expect(JSON.stringify(value)).not.toContain('private')
  })
  it('preserves only a connected exact website or www-twin reporting scope', () => {
    const mapped = sanitizeGA4Snapshot({ ...snapshot, reportingSite: 'https://www.example.org/', token: 'private' })
    expect(mapped).toEqual({ ...snapshot, reportingSite: 'https://www.example.org/' })
    expect(JSON.stringify(mapped)).not.toContain('private')
    for (const reportingSite of ['https://analytics.example.org/', 'https://www.example.org/path', 'http://www.example.org/']) {
      expect(() => sanitizeGA4Snapshot({ ...snapshot, reportingSite })).toThrow()
    }
    expect(() => sanitizeGA4Snapshot({ ...snapshot, status: 'unavailable', reason: 'requires_consent', totals: null, days: [], reportingSite: 'https://www.example.org/' })).toThrow()
  })
  it('accepts real zeros and unavailable null totals without fabricating days', () => {
    const zero = Object.fromEntries(Object.keys(metrics).map((key) => [key, 0]))
    expect(sanitizeGA4Snapshot({ ...snapshot, totals: zero, days: [] }).totals?.users).toBe(0)
    expect(sanitizeGA4Snapshot({ ...snapshot, status: 'unavailable', reason: 'requires_consent', totals: null, days: [] }).totals).toBeNull()
    expect(sanitizeGA4Snapshot({ ...snapshot, totals: null, days: [] }).totals).toBeNull()
  })
  it('only preserves the three agreed quality booleans', () => {
    const flags = { thresholded: true, sampled: false, dataLoss: false }
    expect(sanitizeGA4Snapshot({ ...snapshot, quality: { ...flags, privateMetadata: 'private' } }).quality).toEqual(flags)
    expect(() => sanitizeGA4Snapshot({ ...snapshot, quality: { ...flags, sampled: 'false' } })).toThrow()
    expect(() => sanitizeGA4Snapshot({ ...snapshot, totals: null })).toThrow()
    expect(sanitizeGA4Snapshot({ ...snapshot, totals: { ...metrics, keyEvents: Number.MAX_VALUE } }).totals?.keyEvents).toBe(Number.MAX_VALUE)
  })
  it.each([
    { source: 'provider_raw' }, { site: 'https://localhost/' }, { status: 'pending' },
    { totals: { ...metrics, users: -1 } }, { totals: { ...metrics, sessions: 1.2 } },
    { totals: { ...metrics, pageViews: Number.MAX_SAFE_INTEGER + 1 } }, { totals: { ...metrics, engagementSeconds: Infinity } },
    { totals: { ...metrics, keyEvents: '2' } }, { totals: { ...metrics, users: undefined } },
    { days: [{ date: '2026-09-30', ...metrics }] }, { days: [snapshot.days[0], snapshot.days[0]] },
    { period: { startDate: '2026-02-30', endDate: '2026-03-03' } }, { timeZone: 'fake/private' },
    { reason: 'requires_consent' }, { updatedAt: 'invalid' },
    { status: 'unavailable', reason: 'private-provider-error', totals: null, days: [] },
    { status: 'unavailable', reason: 'not_connected' },
  ])('rejects invalid, incoherent or out-of-scope payload %j', (patch) => {
    expect(() => sanitizeGA4Snapshot({ ...snapshot, ...patch })).toThrow()
  })
  it('uses only the selected website context, quarter and no browser credentials', async () => {
    const fetcher = vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: snapshot }))); vi.stubGlobal('fetch', fetcher)
    expect(await loadGA4('Q4', site, websiteId)).toEqual(snapshot)
    expect(fetcher).toHaveBeenCalledWith(`/api/v1/visibility/ga4?quarter=Q4&site=${encodeURIComponent(site)}&websiteId=${websiteId}`, expect.objectContaining({ credentials: 'include', headers: { Accept: 'application/json' } }))
    await expect(loadGA4('Q4', site, 'bad-id')).rejects.toThrow()
    expect(fetcher).toHaveBeenCalledTimes(1)
  })
  it('accepts the current full-year period without relaxing other quarter validation', async () => {
    const fetcher = vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: snapshot })))
    vi.stubGlobal('fetch', fetcher)
    await expect(loadGA4('FY', site, websiteId)).resolves.toEqual(snapshot)
    expect(fetcher.mock.calls[0]?.[0]).toContain('quarter=FY')
  })
  it('rejects another site and registry errors without relaying raw provider errors', async () => {
    const fetcher = vi.fn().mockResolvedValueOnce(new Response(JSON.stringify({ data: { ...snapshot, site: 'https://other.example/' } })))
      .mockResolvedValueOnce(new Response('private-provider-error', { status: 409 }))
    vi.stubGlobal('fetch', fetcher)
    await expect(loadGA4('Q4', site, websiteId)).rejects.toThrow('Unexpected Analytics website')
    await expect(loadGA4('Q4', site, websiteId)).rejects.toThrow('Analytics is unavailable')
  })
})
