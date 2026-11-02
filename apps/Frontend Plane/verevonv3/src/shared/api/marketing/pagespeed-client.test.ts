import { afterEach, describe, expect, it, vi } from 'vitest'
import { loadPageSpeed, startPageSpeed, sanitizePageSpeedSnapshot } from './pagespeed-client.ts'

const site = 'https://aquatiq.com/'
const report = { performance: 89, accessibility: null, bestPractices: 100, seo: 90, lcpMs: 2500, cls: 0, tbtMs: 18, fcpMs: null, speedIndexMs: 2000, fetchTime: '2026-10-21T00:00:00Z', lighthouseVersion: '12.8.2' }
const complete = { site, source: 'google_pagespeed', strategy: 'mobile', status: 'complete', report }
describe('PageSpeed client boundary', () => {
  afterEach(() => vi.unstubAllGlobals())
  it('preserves the last successful report during a failed daily refresh and strips cache extras', () => {
    const cache = { stale: true, refreshStatus: 'failed', refreshReason: 'quota_limited', lastSuccessAt: report.fetchTime, lastAttemptAt: '2026-10-22T00:00:00Z', nextRefreshAt: '2026-10-22T01:00:00Z' }
    expect(sanitizePageSpeedSnapshot({ ...complete, cache: { ...cache, credential: 'private' } })).toEqual({ ...complete, cache })
    expect(sanitizePageSpeedSnapshot({ ...complete, cache: { ...cache, refreshStatus: 'running', refreshReason: undefined } }).report).toEqual(report)
    for (const invalid of [{ ...cache, stale: 'true' }, { ...cache, refreshStatus: 'unknown' }, { ...cache, lastSuccessAt: '2026-10-20T00:00:00Z' }, { ...cache, nextRefreshAt: 'invalid' }, { ...cache, refreshReason: 'raw error' }, { ...cache, refreshStatus: 'running' }]) expect(() => sanitizePageSpeedSnapshot({ ...complete, cache: invalid })).toThrow()
    expect(() => sanitizePageSpeedSnapshot({ site, source: 'google_pagespeed', strategy: 'mobile', status: 'idle', cache })).toThrow()
  })
  it('strips every private extra field while preserving null and measured zero', () => {
    const value = sanitizePageSpeedSnapshot({ ...complete, token: 'private', report: { ...report, secret: 'private' } })
    expect(value).toEqual(complete)
    expect(value.report?.accessibility).toBeNull()
    expect(value.report?.cls).toBe(0)
  })
  it('rejects unknown states, invalid metrics, dates and incomplete reports', () => {
    for (const value of [
      { ...complete, status: 'queued' }, { ...complete, source: 'provider_raw' },
      { ...complete, report: { ...report, performance: 101 } },
      { ...complete, report: { ...report, cls: -1 } },
      { ...complete, report: { ...report, tbtMs: Infinity } },
      { ...complete, report: { ...report, fetchTime: 'invalid' } },
      { ...complete, report: { ...report, performance: undefined } },
      { ...complete, status: 'running' }, { ...complete, reason: 'quota_limited' },
    ]) expect(() => sanitizePageSpeedSnapshot(value)).toThrow()
  })
  it('rebuilds normalized technical checks without nested provider fields', () => {
    const checks = [{ id: 'canonical', status: 'failed', score: 0, title: 'secret', details: { url: 'private' } }, { id: 'structured-data', status: 'manual', score: null }]
    const value = sanitizePageSpeedSnapshot({ ...complete, report: { ...report, checks } })
    expect(value.report?.checks).toEqual([{ id: 'canonical', status: 'failed', score: 0 }, { id: 'structured-data', status: 'manual', score: null }])
    expect(JSON.stringify(value)).not.toMatch(/private|secret|details/)
    expect(sanitizePageSpeedSnapshot(complete).report).not.toHaveProperty('checks')
  })
  it('rejects invalid IDs, duplicate checks, oversize lists and status/score mismatches', () => {
    const invalid = [null, {}, [{ id: 'unknown', status: 'passed', score: 1 }],
      [{ id: 'canonical', status: 'passed', score: 1 }, { id: 'canonical', status: 'failed', score: 0 }],
      Array.from({ length: 12 }, () => ({ id: 'canonical', status: 'passed', score: 1 })),
      [{ id: 'canonical', status: 'passed', score: 0 }], [{ id: 'canonical', status: 'failed', score: 1 }],
      [{ id: 'canonical', status: 'manual', score: 1 }], [{ id: 'canonical', status: 'error', score: 0 }],
      [{ id: 'canonical', status: 'not_reported', score: undefined }], [{ id: 'canonical', status: 'complete', score: null }],
      [{ id: 'canonical', status: ['manual'], score: null }],
    ]
    for (const checks of invalid) expect(() => sanitizePageSpeedSnapshot({ ...complete, report: { ...report, checks } })).toThrow()
  })
  it('accepts the entire fixed scope and every normalized status without inventing scores', () => {
    const checks = [
      { id: 'document-title', status: 'passed', score: 1 }, { id: 'meta-description', status: 'failed', score: 0 },
      { id: 'http-status-code', status: 'informative', score: null }, { id: 'is-crawlable', status: 'error', score: null },
      { id: 'robots-txt', status: 'not_reported', score: null }, { id: 'canonical', status: 'not_applicable', score: null },
      { id: 'hreflang', status: 'passed', score: 1 }, { id: 'link-text', status: 'failed', score: 0 },
      { id: 'crawlable-anchors', status: 'passed', score: 1 }, { id: 'image-alt', status: 'passed', score: 1 },
      { id: 'structured-data', status: 'manual', score: null },
    ]
    expect(sanitizePageSpeedSnapshot({ ...complete, report: { ...report, checks } }).report?.checks).toEqual(checks)
  })
  it('only starts audits explicitly with a fixed JSON command and no credentials', async () => {
    const fetchMock = vi.fn().mockImplementation(async () => new Response(JSON.stringify({ data: { site, source: 'google_pagespeed', strategy: 'mobile', status: 'running' } })))
    vi.stubGlobal('fetch', fetchMock)
    await loadPageSpeed(site, 'mobile')
    expect(fetchMock.mock.calls[0]?.[1].method).toBe('GET')
    await startPageSpeed(site, 'mobile')
    expect(fetchMock.mock.calls[1]?.[0]).toBe('/api/v1/visibility/pagespeed')
    expect(fetchMock.mock.calls[1]?.[1]).toMatchObject({ method: 'POST', credentials: 'include', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ site, strategy: 'mobile' }) })
    await expect(startPageSpeed('http://localhost/', 'mobile')).rejects.toThrow()
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })
  it('does not accept another website, device or HTTP failure', async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(new Response(JSON.stringify({ data: { ...complete, site: 'https://example.com/' } })))
      .mockResolvedValueOnce(new Response(JSON.stringify({ data: { ...complete, strategy: 'desktop' } })))
      .mockResolvedValueOnce(new Response('{}', { status: 502 }))
    vi.stubGlobal('fetch', fetchMock)
    for (let index = 0; index < 3; index++) await expect(loadPageSpeed(site, 'mobile')).rejects.toThrow()
  })
})
