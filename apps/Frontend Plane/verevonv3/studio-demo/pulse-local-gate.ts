import { readFileSync } from 'node:fs'
import type { IncomingMessage, ServerResponse } from 'node:http'
import { isPulseSnapshot } from './pulse-client.ts'
import { normalizePulseSite } from './pulse-site-stack.ts'
import { sanitizeSearchSnapshot } from './search-client.ts'
import { sanitizePageSpeedSnapshot } from './pagespeed-client.ts'
import { handleWebsiteProxy } from './website-proxy.ts'
import { isWebsiteId } from './website-client.ts'
import { sanitizeGA4Snapshot } from './ga4-client.ts'
import { sanitizeClaritySnapshot } from './clarity-client.ts'
import { COMPARISON_PATH } from './search-comparison-client.ts'
import { handleComparisonProxy } from './search-comparison-proxy.ts'
import { handleMarketingWorkProxy } from './marketing-work-proxy.ts'
import { handleWebsiteDiscoveryProxy } from './discovery-proxy.ts'

type Next = () => void
type ProxyConfig = { serviceKey: string; upstreamUrl: string; publicPort: number }
const PULSE_PATH = '/api/v1/marketing/pulse'
const SEARCH_PATH = '/api/v1/marketing/search'
const PAGESPEED_PATH = '/api/v1/marketing/pagespeed'
const GA4_PATH = '/api/v1/marketing/ga4'
const CLARITY_PATH = '/api/v1/marketing/clarity'
const PANEL_PATHS: readonly string[] = [PULSE_PATH, SEARCH_PATH, COMPARISON_PATH, PAGESPEED_PATH, GA4_PATH, CLARITY_PATH]

function send(response: ServerResponse, status: number, code: string, message: string) {
  response.statusCode = status
  response.setHeader('Content-Type', 'application/json; charset=utf-8')
  response.end(JSON.stringify({ error: { code, message } }))
}

function prepare(response: ServerResponse) {
  response.setHeader('Cache-Control', 'no-store')
  response.setHeader('X-Content-Type-Options', 'nosniff')
  response.setHeader('Referrer-Policy', 'no-referrer')
}

/** Registry binding failures are client-recoverable, not provider outages.
 * Rebuild the error locally; never read or forward the upstream error body. */
async function sendRegistryBoundaryError(response: ServerResponse, upstream: Response): Promise<boolean> {
  const error = upstream.status === 409
    ? { code: 'WEBSITE_CONTEXT_CONFLICT', message: 'Website URL changed. Reload the website list before requesting data.' }
    : upstream.status === 404
      ? { code: 'WEBSITE_NOT_FOUND', message: 'Website was not found. Reload the website list.' }
      : null
  if (!error) return false
  await upstream.body?.cancel().catch(() => undefined)
  send(response, upstream.status, error.code, error.message)
  return true
}

function originFor(request: IncomingMessage, publicPort: number): string | null {
  const host = request.headers.host
  if (!host || Array.isArray(host)) return null
  try {
    const url = new URL(`http://${host}`)
    if (!['localhost', '127.0.0.1'].includes(url.hostname) || Number(url.port || 80) !== publicPort) return null
    return url.origin
  } catch { return null }
}

async function readLimitedBody(response: Response, maxBytes: number): Promise<Buffer> {
  const length = Number(response.headers.get('content-length'))
  if (Number.isFinite(length) && length > maxBytes) throw new Error('Upstream response too large')
  if (!response.body) throw new Error('Empty upstream response')
  const reader = response.body.getReader()
  const chunks: Buffer[] = []
  let size = 0
  try {
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      size += value.byteLength
      if (size > maxBytes) throw new Error('Upstream response too large')
      chunks.push(Buffer.from(value))
    }
    return Buffer.concat(chunks, size)
  } catch (error) {
    await reader.cancel().catch(() => undefined)
    throw error
  }
}

