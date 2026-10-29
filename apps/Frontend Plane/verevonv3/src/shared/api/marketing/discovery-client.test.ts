import { afterEach, describe, expect, it, vi } from 'vitest'
import { loadWebsiteDiscovery, sanitizeWebsiteDiscovery, startWebsiteDiscovery } from './discovery-client.ts'

const context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 7 }
const complete = { ...context, source: 'quarry_map', status: 'complete', limit: 25, partial: true, pages: [{ url: 'https://triodelab.no/innsikt', title: 'Innsikt' }], discoveredAt: '2026-10-22T12:00:00Z' }
describe('website discovery projection', () => {
  afterEach(() => vi.unstubAllGlobals())
  it('rebuilds the narrow projection and strips provider metadata', () => {
    expect(sanitizeWebsiteDiscovery({ ...complete, token: 'private', pages: [{ ...complete.pages[0], html: 'private' }] }, context)).toEqual(complete)
  })
  it('rejects mixed contexts and inconsistent state', () => {
    for (const update of [{ websiteId: `web_${'2'.repeat(32)}` }, { site: 'https://agenci.no/' }, { websiteRevision: 8 }, { source: 'crawler' }, { limit: 50 }, { partial: false }, { status: ['complete'] }, { status: 'running' }, { status: 'failed', pages: [] }, { status: 'failed', pages: [], reason: ['not_configured'], discoveredAt: undefined }, { discoveredAt: undefined }, { discoveredAt: '2026-02-30T12:00:00Z' }, { discoveredAt: '2026-10-22' }, { reason: 'secret' }]) expect(() => sanitizeWebsiteDiscovery({ ...complete, ...update }, context)).toThrow()
    expect(sanitizeWebsiteDiscovery({ ...context, source: 'quarry_map', status: 'idle', limit: 25, partial: true, pages: [] }, context).status).toBe('idle')
    expect(sanitizeWebsiteDiscovery({ ...context, source: 'quarry_map', status: 'failed', limit: 25, partial: true, pages: [], reason: 'not_configured' }, context).reason).toBe('not_configured')
  })
  it('rejects unsafe, duplicate, oversized or off-origin URLs and unsafe titles', () => {
    for (const url of ['http://triodelab.no/x', 'https://news.triodelab.no/x', 'https://evil.example/', 'https://user@triodelab.no/', 'https://triodelab.no/x?q=private', 'https://triodelab.no/x#private', 'https://triodelab.no/x%0afoo', 'https://triodelab.no/x%250afoo', 'https://triodelab.no/x\nfoo', 'https://triodelab.no\\evil.example/', 'https://triodelab.no/a/../admin', 'https://triodelab.no/%2e%2e/api', 'https://triodelab.no/Admin/users', 'https://triodelab.no/%2561pi/users', 'https://triodelab.no/wp-login.php']) expect(() => sanitizeWebsiteDiscovery({ ...complete, pages: [{ url }] }, context)).toThrow()
    for (const pages of [[...complete.pages, ...complete.pages], Array.from({ length: 26 }, (_, i) => ({ url: `https://triodelab.no/${i}` })), [{ ...complete.pages[0], title: 'x'.repeat(201) }], [{ ...complete.pages[0], title: 'x\n' }]]) expect(() => sanitizeWebsiteDiscovery({ ...complete, pages }, context)).toThrow()
  })
  it('loads with GET only and starts once only when explicitly requested', async () => {
    const fetcher = vi.fn<(url: string, init?: RequestInit) => Promise<Response>>(async () => new Response(JSON.stringify({ data: complete }), { status: 200 }))
    vi.stubGlobal('fetch', fetcher)
    await loadWebsiteDiscovery(context)
    expect(fetcher.mock.calls[0]).toMatchObject([expect.stringContaining('/api/v1/visibility/discovery?'), { method: 'GET', credentials: 'include' }])
    await startWebsiteDiscovery(context)
    expect(fetcher.mock.calls[1]).toMatchObject(['/api/v1/visibility/discovery', { method: 'POST', body: JSON.stringify(context), credentials: 'include' }])
    expect(fetcher).toHaveBeenCalledTimes(2)
  })
  it('preserves observed apex/www URLs but never admits arbitrary subdomains', () => {
    for (const site of ['https://triodelab.no/', 'https://www.triodelab.no/']) {
      const expected = { ...context, site }
      const pages = [{ url: 'https://www.triodelab.no/innsikt', title: 'Innsikt' }, { url: 'https://triodelab.no/contact' }]
      expect(sanitizeWebsiteDiscovery({ ...complete, site, pages }, expected).pages).toEqual(pages)
      for (const url of ['https://news.triodelab.no/x', 'https://www.www.triodelab.no/x', 'https://www.triodelab.no:444/x', 'https://www.triodelab.no/admin']) {
        expect(() => sanitizeWebsiteDiscovery({ ...complete, site, pages: [{ url }] }, expected)).toThrow()
      }
    }
  })
  it('fails closed without retrying a rejected start or malformed local context', async () => {
    const fetcher = vi.fn(async () => new Response('{}', { status: 503 })); vi.stubGlobal('fetch', fetcher)
    await expect(startWebsiteDiscovery(context)).rejects.toThrow()
    expect(fetcher).toHaveBeenCalledTimes(1)
    await expect(startWebsiteDiscovery({ ...context, websiteRevision: 0 })).rejects.toThrow()
    expect(fetcher).toHaveBeenCalledTimes(1)
  })
})
