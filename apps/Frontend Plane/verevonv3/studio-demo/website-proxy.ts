import type { IncomingMessage, ServerResponse } from 'node:http'
import { normalizePulseSite } from './pulse-site-stack.ts'
import { isWebsiteId, sanitizeWebsite, sanitizeWebsiteList, sanitizeGoogleAuthorization, sanitizeGoogleStatus } from './website-client.ts'

export type WebsiteProxyContext = {
  upstreamUrl: string; serviceKey: string; origin: string
  limited: (write: boolean) => boolean
  send: (status: number, code: string, message: string) => void
  readLimitedBody: (response: Response, max: number) => Promise<Buffer>
}
const base = '/api/v1/marketing/websites'
class InvalidCommand extends Error {}
function route(pathname: string): { suffix: string; id?: string; methods: string[] } | null {
  if (pathname === base) return { suffix: '', methods: ['GET', 'POST'] }
  const match = pathname.slice(base.length).match(/^\/(web_[a-f0-9]{32})(\/(?:verify|mappings|google\/connect|google\/status|ownership\/challenge|ownership\/verify))?$/)
  if (!pathname.startsWith(base) || !match || !isWebsiteId(match[1])) return null
  const suffix = match[2] ?? ''
  return { suffix, id: match[1], methods: suffix === '' ? ['GET', 'PATCH'] : suffix === '/google/status' ? ['GET'] : suffix === '/mappings' ? ['PATCH'] : ['POST'] }
}
async function readCommand(request: IncomingMessage): Promise<Record<string, unknown>> {
  const body = await new Promise<string>((resolve, reject) => {
    let bytes = 0; let text = ''; let settled = false
    const done = (error?: Error) => {
      if (settled) return
      settled = true; clearTimeout(timer); request.off('data', data); request.off('end', end); request.off('error', failure)
      if (error) { request.resume(); reject(error) } else resolve(text)
    }
    const data = (chunk: Buffer) => { bytes += chunk.length; if (bytes > 2048) done(new InvalidCommand()); else text += chunk.toString('utf8') }
    const end = () => done()
    const failure = () => done(new InvalidCommand())
    const timer = setTimeout(failure, 3000)
    request.on('data', data); request.once('end', end); request.once('error', failure)
  })
  let value: unknown
  try { value = JSON.parse(body) as unknown } catch { throw new InvalidCommand() }
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new InvalidCommand()
  return value as Record<string, unknown>
}
function validateCommand(value: Record<string, unknown>, suffix: string, method: string, collection: boolean): object {
  const keys = Object.keys(value).sort().join(',')
  if ((collection && method === 'POST') || (!collection && !suffix && method === 'PATCH')) {
    if (keys !== (collection ? 'site' : 'revision,site') || typeof value.site !== 'string') throw new InvalidCommand()
    try { if (normalizePulseSite(value.site) !== value.site) throw new InvalidCommand() } catch { throw new InvalidCommand() }
    if (!collection && (!Number.isSafeInteger(value.revision) || Number(value.revision) < 1)) throw new InvalidCommand()
    return collection ? { site: value.site } : { site: value.site, revision: value.revision }
  }
  if (suffix === '/mappings') {
    if (!Number.isSafeInteger(value.revision) || Number(value.revision) < 1) throw new InvalidCommand()
    if (keys === 'clarityProjectId,revision' && typeof value.clarityProjectId === 'string' && /^[a-z0-9]{4,32}$/.test(value.clarityProjectId)) return { clarityProjectId: value.clarityProjectId, revision: value.revision }
    if (keys === 'ga4ReportingSite,revision' && typeof value.ga4ReportingSite === 'string') {
      try { if (normalizePulseSite(value.ga4ReportingSite) !== value.ga4ReportingSite) throw new InvalidCommand() } catch { throw new InvalidCommand() }
      return { ga4ReportingSite: value.ga4ReportingSite, revision: value.revision }
    }
    throw new InvalidCommand()
  }
  if (keys !== '') throw new InvalidCommand()
  return {}
}

