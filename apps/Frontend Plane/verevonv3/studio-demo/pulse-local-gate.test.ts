import { createServer, request } from 'node:http'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createPulseProxyMiddleware } from './pulse-local-gate'

const serviceKey = 'test-service-key'
const pulsePath = '/api/v1/marketing/pulse?quarter=Q4&site=https%3A%2F%2Faquatiq.com%2F'
const projection = { data: {
  site: 'https://aquatiq.com/', source: 'google_search_console', status: 'connected', propertyScope: 'domain',
  period: { startDate: '2026-10-01', endDate: '2026-10-20' },
  totals: { clicks: 17, impressions: 440 }, days: [],
} }
const searchPath = pulsePath.replace('/marketing/pulse?', '/marketing/search?')
const searchRow = { key: 'water hygiene', clicks: 4, impressions: 200, ctr: 0.02, position: 7.2 }
const searchProjection = { data: {
  site: 'https://aquatiq.com/', source: 'google_search_console', status: 'connected', propertyScope: 'domain',
  period: { startDate: '2026-10-01', endDate: '2026-10-20' },
  queries: [searchRow],
  pages: [{ key: 'https://aquatiq.com/water/', clicks: 10, impressions: 1000, ctr: 0.01, position: 3.5 }],
  opportunities: [{ ...searchRow, type: 'title_meta' }],
  rowLimit: 100, queriesAtLimit: false, pagesAtLimit: false,
} }
const comparisonPath = '/api/v1/marketing/search/compare?site=https%3A%2F%2Faquatiq.com%2F&websiteId=web_' + '1'.repeat(32) + '&dimension=query&currentStart=2026-09-22&currentEnd=2026-10-19&previousStart=2026-08-25&previousEnd=2026-09-21&rowLimit=100'
const comparisonProjection = { data: {
  site: 'https://aquatiq.com/', source: 'google_search_console', status: 'connected', dimension: 'query',
  currentPeriod: { startDate: '2026-09-22', endDate: '2026-10-19' }, previousPeriod: { startDate: '2026-08-25', endDate: '2026-09-21' },
  rows: [{ key: searchRow.key, state: 'not_comparable', current: searchRow }],
  rowLimit: 100, rowsAtLimit: false, comparableRows: 0, unmatchedRows: 1,
} }
const ga4Path = '/api/v1/marketing/ga4?quarter=Q4&site=https%3A%2F%2Faquatiq.com%2F&websiteId=web_' + '1'.repeat(32)
const ga4Projection = { data: {
  site: 'https://aquatiq.com/', reportingSite: 'https://www.aquatiq.com/', source: 'google_analytics', status: 'connected',
  period: { startDate: '2026-10-01', endDate: '2026-10-20' }, timeZone: 'Europe/Oslo',
  totals: { users: 8, sessions: 12, engagedSessions: 7, pageViews: 20, keyEvents: 1.5, engagementSeconds: 46.25 }, days: [], token: 'private-provider-token', propertyId: 'private-property',
} }

