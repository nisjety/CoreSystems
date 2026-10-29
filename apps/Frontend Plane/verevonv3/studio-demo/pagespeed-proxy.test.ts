import { createServer, request } from 'node:http'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createPulseProxyMiddleware } from './pulse-local-gate'
const site = 'https://aquatiq.com/'
const path = '/api/v1/marketing/pagespeed'
const command = JSON.stringify({ site, strategy: 'mobile' })
const idle = { site, source: 'google_pagespeed', strategy: 'mobile', status: 'idle' }
const complete = { ...idle, status: 'complete', report: { performance: 90, accessibility: 100, bestPractices: 100, seo: 90, lcpMs: 1000, cls: 0, tbtMs: 0, fcpMs: 800, speedIndexMs: 1200, fetchTime: '2026-10-22T10:00:00Z', lighthouseVersion: '13.0.0' } }
describe('PageSpeed local proxy', () => {
  const servers: ReturnType<typeof createServer>[] = []
  afterEach(async () => { vi.unstubAllGlobals(); await Promise.all(servers.map((server) => new Promise<void>((resolve) => server.close(() => resolve())))); servers.length = 0 })
  async function setup() {
    const fetchMock = vi.fn().mockImplementation(async () => new Response(JSON.stringify({ data: { ...idle, secret: 'private' } }), { headers: { 'content-type': 'application/json' } }))
    vi.stubGlobal('fetch', fetchMock)
    const server = createServer(); servers.push(server)
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
    const port = (server.address() as { port: number }).port
    const middleware = createPulseProxyMiddleware({ serviceKey: 'private-server-key', upstreamUrl: 'http://127.0.0.1:3165', publicPort: port })
    server.on('request', (incoming, outgoing) => middleware(incoming, outgoing, () => outgoing.writeHead(404).end()))
    const origin = `http://127.0.0.1:${port}`
    const call = (suffix = '', method = 'GET', body?: string, headers: Record<string, string> = {}) => new Promise<{ status: number; body: string }>((resolve, reject) => {
      const outgoing = request(origin + path + suffix, { method, headers }, (incoming) => {
        let result = ''; incoming.on('data', (chunk) => { result += String(chunk) }); incoming.on('end', () => resolve({ status: incoming.statusCode ?? 0, body: result }))
      }); outgoing.on('error', reject); outgoing.end(body)
    })
    return { call, origin, fetchMock }
  }
  it('reads fixed status and strips private metadata', async () => {
    const { call, fetchMock } = await setup()
    const response = await call(`?site=${encodeURIComponent(site)}&strategy=mobile`)
    expect(response.status).toBe(200)
    expect(JSON.parse(response.body)).toEqual({ data: idle })
    expect(fetchMock.mock.calls[0]?.[1]).toMatchObject({ method: 'GET', redirect: 'manual', headers: { 'X-Internal-API-Key': 'private-server-key' } })
  })
  it('requires exact same Origin plus JSON for manual audits', async () => {
    const { call, origin, fetchMock } = await setup()
    expect((await call('', 'POST', command, { 'content-type': 'application/json' })).status).toBe(403)
    expect((await call('', 'POST', command, { origin: 'http://evil.example', 'content-type': 'application/json' })).status).toBe(403)
    expect((await call('', 'POST', command, { origin, 'content-type': 'text/plain' })).status).toBe(415)
    expect((await call('', 'POST', command, { origin, 'content-type': 'application/json', 'sec-fetch-site': 'cross-site' })).status).toBe(403)
    expect(fetchMock).not.toHaveBeenCalled()
    expect((await call('', 'POST', command, { origin, 'content-type': 'application/json' })).status).toBe(200)
    expect(fetchMock.mock.calls[0]?.[1]).toMatchObject({ method: 'POST', body: command, redirect: 'manual' })
  })
  it('preserves normalized diagnostics but never sends nested provider evidence to the browser', async () => {
    const { call, fetchMock } = await setup()
    fetchMock.mockResolvedValueOnce(new Response(JSON.stringify({ data: { ...complete, report: { ...complete.report, checks: [{ id: 'canonical', status: 'passed', score: 1, title: 'private title', details: { url: 'private url' } }] } } }), { headers: { 'content-type': 'application/json' } }))
    const response = await call(`?site=${encodeURIComponent(site)}&strategy=mobile`)
    expect(response.status).toBe(200)
    expect(JSON.parse(response.body).data.report.checks).toEqual([{ id: 'canonical', status: 'passed', score: 1 }])
    expect(response.body).not.toMatch(/private|title|details/)
  })
  it('rejects malformed diagnostics rather than showing a partial trustworthy report', async () => {
    const { call, fetchMock } = await setup()
    fetchMock.mockResolvedValueOnce(new Response(JSON.stringify({ data: { ...complete, report: { ...complete.report, checks: [{ id: 'canonical', status: 'passed', score: 0 }] } } }), { headers: { 'content-type': 'application/json' } }))
    const response = await call(`?site=${encodeURIComponent(site)}&strategy=mobile`)
    expect(response.status).toBe(502)
    expect(response.body).not.toContain('canonical')
  })
  it('rejects noncanonical sites, unknown fields, query overrides and oversized JSON', async () => {
    const { call, origin, fetchMock } = await setup()
    const headers = { origin, 'content-type': 'application/json' }
    for (const body of ['{}', command.replace('mobile', 'tablet'), command.replace('aquatiq.com', 'localhost'), command.replace('}', ',"operation":"custom"}'), command + '{}', JSON.stringify({ site: 'x'.repeat(3000), strategy: 'mobile' })]) expect((await call('', 'POST', body, headers)).status).toBe(400)
    expect((await call('?operation=custom', 'POST', command, headers)).status).toBe(400)
    expect((await call('?site=https://aquatiq.com/&strategy=mobile&strategy=desktop')).status).toBe(400)
    expect(fetchMock).not.toHaveBeenCalled()
  })
  it('limits manual audits separately from status reads', async () => {
    const { call, origin, fetchMock } = await setup()
    const headers = { origin, 'content-type': 'application/json' }
    for (let index = 0; index < 3; index++) expect((await call('', 'POST', command, headers)).status).toBe(200)
    expect((await call('', 'POST', command, headers)).status).toBe(429)
    expect((await call('?site=https://aquatiq.com/&strategy=mobile')).status).toBe(200)
    expect(fetchMock).toHaveBeenCalledTimes(4)
  })
})
