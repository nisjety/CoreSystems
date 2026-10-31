import { afterEach, describe, expect, it, vi } from 'vitest'
import { comparisonWindows, comparisonQuery, loadSearchComparison, sanitizeSearchComparison } from './search-comparison-client.ts'

const site = 'https://triodelab.no/'
const websiteId = 'web_' + '1'.repeat(32)
const row = { key: 'webdesign', clicks: 4, impressions: 200, ctr: 0.02, position: 8 }
const previous = { ...row, clicks: 10, impressions: 250, ctr: 0.04, position: 6 }
export const comparisonFixture = () => ({
  site, source: 'google_search_console', status: 'connected', dimension: 'query',
  currentPeriod: { startDate: '2026-09-22', endDate: '2026-10-19' },
  previousPeriod: { startDate: '2026-08-25', endDate: '2026-09-21' },
  rows: [{ key: row.key, state: 'compared', current: row, previous, clicksChange: -6, impressionsChange: -50, positionChange: 2, clicksChangePercent: -60 }],
  rowLimit: 100, rowsAtLimit: false, comparableRows: 1, unmatchedRows: 0,
})

describe('Search Console comparison boundary', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('builds equal adjacent windows ending three UTC calendar days ago, including year boundaries', () => {
    expect(comparisonWindows(28, new Date('2026-10-22T23:59:00Z'))).toEqual({ currentStart: '2026-09-22', currentEnd: '2026-10-19', previousStart: '2026-08-25', previousEnd: '2026-09-21' })
    const result = comparisonWindows(90, new Date('2026-01-02T01:00:00Z'))
    expect(result.currentEnd).toBe('2025-12-30')
    expect(Date.parse(result.currentStart) - Date.parse(result.previousStart)).toBe(90 * 86400000)
    expect(() => comparisonWindows(91 as 28)).toThrow()
  })

  it('projects only validated evidence and strips unexpected fields at every level', () => {
    const value = comparisonFixture()
    expect(sanitizeSearchComparison({ ...value, token: 'private', rows: [{ ...value.rows[0], connectionId: 'private', current: { ...row, token: 'private' } }] })).toEqual(value)
  })

  it('preserves one-sided observations without invented deltas and omits percentage for a zero baseline', () => {
    const fixture = comparisonFixture()
    expect(sanitizeSearchComparison({ ...fixture, comparableRows: 0, unmatchedRows: 1, rows: [{ key: row.key, state: 'not_comparable', current: row }] }).rows[0]).toEqual({ key: row.key, state: 'not_comparable', current: row })
    const baseline = { ...previous, clicks: 0, ctr: 0 }
    expect(sanitizeSearchComparison({ ...fixture, rows: [{ ...fixture.rows[0], previous: baseline, clicksChange: 4, clicksChangePercent: undefined }] }).rows[0]?.clicksChangePercent).toBeUndefined()
  })

  it.each([
    { rows: [{ key: row.key, state: 'not_comparable', current: row, clicksChange: -4 }] },
    { rows: [{ ...comparisonFixture().rows[0], clicksChange: 100 }] },
    { rows: [{ ...comparisonFixture().rows[0], current: { ...row, key: 'other' } }] },
    { rows: [{ ...comparisonFixture().rows[0], current: { ...row, clicks: -1 } }] },
    { rows: [{ ...comparisonFixture().rows[0], clicksChangePercent: -0.6 }] },
    { currentPeriod: { startDate: '2026-02-30', endDate: '2026-03-01' } },
    { previousPeriod: { startDate: '2026-08-24', endDate: '2026-09-20' } },
    { comparableRows: 0 }, { rowLimit: 1001 }, { status: 'unavailable', reason: 'not_configured' },
  ])('rejects contradictory evidence %#', (patch) => {
    expect(() => sanitizeSearchComparison({ ...comparisonFixture(), ...patch })).toThrow()
  })

  it('accepts empty reports and explicitly unavailable reports', () => {
    const empty = { ...comparisonFixture(), rows: [], comparableRows: 0 }
    expect(sanitizeSearchComparison(empty).rows).toEqual([])
    expect(sanitizeSearchComparison({ ...empty, status: 'unavailable', reason: 'property_unverified' }).reason).toBe('property_unverified')
  })

  it('builds and validates a bounded website-scoped request', async () => {
    const input = { site, websiteId, dimension: 'query' as const, ...comparisonWindows(28, new Date('2026-10-22Z')) }
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: comparisonFixture() })))
    vi.stubGlobal('fetch', fetchMock)
    expect((await loadSearchComparison(input)).rows[0]?.clicksChange).toBe(-6)
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/v1/visibility/search/compare?' + comparisonQuery(input))
    expect(fetchMock.mock.calls[0]?.[1]).toMatchObject({ credentials: 'include', headers: { Accept: 'application/json' } })
    expect(() => comparisonQuery({ ...input, websiteId: 'other' })).toThrow()
    fetchMock.mockResolvedValueOnce(new Response(JSON.stringify({ data: { ...comparisonFixture(), site: 'https://other.no/' } })))
    await expect(loadSearchComparison(input)).rejects.toThrow()
    fetchMock.mockResolvedValueOnce(new Response('{}', { status: 502 }))
    await expect(loadSearchComparison(input)).rejects.toThrow()
  })
})
