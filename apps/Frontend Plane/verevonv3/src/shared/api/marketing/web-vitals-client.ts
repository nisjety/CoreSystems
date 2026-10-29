import { marketingWebVitalsContracts } from '../../actions/marketing-web-vitals'
import { validateDiscoveryContext, type DiscoveryContext } from './discovery-client'

export type VitalsEnvironment = 'production' | 'development'
export type VitalsMetric = { count: number; p75: number | null }
export type VitalsGroup = { viewport: 'narrow' | 'wide'; metrics: { lcp: VitalsMetric; inp: VitalsMetric; cls: VitalsMetric } }
export type WebVitalsReport = DiscoveryContext & { source: 'first_party_web_vitals'; environment: VitalsEnvironment; status: 'not_configured' | 'awaiting_samples' | 'connected'; reportingOrigin?: string; window: { startDate: string; endDate: string }; asOf: string; retentionDays: 7; sampleCount: number; groups: VitalsGroup[] }
const record = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value)
const environment = (value: unknown): value is VitalsEnvironment => value === 'production' || value === 'development'
function day(value: unknown): value is string {
  return typeof value === 'string' && /^\d{4}-\d\d-\d\d$/.test(value) && Number.isFinite(Date.parse(value)) && new Date(`${value}T00:00:00Z`).toISOString().slice(0, 10) === value
}
function timestamp(value: unknown): value is string {
  return typeof value === 'string' && value.length <= 40 && /^\d{4}-\d\d-\d\dT(?:[01]\d|2[0-3]):[0-5]\d:[0-5]\d(?:\.\d{1,9})?(?:Z|[+-](?:[01]\d|2[0-3]):[0-5]\d)$/.test(value) && day(value.slice(0, 10)) && Number.isFinite(Date.parse(value))
}
function metric(value: unknown, limit: number): VitalsMetric {
  if (!record(value) || !Number.isSafeInteger(value.count) || Number(value.count) < 0 || Number(value.count) > 5000 || (value.count === 0 ? value.p75 !== null : typeof value.p75 !== 'number' || !Number.isFinite(value.p75) || value.p75 < 0 || value.p75 > limit)) throw new Error('Invalid browser metric')
  return { count: Number(value.count), p75: value.p75 as number | null }
}
function reportingOrigin(value: unknown, site: string, selected: VitalsEnvironment): string {
  if (typeof value !== 'string' || value.length > 300) throw new Error('Invalid browser reporting origin')
  const url = new URL(value), root = new URL(site).hostname.replace(/^www\./, '')
  if (value !== url.origin || url.username || url.password || url.search || url.hash) throw new Error('Invalid browser reporting origin')
  if (selected === 'production' ? url.protocol !== 'https:' || !!url.port || ![root, `www.${root}`].includes(url.hostname) : !['http://localhost:3000', 'http://127.0.0.1:3000', 'http://localhost:3001', 'http://127.0.0.1:3001'].includes(value)) throw new Error('Wrong browser reporting environment')
  return value
}
/** Rebuild only normalized seven-day aggregates; raw events, paths, identities and secrets are excluded. */
export function sanitizeWebVitals(value: unknown, expected: DiscoveryContext, selected: VitalsEnvironment): WebVitalsReport {
  const current = validateDiscoveryContext(value), context = validateDiscoveryContext(expected)
  if (!record(value) || typeof value.status !== 'string') throw new Error('Invalid browser performance status')
  if (!record(value) || !environment(selected) || current.websiteId !== context.websiteId || current.site !== context.site || current.websiteRevision !== context.websiteRevision || value.source !== 'first_party_web_vitals' || value.environment !== selected || !['not_configured', 'awaiting_samples', 'connected'].includes(String(value.status)) || !record(value.window) || !day(value.window.startDate) || !day(value.window.endDate) || Date.parse(value.window.endDate) - Date.parse(value.window.startDate) !== 6 * 86400000 || !timestamp(value.asOf) || value.window.endDate !== new Date(value.asOf).toISOString().slice(0, 10) || value.retentionDays !== 7 || !Number.isSafeInteger(value.sampleCount) || Number(value.sampleCount) < 0 || Number(value.sampleCount) > 5000 || !Array.isArray(value.groups) || value.groups.length !== 2) throw new Error('Invalid browser performance report')
  const origin = value.reportingOrigin === undefined ? undefined : reportingOrigin(value.reportingOrigin, context.site, selected)
  const groups: VitalsGroup[] = value.groups.map((item: unknown, index) => {
    if (!record(item) || item.viewport !== ['narrow', 'wide'][index] || !record(item.metrics)) throw new Error('Invalid browser viewport group')
    return { viewport: item.viewport as VitalsGroup['viewport'], metrics: { lcp: metric(item.metrics.lcp, 120000), inp: metric(item.metrics.inp, 60000), cls: metric(item.metrics.cls, 100) } }
  })
  const count = groups.reduce((total, group) => total + Object.values(group.metrics).reduce((sum, item) => sum + item.count, 0), 0)
  if (count !== value.sampleCount || (value.status === 'connected' ? count === 0 || !origin : count !== 0) || (value.status === 'not_configured' ? origin !== undefined : !origin)) throw new Error('Invalid browser performance state')
  return { ...context, source: 'first_party_web_vitals', environment: selected, status: value.status as WebVitalsReport['status'], ...(origin ? { reportingOrigin: origin } : {}), window: { startDate: value.window.startDate, endDate: value.window.endDate }, asOf: value.asOf, retentionDays: 7, sampleCount: count, groups }
}
async function request(context: DiscoveryContext, selected: VitalsEnvironment, signal: AbortSignal | undefined, sync: boolean): Promise<WebVitalsReport> {
  const current = validateDiscoveryContext(context)
  if (!environment(selected)) throw new Error('Invalid browser measurement environment')
  const query = new URLSearchParams({ websiteId: current.websiteId, site: current.site, environment: selected })
  const response = await fetch(sync ? marketingWebVitalsContracts.sync.path : `${marketingWebVitalsContracts.read.path}?${query}`, { method: sync ? 'POST' : 'GET', credentials: 'include', redirect: 'error', signal, headers: { Accept: 'application/json', ...(sync ? { 'Content-Type': 'application/json' } : {}) }, ...(sync ? { body: JSON.stringify({ ...current, environment: selected }) } : {}) })
  if (response.status !== 200) throw new Error('web_vitals_unavailable')
  const body: unknown = await response.json()
  return sanitizeWebVitals(record(body) ? body.data : undefined, current, selected)
}
export const loadWebVitals = (context: DiscoveryContext, selected: VitalsEnvironment, signal?: AbortSignal) => request(context, selected, signal, false)
export const syncWebVitals = (context: DiscoveryContext, selected: VitalsEnvironment, signal?: AbortSignal) => request(context, selected, signal, true)
