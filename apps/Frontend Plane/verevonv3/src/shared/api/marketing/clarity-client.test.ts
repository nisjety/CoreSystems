import { afterEach, describe, expect, it, vi } from 'vitest'
import { loadClarity, sanitizeClaritySnapshot } from './clarity-client.ts'
const site = 'https://example.org/'
const websiteId = 'web_' + '1'.repeat(32)
const snapshot = { site, source: 'microsoft_clarity', status: 'connected', scope: 'clarity_project', projectId: 'project123', window: { numOfDays: 3, timeZone: 'UTC', observedAt: '2026-10-21T12:00:00Z' }, metrics: { sessions: 12, botSessions: 0, users: null, pagesPerSession: 1.5 }, updatedAt: '2026-10-21T12:00:00Z' }
describe('Clarity projection boundary', () => {
  it('keeps validated UX aggregates without private replay data', () => {
    const value = sanitizeClaritySnapshot({ ...snapshot, metrics: { ...snapshot.metrics, ux: { deadClickPercent: 0, averageScrollDepth: 30.27, activeSeconds: 37, totalSeconds: 59, recordingUrl: 'private' } } })
    expect(value.metrics?.ux?.deadClickPercent).toBe(0)
    expect(value.metrics?.ux?.rageClickPercent).toBeNull()
    expect(value.metrics?.ux?.averageScrollDepth).toBe(30.27)
    expect(JSON.stringify(value)).not.toContain('private')
    for (const ux of [{ rageClickPercent: 101 }, { averageScrollDepth: -1 }, { activeSeconds: 60, totalSeconds: 59 }, { scriptErrorPercent: '0' }]) expect(() => sanitizeClaritySnapshot({ ...snapshot, metrics: { ...snapshot.metrics, ux } })).toThrow()
  })
  afterEach(() => vi.unstubAllGlobals())
  it('preserves measured zero versus unavailable metrics, strips unsupported UX and secret fields', () => {
    const value = sanitizeClaritySnapshot({ ...snapshot, token: 'private', window: { ...snapshot.window, projectToken: 'private' }, metrics: { ...snapshot.metrics, rageClicks: 99, recordingUrl: 'private' } })
    expect(value).toEqual(snapshot)
    expect(value.metrics?.botSessions).toBe(0)
    expect(value.metrics?.users).toBeNull()
    expect(JSON.stringify(value)).not.toContain('private')
    expect(sanitizeClaritySnapshot({ ...snapshot, metrics: { sessions: 0 } }).metrics).toEqual({ sessions: 0, botSessions: null, users: null, pagesPerSession: null })
  })
  it('accepts a safe unavailable state without project or invented metrics', () => {
    const value = sanitizeClaritySnapshot({ site, source: 'microsoft_clarity', status: 'unavailable', scope: 'clarity_project', reason: 'not_configured', window: { numOfDays: 3, timeZone: 'UTC' }, metrics: null })
    expect(value.metrics).toBeNull()
    expect(value.projectId).toBeUndefined()
  })
  it.each([
    { scope: 'exact_url' }, { source: 'provider_raw' }, { projectId: '../project' }, { status: 'pending' },
    { window: { numOfDays: 1, timeZone: 'UTC' } }, { window: { numOfDays: 3, timeZone: 'Europe/Oslo' } },
    { window: { ...snapshot.window, observedAt: 'yesterday' } }, { reason: 'authorization_required' },
    { metrics: { sessions: -1 } }, { metrics: { sessions: 1.2 } }, { metrics: { users: Number.MAX_SAFE_INTEGER + 1 } },
    { metrics: { pagesPerSession: Infinity } }, { metrics: { pagesPerSession: '1.5' } },
    { status: 'unavailable', reason: 'private-provider-error', metrics: null },
    { status: 'unavailable', reason: 'quota_limited' },
  ])('rejects false scope, unsupported windows and invalid metrics %j', (patch) => {
    expect(() => sanitizeClaritySnapshot({ ...snapshot, ...patch })).toThrow()
  })
  it('requests fixed recent project data by selected website, without quarter or provider credentials', async () => {
    const fetcher = vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: snapshot }))); vi.stubGlobal('fetch', fetcher)
    expect(await loadClarity(site, websiteId)).toEqual(snapshot)
    expect(fetcher).toHaveBeenCalledWith(`/api/v1/visibility/clarity?site=${encodeURIComponent(site)}&websiteId=${websiteId}`, expect.objectContaining({ credentials: 'include' }))
    await expect(loadClarity(site, 'bad-id')).rejects.toThrow()
    expect(fetcher).toHaveBeenCalledTimes(1)
  })
  it('rejects another site and HTTP errors without relaying private error bodies', async () => {
    const fetcher = vi.fn().mockResolvedValueOnce(new Response(JSON.stringify({ data: { ...snapshot, site: 'https://other.example/' } })))
      .mockResolvedValueOnce(new Response('private-provider-error', { status: 500 }))
    vi.stubGlobal('fetch', fetcher)
    await expect(loadClarity(site, websiteId)).rejects.toThrow('Unexpected Clarity website')
    await expect(loadClarity(site, websiteId)).rejects.toThrow('Clarity is unavailable')
  })
})
