import { normalizePulseSite } from './pulse-site-stack.ts'
import { isWebsiteId } from './website-client.ts'

export type SitemapCoverage={sitemapFiles:number;sitemapValid:number;sitemapPartial:boolean;candidates:number;robotsFiltered:number;truncated:boolean}
export function sanitizeSitemapCoverage(value:unknown):SitemapCoverage{if(!record(value))throw Error('Invalid sitemap coverage');for(const k of ['sitemapFiles','sitemapValid','candidates','robotsFiltered'])if(!Number.isSafeInteger(value[k])||Number(value[k])<0||Number(value[k])>(k==='sitemapFiles'||k==='sitemapValid'?20:2000))throw Error('Invalid sitemap count');if(Number(value.sitemapValid)>Number(value.sitemapFiles)||typeof value.sitemapPartial!=='boolean'||typeof value.truncated!=='boolean')throw Error('Invalid sitemap coverage');return {sitemapFiles:Number(value.sitemapFiles),sitemapValid:Number(value.sitemapValid),sitemapPartial:value.sitemapPartial,candidates:Number(value.candidates),robotsFiltered:Number(value.robotsFiltered),truncated:value.truncated}}
export type DiscoveryContext = { websiteId: string; site: string; websiteRevision: number }
export type WebsiteDiscovery = DiscoveryContext & {
  coverage?:SitemapCoverage;source: 'quarry_map'; status: 'idle' | 'running' | 'complete' | 'failed'; limit: 25 | 250; partial: true
  pages: { url: string; title?: string }[]; discoveredAt?: string
  reason?: 'not_configured' | 'source_unavailable' | 'access_denied' | 'policy_blocked' | 'quota_limited' | 'invalid_source'
}
const record = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value)
const controls = /[\x00-\x1f\x7f-\x9f]/
const reasons = ['not_configured', 'source_unavailable', 'access_denied', 'policy_blocked', 'quota_limited', 'invalid_source']
const sensitiveSegments = new Set(['admin', 'api', 'oauth', 'auth', 'login', 'signin', 'sign-in', 'callback', 'account', 'dashboard', 'wp-admin', 'wp-login.php', 'checkout', 'reset-password', 'unsubscribe'])
const path = '/api/v1/visibility/discovery'

export function validateDiscoveryContext(value: unknown): DiscoveryContext {
  if (!record(value) || typeof value.websiteId !== 'string' || !isWebsiteId(value.websiteId) || typeof value.site !== 'string' || normalizePulseSite(value.site) !== value.site || !Number.isSafeInteger(value.websiteRevision) || Number(value.websiteRevision) < 1) throw new Error('Invalid discovery context')
  return { websiteId: value.websiteId, site: value.site, websiteRevision: Number(value.websiteRevision) }
}

function timestamp(value: unknown): value is string {
  if (typeof value !== 'string' || value.length > 40 || !/^\d{4}-\d\d-\d\dT(?:[01]\d|2[0-3]):[0-5]\d:[0-5]\d(?:\.\d{1,9})?(?:Z|[+-](?:[01]\d|2[0-3]):[0-5]\d)$/.test(value)) return false
  const day = new Date(`${value.slice(0, 10)}T00:00:00Z`)
  return Number.isFinite(Date.parse(value)) && Number.isFinite(day.getTime()) && day.toISOString().slice(0, 10) === value.slice(0, 10)
}