function projectPayload(path: string, payload: unknown, site: string) {
  if (typeof payload !== 'object' || payload === null || !('data' in payload)) throw new Error('Unexpected upstream projection')
  if (path === GA4_PATH || path === CLARITY_PATH) {
    const data = path === GA4_PATH ? sanitizeGA4Snapshot(payload.data) : sanitizeClaritySnapshot(payload.data)
    if (data.site !== site) throw new Error('Unexpected upstream site')
    return { data }
  }
  if (path === PAGESPEED_PATH) {
    const data = sanitizePageSpeedSnapshot(payload.data)
    if (data.site !== site) throw new Error('Unexpected upstream site')
    return { data }
  }
  if (path === SEARCH_PATH) {
    const data = sanitizeSearchSnapshot(payload.data)
    if (data.site !== site) throw new Error('Unexpected upstream site')
    return { data }
  }
  if (!isPulseSnapshot(payload.data) || payload.data.site !== site) throw new Error('Unexpected upstream projection')
  const data = payload.data
  return { data: {
    site: data.site, source: data.source, status: data.status,
    ...(data.status === 'connected' && data.propertyScope ? { propertyScope: data.propertyScope } : {}),
    period: { startDate: data.period.startDate, endDate: data.period.endDate },
    totals: data.totals === null ? null : { clicks: data.totals.clicks, impressions: data.totals.impressions },
    days: data.days.map((day) => ({ date: day.date, clicks: day.clicks, impressions: day.impressions })),
    ...(typeof data.updatedAt === 'string' ? { updatedAt: data.updatedAt } : {}),
    ...(['not_configured', 'property_unverified', 'source_unavailable'].includes(data.reason ?? '') ? { reason: data.reason } : {}),
  } }
}

