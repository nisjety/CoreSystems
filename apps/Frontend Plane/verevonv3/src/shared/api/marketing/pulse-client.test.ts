import { afterEach, describe, expect, it, vi } from 'vitest'
import { buildPulseChart, loadPulse, scalePulseBars } from './pulse-client.ts'

describe('Visibility read-only client', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('loads only the selected verified site projection without sending credentials', async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: {
      site: 'https://aquatiq.com/', source: 'google_search_console', status: 'connected', propertyScope: 'domain',
      period: { startDate: '2026-10-01', endDate: '2026-10-20' },
      totals: { clicks: 12, impressions: 300 },
      days: [{ date: '2026-10-01', clicks: 12, impressions: 300 }],
    } }), { status: 200, headers: { 'Content-Type': 'application/json' } }))
    vi.stubGlobal('fetch', fetchMock)

    const result = await loadPulse('Q4', 'https://aquatiq.com/')

    expect(fetchMock).toHaveBeenCalledWith('/api/v1/visibility/pulse?quarter=Q4&site=https%3A%2F%2Faquatiq.com%2F', expect.objectContaining({ credentials: 'include' }))
    expect(result.totals?.clicks).toBe(12)
    expect(result.propertyScope).toBe('domain')
  })

  it('rejects data for a previously selected site', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: {
      site: 'https://aquatiq.com/', source: 'google_search_console', status: 'connected',
      period: { startDate: '2026-10-01', endDate: '2026-10-20' },
      totals: { clicks: 12, impressions: 300 }, days: [],
    } }), { status: 200 })))
    await expect(loadPulse('Q4', 'https://example.com/')).rejects.toThrow('Invalid Visibility response')
  })

  it('rejects an invalid backend payload instead of inventing analytics', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: { totals: { clicks: '12' } } }), { status: 200 })))
    await expect(loadPulse('Q4', 'https://aquatiq.com/')).rejects.toThrow('Invalid Visibility response')
  })

  it('treats a 401 as an unavailable API, without an unlock request', async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response('{}', { status: 401 }))
    vi.stubGlobal('fetch', fetchMock)
    await expect(loadPulse('Q4', 'https://aquatiq.com/')).rejects.toThrow('Visibility data is unavailable')
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it('preserves an honest unconfigured state without making up zero metrics', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: {
      site: 'https://aquatiq.com/', source: 'google_search_console', status: 'unavailable', reason: 'not_configured',
      period: { startDate: '2026-10-01', endDate: '2026-10-20' },
      totals: null, days: [],
    } }), { status: 200 })))
    const result = await loadPulse('Q4', 'https://aquatiq.com/')
    expect(result).toMatchObject({ status: 'unavailable', reason: 'not_configured', totals: null, days: [] })
  })

  it('scales only actual daily values and retains zeros', () => {
    expect(scalePulseBars([{ date: '2026-10-01', clicks: 0, impressions: 0 }, { date: '2026-10-02', clicks: 5, impressions: 10 }], 'clicks')).toEqual([0, 100])
  })

  it('places sparse Google rows on their real calendar dates', () => {
    const slots = buildPulseChart(
      { startDate: '2026-10-01', endDate: '2026-10-03' },
      [{ date: '2026-10-01', clicks: 2, impressions: 4 }, { date: '2026-10-03', clicks: 4, impressions: 8 }],
      'clicks',
    )
    expect(slots.map((slot) => [slot.date, slot.height, slot.hasValue])).toEqual([
      ['2026-10-01', 50, true], ['2026-10-02', 0, false], ['2026-10-03', 100, true],
    ])
  })

  it('distinguishes a measured zero from a day without a Google row', () => {
    expect(buildPulseChart(
      { startDate: '2026-10-01', endDate: '2026-10-02' },
      [{ date: '2026-10-01', clicks: 0, impressions: 0 }], 'clicks',
    )).toEqual([
      { date: '2026-10-01', height: 0, hasValue: true },
      { date: '2026-10-02', height: 0, hasValue: false },
    ])
  })

  it('ignores out-of-period rows when scaling the selected calendar', () => {
    expect(buildPulseChart(
      { startDate: '2026-10-01', endDate: '2026-10-01' },
      [{ date: '2026-09-30', clicks: 1000, impressions: 10000 }, { date: '2026-10-01', clicks: 5, impressions: 10 }], 'clicks',
    )).toEqual([{ date: '2026-10-01', height: 100, hasValue: true }])
    expect(buildPulseChart({ startDate: '2026-02-30', endDate: '2026-03-02' }, [], 'clicks')).toEqual([])
  })
})
