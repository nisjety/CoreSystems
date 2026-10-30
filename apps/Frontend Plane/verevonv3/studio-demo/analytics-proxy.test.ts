import { createServer, request } from 'node:http'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createPulseProxyMiddleware } from './pulse-local-gate'
const site = 'https://example.org/'
const websiteId = 'web_' + '1'.repeat(32)
const query = `site=${encodeURIComponent(site)}&websiteId=${websiteId}`
const metrics = { users: 8, sessions: 12, engagedSessions: 7, pageViews: 20, keyEvents: 1.5, engagementSeconds: 46.25 }
const ga4 = { site, source: 'google_analytics', status: 'connected', period: { startDate: '2026-10-01', endDate: '2026-10-03' }, timeZone: 'Europe/Oslo', totals: metrics, days: [{ date: '2026-10-01', ...metrics }], quality: { thresholded: true, sampled: false, dataLoss: false } }
const clarity = { site, source: 'microsoft_clarity', scope: 'clarity_project', status: 'connected', projectId: 'project123', window: { numOfDays: 3, timeZone: 'UTC' }, metrics: { sessions: 12, botSessions: 0, users: null, pagesPerSession: 1.5 } }
describe('GA4 and Clarity BFF boundaries', () => {
  const servers: ReturnType<typeof createServer>[] = []
  afterEach(async () => { vi.unstubAllGlobals(); await Promise.all(servers.map((server) => new Promise<void>((resolve) => server.close(() => resolve())))); servers.length = 0 })
  async function setup(payload: unknown) {
    const fetcher = vi.fn(async (url: URL, init?: RequestInit) => { void url; void init; return new Response(JSON.stringify({ data: payload }), { headers: { 'content-type': 'application/json' } }) }); vi.stubGlobal('fetch', fetcher)
    const server = createServer(); servers.push(server)
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
    const port = (server.address() as { port: number }).port
    const middleware = createPulseProxyMiddleware({ serviceKey: 'server-only-test-key', upstreamUrl: 'http://127.0.0.1:3165', publicPort: port })
    server.on('request', (incoming, outgoing) => middleware(incoming, outgoing, () => outgoing.writeHead(404).end()))
    const call = (path: string, method = 'GET', headers: Record<string, string> = {}) => new Promise<{ status: number; body: string; headers: Record<string, string | string[] | undefined> }>((resolve, reject) => {
      const outgoing = request(`http://127.0.0.1:${port}${path}`, { method, headers }, (incoming) => {
        let body = ''; incoming.on('data', (chunk) => { body += String(chunk) }); incoming.on('end', () => resolve({ status: incoming.statusCode ?? 0, body, headers: incoming.headers }))
      }); outgoing.on('error', reject); outgoing.end()
    })
    return { call, fetcher }
  }
  it('relays only the bounded GA4 projection with quality flags and server-only authority', async () => {
    const { call, fetcher } = await setup({ ...ga4, token: 'private', totals: { ...metrics, token: 'private' }, days: [{ ...ga4.days[0], userId: 'private' }], quality: { ...ga4.quality, providerMetadata: 'private' } })
    const response = await call('/api/v1/marketing/ga4?quarter=Q4&' + query)
    expect(response.status).toBe(200)
    expect(JSON.parse(response.body)).toEqual({ data: ga4 })
    expect(response.body).not.toContain('private')
    expect(response.headers['cache-control']).toBe('no-store')
    expect(fetcher.mock.calls[0]?.[1]).toMatchObject({ method: 'GET', redirect: 'manual', headers: { 'X-Internal-API-Key': 'server-only-test-key' } })
    expect(fetcher.mock.calls[0]?.[1]?.credentials).toBeUndefined()
    expect(String(fetcher.mock.calls[0]?.[0])).toContain('websiteId=' + websiteId)
  })
  it('relays only fixed-window Clarity traffic with missing metrics kept null', async () => {
    const { call } = await setup({ ...clarity, token: 'private', metrics: { ...clarity.metrics, recordingUrl: 'private' } })
    const response = await call('/api/v1/marketing/clarity?' + query)
    expect(response.status).toBe(200)
    expect(JSON.parse(response.body)).toEqual({ data: clarity })
    expect(response.body).not.toContain('private')
  })
  it('requires registered context and freezes each source query contract before upstream access', async () => {
    const { call, fetcher } = await setup(ga4)
    const invalid = [
      '/api/v1/marketing/ga4?quarter=Q4&site=' + encodeURIComponent(site),
      '/api/v1/marketing/ga4?' + query,
      '/api/v1/marketing/ga4?quarter=Q5&' + query,
      '/api/v1/marketing/ga4?quarter=Q4&' + query + '&propertyId=123',
      '/api/v1/marketing/ga4?quarter=Q4&' + query + '&websiteId=' + websiteId,
      '/api/v1/marketing/clarity?' + query + '&quarter=Q4',
      '/api/v1/marketing/clarity?' + query + '&numOfDays=1',
      '/api/v1/marketing/clarity?' + query.replace(websiteId, 'invalid'),
      '/api/v1/marketing/clarity?' + query.replace(encodeURIComponent(site), encodeURIComponent('https://localhost/')),
    ]
    for (const path of invalid) expect((await call(path)).status).toBe(400)
    expect((await call('/api/v1/marketing/ga4?quarter=Q4&' + query, 'POST')).status).toBe(405)
    expect((await call('/api/v1/marketing/clarity?' + query, 'POST')).status).toBe(405)
    expect((await call('/api/v1/marketing/clarity?' + query, 'GET', { origin: 'http://evil.example' })).status).toBe(403)
    expect(fetcher).not.toHaveBeenCalled()
  })
  it('rejects another site or false Clarity scope rather than leaking unrelated data', async () => {
    const { call, fetcher } = await setup({ ...ga4, site: 'https://other.example/' })
    expect((await call('/api/v1/marketing/ga4?quarter=Q4&' + query)).status).toBe(502)
    fetcher.mockResolvedValueOnce(new Response(JSON.stringify({ data: { ...clarity, scope: 'exact_url' } }), { headers: { 'content-type': 'application/json' } }))
    expect((await call('/api/v1/marketing/clarity?' + query)).status).toBe(502)
  })
})