export function createPulseProxyMiddleware(config: ProxyConfig) {
  if (!config.serviceKey || !Number.isInteger(config.publicPort)) throw new Error('Invalid local Pulse proxy configuration')
  const reads = new Map<string, { count: number; expires: number }>()
  const audits = new Map<string, { count: number; expires: number }>()
  const websites = new Map<string, { count: number; expires: number }>()
  const work = new Map<string, { count: number; expires: number }>()
  const discovery = new Map<string, { count: number; expires: number }>()
  const limited = (bucket: Map<string, { count: number; expires: number }>, id: string, max: number) => {
    const now = Date.now()
    const prior = bucket.get(id)
    const entry = prior && prior.expires > now ? { ...prior, count: prior.count + 1 } : { count: 1, expires: now + 60_000 }
    bucket.set(id, entry)
    return entry.count > max
  }
  return (request: IncomingMessage, response: ServerResponse, next: Next) => {
    const url = new URL(request.url ?? '/', 'http://studio-demo.local')
    if (!PANEL_PATHS.includes(url.pathname) && url.pathname !== '/api/v1/marketing/discovery' && !url.pathname.startsWith('/api/v1/marketing/websites') && !url.pathname.startsWith('/api/v1/marketing/work')) { next(); return }
    prepare(response)
    const origin = originFor(request, config.publicPort)
    if (!origin || request.headers['sec-fetch-site'] === 'cross-site' || (request.headers.origin && request.headers.origin !== origin)) {
      send(response, 403, 'FORBIDDEN', 'Local same-origin access only'); return
    }
    if (handleWebsiteProxy(request, response, url, { ...config, origin, readLimitedBody, send: (status, code, message) => send(response, status, code, message), limited: (write) => limited(write ? websites : reads, request.socket.remoteAddress ?? 'unknown', write ? 12 : 30) })) return
    if (url.pathname.startsWith('/api/v1/marketing/websites')) { send(response, 404, 'NOT_FOUND', 'Not found'); return }
    if (handleMarketingWorkProxy(request, response, url, { ...config, readLimitedBody, send: (status, code, message) => send(response, status, code, message), limited: (write) => limited(work, request.socket.remoteAddress ?? 'unknown', write ? 15 : 30) }, origin)) return
    if (handleWebsiteDiscoveryProxy(request, response, url, { ...config, readLimitedBody, send: (status, code, message) => send(response, status, code, message), limited: (write) => limited(write ? discovery : reads, request.socket.remoteAddress ?? 'unknown', write ? 3 : 60) }, origin)) return
    if (handleComparisonProxy(request, response, url, { ...config, readLimitedBody, registryError: sendRegistryBoundaryError,
      send: (status, code, message) => send(response, status, code, message), limited: () => limited(reads, request.socket.remoteAddress ?? 'unknown', 30) })) return
    if (url.pathname === PAGESPEED_PATH) {
      if (request.method !== 'GET' && request.method !== 'POST') { send(response, 405, 'METHOD_NOT_ALLOWED', 'Method not allowed'); return }
      if (request.method === 'POST' && request.headers.origin !== origin) { send(response, 403, 'FORBIDDEN', 'Same-origin audit request required'); return }
      if (request.method === 'POST' && !/^application\/json(?:\s*;\s*charset=utf-8)?$/i.test(String(request.headers['content-type'] ?? ''))) { send(response, 415, 'INVALID_CONTENT_TYPE', 'JSON is required'); return }
      const id = request.socket.remoteAddress ?? 'unknown'
      void pageSpeedInput(request, url).then(async (command) => {
        if (limited(request.method === 'POST' ? audits : reads, id, request.method === 'POST' ? 3 : 30)) { send(response, 429, 'RATE_LIMITED', 'Too many audit requests'); return }
        const target = new URL(PAGESPEED_PATH, config.upstreamUrl)
        if (request.method === 'GET') { target.searchParams.set('site', command.site); target.searchParams.set('strategy', command.strategy); if (command.websiteId) target.searchParams.set('websiteId', command.websiteId) }
        const upstream = await fetch(target, {
          method: request.method, redirect: 'manual', signal: AbortSignal.timeout(10_000),
          headers: { Accept: 'application/json', 'X-Internal-API-Key': config.serviceKey, ...(request.method === 'POST' ? { 'Content-Type': 'application/json' } : {}) },
          ...(request.method === 'POST' ? { body: JSON.stringify(command) } : {}),
        })
        if (await sendRegistryBoundaryError(response, upstream)) return
        if (upstream.status !== 200 || !upstream.headers.get('content-type')?.startsWith('application/json')) throw new Error('Upstream unavailable')
        const bytes = await readLimitedBody(upstream, 32_768)
        const payload: unknown = JSON.parse(bytes.toString('utf8'))
        const sanitized = projectPayload(PAGESPEED_PATH, payload, command.site)
        if (!('strategy' in sanitized.data) || sanitized.data.strategy !== command.strategy) throw new Error('Unexpected audit device')
        response.setHeader('Content-Type', 'application/json; charset=utf-8')
        response.end(JSON.stringify(sanitized))
      }).catch((error: unknown) => {
        if (error instanceof InvalidAuditInput) send(response, 400, 'INVALID_COMMAND', 'Choose a public HTTPS website and device')
        else send(response, 502, 'SOURCE_UNAVAILABLE', 'PageSpeed is unavailable')
      })
      return
    }
    if (request.method !== 'GET') { send(response, 405, 'METHOD_NOT_ALLOWED', 'Method not allowed'); return }
    const quarter = url.searchParams.get('quarter')
    const site = url.searchParams.get('site')
    const websiteId = url.searchParams.get('websiteId')
    const clarity = url.pathname === CLARITY_PATH
    const requiresRegistry = clarity || url.pathname === GA4_PATH
    const allowed = clarity ? ['site', 'websiteId'] : ['quarter', 'site', 'websiteId']
    if ((!clarity && (!quarter || !/^Q[1-4]$/.test(quarter) || url.searchParams.getAll('quarter').length !== 1)) || !site || [...url.searchParams.keys()].some((key) => !allowed.includes(key)) || url.searchParams.getAll('site').length !== 1 || url.searchParams.getAll('websiteId').length > 1 || (requiresRegistry && !websiteId) || websiteId !== null && !isWebsiteId(websiteId)) {
      send(response, 400, 'INVALID_QUERY', 'Choose a valid quarter and website'); return
    }
    try { if (normalizePulseSite(site) !== site) throw new Error('Noncanonical site') } catch { send(response, 400, 'INVALID_SITE', 'Choose a public HTTPS website'); return }
    if (limited(reads, request.socket.remoteAddress ?? 'unknown', 30)) { send(response, 429, 'RATE_LIMITED', 'Too many data requests'); return }
    const target = new URL(url.pathname, config.upstreamUrl)
    if (quarter) target.searchParams.set('quarter', quarter)
    target.searchParams.set('site', site)
    if (websiteId) target.searchParams.set('websiteId', websiteId)
    void fetch(target, {
      method: 'GET', redirect: 'manual', signal: AbortSignal.timeout(10_000),
      headers: { Accept: 'application/json', 'X-Internal-API-Key': config.serviceKey },
    }).then(async (upstream) => {
      if (await sendRegistryBoundaryError(response, upstream)) return
      if (upstream.status !== 200 || !upstream.headers.get('content-type')?.startsWith('application/json')) throw new Error('Upstream unavailable')
      // Go's JSON encoder can expand each bounded key byte into a six-byte
      // HTML escape. Keep a hard cap that also admits valid escaped rows.
      const bytes = await readLimitedBody(upstream, url.pathname === SEARCH_PATH ? 2 << 20 : clarity ? 32_768 : 120_000)
      const payload: unknown = JSON.parse(bytes.toString('utf8'))
      const sanitized = projectPayload(url.pathname, payload, site)
      response.setHeader('Content-Type', 'application/json; charset=utf-8')
      response.end(JSON.stringify(sanitized))
    }).catch(() => send(response, 502, 'SOURCE_UNAVAILABLE', 'Marketing data is unavailable'))
  }
}

