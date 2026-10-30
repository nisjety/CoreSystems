import { afterEach, describe, expect, it, vi } from 'vitest'
import { loadAnalyticsSync, requestAnalyticsSync, sanitizeAnalyticsSync, setAnalyticsSyncEnabled } from './analytics-sync-client'

const context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 3, quarter: 'Q4' as const }
const keys = ['gsc', 'ga4', 'clarity']
const idle = { ...context, source: 'marketing_analytics_sync', enabled: false, status: 'idle', steps: keys.map((key) => ({ key, status: 'queued' })) }
const queued = { ...idle, enabled: true, status: 'queued', jobId: `sync_${'2'.repeat(32)}`, queuedAt: '2026-10-23T10:00:00Z' }
const complete = { ...queued, status: 'complete', startedAt: '2026-10-23T10:00:01Z', completedAt: '2026-10-23T10:01:00Z', expiresAt: '2026-10-23T16:01:00Z', nextSyncAt: '2026-10-23T16:01:00Z', steps: keys.map((key) => ({ key, status: 'connected', observedAt: '2026-10-23T10:01:00Z' })) }
const response = (data: unknown, status = 200) => new Response(JSON.stringify({ data }), { status })

describe('analytics sync boundary', () => {
  afterEach(() => vi.unstubAllGlobals())
  it('rebuilds metadata only, preserving exact site, revision, quarter and source', () => {
    expect(sanitizeAnalyticsSync({ ...complete, token: 'private', metrics: { users: 200 }, steps: complete.steps.map((step) => ({ ...step, rawProvider: 'private' })) }, context)).toEqual(complete)
    expect(sanitizeAnalyticsSync(idle, context)).toEqual(idle)
    for (const override of [{ websiteId: `web_${'3'.repeat(32)}` }, { site: 'https://example.com/' }, { websiteRevision: 4 }, { quarter: 'Q3' }, { source: 'provider' }]) expect(() => sanitizeAnalyticsSync({ ...complete, ...override }, context)).toThrow()
  })
  it('accepts the current full-year context', () => {
    const fullYear = { ...context, quarter: 'FY' as const }
    const fullYearIdle = { ...idle, quarter: 'FY' as const }
    expect(sanitizeAnalyticsSync(fullYearIdle, fullYear)).toEqual(fullYearIdle)
  })
  it('rejects missing, duplicated, unrecognized or contradictory steps and lifecycles', () => {
    for (const steps of [[], complete.steps.slice(0, 2), [...complete.steps, complete.steps[0]], [complete.steps[0], complete.steps[0], complete.steps[2]], [{ key: 'ads', status: 'connected' }, ...complete.steps.slice(1)], [{ key: 'gsc', status: 'connected', reason: 'no_data' }, ...complete.steps.slice(1)], [{ key: 'gsc', status: 'unavailable', reason: 'token=private', observedAt: complete.completedAt }, ...complete.steps.slice(1)]]) expect(() => sanitizeAnalyticsSync({ ...complete, steps }, context)).toThrow()
    for (const override of [{ status: 'healthy' }, { status: 'idle' }, { status: 'queued' }, { status: 'running' }, { status: 'failed' }, { status: 'partial' }, { jobId: 'private' }, { completedAt: '2026-10-23T09:00:00Z' }, { startedAt: undefined }, { expiresAt: '2026-10-23T23:01:00Z' }]) expect(() => sanitizeAnalyticsSync({ ...complete, ...override }, context)).toThrow()
  })
  it('accepts honest partial and unavailable results but never unmeasured zero metrics', () => {
    const partial = { ...complete, status: 'partial', steps: [complete.steps[0], { key: 'ga4', status: 'unavailable', reason: 'not_found', observedAt: complete.completedAt }, complete.steps[2]] }
    expect(sanitizeAnalyticsSync(partial, context).steps[1]).toEqual(partial.steps[1])
    expect(sanitizeAnalyticsSync({ ...partial, enabled: false, nextSyncAt: undefined }, context).enabled).toBe(false)
    const failed = { ...complete, status: 'failed', expiresAt: undefined, steps: keys.map((key) => ({ key, status: 'unavailable', reason: 'timeout', observedAt: complete.completedAt })) }
    expect(sanitizeAnalyticsSync(failed, context).status).toBe('failed')
    const running = { ...queued, enabled: false, status: 'running', startedAt: complete.startedAt, steps: keys.map((key) => ({ key, status: 'running' })) }
    expect(sanitizeAnalyticsSync(running, context).enabled).toBe(false)
    for (const override of [{ nextSyncAt: undefined }, { expiresAt: undefined }, { nextSyncAt: '2026-10-23T09:00:00Z' }, { queuedAt: '2026-02-30T10:00:00Z' }, { steps: complete.steps.map((step) => ({ ...step, observedAt: '2026-10-23T10:02:00Z' })) }]) expect(() => sanitizeAnalyticsSync({ ...complete, ...override }, context)).toThrow()
  })
  it('preserves bounded success history across failures without accepting future or idle success', () => {
    const lastSuccessAt = '2026-10-22T10:00:00Z'
    const failed = { ...complete, status: 'failed', expiresAt: undefined, steps: keys.map((key) => ({ key, status: 'unavailable', reason: 'timeout', observedAt: complete.completedAt, lastSuccessAt })) }
    expect(sanitizeAnalyticsSync(failed, context).steps[0]?.lastSuccessAt).toBe(lastSuccessAt)
    for (const invalid of ['bad', '2026-10-24T00:00:00Z']) expect(() => sanitizeAnalyticsSync({ ...failed, steps: failed.steps.map((s) => ({ ...s, lastSuccessAt: invalid })) }, context)).toThrow()
    expect(() => sanitizeAnalyticsSync({ ...idle, steps: idle.steps.map((s) => ({ ...s, lastSuccessAt })) }, context)).toThrow()
  })
  it('uses authenticated same-origin GET for metadata, explicit POST and PATCH with only selected authority', async () => {
    const fetcher = vi.fn().mockResolvedValueOnce(response(idle)).mockResolvedValueOnce(response(queued, 202)).mockResolvedValueOnce(response({ ...complete, enabled: false, nextSyncAt: undefined }))
    vi.stubGlobal('fetch', fetcher)
    await loadAnalyticsSync(context); await requestAnalyticsSync(context); await setAnalyticsSyncEnabled(context, false)
    expect(fetcher.mock.calls[0]?.[0]).toContain('/api/v1/visibility/analytics-sync?websiteId=')
    expect(fetcher.mock.calls[0]?.[0]).toContain('quarter=Q4')
    expect(fetcher.mock.calls.map((call) => call[1].method)).toEqual(['GET', 'POST', 'PATCH'])
    expect(JSON.parse(fetcher.mock.calls[1]?.[1].body)).toEqual(context)
    expect(JSON.parse(fetcher.mock.calls[2]?.[1].body)).toEqual({ ...context, enabled: false })
    expect(fetcher.mock.calls.every((call) => call[1].credentials === 'include' && call[1].redirect === 'error')).toBe(true)
  })
  it('rejects invalid context before fetch and never exposes provider error text', async () => {
    const fetcher = vi.fn().mockResolvedValue(new Response('token=private', { status: 401 })); vi.stubGlobal('fetch', fetcher)
    await expect(loadAnalyticsSync({ ...context, quarter: 'Q5' as 'Q4' })).rejects.toThrow()
    expect(fetcher).not.toHaveBeenCalled()
    await expect(loadAnalyticsSync(context)).rejects.toThrow('analytics_sync_unavailable')
    fetcher.mockResolvedValue(response(queued, 200))
    await expect(requestAnalyticsSync(context)).rejects.toThrow('analytics_sync_unavailable')
  })
})
