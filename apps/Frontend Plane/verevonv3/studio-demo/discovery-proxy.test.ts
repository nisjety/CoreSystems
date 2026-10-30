import { createServer, request } from 'node:http'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { handleWebsiteDiscoveryProxy } from './discovery-proxy'

const context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 7 }
const projection = { ...context, source: 'quarry_map', status: 'complete', limit: 25, partial: true, pages: [{ url: 'https://triodelab.no/innsikt' }], discoveredAt: '2026-10-22T12:00:00Z' }
const base = '/api/v1/marketing/discovery'
const query = `${base}?${new URLSearchParams({ websiteId: context.websiteId, site: context.site })}`
describe('discovery BFF boundary', () => {
  const servers: ReturnType<typeof createServer>[] = []
  afterEach(async () => { vi.unstubAllGlobals(); await Promise.all(servers.splice(0).map((server) => new Promise<void>((resolve) => { server.closeAllConnections(); server.close(() => resolve()) }))) })
  async function setup(status = 200, data: unknown = projection, limited = false) {
    const fetcher = vi.fn<(url: URL, init?: RequestInit) => Promise<Response>>(async () => new Response(JSON.stringify({ data, error: { message: 'private-provider-error' } }), { status, headers: { 'content-type': 'application/json' } })); vi.stubGlobal('fetch', fetcher)
    const server = createServer(); servers.push(server)
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
    const origin = `http://127.0.0.1:${(server.address() as { port: number }).port}`
    server.on('request', (incoming, outgoing) => {
      const send = (code: number, error: string, message: string) => { outgoing.statusCode = code; outgoing.end(JSON.stringify({ error: { code: error, message } })) }
      if (!handleWebsiteDiscoveryProxy(incoming, outgoing, new URL(incoming.url ?? '/', origin), { serviceKey: 'server-secret', upstreamUrl: 'http://127.0.0.1:3165', limited: () => limited, send, readLimitedBody: async (response, maximum) => { const bytes = Buffer.from(await response.arrayBuffer()); if (bytes.length > maximum) throw new Error('too big'); return bytes } }, origin)) outgoing.writeHead(404).end()
    })
    const call = (path = query, method = 'GET', body?: string, headers: Record<string, string> = {}) => new Promise<{ status: number; body: string }>((resolve, reject) => {
      const outgoing = request(origin + path, { method, headers }, (incoming) => { let text = ''; incoming.on('data', (chunk) => { text += String(chunk) }); incoming.on('end', () => resolve({ status: incoming.statusCode ?? 0, body: text })) }); outgoing.on('error', reject); outgoing.end(body)
    })
    return { call, origin, fetcher }
  }
  it('reads only fixed upstream path with server key and strips all provider extras', async () => {
    const { call, fetcher } = await setup(200, { ...projection, token: 'private', pages: [{ ...projection.pages[0], html: 'private' }] })
    const result = await call()
    expect(result.status).toBe(200); expect(JSON.parse(result.body)).toEqual({ data: projection })
    expect(fetcher.mock.calls[0]).toMatchObject([expect.objectContaining({ pathname: base }), { method: 'GET', redirect: 'manual', headers: { 'X-Internal-API-Key': 'server-secret' } }])
  })
  it('allows one explicit same-origin JSON start but rejects malformed contexts before upstream', async () => {
    const { call, origin, fetcher } = await setup()
    const headers = { origin, 'content-type': 'application/json' }
    expect((await call(base, 'POST', JSON.stringify(context))).status).toBe(403)
    expect((await call(base, 'POST', JSON.stringify(context), { ...headers, origin: 'https://evil.example' })).status).toBe(403)
    expect((await call(base, 'POST', JSON.stringify(context), { origin, 'content-type': 'text/plain' })).status).toBe(415)
    for (const body of ['{}', '{', 'x'.repeat(4097), JSON.stringify({ ...context, websiteRevision: 0 }), JSON.stringify({ ...context, url: 'https://internal/' }), JSON.stringify({ ...context, site: 'https://localhost/' })]) expect((await call(base, 'POST', body, headers)).status).toBe(400)
    expect(fetcher).not.toHaveBeenCalled()
    expect((await call(base, 'POST', JSON.stringify(context), headers)).status).toBe(200)
    expect(fetcher).toHaveBeenCalledTimes(1)
    expect(fetcher.mock.calls[0]?.[1]).toMatchObject({ method: 'POST', body: JSON.stringify(context) })
  })
  it('rejects query extras, duplicates, wrong methods and stale or unsafe provider results', async () => {
    const { call, fetcher } = await setup()
    for (const path of [base, `${query}&extra=1`, `${query}&websiteId=${context.websiteId}`]) expect((await call(path)).status).toBe(400)
    expect((await call(base, 'DELETE')).status).toBe(405)
    expect(fetcher).not.toHaveBeenCalled()
    const unsafe = await setup(200, { ...projection, pages: [{ url: 'https://evil.example/' }] })
    expect((await unsafe.call()).status).toBe(502)
    const stale = await setup(200, { ...projection, websiteRevision: 8 })
    expect((await stale.call(base, 'POST', JSON.stringify(context), { origin: stale.origin, 'content-type': 'application/json' })).status).toBe(502)
  })
  it('sanitizes provider failures and does not retry or bypass rate limits', async () => {
    const conflict = await setup(409)
    const result = await conflict.call()
    expect(result.status).toBe(409); expect(result.body).not.toContain('private'); expect(conflict.fetcher).toHaveBeenCalledTimes(1)
    const redirect = await setup(302)
    expect((await redirect.call()).status).toBe(502); expect(redirect.fetcher).toHaveBeenCalledTimes(1)
    const limited = await setup(200, projection, true)
    expect((await limited.call()).status).toBe(429); expect(limited.fetcher).not.toHaveBeenCalled()
  })
})