class InvalidAuditInput extends Error {}

async function pageSpeedInput(request: IncomingMessage, url: URL): Promise<{ site: string; strategy: string; websiteId?: string }> {
  let value: unknown
  if (request.method === 'GET') {
    if ([...url.searchParams.keys()].some((key) => !['site', 'strategy', 'websiteId'].includes(key)) || url.searchParams.getAll('site').length !== 1 || url.searchParams.getAll('strategy').length !== 1 || url.searchParams.getAll('websiteId').length > 1) throw new InvalidAuditInput()
    value = { site: url.searchParams.get('site'), strategy: url.searchParams.get('strategy'), ...(url.searchParams.has('websiteId') ? { websiteId: url.searchParams.get('websiteId') } : {}) }
  } else {
    if (url.search) throw new InvalidAuditInput()
    const body = await new Promise<string>((resolve, reject) => {
      let text = ''
      let size = 0
      const finish = (error?: Error) => { clearTimeout(timer); request.off('data', data); request.off('end', end); request.off('error', failure); if (error) { request.resume(); reject(error) } else resolve(text) }
      const data = (chunk: Buffer) => { size += chunk.length; if (size > 2048) finish(new InvalidAuditInput()); else text += chunk.toString('utf8') }
      const end = () => finish()
      const failure = () => finish(new InvalidAuditInput())
      const timer = setTimeout(failure, 3000)
      request.on('data', data); request.once('end', end); request.once('error', failure)
    })
    try { value = JSON.parse(body) as unknown } catch { throw new InvalidAuditInput() }
  }
  if (typeof value !== 'object' || value === null || Array.isArray(value) || !['site,strategy', 'site,strategy,websiteId'].includes(Object.keys(value).sort().join(',')) || !('site' in value) || typeof value.site !== 'string' || !('strategy' in value) || typeof value.strategy !== 'string' || !['mobile', 'desktop'].includes(value.strategy) || 'websiteId' in value && !isWebsiteId(value.websiteId)) throw new InvalidAuditInput()
  try { if (normalizePulseSite(value.site) !== value.site) throw new Error('Noncanonical site') } catch { throw new InvalidAuditInput() }
  return { site: value.site, strategy: value.strategy, ...('websiteId' in value ? { websiteId: value.websiteId as string } : {}) }
}

export function pulseProxyConfigFromEnvironment(): ProxyConfig | null {
  const keyPath = process.env.STUDIO_MARKETING_CORE_KEY_FILE
  const upstreamUrl = process.env.STUDIO_MARKETING_CORE_URL
  if (!keyPath || !upstreamUrl) return null
  const target = new URL(upstreamUrl)
  if (target.protocol !== 'http:' || !['marketing-core', '127.0.0.1', 'localhost'].includes(target.hostname) || target.pathname !== '/' || target.search || target.hash || target.username || target.password) throw new Error('Invalid Marketing Core URL')
  const publicPort = Number(process.env.STUDIO_DEMO_PUBLIC_PORT ?? '5174')
  if (!Number.isInteger(publicPort) || publicPort < 1 || publicPort > 65535) throw new Error('Invalid Studio demo public port')
  return { serviceKey: readFileSync(keyPath, 'utf8').trim(), upstreamUrl, publicPort }
}
