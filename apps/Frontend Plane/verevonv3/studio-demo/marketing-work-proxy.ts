import type { IncomingMessage, ServerResponse } from 'node:http'
import { normalizePulseSite } from './pulse-site-stack.ts'
import { isWebsiteId } from './website-client.ts'
import { sanitizeMarketingWorkSnapshot } from './marketing-work-client.ts'
import type { MarketingTaskStatus } from './marketing-work-client.ts'
import { pageSpeedCheckIds } from '../src/shared/actions/marketing-pagespeed.ts'

const base = '/api/v1/marketing/work'
const taskPath = /^\/api\/v1\/marketing\/work\/(task_[a-f0-9]{32})$/
const verifyPath = /^\/api\/v1\/marketing\/work\/(task_[a-f0-9]{32})\/verify$/
const technicalPath = `${base}/technical`
const statuses: readonly MarketingTaskStatus[] = ['todo', 'in_progress', 'done', 'dismissed']
class InvalidInput extends Error {}
type Boundary = {
  serviceKey: string; upstreamUrl: string
  readLimitedBody: (response: Response, maxBytes: number) => Promise<Buffer>
  send: (status: number, code: string, message: string) => void
  limited: (write: boolean) => boolean
}
type WorkCommand = { websiteId: string; site: string; websiteRevision?: number; quarter?: string; query?: string; revision?: number; status?: MarketingTaskStatus; strategy?: string; checkId?: string; fetchTime?: string }

function context(value: Record<string, unknown>): WorkContext {
  if (typeof value.websiteId !== 'string' || !isWebsiteId(value.websiteId) || typeof value.site !== 'string') throw new InvalidInput()
  try { if (normalizePulseSite(value.site) !== value.site) throw new Error('noncanonical') } catch { throw new InvalidInput() }
  return { websiteId: value.websiteId, site: value.site }
}
type WorkContext = { websiteId: string; site: string }

function validateQuery(url: URL): WorkContext {
  const keys = [...url.searchParams.keys()]
  if (keys.length !== 2 || keys.some((key) => key !== 'websiteId' && key !== 'site') || url.searchParams.getAll('websiteId').length !== 1 || url.searchParams.getAll('site').length !== 1) throw new InvalidInput()
  return context({ websiteId: url.searchParams.get('websiteId'), site: url.searchParams.get('site') })
}

function validateCommand(value: unknown, method: string, pathTaskID?: string, operation?: 'technical' | 'verify'): WorkCommand {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) throw new InvalidInput()
  const body = value as Record<string, unknown>
  const keys = Object.keys(body).sort().join(',')
  const baseContext = context(body)
  if (operation) {
    if (!Number.isSafeInteger(body.websiteRevision) || Number(body.websiteRevision) < 1) throw new InvalidInput()
    if (operation === 'technical') {
      if (keys !== 'checkId,fetchTime,site,strategy,websiteId,websiteRevision' || typeof body.strategy !== 'string' || !['mobile', 'desktop'].includes(body.strategy) || typeof body.checkId !== 'string' || !pageSpeedCheckIds.includes(body.checkId as typeof pageSpeedCheckIds[number]) || typeof body.fetchTime !== 'string' || body.fetchTime.length > 40 || !/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d{1,9})?(?:Z|[+-]\d\d:\d\d)$/.test(body.fetchTime) || !Number.isFinite(Date.parse(body.fetchTime))) throw new InvalidInput()
      return { ...baseContext, websiteRevision: Number(body.websiteRevision), strategy: body.strategy, checkId: body.checkId, fetchTime: body.fetchTime }
    }
    if (!pathTaskID || keys !== 'revision,site,websiteId,websiteRevision' || !Number.isSafeInteger(body.revision) || Number(body.revision) < 1) throw new InvalidInput()
    return { ...baseContext, websiteRevision: Number(body.websiteRevision), revision: Number(body.revision) }
  }
  if (method === 'POST') {
    if (keys !== 'quarter,query,site,websiteId,websiteRevision' || !Number.isSafeInteger(body.websiteRevision) || Number(body.websiteRevision) < 1 || typeof body.quarter !== 'string' || !/^Q[1-4]$/.test(body.quarter) || typeof body.query !== 'string' || !body.query.trim() || body.query.length > 512 || /[\x00-\x1f\x7f]/.test(body.query)) throw new InvalidInput()
    return { ...baseContext, websiteRevision: Number(body.websiteRevision), quarter: body.quarter, query: body.query }
  }
  if (method === 'PATCH') {
    if (!pathTaskID || keys !== 'revision,site,status,websiteId,websiteRevision' || !Number.isSafeInteger(body.websiteRevision) || Number(body.websiteRevision) < 1 || !Number.isSafeInteger(body.revision) || Number(body.revision) < 1 || typeof body.status !== 'string' || !statuses.includes(body.status as MarketingTaskStatus)) throw new InvalidInput()
    return { ...baseContext, websiteRevision: Number(body.websiteRevision), revision: Number(body.revision), status: body.status as MarketingTaskStatus }
  }
  throw new InvalidInput()
}

