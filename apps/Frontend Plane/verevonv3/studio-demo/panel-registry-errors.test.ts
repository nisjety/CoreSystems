import { createServer, request } from 'node:http'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createPulseProxyMiddleware } from './pulse-local-gate'

const websiteId = 'web_' + '1'.repeat(32)
const site = 'https://example.org/'
const cases = [
  { path: `/api/v1/marketing/pulse?quarter=Q4&site=${encodeURIComponent(site)}&websiteId=${websiteId}`, method: 'GET' },
  { path: `/api/v1/marketing/search?quarter=Q4&site=${encodeURIComponent(site)}&websiteId=${websiteId}`, method: 'GET' },
  { path: `/api/v1/marketing/ga4?quarter=Q4&site=${encodeURIComponent(site)}&websiteId=${websiteId}`, method: 'GET' },
  { path: `/api/v1/marketing/clarity?site=${encodeURIComponent(site)}&websiteId=${websiteId}`, method: 'GET' },
  { path: `/api/v1/marketing/pagespeed?site=${encodeURIComponent(site)}&strategy=mobile&websiteId=${websiteId}`, method: 'GET' },
  { path: '/api/v1/marketing/pagespeed', method: 'POST' },
] as const

describe('panel registry error boundary', () => {
  const servers: ReturnType<typeof createServer>[] = []
  afterEach(async () => {
    vi.unstubAllGlobals()
    await Promise.all(servers.map((server) => new Promise<void>((resolve) => server.close(() => resolve()))))
    servers.length = 0
  })
  async function setup(status: number) {
    const fetcher = vi.fn(async () => new Response('private-provider-error-token-and-connection-id', { status, headers: { 'content-type': 'text/html', 'set-cookie': 'private-cookie' } }))
    vi.stubGlobal('fetch', fetcher)
    const server = createServer(); servers.push(server)
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
    const port = (server.address() as { port: number }).port
    const origin = `http://127.0.0.1:${port}`
    const middleware = createPulseProxyMiddleware({ serviceKey: 'server-only-test-key', upstreamUrl: 'http://127.0.0.1:3165', publicPort: port })
    server.on('request', (incoming, outgoing) => middleware(incoming, outgoing, () => outgoing.writeHead(404).end()))
    const call = (endpoint: typeof cases[number]) => new Promise<{ status: number; body: string; headers: Record<string, string | string[] | undefined> }>((resolve, reject) => {
      const outgoing = request(origin + endpoint.path, { method: endpoint.method, headers: endpoint.method === 'POST' ? { origin, 'content-type': 'application/json' } : {} }, (incoming) => {
        let body = ''; incoming.on('data', (chunk) => { body += String(chunk) }); incoming.on('end', () => resolve({ status: incoming.statusCode ?? 0, body, headers: incoming.headers }))
      })
      outgoing.on('error', reject)
      outgoing.end(endpoint.method === 'POST' ? JSON.stringify({ site, strategy: 'mobile', websiteId }) : undefined)
    })
    return { call, fetcher }
  }
  it.each([404, 409])('preserves expected registry status %s with fixed safe errors on every panel route', async (status) => {
    const { call, fetcher } = await setup(status)
    for (const endpoint of cases) {
      const result = await call(endpoint)
      expect(result.status).toBe(status)
      expect(JSON.parse(result.body)).toEqual({ error: status === 409
        ? { code: 'WEBSITE_CONTEXT_CONFLICT', message: 'Website URL changed. Reload the website list before requesting data.' }
        : { code: 'WEBSITE_NOT_FOUND', message: 'Website was not found. Reload the website list.' } })
      expect(result.body).not.toContain('private')
      expect(result.headers['set-cookie']).toBeUndefined()
      expect(result.headers['cache-control']).toBe('no-store')
    }
    expect(fetcher).toHaveBeenCalledTimes(cases.length)
  })
  it.each([401, 403, 412, 429, 500])('does not pass through other upstream status %s or private provider errors', async (status) => {
    const { call } = await setup(status)
    for (const endpoint of cases) {
      const result = await call(endpoint)
      expect(result.status).toBe(502)
      expect(JSON.parse(result.body).error.code).toBe('SOURCE_UNAVAILABLE')
      expect(result.body).not.toContain('private')
    }
  })
})
