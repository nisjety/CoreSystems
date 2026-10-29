import { createServer, request } from 'node:http'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createPulseProxyMiddleware } from './pulse-local-gate'
const website = { id: 'web_' + '1'.repeat(32), site: 'https://aquatiq.com/', revision: 1, createdAt: '2026-10-21T00:00:00Z', updatedAt: '2026-10-21T00:00:00Z', ownership: { status: 'google_owner' }, integrations: { searchConsole: { status: 'connected', property: 'sc-domain:aquatiq.com' }, ga4: { status: 'requires_consent' }, clarity: { status: 'not_connected' }, pageSpeed: { status: 'available' } } }
const path = '/api/v1/marketing/websites'
describe('website BFF boundary', () => {
  const servers: ReturnType<typeof createServer>[] = []
  afterEach(async () => { vi.unstubAllGlobals(); await Promise.all(servers.map((server) => new Promise<void>((resolve) => server.close(() => resolve())))); servers.length = 0 })
  async function setup(value: unknown = website) {
    const fetcher = vi.fn(async (url: URL, init?: RequestInit) => { void url; void init; return new Response(JSON.stringify({ data: value }), { headers: { 'Content-Type': 'application/json' } }) })
    vi.stubGlobal('fetch', fetcher)
    const server = createServer(); servers.push(server)
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
    const port = (server.address() as { port: number }).port
    const origin = `http://127.0.0.1:${port}`
    const middleware = createPulseProxyMiddleware({ serviceKey: 'server-only-key', upstreamUrl: 'http://localhost:3165', publicPort: port })
    server.on('request', (incoming, outgoing) => middleware(incoming, outgoing, () => outgoing.writeHead(404).end()))
    const call = (suffix: string, method = 'GET', body?: object, headers: Record<string, string> = {}) => new Promise<{ status: number; body: string }>((resolve, reject) => {
      const outgoing = request(origin + path + suffix, { method, headers: { ...(body ? { origin, 'content-type': 'application/json' } : {}), ...headers } }, (incoming) => {
        let result = ''; incoming.on('data', (chunk) => { result += String(chunk) }); incoming.on('end', () => resolve({ status: incoming.statusCode ?? 0, body: result }))
      }); outgoing.on('error', reject); outgoing.end(body ? JSON.stringify(body) : undefined)
    })
    return { call, fetcher }
  }
  it('sanitizes every nested website mapping and injects the key only upstream', async () => {
    const { call, fetcher } = await setup({ websites: [{ ...website, token: 'private', integrations: { ...website.integrations, searchConsole: { ...website.integrations.searchConsole, connectionId: 'private' } } }] })
    const response = await call('')
    expect(response.status).toBe(200)
    expect(JSON.parse(response.body)).toEqual({ data: { websites: [website] } })
    expect(response.body).not.toContain('private')
    expect(fetcher.mock.calls[0]?.[1]).toMatchObject({ redirect: 'manual', headers: { 'X-Internal-API-Key': 'server-only-key' } })
  })
  it('requires same-origin JSON and exact revision commands', async () => {
    const { call, fetcher } = await setup()
    expect((await call('', 'POST', { site: website.site }, { origin: 'http://evil.example' })).status).toBe(403)
    expect((await call('', 'POST', { site: website.site }, { 'content-type': 'text/plain' })).status).toBe(415)
    for (const body of [{ site: website.site, orgId: 'spoof' }, { site: 'https://localhost/' }, { site: website.site, connectionId: 'spoof' }]) expect((await call('', 'POST', body)).status).toBe(400)
    expect((await call('/' + website.id, 'PATCH', { site: website.site })).status).toBe(400)
    expect((await call('/' + website.id + '/verify', 'POST', { forceOwner: true })).status).toBe(400)
    expect(fetcher).not.toHaveBeenCalled()
    expect((await call('/' + website.id, 'PATCH', { site: website.site, revision: 1 })).status).toBe(200)
  })
  it('permits only a canonical GA4 reporting site mapping command and strips upstream private fields', async () => {
    const mapped = { ...website, integrations: { ...website.integrations, searchConsole: { ...website.integrations.searchConsole, accessLevel: 'siteOwner' }, ga4: { status: 'connected', reportingSite: 'https://www.aquatiq.com/', connectionId: 'private' } } }
    const { call, fetcher } = await setup(mapped)
    const response = await call('/' + website.id + '/mappings', 'PATCH', { ga4ReportingSite: 'https://www.aquatiq.com/', revision: 1 })
    expect(response.status).toBe(200)
    expect(response.body).toContain('reportingSite')
    expect(response.body).not.toContain('private')
    expect(fetcher.mock.calls[0]?.[1]?.body).toBe(JSON.stringify({ ga4ReportingSite: 'https://www.aquatiq.com/', revision: 1 }))
    for (const body of [
      { ga4ReportingSite: 'https://www.aquatiq.com/path', revision: 1 },
      { ga4ReportingSite: 'https://www.aquatiq.com/', revision: 1, propertyId: 'spoof' },
      { ga4ReportingSite: 'https://www.aquatiq.com/', revision: 1, clarityProjectId: 'z1abticqmb' },
    ]) expect((await call('/' + website.id + '/mappings', 'PATCH', body)).status).toBe(400)
    expect(fetcher).toHaveBeenCalledTimes(1)
  })
  it('blocks arbitrary provider operations, unbound website responses and invalid status queries', async () => {
    const { call, fetcher } = await setup({ ...website, id: 'web_' + '2'.repeat(32) })
    expect((await call('/' + website.id + '/delete-provider', 'POST', {})).status).toBe(404)
    expect((await call('/' + website.id + '/google/status?sessionId=x&orgId=spoof')).status).toBe(400)
    expect((await call('/' + website.id)).status).toBe(502)
    expect(fetcher).toHaveBeenCalledTimes(1)
  })
})