describe('local read-only Pulse proxy', () => {
  const servers: ReturnType<typeof createServer>[] = []
  afterEach(async () => {
    vi.unstubAllGlobals()
    vi.restoreAllMocks()
    await Promise.all(servers.map((server) => new Promise<void>((resolve) => server.close(() => resolve()))))
    servers.length = 0
  })

  async function setup(payload: unknown = projection) {
    let upstreamCalls = 0
    vi.stubGlobal('fetch', vi.fn(async (_url: URL, options: RequestInit) => {
      upstreamCalls++
      expect(options.method).toBe('GET')
      expect(options.redirect).toBe('manual')
      expect((options.headers as Record<string, string>)['X-Internal-API-Key']).toBe(serviceKey)
      return new Response(JSON.stringify(payload), { status: 200, headers: { 'content-type': 'application/json' } })
    }))
    const server = createServer()
    servers.push(server)
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
    const port = (server.address() as { port: number }).port
    const middleware = createPulseProxyMiddleware({ serviceKey, upstreamUrl: 'http://127.0.0.1:3165', publicPort: port })
    server.on('request', (incoming, outgoing) => middleware(incoming, outgoing, () => outgoing.writeHead(404).end()))
    const base = `http://127.0.0.1:${port}`
    const call = (path: string, options: { method?: string; headers?: Record<string, string> } = {}) => new Promise<{ status: number; headers: Record<string, string | string[] | undefined>; json: () => unknown }>((resolve, reject) => {
      const outgoing = request(`${base}${path}`, { method: options.method ?? 'GET', headers: options.headers }, (incoming) => {
        let body = ''
        incoming.on('data', (chunk) => { body += String(chunk) })
        incoming.on('end', () => resolve({ status: incoming.statusCode ?? 0, headers: incoming.headers, json: () => JSON.parse(body) as unknown }))
      })
      outgoing.on('error', reject)
      outgoing.end()
    })
    return { base, call, calls: () => upstreamCalls }
  }

  it('serves the sanitized projection without a code and blocks other API routes', async () => {
    const { call, calls } = await setup()
    const result = await call(pulsePath)
    expect(result.status).toBe(200)
    expect(result.headers['set-cookie']).toBeUndefined()
    expect(result.headers['cache-control']).toBe('no-store')
    expect((result.json() as typeof projection).data.totals?.clicks).toBe(17)
    expect(calls()).toBe(1)
    expect((await call('/api/v1/marketing/pulse/access')).status).toBe(404)
    expect((await call('/api/v1/marketing/other')).status).toBe(404)
  })

  it('passes through an honest unconfigured state without invented metrics', async () => {
    const unavailable = { data: { ...projection.data, status: 'unavailable', reason: 'not_configured', totals: null, days: [] } }
    const { call } = await setup(unavailable)
    const result = await call(pulsePath)
    expect(result.status).toBe(200)
    expect((result.json() as typeof unavailable).data).toMatchObject({ status: 'unavailable', reason: 'not_configured', totals: null, days: [] })
  })

  it('rejects unsafe methods, quarters, and sites before upstream access', async () => {
    const { call, calls } = await setup()
    expect((await call(pulsePath, { method: 'POST' })).status).toBe(405)
    expect((await call('/api/v1/marketing/pulse?quarter=ALL&site=https%3A%2F%2Faquatiq.com%2F')).status).toBe(400)
    for (const site of ['http://127.0.0.1/', 'https://aquatiq.com/path', 'https://user@aquatiq.com/']) {
      expect((await call(`/api/v1/marketing/pulse?quarter=Q4&site=${encodeURIComponent(site)}`)).status).toBe(400)
    }
    expect(calls()).toBe(0)
  })

  it('rejects other origins and cross-site browser requests', async () => {
    const { call, calls } = await setup()
    expect((await call(pulsePath, { headers: { origin: 'http://evil.example' } })).status).toBe(403)
    expect((await call(pulsePath, { headers: { 'sec-fetch-site': 'cross-site' } })).status).toBe(403)
    expect((await call(pulsePath, { headers: { host: 'evil.example' } })).status).toBe(403)
    expect(calls()).toBe(0)
  })

  it('throttles repeated local reads', async () => {
    const { call, calls } = await setup()
    const statuses = []
    for (let index = 0; index < 31; index++) statuses.push((await call(pulsePath)).status)
    expect(statuses.slice(0, 30).every((status) => status === 200)).toBe(true)
    expect(statuses[30]).toBe(429)
    expect(calls()).toBe(30)
  })

  it('strips unexpected upstream fields from the browser response', async () => {
    const { call } = await setup({ ...projection, secret: 'upstream-secret', data: { ...projection.data, token: 'do-not-relay' } })
    const result = await call(pulsePath)
    expect(result.status).toBe(200)
    expect(JSON.stringify(result.json())).not.toContain('secret')
    expect(JSON.stringify(result.json())).not.toContain('token')
  })

  it('does not forward malformed upstream responses', async () => {
    const { call } = await setup({ data: { totals: { clicks: 'invalid' } } })
    expect((await call(pulsePath)).status).toBe(502)
  })

  it('passes only a paired GA4 reporting site to the browser and strips provider credentials', async () => {
    const { call } = await setup(ga4Projection)
    const response = await call(ga4Path)
    expect(response.status).toBe(200)
    expect(response.json()).toEqual({ data: {
      site: 'https://aquatiq.com/', reportingSite: 'https://www.aquatiq.com/', source: 'google_analytics', status: 'connected',
      period: { startDate: '2026-10-01', endDate: '2026-10-20' }, timeZone: 'Europe/Oslo',
      totals: { users: 8, sessions: 12, engagedSessions: 7, pageViews: 20, keyEvents: 1.5, engagementSeconds: 46.25 }, days: [],
    } })
    expect(JSON.stringify(response.json())).not.toMatch(/private|token/i)
  })

  it('allows only the fixed search projection and strips private extra fields at every level', async () => {
    const { call, calls } = await setup({ data: {
      ...searchProjection.data, connectionId: 'do-not-relay', token: 'private-token',
      queries: [{ ...searchRow, secret: 'private-query-metadata' }],
      opportunities: [{ ...searchRow, type: 'title_meta', raw: 'private-provider-body' }],
    } })
    const response = await call(searchPath)
    expect(response.status).toBe(200)
    expect(response.headers['cache-control']).toBe('no-store')
    expect(response.json()).toEqual(searchProjection)
    expect(JSON.stringify(response.json())).not.toMatch(/private-|do-not-relay/)
    expect(calls()).toBe(1)
    expect(vi.mocked(fetch).mock.calls[0]?.[0].toString()).toContain('/api/v1/marketing/search?')
    expect((await call('/api/v1/marketing/search/actions')).status).toBe(404)
  })

  it('does not let the search endpoint become a provider proxy', async () => {
    const { call, calls } = await setup(searchProjection)
    expect((await call(searchPath + '&operation=gsc.sites.list')).status).toBe(400)
    expect((await call(searchPath + '&site=https%3A%2F%2Fevil.example%2F')).status).toBe(400)
    expect((await call(searchPath, { method: 'POST' })).status).toBe(405)
    expect((await call(searchPath, { headers: { origin: 'http://evil.example' } })).status).toBe(403)
    expect((await call(searchPath, { headers: { 'sec-fetch-site': 'cross-site' } })).status).toBe(403)
    expect(calls()).toBe(0)
  })

  it('accepts bounded long page rows without lifting the existing Pulse response cap', async () => {
    const pages = Array.from({ length: 100 }, (_, index) => ({
      key: `https://aquatiq.com/${'a'.repeat(1800)}-${index}`, clicks: 1, impressions: 100, ctr: 0.01, position: 8,
    }))
    const { call } = await setup({ data: { ...searchProjection.data, pages, pagesAtLimit: true } })
    const response = await call(searchPath)
    expect(response.status).toBe(200)
    expect((response.json() as typeof searchProjection).data.pages).toHaveLength(100)
    expect((await call(pulsePath)).status).toBe(502)
  })

  it('rejects oversized search responses and invalid or cross-site projections', async () => {
    const { call } = await setup({ data: { ...searchProjection.data, padding: 'a'.repeat((2 << 20) + 1) } })
    expect((await call(searchPath)).status).toBe(502)
    const fetchMock = vi.mocked(fetch)
    for (const data of [
      { ...searchProjection.data, site: 'https://example.com/' },
      { ...searchProjection.data, queries: [{ ...searchRow, clicks: -1 }] },
      { ...searchProjection.data, opportunities: [{ ...searchRow, type: 'publish_ads' }] },
    ]) {
      fetchMock.mockResolvedValueOnce(new Response(JSON.stringify({ data }), { status: 200, headers: { 'content-type': 'application/json' } }))
      expect((await call(searchPath)).status).toBe(502)
    }
  })

  it('admits valid page keys even when the Go encoder HTML-escapes their JSON', async () => {
    const { call } = await setup(searchProjection)
    const pages = Array.from({ length: 100 }, (_, index) => ({
      key: `https://aquatiq.com/page-${index}?x=${'&'.repeat(1900)}`, clicks: 1, impressions: 100, ctr: 0.01, position: 8,
    }))
    const escaped = JSON.stringify({ data: { ...searchProjection.data, pages, pagesAtLimit: true } }).replaceAll('&', '\\u0026')
    expect(escaped.length).toBeGreaterThan(512_000)
    vi.mocked(fetch).mockResolvedValueOnce(new Response(escaped, { status: 200, headers: { 'content-type': 'application/json' } }))
    const response = await call(searchPath)
    expect(response.status).toBe(200)
    expect((response.json() as typeof searchProjection).data.pages).toEqual(pages)
  })

  it('shares the existing request budget between Pulse and search reads', async () => {
    const { call, calls } = await setup(searchProjection)
    for (let index = 0; index < 30; index++) await call(searchPath)
    expect((await call(pulsePath)).status).toBe(429)
    expect(calls()).toBe(30)
  })

  it('forwards a fixed, site-bound comparison and strips nested upstream secrets', async () => {
    const { call } = await setup({ data: { ...comparisonProjection.data, token: 'private', rows: [{ ...comparisonProjection.data.rows[0], current: { ...searchRow, secret: 'private' } }] } })
    const response = await call(comparisonPath)
    expect(response.status).toBe(200)
    expect(response.json()).toEqual(comparisonProjection)
    const target = new URL(String(vi.mocked(fetch).mock.calls[0]?.[0]))
    expect(target.pathname).toBe('/api/v1/marketing/search/compare')
    expect(target.searchParams.get('websiteId')).toBe('web_' + '1'.repeat(32))
    expect(response.headers['cache-control']).toBe('no-store')
  })

  it('rejects comparison writes, arbitrary fields, duplicate values and unscoped requests', async () => {
    const { call, calls } = await setup(comparisonProjection)
    for (const path of [comparisonPath + '&action=ads.create', comparisonPath + '&dimension=page', comparisonPath.replace(/&websiteId=web_[a-f0-9]+/, ''), comparisonPath.replace('rowLimit=100', 'rowLimit=1000'), comparisonPath.replace('2026-09-22', '2026-09-23')]) {
      expect((await call(path)).status).toBe(400)
    }
    expect((await call(comparisonPath, { method: 'POST' })).status).toBe(405)
    expect((await call(comparisonPath, { headers: { origin: 'https://other.example' } })).status).toBe(403)
    expect(calls()).toBe(0)
  })

  it('rejects a valid comparison returned for a different period or dimension', async () => {
    const { call } = await setup({ data: { ...comparisonProjection.data, dimension: 'page' } })
    expect((await call(comparisonPath)).status).toBe(502)
  })
})
