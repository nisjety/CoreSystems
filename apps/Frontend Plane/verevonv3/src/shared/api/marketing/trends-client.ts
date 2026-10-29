import { marketingTrendsContract, marketingAutomaticTrendsContract } from '@/shared/actions/marketing-trends'
import { validateDiscoveryContext, type DiscoveryContext } from './discovery-client'

export type TrendsQuery = { terms: string[]; geo: string; timeframe: 'today 3-m' | 'today 12-m' | 'today 5-y' }
export type TrendsReport = TrendsQuery & {
  source: 'pytrends_dev'; scale: 'relative_0_100_per_request';
  status: 'available' | 'no_data' | 'disabled' | 'rate_limited' | 'blocked' | 'timeout' | 'upstream_error' | 'cooldown';
  observedAt: string; cached: boolean; retryAt?: string;
  rows: { date: string; values: number[]; partial: boolean }[];
}
const record = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
export function validTrendsQuery(q: TrendsQuery): boolean {
  return Array.isArray(q.terms) && q.terms.length > 0 && q.terms.length <= 5 && new Set(q.terms).size === q.terms.length
    && q.terms.every((term) => typeof term === 'string' && term.trim() === term && term.length > 0 && new TextEncoder().encode(term).length <= 80 && !/\p{C}/u.test(term))
    && /^([A-Z]{2})?$/.test(q.geo) && ['today 3-m', 'today 12-m', 'today 5-y'].includes(q.timeframe)
}
export function sanitizeTrends(value: unknown, context: DiscoveryContext, query: TrendsQuery): TrendsReport {
  const selected = validateDiscoveryContext(context), current = validateDiscoveryContext(value)
  if (!record(value) || current.websiteId !== selected.websiteId || current.site !== selected.site || current.websiteRevision !== selected.websiteRevision || !record(value.report)) throw new Error('Invalid Trends context')
  const r = value.report
  const timestamp = (v: unknown): v is string => typeof v === 'string' && /^\d{4}-\d\d-\d\dT/.test(v) && Number.isFinite(Date.parse(v))
  if (r.source !== 'pytrends_dev' || r.scale !== 'relative_0_100_per_request' || JSON.stringify(r.terms) !== JSON.stringify(query.terms) || r.geo !== query.geo || r.timeframe !== query.timeframe || typeof r.cached !== 'boolean'
    || !timestamp(r.observedAt) || Date.parse(r.observedAt) > Date.now() + 60000 || Date.parse(r.observedAt) < Date.now() - 25 * 3600000
    || !['available', 'no_data', 'disabled', 'rate_limited', 'blocked', 'timeout', 'upstream_error', 'cooldown'].includes(String(r.status)) || !Array.isArray(r.rows) || r.rows.length > 400
    || (r.retryAt !== undefined && (!timestamp(r.retryAt) || Date.parse(r.retryAt) > Date.now() + 7200000))
    || (r.status === 'available' ? r.rows.length === 0 || r.retryAt !== undefined : r.rows.length !== 0)) throw new Error('Invalid Trends report')
  let previous = ''
  const rows = r.rows.map((row: unknown) => {
    if (!record(row) || typeof row.date !== 'string' || !/^\d{4}-\d\d-\d\d$/.test(row.date) || !Number.isFinite(Date.parse(row.date)) || new Date(row.date).toISOString().slice(0, 10) !== row.date || row.date <= previous || Date.parse(row.date) > Date.now()
      || typeof row.partial !== 'boolean' || !Array.isArray(row.values) || row.values.length !== query.terms.length || !row.values.every((v) => Number.isInteger(v) && v >= 0 && v <= 100)) throw new Error('Invalid Trends series')
    previous = row.date
    return { date: row.date, partial: row.partial, values: row.values as number[] }
  })
  return { ...query, source: 'pytrends_dev', scale: 'relative_0_100_per_request', status: r.status as TrendsReport['status'], observedAt: r.observedAt, cached: r.cached, ...(typeof r.retryAt === 'string' ? { retryAt: r.retryAt } : {}), rows }
}
export async function requestTrends(context: DiscoveryContext, query: TrendsQuery, signal?: AbortSignal): Promise<TrendsReport> {
  const selected = validateDiscoveryContext(context)
  if (!validTrendsQuery(query)) throw new Error('invalid_trends_query')
  const response = await fetch(marketingTrendsContract.path, { method: marketingTrendsContract.method, signal, credentials: 'include', redirect: 'error', headers: { Accept: 'application/json', 'Content-Type': 'application/json' }, body: JSON.stringify({ ...selected, ...query }) })
  if (!response.ok) throw new Error(response.status === 409 ? 'trends_context_changed' : 'trends_unavailable')
  const body: unknown = await response.json()
  return sanitizeTrends(record(body) ? body.data : undefined, selected, query)
}

