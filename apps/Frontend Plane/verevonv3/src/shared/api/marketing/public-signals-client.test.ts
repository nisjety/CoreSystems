import { afterEach, describe, expect, it, vi } from 'vitest'
import { loadPublicSignals, requestPublicSignals, sanitizePublicSignals, setPublicSignalsEnabled } from './public-signals-client'
const context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 3 }
const idle = { ...context, source: 'marketing_public_signals', status: 'idle', enabled: false }
const queued = { ...idle, status: 'queued', enabled: true, jobId: `public_${'2'.repeat(32)}`, queuedAt: '2026-10-23T10:00:00Z' }
const report = { site: context.site, source: 'public_web_signals', observedAt: '2026-10-23T10:01:00Z', crux: [{ formFactor: 'PHONE', status: 'connected', period: { startDate: '2026-09-24', endDate: '2026-10-21' }, metrics: { lcpMs: 2200, inpMs: null, cls: 0 } }, { formFactor: 'DESKTOP', status: 'unavailable', reason: 'no_data' }], commonCrawl: { status: 'connected', index: 'CC-MAIN-2026-43', captures: [{ url: 'https://triodelab.no/innsikt', capturedAt: '2026-10-20T10:01:00Z', httpStatus: 200 }], limit: 20, truncated: false } }
const complete = { ...queued, status: 'complete', startedAt: '2026-10-23T10:00:01Z', completedAt: '2026-10-23T10:01:00Z', expiresAt: '2026-10-24T10:01:00Z', nextSyncAt: '2026-10-24T10:01:00Z', report }
const response = (data: unknown, status = 200) => new Response(JSON.stringify({ data }), { status })
describe('public signals boundary', () => {
  it('retains archive alias provenance and requires every capture to match it', () => {
    const archive = { ...report.commonCrawl, reportingHost: 'www.triodelab.no', captures: report.commonCrawl.captures.map((v)=>({...v,url:v.url.replace('triodelab.no','www.triodelab.no')})) }
    expect(sanitizePublicSignals({...complete,report:{...report,commonCrawl:archive}},context).report?.commonCrawl.reportingHost).toBe('www.triodelab.no')
    for(const host of ['other.no','blog.triodelab.no','www.triodelab.no:443'])expect(()=>sanitizePublicSignals({...complete,report:{...report,commonCrawl:{...archive,reportingHost:host}}},context)).toThrow()
    expect(()=>sanitizePublicSignals({...complete,report:{...report,commonCrawl:{...archive,captures:report.commonCrawl.captures}}},context)).toThrow()
  })
  it('preserves only the exact selected or discovered www reporting origin', () => {
    const withOrigin = (reportingOrigin: unknown) => ({ ...complete, report: { ...report, crux: [{ ...report.crux[0], reportingOrigin }, report.crux[1]] } })
    expect(sanitizePublicSignals(withOrigin('https://www.triodelab.no'), context).report?.crux[0]?.reportingOrigin).toBe('https://www.triodelab.no')
    expect(sanitizePublicSignals(withOrigin('https://triodelab.no'), context).report?.crux[0]?.reportingOrigin).toBe('https://triodelab.no')
    for (const origin of ['https://other.no', 'https://blog.triodelab.no', 'https://www.triodelab.no/', 'https://www.triodelab.no:443', 'http://triodelab.no', { token: 'private' }]) expect(() => sanitizePublicSignals(withOrigin(origin), context)).toThrow()
    expect(() => sanitizePublicSignals({ ...complete, report: { ...report, crux: [report.crux[0], { ...report.crux[1], reportingOrigin: 'https://triodelab.no' }] } }, context)).toThrow()
  })
  afterEach(() => vi.unstubAllGlobals())
  it('preserves bounded provider timeouts and index provenance without accepting raw errors', () => {
    const unavailable = { status: 'unavailable', reason: 'timeout', index: 'CC-MAIN-2026-43', captures: [], limit: 20, truncated: false }
    const value = sanitizePublicSignals({ ...complete, report: { ...report, crux: report.crux.map((device) => ({ formFactor: device.formFactor, status: 'unavailable', reason: 'timeout' })), commonCrawl: unavailable } }, context)
    expect(value.report?.commonCrawl).toEqual(unavailable)
    expect(value.report?.crux.every((device) => device.reason === 'timeout')).toBe(true)
    expect(() => sanitizePublicSignals({ ...complete, report: { ...report, commonCrawl: { ...unavailable, reason: 'nginx token=private' } } }, context)).toThrow()
  })
  it('rebuilds only scoped report fields and preserves missing metrics rather than inventing zero', () => {
    expect(sanitizePublicSignals({ ...complete, token: 'private', report: { ...report, providerSecret: 'private', crux: report.crux.map((item) => ({ ...item, raw: 'private' })), commonCrawl: { ...report.commonCrawl, raw: 'private', captures: report.commonCrawl.captures.map((item) => ({ ...item, token: 'private' })) } } }, context)).toEqual(complete)
    expect(sanitizePublicSignals(idle, context)).toEqual(idle)
    expect(sanitizePublicSignals({ ...complete, report: undefined, enabled: false, nextSyncAt: undefined }, context).expiresAt).toBe(complete.expiresAt)
  })
  it('rejects mismatched context, invalid job lifecycle and nonclosed reason fields', () => {
    for (const change of [{ site: 'https://example.com/' }, { websiteRevision: 4 }, { source: 'provider' }, { status: 'healthy' }, { status: 'idle' }, { status: 'queued' }, { status: 'running' }, { status: 'failed', reason: 'source_unavailable' }, { expiresAt: '2026-10-25T10:01:00Z' }, { reason: 'token=private' }, { nextSyncAt: undefined }, { jobId: 'wrong' }, { completedAt: '2026-02-30T10:01:00Z' }]) expect(() => sanitizePublicSignals({ ...complete, ...change }, context)).toThrow()
    const failed = { ...complete, status: 'failed', reason: 'timeout', report: undefined, expiresAt: undefined, nextSyncAt: '2026-10-23T16:01:00Z' }
    expect(sanitizePublicSignals(failed, context).reason).toBe('timeout')
  })
  it('validates exact CrUX origins, form factors, actual dates and finite nullable metrics', () => {
    for (const crux of [[], [report.crux[0], report.crux[0]], [report.crux[0], { ...report.crux[1], reason: 'secret' }], [{ ...report.crux[0], metrics: { lcpMs: null, inpMs: null, cls: null } }, report.crux[1]], [{ ...report.crux[0], metrics: { lcpMs: -1, inpMs: null, cls: 0 } }, report.crux[1]], [{ ...report.crux[0], period: { startDate: '2026-01-01', endDate: '2026-10-21' } }, report.crux[1]], [{ ...report.crux[0], period: { startDate: '2026-02-30', endDate: '2026-10-21' } }, report.crux[1]]]) expect(() => sanitizePublicSignals({ ...complete, report: { ...report, crux } }, context)).toThrow()
    expect(() => sanitizePublicSignals({ ...complete, report: { ...report, site: 'https://www.triodelab.no/' } }, context)).toThrow()
  })
  it('accepts only bounded exact-host historical captures and strips all upstream extras', () => {
    for (const url of ['https://www.triodelab.no/', 'https://other.example/', 'https://triodelab.no/?token=secret', 'https://triodelab.no/login', 'https://triodelab.no/%2561dmin', 'https://triodelab.no/#fragment', 'https://name:secret@triodelab.no/', 'javascript:alert(1)']) expect(() => sanitizePublicSignals({ ...complete, report: { ...report, commonCrawl: { ...report.commonCrawl, captures: [{ ...report.commonCrawl.captures[0], url }] } } }, context)).toThrow()
    expect(() => sanitizePublicSignals({ ...complete, report: { ...report, commonCrawl: { ...report.commonCrawl, captures: Array.from({ length: 21 }, () => report.commonCrawl.captures[0]) } } }, context)).toThrow()
    expect(() => sanitizePublicSignals({ ...complete, report: { ...report, commonCrawl: { ...report.commonCrawl, index: 'bad-index' } } }, context)).toThrow()
    expect(sanitizePublicSignals({ ...complete, report: { ...report, commonCrawl: { status: 'unavailable', reason: 'no_data', captures: [], limit: 20, truncated: false } } }, context).report?.commonCrawl.captures).toEqual([])
  })
  it('accepts a separately validated prior report during refresh, never across site context', () => {
    const value = sanitizePublicSignals({ ...queued, cachedReport: { ...report, observedAt: '2026-10-22T10:00:00Z' } }, context)
    expect(value.report).toBeUndefined()
    expect(value.cachedReport?.observedAt).toBe('2026-10-22T10:00:00Z')
    expect(() => sanitizePublicSignals({ ...queued, cachedReport: { ...report, site: 'https://other.no/' } }, context)).toThrow()
    expect(() => sanitizePublicSignals({ ...queued, cachedReport: report }, context)).toThrow()
    expect(() => sanitizePublicSignals({ ...idle, cachedReport: report }, context)).toThrow()
  })
  it('uses GET-only reads and explicit authority-bound writes with exact HTTP response status', async () => {
    const fetcher = vi.fn().mockResolvedValueOnce(response(idle)).mockResolvedValueOnce(response(queued, 202)).mockResolvedValueOnce(response({ ...complete, enabled: false, nextSyncAt: undefined })); vi.stubGlobal('fetch', fetcher)
    await loadPublicSignals(context); await requestPublicSignals(context); await setPublicSignalsEnabled(context, false)
    expect(fetcher.mock.calls.map((call) => call[1].method)).toEqual(['GET', 'POST', 'PATCH'])
    expect(fetcher.mock.calls[0]?.[0]).toContain('/public-signals?websiteId=')
    expect(JSON.parse(fetcher.mock.calls[1]?.[1].body)).toEqual(context)
    expect(JSON.parse(fetcher.mock.calls[2]?.[1].body)).toEqual({ ...context, enabled: false })
    expect(fetcher.mock.calls.every((call) => call[1].credentials === 'include' && call[1].redirect === 'error')).toBe(true)
    fetcher.mockResolvedValue(new Response('private', { status: 401 })); await expect(loadPublicSignals(context)).rejects.toThrow('public_signals_unavailable')
    fetcher.mockResolvedValue(response(queued)); await expect(requestPublicSignals(context)).rejects.toThrow('public_signals_unavailable')
  })
})
