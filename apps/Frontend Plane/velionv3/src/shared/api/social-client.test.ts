import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  listSocialCatalogProducts,
  listSocialCatalogs,
  listSocialMetrics,
  type SocialMetric,
} from '@/shared/api/social-client'

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('social-client metrics + catalog reads', () => {
  it('requests metrics with org header and optional query params', async () => {
    const metrics: SocialMetric[] = [
      {
        accountId: 'soc_acct_1',
        connectionId: 'conn_1',
        providerKey: 'facebook',
        metricName: 'impressions',
        metricValue: 1234,
        dimensions: { campaign: 'launch' },
        snapshotDate: '2026-07-07T00:00:00Z',
      },
    ]
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ metrics })))

    const result = await listSocialMetrics('org_acme', { snapshotDate: '2026-07-07' })

    const [url, init] = vi.mocked(fetch).mock.calls[0]!
    const headers = init?.headers as Headers
    expect(String(url)).toBe('/api/v1/social/metrics?snapshotDate=2026-07-07')
    expect(headers.get('x-velion-org-id')).toBe('org_acme')
    expect(init?.credentials).toBe('include')
    expect(result.metrics).toEqual(metrics)
  })

  it('omits blank metric query params', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ metrics: [] })))

    await listSocialMetrics('org_acme', { accountId: '   ', snapshotDate: '' })

    const [url] = vi.mocked(fetch).mock.calls[0]!
    expect(String(url)).toBe('/api/v1/social/metrics')
  })

  it('parses the catalogs envelope', async () => {
    const catalogs = [{ id: 'cat_1', name: 'Spring', account_id: 'soc_acct_1', provider_key: 'facebook' }]
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ catalogs })))

    const result = await listSocialCatalogs('org_acme')

    const [url] = vi.mocked(fetch).mock.calls[0]!
    expect(String(url)).toBe('/api/v1/social/catalogs')
    expect(result.catalogs).toEqual(catalogs)
  })

  it('encodes the catalog id and forwards the account id for products', async () => {
    const products = [{ id: 'prod_1', name: 'Widget', retailer_id: 'sku-1' }]
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ products })))

    const result = await listSocialCatalogProducts('org_acme', 'cat/1', 'soc_acct_1')

    const [url, init] = vi.mocked(fetch).mock.calls[0]!
    const headers = init?.headers as Headers
    expect(String(url)).toBe('/api/v1/social/catalogs/cat%2F1/products?accountId=soc_acct_1')
    expect(headers.get('x-velion-org-id')).toBe('org_acme')
    expect(result.products).toEqual(products)
  })
})

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify({ data: body }), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}
