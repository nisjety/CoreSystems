import { describe, expect, it } from 'vitest'
import { sanitizeAutomaticTrends, sanitizeTrends, validTrendsQuery } from './trends-client'
const context = { websiteId: `web_${'a'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 2 }
const query = { terms: ['SEO'], geo: '', timeframe: 'today 12-m' as const }
const result = () => ({ ...context, report: { ...query, source: 'pytrends_dev', scale: 'relative_0_100_per_request', status: 'available', observedAt: new Date().toISOString(), cached: true, rows: [{ date: '2026-01-01', values: [0], partial: false }] } })
describe('Trends receipt boundary', () => {
  it('validates bounded terms without silently truncating research', () => {
    expect(validTrendsQuery(query)).toBe(true)
    for (const terms of [[], ['a', 'a'], ['a\nb'], Array.from({ length: 6 }, (_, i) => `${i}`), ['ø'.repeat(41)]]) expect(validTrendsQuery({ ...query, terms })).toBe(false)
  })
  it('preserves measured zero but rejects wrong context, market, indexes and missing failure data', () => {
    expect(sanitizeTrends(result(), context, query).rows[0]!.values).toEqual([0])
    for (const data of [{ ...result(), site: 'https://other.no/' }, { ...result(), report: { ...result().report, geo: 'US' } }, { ...result(), report: { ...result().report, status: 'rate_limited' } }, { ...result(), report: { ...result().report, rows: [{ date: '2026-01-01', values: [101], partial: false }] } }]) expect(() => sanitizeTrends(data, context, query)).toThrow()
  })
  it('requires a scoped, fresh report and authorized keyword seed for automatic data', () => {
    const r = result().report
    const ready = { ...context, source: 'marketing_trends_auto', status: 'complete', expiresAt: new Date(Date.now() + 3600000).toISOString(), seed: { source: 'google_search_console', selection: 'impressions_then_clicks', terms: query.terms, observedAt: r.observedAt, period: { startDate: '2026-10-01', endDate: '2026-10-21' } }, report: r }
    expect(sanitizeAutomaticTrends(ready, context).report?.rows[0]?.values[0]).toBe(0)
    for (const bad of [{ ...ready, websiteRevision: 3 }, { ...ready, seed: undefined }, { ...ready, expiresAt: new Date(Date.now() - 1000).toISOString() }, { ...ready, status: 'waiting_for_search_console' }, { ...ready, seed: { ...ready.seed, terms: ['different'] } }]) expect(() => sanitizeAutomaticTrends(bad, context)).toThrow()
  })
})
