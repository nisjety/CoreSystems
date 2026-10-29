import { marketingPublicSignalsContracts } from '../../actions/marketing-public-signals'
import { isPublicSitePage, validateDiscoveryContext, type DiscoveryContext } from './discovery-client'

export type PublicProviderReason = 'not_configured' | 'no_data' | 'quota_limited' | 'source_unavailable' | 'timeout'
export type PublicSignalsReason = 'source_unavailable' | 'timeout' | 'interrupted' | 'context_changed'
export type CruxSignal = { formFactor: 'PHONE' | 'DESKTOP'; status: 'connected' | 'unavailable'; reportingOrigin?: string; reason?: PublicProviderReason; period?: { startDate: string; endDate: string }; metrics?: { lcpMs: number | null; inpMs: number | null; cls: number | null } }
export type CommonCrawlSignal = { status: 'connected' | 'unavailable'; reportingHost?: string; reason?: PublicProviderReason; index?: string; captures: { url: string; capturedAt: string; httpStatus: number }[]; limit: 20; truncated: boolean }
export type PublicSignalsReport = { site: string; source: 'public_web_signals'; observedAt: string; crux: CruxSignal[]; commonCrawl: CommonCrawlSignal }
export type PublicSignalsSnapshot = DiscoveryContext & { source: 'marketing_public_signals'; status: 'idle' | 'queued' | 'running' | 'complete' | 'failed'; enabled: boolean; jobId?: string; queuedAt?: string; startedAt?: string; completedAt?: string; nextSyncAt?: string; expiresAt?: string; report?: PublicSignalsReport; cachedReport?: PublicSignalsReport; reason?: PublicSignalsReason }
const record = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value)
function member<T extends string>(value: unknown, choices: readonly T[]): value is T { return typeof value === 'string' && choices.includes(value as T) }
function day(value: unknown): value is string {
  return typeof value === 'string' && /^\d{4}-\d\d-\d\d$/.test(value) && Number.isFinite(Date.parse(value)) && new Date(`${value}T00:00:00Z`).toISOString().slice(0, 10) === value
}
function timestamp(value: unknown): value is string {
  return typeof value === 'string' && value.length <= 40 && /^\d{4}-\d\d-\d\dT(?:[01]\d|2[0-3]):[0-5]\d:[0-5]\d(?:\.\d{1,9})?(?:Z|[+-](?:[01]\d|2[0-3]):[0-5]\d)$/.test(value) && day(value.slice(0, 10)) && Number.isFinite(Date.parse(value))
}
function providerReason(value: unknown): value is PublicProviderReason { return member(value, ['not_configured', 'no_data', 'quota_limited', 'source_unavailable', 'timeout']) }
function metric(value: unknown): value is number | null { return value === null || typeof value === 'number' && Number.isFinite(value) && value >= 0 && value <= Number.MAX_SAFE_INTEGER }
function crux(value: unknown, observedAt: string, site: string): CruxSignal {
  if (!record(value) || !member(value.formFactor, ['PHONE', 'DESKTOP']) || !member(value.status, ['connected', 'unavailable'])) throw new Error('Invalid CrUX signal')
  if (value.status === 'unavailable') {
    if (!providerReason(value.reason) || value.period !== undefined || value.metrics !== undefined || value.reportingOrigin !== undefined) throw new Error('Invalid unavailable CrUX signal')
    return { formFactor: value.formFactor, status: value.status, reason: value.reason }
  }
  if (value.reason !== undefined || !record(value.period) || !day(value.period.startDate) || !day(value.period.endDate) || value.period.startDate > value.period.endDate || value.period.endDate > observedAt.slice(0, 10) || Date.parse(value.period.endDate) - Date.parse(value.period.startDate) !== 27 * 86400000 || !record(value.metrics) || !metric(value.metrics.lcpMs) || !metric(value.metrics.inpMs) || !metric(value.metrics.cls) || [value.metrics.lcpMs, value.metrics.inpMs, value.metrics.cls].every((item) => item === null)) throw new Error('Invalid connected CrUX signal')
  const selected = new URL(site), alternate = selected.hostname.startsWith('www.') ? `https://${selected.hostname.slice(4)}` : `https://www.${selected.hostname}`
  if (value.reportingOrigin !== undefined && value.reportingOrigin !== selected.origin && value.reportingOrigin !== alternate) throw new Error('Invalid CrUX reporting origin')
  return { formFactor: value.formFactor, status: value.status, ...(value.reportingOrigin === undefined ? {} : { reportingOrigin: value.reportingOrigin as string }), period: { startDate: value.period.startDate, endDate: value.period.endDate }, metrics: { lcpMs: value.metrics.lcpMs, inpMs: value.metrics.inpMs, cls: value.metrics.cls } }
}
function crawl(value: unknown, site: string, observedAt: string): CommonCrawlSignal {
  if (!record(value) || !member(value.status, ['connected', 'unavailable']) || value.limit !== 20 || typeof value.truncated !== 'boolean' || !Array.isArray(value.captures) || value.captures.length > 20 || value.index !== undefined && (typeof value.index !== 'string' || !/^CC-MAIN-\d{4}-(?:0[1-9]|[1-4]\d|5[0-3])$/.test(value.index))) throw new Error('Invalid Common Crawl signal')
  if (value.status === 'unavailable' ? !providerReason(value.reason) || value.captures.length !== 0 || value.truncated || value.reportingHost !== undefined : value.reason !== undefined || value.captures.length === 0 || value.index === undefined) throw new Error('Invalid Common Crawl outcome')
  const host = new URL(site).hostname, alternate = host.startsWith('www.') ? host.slice(4) : `www.${host}`
  if (value.reportingHost !== undefined && value.reportingHost !== host && value.reportingHost !== alternate) throw new Error('Invalid Common Crawl reporting host')
  const reportingSite = value.reportingHost === undefined ? site : `https://${String(value.reportingHost)}/`
  const captures = value.captures.map((item: unknown) => {
    if (!record(item) || typeof item.url !== 'string' || !/^https?:\/\//.test(item.url) || !isPublicSitePage(reportingSite, item.url.replace(/^http:/, 'https:')) || new URL(item.url).hostname !== new URL(reportingSite).hostname || !timestamp(item.capturedAt) || Date.parse(item.capturedAt) > Date.parse(observedAt) || !Number.isSafeInteger(item.httpStatus) || Number(item.httpStatus) < 100 || Number(item.httpStatus) > 599) throw new Error('Invalid Common Crawl capture')
    return { url: item.url, capturedAt: item.capturedAt, httpStatus: Number(item.httpStatus) }
  })
  if (new Set(captures.map((item) => `${item.url}|${item.capturedAt}`)).size !== captures.length) throw new Error('Duplicate Common Crawl capture')
  return { status: value.status, captures, limit: 20, truncated: value.truncated, ...(value.reportingHost === undefined ? {} : { reportingHost: value.reportingHost as string }), ...(value.reason === undefined ? {} : { reason: value.reason as PublicProviderReason }), ...(value.index === undefined ? {} : { index: value.index as string }) }
}
function report(value: unknown, site: string): PublicSignalsReport {
  if (!record(value) || value.site !== site || value.source !== 'public_web_signals' || !timestamp(value.observedAt) || !Array.isArray(value.crux) || value.crux.length !== 2) throw new Error('Invalid public signals report')
  const values = value.crux.map((item) => crux(item, value.observedAt as string, site))
  if (new Set(values.map((item) => item.formFactor)).size !== 2) throw new Error('Duplicate CrUX form factor')
  return { site, source: value.source, observedAt: value.observedAt, crux: values, commonCrawl: crawl(value.commonCrawl, site, value.observedAt) }
}

/** Saved normalized evidence only, not provider credentials or archive page contents. */
export function sanitizePublicSignals(value: unknown, expected: DiscoveryContext): PublicSignalsSnapshot {
  const selected = validateDiscoveryContext(expected), current = validateDiscoveryContext(value)
  if (!record(value) || current.websiteId !== selected.websiteId || current.site !== selected.site || current.websiteRevision !== selected.websiteRevision || value.source !== 'marketing_public_signals' || !member(value.status, ['idle', 'queued', 'running', 'complete', 'failed']) || typeof value.enabled !== 'boolean') throw new Error('Invalid public signals snapshot')
  const times = ['queuedAt', 'startedAt', 'completedAt', 'nextSyncAt', 'expiresAt'] as const
  if (times.some((key) => value[key] !== undefined && !timestamp(value[key]))) throw new Error('Invalid public signals timestamp')
  const result = value.report === undefined ? undefined : report(value.report, selected.site)
  const cached = value.cachedReport === undefined ? undefined : report(value.cachedReport, selected.site)
  const boundary = value.completedAt ?? value.startedAt ?? value.queuedAt
  if (cached && (value.status === 'idle' || !timestamp(boundary) || Date.parse(cached.observedAt) > Date.parse(boundary))) throw new Error('Invalid cached public signals evidence')
  if (value.status === 'idle') {
    if (value.enabled || value.jobId !== undefined || times.some((key) => value[key] !== undefined) || value.reason !== undefined || result) throw new Error('Invalid idle public signals snapshot')
  } else {
    if (typeof value.jobId !== 'string' || !/^public_[a-f0-9]{32}$/.test(value.jobId) || !timestamp(value.queuedAt)) throw new Error('Invalid public signals job')
    if (value.status === 'queued') {
      if (times.slice(1).some((key) => value[key] !== undefined) || value.reason !== undefined || result) throw new Error('Invalid queued public signals snapshot')
    } else {
      if (!timestamp(value.startedAt) || Date.parse(value.startedAt) < Date.parse(value.queuedAt)) throw new Error('Invalid public signals start')
      if (value.status === 'running') {
        if (value.completedAt !== undefined || value.nextSyncAt !== undefined || value.expiresAt !== undefined || value.reason !== undefined || result) throw new Error('Invalid running public signals snapshot')
      } else {
        if (!timestamp(value.completedAt) || Date.parse(value.completedAt) < Date.parse(value.startedAt) || (value.enabled ? !timestamp(value.nextSyncAt) : value.nextSyncAt !== undefined) || value.nextSyncAt !== undefined && Date.parse(value.nextSyncAt as string) < Date.parse(value.completedAt)) throw new Error('Invalid terminal public signals snapshot')
        if (value.status === 'failed') {
          if (!member(value.reason, ['source_unavailable', 'timeout', 'interrupted', 'context_changed']) || value.expiresAt !== undefined || result) throw new Error('Invalid failed public signals snapshot')
        } else if (value.reason !== undefined || !timestamp(value.expiresAt) || Date.parse(value.expiresAt) <= Date.parse(value.completedAt) || Date.parse(value.expiresAt) > Date.parse(value.completedAt) + 86400000 || result && (Date.parse(result.observedAt) < Date.parse(value.startedAt) || Date.parse(result.observedAt) > Date.parse(value.completedAt))) throw new Error('Invalid complete public signals snapshot')
      }
    }
  }
  return { ...current, source: 'marketing_public_signals', status: value.status, enabled: value.enabled, ...(value.jobId === undefined ? {} : { jobId: value.jobId as string }), ...Object.fromEntries(times.filter((key) => value[key] !== undefined).map((key) => [key, value[key]])), ...(result ? { report: result } : {}), ...(cached ? { cachedReport: cached } : {}), ...(value.reason === undefined ? {} : { reason: value.reason as PublicSignalsReason }) }
}
async function request(context: DiscoveryContext, method: 'GET' | 'POST' | 'PATCH', signal?: AbortSignal, enabled?: boolean): Promise<PublicSignalsSnapshot> {
  const selected = validateDiscoveryContext(context), query = new URLSearchParams({ websiteId: selected.websiteId, site: selected.site })
  if (method === 'PATCH' && typeof enabled !== 'boolean') throw new Error('Invalid public signals configuration')
  const path = marketingPublicSignalsContracts.read.path
  const response = await fetch(method === 'GET' ? `${path}?${query}` : path, { method, signal, credentials: 'include', redirect: 'error', headers: { Accept: 'application/json', ...(method === 'GET' ? {} : { 'Content-Type': 'application/json' }) }, ...(method === 'GET' ? {} : { body: JSON.stringify({ ...selected, ...(method === 'PATCH' ? { enabled } : {}) }) }) })
  if (response.status !== (method === 'POST' ? 202 : 200)) throw new Error(response.status === 409 ? 'public_signals_conflict' : [401,403,404].includes(response.status) ? 'public_signals_unavailable_authority' : 'public_signals_unavailable')
  const body: unknown = await response.json()
  return sanitizePublicSignals(record(body) ? body.data : undefined, selected)
}
export const loadPublicSignals = (context: DiscoveryContext, signal?: AbortSignal) => request(context, 'GET', signal)
export const requestPublicSignals = (context: DiscoveryContext, signal?: AbortSignal) => request(context, 'POST', signal)
export const setPublicSignalsEnabled = (context: DiscoveryContext, enabled: boolean, signal?: AbortSignal) => request(context, 'PATCH', signal, enabled)