function page(value: unknown, origin: string): WebsiteDiscovery['pages'][number] {
  if (!record(value) || typeof value.url !== 'string' || value.url.length > 2048) throw new Error('Invalid discovery page')
  let decoded = value.url
  for (let level = 0; level < 4; level++) {
    if (controls.test(decoded) || decoded.includes('\\')) throw new Error('Unsafe discovery URL')
    const next = decodeURIComponent(decoded)
    if (next === decoded) break
    if (level === 3) throw new Error('Overencoded discovery URL')
    decoded = next
  }
  const url = new URL(value.url)
  const root = new URL(origin).hostname.replace(/^www\./, '')
  const inSite = !url.port && (url.hostname === root || url.hostname === `www.${root}`)
  if (url.protocol !== 'https:' || !inSite || url.username || url.password || url.search || url.hash || url.href !== value.url) throw new Error('Unexpected discovery URL')
  if (new URL(decoded).pathname.toLowerCase().split('/').some((segment) => sensitiveSegments.has(segment))) throw new Error('Sensitive discovery URL')
  if (value.title !== undefined && (typeof value.title !== 'string' || value.title.length > 200 || controls.test(value.title))) throw new Error('Invalid discovery title')
  return { url: value.url, ...(value.title === undefined ? {} : { title: value.title as string }) }
}

/** Display/scope check only; Quarry independently guards DNS and every fetch. */
export function isPublicSitePage(site: string, url: unknown): url is string {
  try { return typeof url === 'string' && Boolean(page({ url }, new URL(normalizePulseSite(site)).origin)) } catch { return false }
}

/** Rebuild a bounded inventory only: Quarry/provider payloads never cross this boundary. */
export function sanitizeWebsiteDiscovery(value: unknown, expected: DiscoveryContext): WebsiteDiscovery {
  const selected = validateDiscoveryContext(expected)
  const context = validateDiscoveryContext(value)
  if (!record(value) || context.websiteId !== selected.websiteId || context.site !== selected.site || context.websiteRevision !== selected.websiteRevision || value.source !== 'quarry_map' || (value.limit !== 25 && value.limit !== 250) || value.partial !== true || typeof value.status !== 'string' || !['idle', 'running', 'complete', 'failed'].includes(value.status) || !Array.isArray(value.pages) || value.pages.length > Number(value.limit)) throw new Error('Unexpected discovery result')
  if (value.status === 'failed' ? typeof value.reason !== 'string' || !reasons.includes(value.reason) : value.reason !== undefined) throw new Error('Invalid discovery reason')
  if (value.status === 'complete' ? !timestamp(value.discoveredAt) : value.discoveredAt !== undefined || value.pages.length !== 0) throw new Error('Invalid discovery state')
  const pages = value.pages.map((item: unknown) => page(item, new URL(selected.site).origin))
  if (new Set(pages.map((item) => item.url)).size !== pages.length) throw new Error('Duplicate discovery page')
  return { ...context, ...(value.coverage===undefined?{}:{coverage:sanitizeSitemapCoverage(value.coverage)}),source: 'quarry_map', status: value.status as WebsiteDiscovery['status'], limit: value.limit as WebsiteDiscovery['limit'], partial: true, pages,
    ...(value.discoveredAt === undefined ? {} : { discoveredAt: value.discoveredAt as string }),
    ...(value.reason === undefined ? {} : { reason: value.reason as WebsiteDiscovery['reason'] }) }
}

async function requestDiscovery(method: 'GET' | 'POST', input: DiscoveryContext, signal?: AbortSignal): Promise<WebsiteDiscovery> {
  const context = validateDiscoveryContext(input)
  const query = new URLSearchParams({ websiteId: context.websiteId, site: context.site })
  const response = await fetch(method === 'GET' ? `${path}?${query}` : path, {
    method, signal, credentials: 'include', redirect: 'error',
    headers: { Accept: 'application/json', ...(method === 'POST' ? { 'Content-Type': 'application/json' } : {}) },
    ...(method === 'POST' ? { body: JSON.stringify(context) } : {}),
  })
  if (!response.ok) throw new Error(response.status === 409 ? 'discovery_conflict' : 'discovery_unavailable')
  const payload: unknown = await response.json()
  return sanitizeWebsiteDiscovery(record(payload) ? payload.data : undefined, context)
}
export const loadWebsiteDiscovery = (context: DiscoveryContext, signal?: AbortSignal) => requestDiscovery('GET', context, signal)
export const startWebsiteDiscovery = (context: DiscoveryContext, signal?: AbortSignal) => requestDiscovery('POST', context, signal)
