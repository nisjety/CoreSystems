import type { IncomingMessage, ServerResponse } from 'node:http'
import { normalizePulseSite } from './pulse-site-stack.ts'
import { isWebsiteId } from './website-client.ts'
import { sanitizeWebsiteDiscovery, validateDiscoveryContext, type DiscoveryContext } from './discovery-client.ts'

const base = '/api/v1/marketing/discovery'
class InvalidInput extends Error {}
type Boundary = {
  serviceKey: string; upstreamUrl: string
  readLimitedBody: (response: Response, maxBytes: number) => Promise<Buffer>
  send: (status: number, code: string, message: string) => void
  limited: (write: boolean) => boolean
}
type Selection = Pick<DiscoveryContext, 'websiteId' | 'site'>
function queryContext(url: URL): Selection {
  const keys = [...url.searchParams.keys()]
  const websiteId = url.searchParams.get('websiteId'); const site = url.searchParams.get('site')
  if (keys.length !== 2 || keys.some((key) => key !== 'websiteId' && key !== 'site') || !websiteId || !isWebsiteId(websiteId) || !site || normalizePulseSite(site) !== site) throw new InvalidInput()
  return { websiteId, site }
}
async function command(request: IncomingMessage): Promise<DiscoveryContext> {
  const text = await new Promise<string>((resolve, reject) => {
    let size = 0; let body = ''; let settled = false
    const done = (error?: Error) => { if (settled) return; settled = true; clearTimeout(timer); request.off('data', data); request.off('end', end); request.off('error', failed); if (error) { request.resume(); reject(error) } else resolve(body) }
    const data = (chunk: Buffer) => { size += chunk.length; if (size > 4096) done(new InvalidInput()); else body += chunk.toString('utf8') }
    const end = () => done(); const failed = () => done(new InvalidInput()); const timer = setTimeout(failed, 3000)
    request.on('data', data); request.once('end', end); request.once('error', failed)
  })
  try {
    const value: unknown = JSON.parse(text)
    if (typeof value !== 'object' || value === null || Object.keys(value).sort().join(',') !== 'site,websiteId,websiteRevision') throw new InvalidInput()
    return validateDiscoveryContext(value)
  } catch { throw new InvalidInput() }
}

/** Read never starts discovery; only an explicit same-origin JSON POST reaches Quarry's bounded map action. */
export function handleWebsiteDiscoveryProxy(request: IncomingMessage, response: ServerResponse, url: URL, boundary: Boundary, origin: string): boolean {
  if (url.pathname !== base) return false
  response.setHeader('Cache-Control', 'no-store'); response.setHeader('X-Content-Type-Options', 'nosniff')
  const method = request.method ?? 'GET'; const write = method === 'POST'
  if (method !== 'GET' && !write) { response.setHeader('Allow', 'GET, POST'); boundary.send(405, 'METHOD_NOT_ALLOWED', 'GET or POST is required'); return true }
  if (write && request.headers.origin !== origin) { boundary.send(403, 'FORBIDDEN', 'Same-origin request required'); return true }
  if (write && !/^application\/json(?:\s*;\s*charset=utf-8)?$/i.test(String(request.headers['content-type'] ?? ''))) { boundary.send(415, 'INVALID_CONTENT_TYPE', 'JSON is required'); return true }
  if (write && url.search || !write && !url.search) { boundary.send(400, 'INVALID_QUERY', 'Choose a registered website'); return true }
  if (boundary.limited(write)) { boundary.send(429, 'RATE_LIMITED', 'Too many discovery requests'); return true }
  void (async () => {
    let input: Selection | DiscoveryContext
    try { input = write ? await command(request) : queryContext(url) } catch { throw new InvalidInput() }
    const target = new URL(base, boundary.upstreamUrl)
    if (!write) { target.searchParams.set('websiteId', input.websiteId); target.searchParams.set('site', input.site) }
    const upstream = await fetch(target, { method, redirect: 'manual', signal: AbortSignal.timeout(15_000), headers: { Accept: 'application/json', 'X-Internal-API-Key': boundary.serviceKey, ...(write ? { 'Content-Type': 'application/json' } : {}) }, ...(write ? { body: JSON.stringify(input) } : {}) })
    if ([400, 404, 409, 429, 503].includes(upstream.status)) {
      await upstream.body?.cancel()
      boundary.send(upstream.status, upstream.status === 409 ? 'DISCOVERY_CONFLICT' : 'DISCOVERY_UNAVAILABLE', upstream.status === 409 ? 'The selected website changed. Refresh before trying again.' : 'Page discovery is unavailable for this website.')
      return
    }
    if (![200, 202].includes(upstream.status) || !upstream.headers.get('content-type')?.startsWith('application/json')) { await upstream.body?.cancel(); throw new Error('Unavailable discovery') }
    const payload: unknown = JSON.parse((await boundary.readLimitedBody(upstream, 256 << 10)).toString('utf8'))
    if (typeof payload !== 'object' || payload === null || !('data' in payload)) throw new Error('Invalid discovery envelope')
    const expected = write ? input as DiscoveryContext : { ...input, websiteRevision: validateDiscoveryContext(payload.data).websiteRevision }
    const data = sanitizeWebsiteDiscovery(payload.data, expected)
    response.statusCode = upstream.status; response.setHeader('Content-Type', 'application/json; charset=utf-8'); response.end(JSON.stringify({ data }))
  })().catch((error: unknown) => boundary.send(error instanceof InvalidInput ? 400 : 502, error instanceof InvalidInput ? 'INVALID_DISCOVERY_REQUEST' : 'SOURCE_UNAVAILABLE', error instanceof InvalidInput ? 'Choose a valid registered website' : 'Page discovery is temporarily unavailable'))
  return true
}
