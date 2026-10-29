import { describe, expect, it } from 'vitest'
import { sanitizeSiteSource } from './site-sources-client'
const context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 7 }
const base = { ...context, provider: 'algolia', status: 'connected', observedAt: '2026-10-23T10:00:00Z', window: { startDate: '2026-10-16', endDate: '2026-10-22' } }
const analytics = { status: 'unavailable', reportedDays: 0 }
const data = (terms: unknown) => ({ ...base, algolia: { indexName: 'public_index', region: 'provider', analytics, topSearches: terms, noResultTerms: { status: 'unavailable', limit: 20 } } })
describe('bounded aggregate Algolia search terms', () => {
  it('keeps independently reported terms and explicit empty arrays without inventing aggregate counts', () => {
    const result = sanitizeSiteSource(data({ status: 'connected', limit: 20, rows: [{ query: '', count: 2 }, { query: '<script>alert(1)</script>', count: 1, averageResults: 1.5, userId: 'drop' }] }), context, 'algolia')
    expect(result.algolia?.topSearches?.rows).toEqual([{ query: '', count: 2 }, { query: '<script>alert(1)</script>', count: 1, averageResults: 1.5 }])
    expect(result.algolia?.analytics.searches).toBeUndefined()
    expect(sanitizeSiteSource(data({ status: 'connected', limit: 20, rows: [] }), context, 'algolia').algolia?.topSearches?.rows).toEqual([])
  })
  it('rejects malformed, excessive, control-bearing and non-UTF8 query observations', () => {
    for (const row of [{ query: '\n', count: 1 }, { query: '\u0085', count: 1 }, { query: '\ud800', count: 1 }, { query: 'x'.repeat(513), count: 1 }, { query: 'x', count: -1 }, { query: 'x', count: 1, averageResults: Infinity }]) expect(() => sanitizeSiteSource(data({ status: 'connected', limit: 20, rows: [row] }), context, 'algolia')).toThrow()
    for (const terms of [{ status: 'connected', limit: 20 }, { status: 'connected', limit: 21, rows: [] }, { status: 'unavailable', limit: 20, rows: [] }, { status: 'connected', limit: 20, rows: Array.from({ length: 21 }, () => ({ query: 'x', count: 1 })) }]) expect(() => sanitizeSiteSource(data(terms), context, 'algolia')).toThrow()
    expect(()=>sanitizeSiteSource(data({status:'connected',limit:20,rows:[{query:'',count:1},{query:'',count:2}]}),context,'algolia')).toThrow()
    expect(sanitizeSiteSource(data({ status: 'connected', limit: 20, rows: [{ query: '😀'.repeat(512), count: Number.MAX_SAFE_INTEGER }] }), context, 'algolia').algolia?.topSearches?.rows).toHaveLength(1)
  })
})