export function handleWebsiteProxy(request: IncomingMessage, response: ServerResponse, url: URL, context: WebsiteProxyContext): boolean {
  if (url.pathname !== base && !url.pathname.startsWith(base + '/')) return false
  if (/^\/api\/v1\/marketing\/websites\/web_[a-f0-9]{32}\/ownership\/(?:challenge|verify)$/.test(url.pathname)) { context.send(410, 'DNS_VERIFICATION_RETIRED', 'Connect the relevant provider account'); return true }
  const targetRoute = route(url.pathname)
  if (!targetRoute) { context.send(404, 'NOT_FOUND', 'Website route not found'); return true }
  const method = request.method ?? 'GET'
  if (!targetRoute.methods.includes(method)) { context.send(405, 'METHOD_NOT_ALLOWED', 'Method not allowed'); return true }
  const write = method !== 'GET'
  if (write && request.headers.origin !== context.origin) { context.send(403, 'FORBIDDEN', 'Same-origin request required'); return true }
  if (write && !/^application\/json(?:\s*;\s*charset=utf-8)?$/i.test(String(request.headers['content-type'] ?? ''))) { context.send(415, 'INVALID_CONTENT_TYPE', 'JSON is required'); return true }
  const session = url.searchParams.get('sessionId')
  if (targetRoute.suffix === '/google/status' ? [...url.searchParams.keys()].length !== 1 || url.searchParams.getAll('sessionId').length !== 1 || !session || !/^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$/.test(session) : Boolean(url.search)) { context.send(400, 'INVALID_QUERY', 'Invalid website query'); return true }
  if (context.limited(write)) { context.send(429, 'RATE_LIMITED', 'Too many website requests'); return true }
  void (async () => {
    const body = write ? validateCommand(await readCommand(request), targetRoute.suffix, method, !targetRoute.id) : undefined
    const target = new URL(url.pathname, context.upstreamUrl)
    if (session) target.searchParams.set('sessionId', session)
    const upstream = await fetch(target, { method, redirect: 'manual', signal: AbortSignal.timeout(15_000), headers: { Accept: 'application/json', 'X-Internal-API-Key': context.serviceKey, ...(write ? { 'Content-Type': 'application/json' } : {}) }, ...(body ? { body: JSON.stringify(body) } : {}) })
    if ([400, 404, 409, 429].includes(upstream.status)) { context.send(upstream.status, upstream.status === 409 ? 'REVISION_CONFLICT' : 'REQUEST_REJECTED', upstream.status === 409 ? 'Website changed. Reload the website list.' : 'The website request could not be completed.'); return }
    if (![200, 201].includes(upstream.status) || !upstream.headers.get('content-type')?.startsWith('application/json')) throw new Error('Website service unavailable')
    const payload: unknown = JSON.parse((await context.readLimitedBody(upstream, 262_144)).toString('utf8'))
    if (!payload || typeof payload !== 'object' || !('data' in payload)) throw new Error('Invalid website response')
    let data: unknown
    if (!targetRoute.id && method === 'GET') data = { websites: sanitizeWebsiteList(payload.data) }
    else if (targetRoute.suffix === '/google/connect') data = sanitizeGoogleAuthorization(payload.data)
    else if (targetRoute.suffix === '/google/status') data = sanitizeGoogleStatus(payload.data)
    else {
      const website = sanitizeWebsite(payload.data)
      if (targetRoute.id && website.id !== targetRoute.id || body && 'site' in body && website.site !== body.site) throw new Error('Unexpected website')
      data = website
    }
    response.statusCode = upstream.status
    response.setHeader('Content-Type', 'application/json; charset=utf-8')
    response.end(JSON.stringify({ data }))
  })().catch((error: unknown) => context.send(error instanceof InvalidCommand ? 400 : 502, error instanceof InvalidCommand ? 'INVALID_COMMAND' : 'SOURCE_UNAVAILABLE', error instanceof InvalidCommand ? 'Choose a valid website command' : 'Website service is unavailable'))
  return true
}