export type AutomaticTrends = DiscoveryContext & {
  source: 'marketing_trends_auto'; status: 'pending' | 'waiting_for_search_console' | 'waiting_for_keywords' | 'running' | 'complete' | 'failed';
  reason?: string; updatedAt?: string; nextSyncAt?: string; expiresAt?: string;
  seed?: { source: 'google_search_console'; selection: 'impressions_then_clicks'; terms: string[]; observedAt: string; period: { startDate: string; endDate: string } };
  report?: TrendsReport;
}
export function sanitizeAutomaticTrends(value: unknown, context: DiscoveryContext): AutomaticTrends {
  const selected = validateDiscoveryContext(context), current = validateDiscoveryContext(value)
  if (!record(value) || current.websiteId !== selected.websiteId || current.site !== selected.site || current.websiteRevision !== selected.websiteRevision || value.source !== 'marketing_trends_auto' || !['pending', 'waiting_for_search_console', 'waiting_for_keywords', 'running', 'complete', 'failed'].includes(String(value.status))) throw new Error('Invalid automatic Trends context')
  for (const field of ['updatedAt', 'nextSyncAt', 'expiresAt']) if (value[field] !== undefined && (typeof value[field] !== 'string' || !Number.isFinite(Date.parse(value[field] as string)))) throw new Error('Invalid automatic Trends time')
  if (value.reason !== undefined && !['report_expired', 'source_unavailable', 'interrupted', 'disabled', 'rate_limited', 'blocked', 'timeout', 'upstream_error', 'cooldown'].includes(String(value.reason))) throw new Error('Invalid automatic Trends reason')
  let seed: AutomaticTrends['seed']
  if (value.seed !== undefined) {
    const v = value.seed
    if (!record(v) || v.source !== 'google_search_console' || v.selection !== 'impressions_then_clicks' || !Array.isArray(v.terms) || !validTrendsQuery({ terms: v.terms, geo: '', timeframe: 'today 12-m' }) || typeof v.observedAt !== 'string' || !Number.isFinite(Date.parse(v.observedAt)) || !record(v.period) || typeof v.period.startDate !== 'string' || typeof v.period.endDate !== 'string' || !/^\d{4}-\d\d-\d\d$/.test(v.period.startDate) || !/^\d{4}-\d\d-\d\d$/.test(v.period.endDate) || v.period.endDate < v.period.startDate) throw new Error('Invalid keyword seed')
    seed = { source: v.source, selection: v.selection, terms: v.terms as string[], observedAt: v.observedAt, period: { startDate: v.period.startDate, endDate: v.period.endDate } }
  }
  const report = value.report === undefined ? undefined : seed ? sanitizeTrends(value, selected, { terms: seed.terms, geo: '', timeframe: 'today 12-m' }) : undefined
  if (report && (typeof value.expiresAt !== 'string' || Date.parse(value.expiresAt) <= Date.now() || Date.parse(value.expiresAt) > Date.parse(report.observedAt) + 86400000) || value.status === 'complete' && report && !['available', 'no_data'].includes(report.status)) throw new Error('Invalid automatic Trends freshness')
  if (value.report !== undefined && !report || value.status === 'complete' && !report || (value.status === 'waiting_for_search_console' || value.status === 'waiting_for_keywords') && (seed || report) || value.status === 'running' && (!seed || report)) throw new Error('Invalid automatic Trends state')
  return { ...current, source: 'marketing_trends_auto', status: value.status as AutomaticTrends['status'], ...(seed ? { seed } : {}), ...(report ? { report } : {}), ...Object.fromEntries(['reason', 'updatedAt', 'nextSyncAt', 'expiresAt'].filter((key) => value[key] !== undefined).map((key) => [key, value[key]])) }
}
export async function loadAutomaticTrends(context: DiscoveryContext, signal?: AbortSignal): Promise<AutomaticTrends> {
  const selected = validateDiscoveryContext(context)
  const query = new URLSearchParams({ websiteId: selected.websiteId, site: selected.site })
  const response = await fetch(`${marketingAutomaticTrendsContract.path}?${query}`, { method: 'GET', signal, credentials: 'include', redirect: 'error', headers: { Accept: 'application/json' } })
  if (!response.ok) throw new Error('automatic_trends_unavailable')
  const body: unknown = await response.json()
  return sanitizeAutomaticTrends(record(body) ? body.data : undefined, selected)
}
