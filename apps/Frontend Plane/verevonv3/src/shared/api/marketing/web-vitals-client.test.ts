import { afterEach, describe, expect, it, vi } from 'vitest'
import { loadWebVitals, sanitizeWebVitals, syncWebVitals } from './web-vitals-client'
const context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 7 }
const empty = () => ({ count: 0, p75: null })
const group = (viewport: string) => ({ viewport, metrics: { lcp: empty(), inp: empty(), cls: empty() } })
const base = { ...context, source: 'first_party_web_vitals', environment: 'production', status: 'awaiting_samples', reportingOrigin: 'https://www.triodelab.no', window: { startDate: '2026-10-17', endDate: '2026-10-23' }, asOf: '2026-10-23T10:00:00Z', retentionDays: 7, sampleCount: 0, groups: [group('narrow'), group('wide')] }
const measured = { ...base, status: 'connected', sampleCount: 2, groups: [{ viewport: 'narrow', metrics: { lcp: { count: 1, p75: 1200 }, inp: empty(), cls: { count: 1, p75: 0 } } }, group('wide')] }
describe('first-party vitals read boundary', () => {
  afterEach(() => vi.unstubAllGlobals())
  it('preserves zero CLS and nullable missing measures while stripping every unknown field', () => {
    expect(sanitizeWebVitals({ ...measured, token: 'secret', groups: measured.groups.map((item) => ({ ...item, secret: 'private' })) }, context, 'production')).toEqual(measured)
    expect(sanitizeWebVitals(base, context, 'production')).toEqual(base)
    expect(sanitizeWebVitals({ ...base, status: 'not_configured', reportingOrigin: undefined }, context, 'production').status).toBe('not_configured')
  })
  it('rejects wrong context, environments, windows, lifecycle and aggregate counts', () => {
    for (const patch of [{ websiteRevision: 8 }, { site: 'https://other.example/' }, { environment: 'development' }, { source: 'crux' }, { status: 'connected' }, { status: 'healthy' }, { status: ['awaiting_samples'] }, { sampleCount: 1 }, { retentionDays: 30 }, { asOf: '2026-02-30T00:00:00Z' }, { window: { startDate: '2026-10-16', endDate: '2026-10-23' } }, { window: { startDate: '2026-10-18', endDate: '2026-10-24' } }, { groups: [group('wide'), group('narrow')] }, { groups: [group('narrow')] }]) expect(() => sanitizeWebVitals({ ...base, ...patch }, context, 'production')).toThrow()
    expect(() => sanitizeWebVitals({ ...measured, status: 'not_configured' }, context, 'production')).toThrow()
  })
  it('requires the exact registered production origin or its www alias and isolated local development', () => {
    for (const reportingOrigin of ['http://triodelab.no', 'https://other.example', 'https://sub.triodelab.no', 'https://triodelab.no/', 'https://triodelab.no?secret=1', 'http://localhost:3000', 'https://name:secret@triodelab.no']) expect(() => sanitizeWebVitals({ ...base, reportingOrigin }, context, 'production')).toThrow()
    for (const reportingOrigin of ['http://localhost:3000', 'http://127.0.0.1:3000', 'http://localhost:3001', 'http://127.0.0.1:3001']) expect(sanitizeWebVitals({ ...base, environment: 'development', reportingOrigin }, context, 'development').reportingOrigin).toBe(reportingOrigin)
    expect(() => sanitizeWebVitals({ ...base, environment: 'development' }, context, 'development')).toThrow()
  })
  it('rejects out-of-bounds, nonfinite or inconsistent metric observations', () => {
    for (const metric of [{ count: 0, p75: 0 }, { count: 1, p75: null }, { count: 1, p75: Infinity }, { count: 1, p75: -1 }, { count: 5001, p75: 1 }, { count: 0.5, p75: 1 }, { count: 1, p75: 120001 }]) expect(() => sanitizeWebVitals({ ...measured, groups: [{ ...measured.groups[0], metrics: { ...measured.groups[0]!.metrics, lcp: metric } }, group('wide')] }, context, 'production')).toThrow()
    for (const [key, limit] of [['inp', 60000], ['cls', 100]] as const) expect(() => sanitizeWebVitals({ ...measured, groups: [{ ...measured.groups[0], metrics: { ...measured.groups[0]!.metrics, [key]: { count: 1, p75: limit + 1 } } }, group('wide')] }, context, 'production')).toThrow()
  })
  it('uses only authenticated GET with exact context and never triggers collection', async () => {
    const fetcher = vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: base }))); vi.stubGlobal('fetch', fetcher)
    await loadWebVitals(context, 'production')
    const [url, init] = fetcher.mock.calls[0]!
    expect(url).toBe(`/api/v1/visibility/web-vitals?websiteId=${context.websiteId}&site=https%3A%2F%2Ftriodelab.no%2F&environment=production`)
    expect(init).toMatchObject({ method: 'GET', credentials: 'include', redirect: 'error' }); expect(init.body).toBeUndefined()
    fetcher.mockResolvedValue(new Response('private', { status: 401 })); await expect(loadWebVitals(context, 'production')).rejects.toThrow('web_vitals_unavailable')
    await expect(loadWebVitals(context, 'invalid' as 'production')).rejects.toThrow(); expect(fetcher).toHaveBeenCalledTimes(2)
  })
  it('imports only through the explicit revision-bound sync contract', async () => {
    const fetcher = vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: base }))); vi.stubGlobal('fetch', fetcher)
    await syncWebVitals(context, 'production')
    expect(fetcher.mock.calls[0]![0]).toBe('/api/v1/visibility/web-vitals/sync')
    expect(fetcher.mock.calls[0]![1]).toMatchObject({ method: 'POST', credentials: 'include', redirect: 'error', body: JSON.stringify({ ...context, environment: 'production' }) })
  })
})
