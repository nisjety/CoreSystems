import { afterEach, describe, expect, it, vi } from 'vitest'
import { loadSiteSource, sanitizeSiteSource, syncSiteSource } from './site-sources-client'
const context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 7 }
const common = { ...context, observedAt: '2026-10-23T10:00:00Z', window: { startDate: '2026-10-16', endDate: '2026-10-22' }, status: 'connected' }
const cookie = { ...common, provider: 'cookiebot', cookiebot: { reportingDomain: 'www.triodelab.no', consents: { status: 'connected', reportedDays: 7, totals: { optIn: 12, optOut: 3, preferences: 8, statistics: 10, marketing: 5 } }, inventory: { status: 'connected', scannedAt: '2026-10-20T09:00:00Z', total: 5, categories: { necessary: 2, preferences: 0, statistics: 1, marketing: 2, unclassified: 0 }, thirdParty: 3, secure: 4, httpOnly: 1 } } }
const algolia = { ...common, provider: 'algolia', algolia: { indexName: 'triodelab_public', region: 'de', analytics: { status: 'connected', reportedDays: 5, searches: 10, noResultSearches: 2, noResultRate: 0.2 } } }
const response = (data: unknown, status = 200) => new Response(JSON.stringify({ data }), { status })
describe('site-source aggregate boundary', () => {
  afterEach(() => vi.unstubAllGlobals())
  it('projects only normalized provider aggregates, never secrets or query logs', () => {
    expect(sanitizeSiteSource({ ...cookie, token: 'secret', cookiebot: { ...cookie.cookiebot, apiKey: 'secret' } }, context, 'cookiebot')).toEqual(cookie)
    expect(sanitizeSiteSource({ ...algolia, algolia: { ...algolia.algolia, queries: ['private search'], analytics: { ...algolia.algolia.analytics, secret: 'secret' } } }, context, 'algolia')).toEqual(algolia)
  })
  it('keeps not-loaded and unconfigured distinct and rejects payloads for them', () => {
    for (const status of ['not_loaded', 'not_configured']) {
      const value = { ...common, provider: 'cookiebot', status }
      expect(sanitizeSiteSource(value, context, 'cookiebot')).toEqual(value)
      expect(() => sanitizeSiteSource({ ...cookie, status }, context, 'cookiebot')).toThrow()
    }
  })
  it('requires exact site/revision/provider and seven completed UTC dates', () => {
    for (const change of [{ websiteRevision: 8 }, { site: 'https://other.example/' }, { provider: 'algolia' }, { status: 'healthy' }, { observedAt: '2026-02-30T00:00:00Z' }, { window: { startDate: '2026-10-17', endDate: '2026-10-23' } }, { window: { startDate: '2026-10-15', endDate: '2026-10-22' } }, { algolia: algolia.algolia }]) expect(() => sanitizeSiteSource({ ...cookie, ...change }, context, 'cookiebot')).toThrow()
  })
  it('accepts honest no-data components and enforces partial/connected rollups', () => {
    const emptyConsent = { status: 'no_data', reportedDays: 0 }
    expect(sanitizeSiteSource({ ...cookie, cookiebot: { ...cookie.cookiebot, consents: emptyConsent } }, context, 'cookiebot').status).toBe('connected')
    const partial = { ...cookie, status: 'partial', cookiebot: { ...cookie.cookiebot, consents: { status: 'plan_required', reportedDays: 0 } } }
    expect(sanitizeSiteSource(partial, context, 'cookiebot').status).toBe('partial')
    expect(() => sanitizeSiteSource({ ...partial, status: 'connected' }, context, 'cookiebot')).toThrow()
    expect(() => sanitizeSiteSource({ ...cookie, cookiebot: { ...cookie.cookiebot, consents: { ...emptyConsent, totals: cookie.cookiebot.consents.totals } } }, context, 'cookiebot')).toThrow()
  })
  it('rejects unsafe Cookiebot domains, counts, scan dates and category sums', () => {
    for (const reportingDomain of ['other.example', 'https://triodelab.no', 'triodelab.no/path', 'user@triodelab.no']) expect(() => sanitizeSiteSource({ ...cookie, cookiebot: { ...cookie.cookiebot, reportingDomain } }, context, 'cookiebot')).toThrow()
    for (const inventory of [{ ...cookie.cookiebot.inventory, total: 6 }, { ...cookie.cookiebot.inventory, secure: 6 }, { ...cookie.cookiebot.inventory, scannedAt: '2026-10-24T00:00:00Z' }, { ...cookie.cookiebot.inventory, status: 'access_denied' }, { ...cookie.cookiebot.inventory, total: -1 }]) expect(() => sanitizeSiteSource({ ...cookie, cookiebot: { ...cookie.cookiebot, inventory } }, context, 'cookiebot')).toThrow()
    for (const consents of [{ ...cookie.cookiebot.consents, reportedDays: 0 }, { ...cookie.cookiebot.consents, reportedDays: 8 }, { ...cookie.cookiebot.consents, totals: { ...cookie.cookiebot.consents.totals, optIn: Number.MAX_SAFE_INTEGER + 1 } }]) expect(() => sanitizeSiteSource({ ...cookie, cookiebot: { ...cookie.cookiebot, consents } }, context, 'cookiebot')).toThrow()
  })
  it('requires observed Algolia rates and preserves explicit zero-search null rates', () => {
    const zero = { ...algolia, algolia: { ...algolia.algolia, analytics: { status: 'connected', reportedDays: 0, searches: 0, noResultSearches: 0, noResultRate: null } } }
    expect(sanitizeSiteSource(zero, context, 'algolia')).toEqual(zero)
    expect(sanitizeSiteSource({ ...algolia, algolia: { ...algolia.algolia, region: 'provider' } }, context, 'algolia').algolia?.region).toBe('provider')
    for (const analytics of [{ ...zero.algolia.analytics, noResultRate: 0 }, { ...algolia.algolia.analytics, noResultRate: 0.8 }, { ...algolia.algolia.analytics, reportedDays: 0 }, { ...algolia.algolia.analytics, noResultSearches: 11 }, { ...algolia.algolia.analytics, searches: -1 }, { ...algolia.algolia.analytics, status: 'unavailable' }]) expect(() => sanitizeSiteSource({ ...algolia, algolia: { ...algolia.algolia, analytics } }, context, 'algolia')).toThrow()
    expect(() => sanitizeSiteSource({ ...algolia, algolia: { ...algolia.algolia, region: 'eu' } }, context, 'algolia')).toThrow()
    expect(() => sanitizeSiteSource({ ...algolia, algolia: { ...algolia.algolia, indexName: 'private\nindex' } }, context, 'algolia')).toThrow()
    expect(() => sanitizeSiteSource({ ...algolia, algolia: { ...algolia.algolia, indexName: 'index name' } }, context, 'algolia')).toThrow()
    expect(sanitizeSiteSource({ ...algolia, algolia: { ...algolia.algolia, indexName: '_existing-public' } }, context, 'algolia').algolia?.indexName).toBe('_existing-public')
  })
  it('reads with exact GET query and syncs only via explicit revision-bound POST', async () => {
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(response(cookie))); vi.stubGlobal('fetch', fetcher)
    await loadSiteSource(context, 'cookiebot'); await syncSiteSource(context, 'cookiebot')
    expect(fetcher.mock.calls[0]![0]).toBe(`/api/v1/visibility/site-sources?websiteId=${context.websiteId}&site=https%3A%2F%2Ftriodelab.no%2F&provider=cookiebot`)
    expect(fetcher.mock.calls[0]![1]).toMatchObject({ method: 'GET', credentials: 'include', redirect: 'error' })
    expect(fetcher.mock.calls[1]![1]).toMatchObject({ method: 'POST', body: JSON.stringify({ ...context, provider: 'cookiebot' }) })
    fetcher.mockResolvedValue(response(cookie, 202)); await expect(syncSiteSource(context, 'cookiebot')).rejects.toThrow('site_source_unavailable')
    await expect(loadSiteSource(context, 'other' as 'cookiebot')).rejects.toThrow(); expect(fetcher).toHaveBeenCalledTimes(3)
  })
})