async function readCommand(request: IncomingMessage): Promise<unknown> {
  const body = await new Promise<string>((resolve, reject) => {
    let size = 0; let text = ''; let settled = false
    const done = (error?: Error) => { if (settled) return; settled = true; clearTimeout(timer); request.off('data', data); request.off('end', end); request.off('error', failed); if (error) { request.resume(); reject(error) } else resolve(text) }
    const data = (chunk: Buffer) => { size += chunk.length; if (size > 4096) done(new InvalidInput()); else text += chunk.toString('utf8') }
    const end = () => done()
    const failed = () => done(new InvalidInput())
    const timer = setTimeout(failed, 3000)
    request.on('data', data); request.once('end', end); request.once('error', failed)
  })
  try { return JSON.parse(body) as unknown } catch { throw new InvalidInput() }
}

function errorMessage(status: number) {
  if (status === 404) return { code: 'WORK_NOT_FOUND', message: 'The saved opportunity was not found.' }
  if (status === 409) return { code: 'WORK_CONFLICT', message: 'The website or task changed. Refresh and try again.' }
  if (status === 503) return { code: 'WORK_SOURCE_UNAVAILABLE', message: 'The authoritative marketing source is temporarily unavailable.' }
  return { code: 'WORK_REQUEST_REJECTED', message: 'The work request could not be completed.' }
}

/** Same-origin BFF for durable work. It forwards only the narrow fixed contract. */
export function handleMarketingWorkProxy(request: IncomingMessage, response: ServerResponse, url: URL, boundary: Boundary, origin: string): boolean {
  if (url.pathname !== base && url.pathname !== technicalPath && !taskPath.test(url.pathname) && !verifyPath.test(url.pathname)) return false
  response.setHeader('Cache-Control', 'no-store')
  response.setHeader('X-Content-Type-Options', 'nosniff')
  const pathMatch = url.pathname.match(taskPath)
  const verifyMatch = url.pathname.match(verifyPath)
  const operation = url.pathname === technicalPath ? 'technical' : verifyMatch ? 'verify' : undefined
  const method = request.method ?? 'GET'
  const allowedMethods = operation ? ['POST'] : pathMatch ? ['PATCH'] : ['GET', 'POST']
  if (!allowedMethods.includes(method)) {
    response.setHeader('Allow', allowedMethods.join(', ')); boundary.send(405, 'METHOD_NOT_ALLOWED', `${allowedMethods.join(' or ')} is required`); return true
  }
  const write = method !== 'GET'
  if (write && request.headers.origin !== origin) { boundary.send(403, 'FORBIDDEN', 'Same-origin request required'); return true }
  if (write && !/^application\/json(?:\s*;\s*charset=utf-8)?$/i.test(String(request.headers['content-type'] ?? ''))) { boundary.send(415, 'INVALID_CONTENT_TYPE', 'JSON is required'); return true }
  if (write && url.search || !write && !url.search) { boundary.send(400, 'INVALID_QUERY', 'Choose a registered website'); return true }
  if (boundary.limited(write)) { boundary.send(429, 'RATE_LIMITED', 'Too many work requests'); return true }

  void (async () => {
    let input: WorkContext | WorkCommand
    let upstreamMethod: string
    let upstreamPath = url.pathname
    if (method === 'GET') { input = validateQuery(url); upstreamMethod = 'GET'; const target = new URL(base, boundary.upstreamUrl); target.searchParams.set('websiteId', input.websiteId); target.searchParams.set('site', input.site); upstreamPath = target.pathname + target.search }
    else { input = validateCommand(await readCommand(request), method, pathMatch?.[1] ?? verifyMatch?.[1], operation); upstreamMethod = method }
    const target = new URL(upstreamPath, boundary.upstreamUrl)
    const upstream = await fetch(target, { method: upstreamMethod, redirect: 'manual', signal: AbortSignal.timeout(15_000), headers: { Accept: 'application/json', 'X-Internal-API-Key': boundary.serviceKey, ...(write ? { 'Content-Type': 'application/json' } : {}) }, ...(write ? { body: JSON.stringify(input) } : {}) })
    if (upstream.status === 400 || upstream.status === 404 || upstream.status === 409 || upstream.status === 503) { await upstream.body?.cancel(); boundary.send(upstream.status, errorMessage(upstream.status).code, errorMessage(upstream.status).message); return }
    if ((upstream.status !== 200 && upstream.status !== 201) || !upstream.headers.get('content-type')?.startsWith('application/json')) throw new Error('Work endpoint unavailable')
    const bytes = await boundary.readLimitedBody(upstream, 2 << 20)
    const payload: unknown = JSON.parse(bytes.toString('utf8'))
    if (typeof payload !== 'object' || payload === null || !('data' in payload)) throw new Error('Invalid work projection')
    const expected = { websiteId: input.websiteId, site: input.site }
    const data = sanitizeMarketingWorkSnapshot(payload.data, expected)
    response.statusCode = upstream.status
    response.setHeader('Content-Type', 'application/json; charset=utf-8')
    response.end(JSON.stringify({ data }))
  })().catch((error: unknown) => boundary.send(error instanceof InvalidInput ? 400 : 502, error instanceof InvalidInput ? 'INVALID_WORK_REQUEST' : 'SOURCE_UNAVAILABLE', error instanceof InvalidInput ? 'Choose a valid task and registered website' : 'Marketing work is unavailable'))
  return true
}
