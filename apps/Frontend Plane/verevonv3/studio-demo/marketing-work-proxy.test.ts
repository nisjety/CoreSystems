import { createServer, request } from 'node:http'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createPulseProxyMiddleware } from './pulse-local-gate'

const site = 'https://aquatiq.com/'
const websiteId = `web_${'1'.repeat(32)}`
const taskId = `task_${'2'.repeat(32)}`
const api = '/api/v1/marketing/work'
const evidence = { kind: 'gsc_opportunity', key: 'water hygiene', opportunityType: 'title_meta', period: { startDate: '2026-10-01', endDate: '2026-10-20' }, clicks: 2, impressions: 200, ctr: 0.01, position: 5.25, observedAt: '2026-10-21T10:00:00Z' }
const task = { id: taskId, websiteId, site, revision: 1, status: 'todo', createdAt: '2026-10-21T10:00:00Z', updatedAt: '2026-10-21T10:00:00Z', evidence }
const data = { websiteId, site, tasks: [task], activity: [{ id: `evt_${'3'.repeat(32)}`, taskId, kind: 'created', status: 'todo', occurredAt: task.createdAt }], summary: { open: 1, inProgress: 0, done: 0, dismissed: 0, total: 1, completionPercent: 0 } }

describe('Marketing work BFF', () => {
  const servers: ReturnType<typeof createServer>[] = []
  afterEach(async () => { vi.unstubAllGlobals(); await Promise.all(servers.map((server) => new Promise<void>((resolve) => { server.closeAllConnections(); server.close(() => resolve()) }))); servers.length = 0 })

  async function setup(upstreamStatus = 200, customProjection?: unknown) {
    const projection = customProjection ?? { ...data, token: 'private', tasks: data.tasks.map((item) => ({ ...item, providerPayload: 'private' })) }
    const fetchMock = vi.fn<(url: URL, init?: RequestInit) => Promise<Response>>(async () => new Response(upstreamStatus === 409 ? JSON.stringify({ error: { message: 'secret revision detail' } }) : JSON.stringify({ data: projection }), { status: upstreamStatus, headers: { 'content-type': 'application/json' } }))
    vi.stubGlobal('fetch', fetchMock)
    const server = createServer(); servers.push(server)
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
    const port = (server.address() as { port: number }).port
    const middleware = createPulseProxyMiddleware({ serviceKey: 'private-server-key', upstreamUrl: 'http://127.0.0.1:3165', publicPort: port })
    server.on('request', (incoming, outgoing) => middleware(incoming, outgoing, () => outgoing.writeHead(404).end()))
    const origin = `http://127.0.0.1:${port}`
    const call = (path: string, method = 'GET', body?: string, headers: Record<string, string> = {}) => new Promise<{ status: number; body: string }>((resolve, reject) => {
      const outgoing = request(origin + path, { method, headers }, (incoming) => { let result = ''; incoming.on('data', (chunk) => { result += String(chunk) }); incoming.on('end', () => resolve({ status: incoming.statusCode ?? 0, body: result })) })
      outgoing.on('error', reject); outgoing.end(body)
    })
    return { call, origin, fetchMock }
  }

  it('reads a sanitized per-site queue using only selected context and server authority', async () => {
    const { call, fetchMock } = await setup()
    const result = await call(`${api}?websiteId=${websiteId}&site=${encodeURIComponent(site)}`)
    expect(result.status).toBe(200)
    expect(JSON.parse(result.body)).toEqual({ data })
    expect(result.body).not.toContain('private')
    expect(fetchMock.mock.calls[0]?.[0].toString()).toContain(`${api}?websiteId=${websiteId}&site=${encodeURIComponent(site)}`)
    expect(fetchMock.mock.calls[0]?.[1]).toMatchObject({ method: 'GET', redirect: 'manual', headers: { 'X-Internal-API-Key': 'private-server-key' } })
  })

  it('requires same-origin JSON and forwards only validated explicit saves/status changes', async () => {
    const { call, origin, fetchMock } = await setup()
    const headers = { origin, 'content-type': 'application/json' }
    const add = { websiteId, site, websiteRevision: 2, quarter: 'Q4', query: 'water hygiene' }
    expect((await call(api, 'POST', JSON.stringify(add), { 'content-type': 'application/json' })).status).toBe(403)
    expect((await call(api, 'POST', JSON.stringify(add), { ...headers, origin: 'http://evil.example' })).status).toBe(403)
    expect((await call(api, 'POST', JSON.stringify(add), { origin, 'content-type': 'text/plain' })).status).toBe(415)
    expect((await call(api, 'POST', JSON.stringify(add), headers)).status).toBe(200)
    expect(fetchMock.mock.calls[0]?.[1]).toMatchObject({ method: 'POST', body: JSON.stringify(add), headers: { 'X-Internal-API-Key': 'private-server-key' } })
    const patch = { websiteId, site, websiteRevision: 2, revision: 1, status: 'in_progress' }
    expect((await call(`${api}/${taskId}`, 'PATCH', JSON.stringify(patch), headers)).status).toBe(200)
    expect(fetchMock.mock.calls[1]?.[0].toString()).toMatch(new RegExp(`${api}/${taskId}$`))
    expect(fetchMock.mock.calls[1]?.[1]).toMatchObject({ method: 'PATCH', body: JSON.stringify(patch) })
  })

  it('rejects malformed/oversized contexts before upstream and sanitizes conflicts', async () => {
    const { call, origin, fetchMock } = await setup()
    const headers = { origin, 'content-type': 'application/json' }
    for (const path of [`${api}?websiteId=${websiteId}&site=${encodeURIComponent('https://localhost/')}`, `${api}?websiteId=${websiteId}&site=${encodeURIComponent(site)}&extra=1`, `${api}?websiteId=${websiteId}&websiteId=${websiteId}&site=${encodeURIComponent(site)}`]) expect((await call(path)).status).toBe(400)
    for (const body of ['{}', JSON.stringify({ websiteId, site, websiteRevision: 2, quarter: 'Q4', query: 'x', operation: 'write' }), JSON.stringify({ websiteId, site, websiteRevision: 0, quarter: 'Q4', query: 'x' }), JSON.stringify({ websiteId, site, websiteRevision: 2, quarter: 'Q5', query: 'x' }), JSON.stringify({ websiteId, site, websiteRevision: 2, quarter: 'Q4', query: 'x'.repeat(513) }), 'x'.repeat(5000)]) {
      const result = await call(api, 'POST', body, headers)
      expect(result.status, result.body).toBe(400)
    }
    expect(fetchMock).not.toHaveBeenCalled()
    const conflict = await setup(409)
    const response = await conflict.call(`${api}/${taskId}`, 'PATCH', JSON.stringify({ websiteId, site, websiteRevision: 2, revision: 1, status: 'done' }), { origin: conflict.origin, 'content-type': 'application/json' })
    expect(response.status).toBe(409)
    expect(response.body).not.toContain('secret')
  })

  it('forwards only explicit technical finding and verification identity, never supplied analytics', async () => {
    const { call, origin, fetchMock } = await setup()
    const headers = { origin, 'content-type': 'application/json' }
    const technical = { websiteId, site, websiteRevision: 2, strategy: 'mobile', checkId: 'structured-data', fetchTime: '2026-10-22T12:28:25.021Z' }
    expect((await call(`${api}/technical`, 'POST', JSON.stringify(technical), headers)).status).toBe(200)
    expect(fetchMock.mock.calls[0]?.[0].pathname).toBe(`${api}/technical`)
    expect(JSON.parse(String(fetchMock.mock.calls[0]?.[1]?.body))).toEqual(technical)
    const verify = { websiteId, site, websiteRevision: 2, revision: 3 }
    expect((await call(`${api}/${taskId}/verify`, 'POST', JSON.stringify(verify), headers)).status).toBe(200)
    expect(fetchMock.mock.calls[1]?.[0].pathname).toBe(`${api}/${taskId}/verify`)
    expect(JSON.parse(String(fetchMock.mock.calls[1]?.[1]?.body))).toEqual(verify)
  })

  it('requires same-origin JSON and rejects technical/recheck forged or extra fields before upstream', async () => {
    const { call, origin, fetchMock } = await setup()
    const headers = { origin, 'content-type': 'application/json' }
    const technical = { websiteId, site, websiteRevision: 2, strategy: 'mobile', checkId: 'canonical', fetchTime: '2026-10-22T12:28:25.021Z' }
    expect((await call(`${api}/technical`, 'POST', JSON.stringify(technical), { 'content-type': 'application/json' })).status).toBe(403)
    expect((await call(`${api}/technical`, 'GET')).status).toBe(405)
    expect((await call(`${api}/${taskId}/verify`, 'PATCH', '{}', headers)).status).toBe(405)
    for (const value of [
      { ...technical, score: 0 }, { ...technical, checkStatus: 'failed' },
      { ...technical, strategy: 'tablet' }, { ...technical, checkId: 'arbitrary-html' },
      { ...technical, websiteRevision: 0 }, { ...technical, fetchTime: 'invalid' },
      { ...technical, fetchTime: '2026-99-22T12:28:25.021Z' },
      { ...technical, site: 'https://localhost/' }, { ...technical, url: 'https://internal/' },
    ]) expect((await call(`${api}/technical`, 'POST', JSON.stringify(value), headers)).status).toBe(400)
    for (const value of [
      { websiteId, site, websiteRevision: 2 },
      { websiteId, site, websiteRevision: 2, revision: 3, status: 'passed' },
      { websiteId, site, websiteRevision: 2, revision: 0 },
    ]) expect((await call(`${api}/${taskId}/verify`, 'POST', JSON.stringify(value), headers)).status).toBe(400)
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('preserves technical observations and recheck receipts without leaking nested provider fields', async () => {
    const verification = { status: 'passed', score: 1, observedAt: '2026-10-22T13:28:25.021Z', lighthouseVersion: '13.5.0', recordedAt: '2026-10-22T13:29:00Z', token: 'private-recheck' }
    const evidence = { kind: 'pagespeed_check', key: 'canonical', observedAt: '2026-10-22T12:28:25.021Z', strategy: 'mobile', checkStatus: 'failed', checkScore: 0, lighthouseVersion: '13.5.0', details: 'private-original' }
    const projected = { ...data, tasks: [{ ...task, createdAt: '2026-10-22T12:29:00Z', updatedAt: verification.recordedAt, revision: 2, evidence, verification }], activity: [{ id: `evt_${'4'.repeat(32)}`, taskId, kind: 'verified', status: 'todo', occurredAt: verification.recordedAt, verification }] }
    const { call } = await setup(200, projected)
    const result = await call(`${api}?websiteId=${websiteId}&site=${encodeURIComponent(site)}`)
    expect(result.status).toBe(200)
    const clean = JSON.parse(result.body).data
    expect(clean.tasks[0]).toMatchObject({ status: 'todo', evidence: { kind: 'pagespeed_check', checkStatus: 'failed' }, verification: { status: 'passed', score: 1 } })
    expect(clean.summary.completionPercent).toBe(0)
    expect(clean.activity[0].kind).toBe('verified')
    expect(result.body).not.toContain('private')
    expect(result.body).not.toContain('clicks')
    const broken = await setup(200, { ...projected, tasks: [{ ...projected.tasks[0], verification: { ...verification, score: null } }] })
    expect((await broken.call(`${api}?websiteId=${websiteId}&site=${encodeURIComponent(site)}`)).status).toBe(502)
  })
})
