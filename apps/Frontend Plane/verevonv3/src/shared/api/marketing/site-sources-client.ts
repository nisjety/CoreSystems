import { marketingSiteSourcesContracts } from '../../actions/marketing-site-sources'
import { validateDiscoveryContext, type DiscoveryContext } from './discovery-client'

export type SiteSourceProvider = 'cookiebot' | 'algolia'
export type SourceComponentStatus = 'connected' | 'no_data' | 'access_denied' | 'plan_required' | 'rate_limited' | 'unavailable'
type ConsentTotals = { optIn: number; optOut: number; preferences: number; statistics: number; marketing: number }
type InventoryCategories = { necessary: number; preferences: number; statistics: number; marketing: number; unclassified: number }
export type CookiebotData = { reportingDomain: string; consents: { status: SourceComponentStatus; reportedDays: number; totals?: ConsentTotals }; inventory: { status: SourceComponentStatus; scannedAt?: string; total?: number; categories?: InventoryCategories; thirdParty?: number; secure?: number; httpOnly?: number } }
export type AlgoliaTerms = { status: 'connected' | 'unavailable'; limit: 20; rows?: { query: string; count: number; averageResults?: number }[] }
export type AlgoliaData = { indexName: string; region: 'us' | 'de' | 'provider'; analytics: { status: SourceComponentStatus; reportedDays: number; searches?: number; noResultSearches?: number; noResultRate?: number | null }; topSearches?: AlgoliaTerms; noResultTerms?: AlgoliaTerms }
export type SiteSourceReport = DiscoveryContext & { provider: SiteSourceProvider; status: 'not_loaded' | 'not_configured' | 'connected' | 'partial' | 'unavailable'; observedAt: string; window: { startDate: string; endDate: string }; cookiebot?: CookiebotData; algolia?: AlgoliaData }
const record = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value)
const provider = (value: unknown): value is SiteSourceProvider => value === 'cookiebot' || value === 'algolia'
const count = (value: unknown): value is number => Number.isSafeInteger(value) && Number(value) >= 0
const status = (value: unknown): value is SourceComponentStatus => typeof value === 'string' && ['connected', 'no_data', 'access_denied', 'plan_required', 'rate_limited', 'unavailable'].includes(value)
const success = (value: SourceComponentStatus) => value === 'connected' || value === 'no_data'
const coverage = (value: unknown): value is number => count(value) && value <= 7
function day(value: unknown): value is string {
  return typeof value === 'string' && /^\d{4}-\d\d-\d\d$/.test(value) && Number.isFinite(Date.parse(value)) && new Date(`${value}T00:00:00Z`).toISOString().slice(0, 10) === value
}
function timestamp(value: unknown): value is string {
  return typeof value === 'string' && value.length <= 40 && /^\d{4}-\d\d-\d\dT(?:[01]\d|2[0-3]):[0-5]\d:[0-5]\d(?:\.\d{1,9})?(?:Z|[+-](?:[01]\d|2[0-3]):[0-5]\d)$/.test(value) && day(value.slice(0, 10)) && Number.isFinite(Date.parse(value))
}
function consent(value: unknown): CookiebotData['consents'] {
  if (!record(value) || !status(value.status) || !coverage(value.reportedDays)) throw new Error('Invalid consent coverage')
  if (value.status !== 'connected') {
    if (value.reportedDays !== 0 || value.totals !== undefined) throw new Error('Invalid missing consent measurements')
    return { status: value.status, reportedDays: 0 }
  }
  const keys = ['optIn', 'optOut', 'preferences', 'statistics', 'marketing'] as const
  if (value.reportedDays < 1 || !record(value.totals) || keys.some((key) => !count((value.totals as Record<string, unknown>)[key]))) throw new Error('Invalid consent totals')
  return { status: value.status, reportedDays: value.reportedDays, totals: Object.fromEntries(keys.map((key) => [key, (value.totals as Record<string, number>)[key]])) as ConsentTotals }
}
function inventory(value: unknown, observedAt: string): CookiebotData['inventory'] {
  if (!record(value) || !status(value.status)) throw new Error('Invalid scan status')
  const fields = ['scannedAt', 'total', 'categories', 'thirdParty', 'secure', 'httpOnly']
  if (value.status !== 'connected') {
    if (fields.some((key) => value[key] !== undefined)) throw new Error('Invalid missing scan metrics')
    return { status: value.status }
  }
  const keys = ['necessary', 'preferences', 'statistics', 'marketing', 'unclassified'] as const
  if (!timestamp(value.scannedAt) || Date.parse(value.scannedAt) > Date.parse(observedAt) || !count(value.total) || !record(value.categories) || keys.some((key) => !count((value.categories as Record<string, unknown>)[key])) || ['thirdParty', 'secure', 'httpOnly'].some((key) => !count(value[key]) || Number(value[key]) > Number(value.total))) throw new Error('Invalid scan metrics')
  const categories = Object.fromEntries(keys.map((key) => [key, (value.categories as Record<string, number>)[key]])) as InventoryCategories
  if (Object.values(categories).reduce((sum, item) => sum + item, 0) !== value.total) throw new Error('Invalid category totals')
  return { status: value.status, scannedAt: value.scannedAt, total: value.total, categories, thirdParty: value.thirdParty as number, secure: value.secure as number, httpOnly: value.httpOnly as number }
}
function cookie(value: unknown, site: string, observedAt: string): CookiebotData {
  const host = new URL(site).hostname.replace(/^www\./, '')
  if (!record(value) || ![host, `www.${host}`].includes(value.reportingDomain as string)) throw new Error('Invalid consent reporting domain')
  return { reportingDomain: value.reportingDomain as string, consents: consent(value.consents), inventory: inventory(value.inventory, observedAt) }
}
function analytics(value: unknown): AlgoliaData['analytics'] {
  if (!record(value) || !status(value.status) || !coverage(value.reportedDays)) throw new Error('Invalid search analytics coverage')
  if (value.status !== 'connected') {
    if (value.reportedDays !== 0 || ['searches', 'noResultSearches', 'noResultRate'].some((key) => value[key] !== undefined)) throw new Error('Invalid missing search metrics')
    return { status: value.status, reportedDays: 0 }
  }
  if (!count(value.searches) || !count(value.noResultSearches) || value.noResultSearches > value.searches || (value.searches === 0 ? value.noResultSearches !== 0 || value.noResultRate !== null : value.reportedDays < 1 || typeof value.noResultRate !== 'number' || !Number.isFinite(value.noResultRate) || value.noResultRate < 0 || value.noResultRate > 1 || Math.abs(value.noResultRate - value.noResultSearches / value.searches) > 1e-6)) throw new Error('Invalid search analytics totals')
  return { status: value.status, reportedDays: value.reportedDays, searches: value.searches, noResultSearches: value.noResultSearches, noResultRate: value.noResultRate as number | null }
}
function search(value: unknown): AlgoliaData {
  if (!record(value) || typeof value.indexName !== 'string' || !/^[A-Za-z0-9_.-]{1,128}$/.test(value.indexName) || !['us', 'de', 'provider'].includes(value.region as string)) throw new Error('Invalid search index mapping')
  return { indexName: value.indexName, region: value.region as AlgoliaData['region'], analytics: analytics(value.analytics), ...(value.topSearches === undefined ? {} : { topSearches: terms(value.topSearches) }), ...(value.noResultTerms === undefined ? {} : { noResultTerms: terms(value.noResultTerms) }) }
}
function terms(value: unknown): AlgoliaTerms {
  if (!record(value) || !['connected','unavailable'].includes(String(value.status)) || value.limit !== 20) throw new Error('Invalid search terms status')
  if (value.status === 'unavailable') { if (value.rows !== undefined) throw new Error('Unavailable search terms contain observations'); return { status: 'unavailable', limit: 20 } }
  if (!Array.isArray(value.rows) || value.rows.length > 20) throw new Error('Invalid search terms coverage')
  const rows = value.rows.map((row: unknown) => {
    if (!record(row) || typeof row.query !== 'string' || Array.from(row.query).length > 512 || new TextEncoder().encode(row.query).length > 2048 || /[\x00-\x1f\x7f-\x9f]/.test(row.query) || Array.from(row.query).some(c => { const p = c.codePointAt(0)!; return p >= 0xd800 && p <= 0xdfff }) || !count(row.count) || row.averageResults !== undefined && (typeof row.averageResults !== 'number' || !Number.isFinite(row.averageResults) || row.averageResults < 0 || row.averageResults > Number.MAX_SAFE_INTEGER)) throw new Error('Invalid search term observation')
    return { query: row.query, count: row.count, ...(row.averageResults === undefined ? {} : { averageResults: row.averageResults as number }) }
  })
  if(new Set(rows.map(row=>row.query)).size!==rows.length)throw new Error('Duplicate search term observation')
  return { status: 'connected', limit: 20, rows }
}
/** Bounded workspace-private aggregate observations only; no user identities or credentials. */
export function sanitizeSiteSource(value: unknown, expected: DiscoveryContext, selected: SiteSourceProvider): SiteSourceReport {
  const context = validateDiscoveryContext(expected), current = validateDiscoveryContext(value)
  if (!record(value) || !provider(selected) || value.provider !== selected || current.websiteId !== context.websiteId || current.site !== context.site || current.websiteRevision !== context.websiteRevision || typeof value.status !== 'string' || !['not_loaded', 'not_configured', 'connected', 'partial', 'unavailable'].includes(value.status) || !timestamp(value.observedAt) || !record(value.window) || !day(value.window.startDate) || !day(value.window.endDate)) throw new Error('Invalid site-source report')
  const end = new Date(`${new Date(value.observedAt).toISOString().slice(0, 10)}T00:00:00Z`).getTime() - 86400000
  if (Date.parse(value.window.endDate) !== end || Date.parse(value.window.startDate) !== end - 6 * 86400000) throw new Error('Invalid completed reporting window')
  const base: SiteSourceReport = { ...current, provider: selected, status: value.status as SiteSourceReport['status'], observedAt: value.observedAt, window: { startDate: value.window.startDate, endDate: value.window.endDate } }
  if (value.status === 'not_loaded' || value.status === 'not_configured') {
    if (value.cookiebot !== undefined || value.algolia !== undefined) throw new Error('Unexpected unavailable provider payload')
    return base
  }
  if (selected === 'cookiebot') {
    if (value.algolia !== undefined) throw new Error('Unexpected provider payload')
    const data = cookie(value.cookiebot, context.site, value.observedAt), ok = [data.consents.status, data.inventory.status].filter(success).length
    if (value.status !== (ok === 2 ? 'connected' : ok === 1 ? 'partial' : 'unavailable')) throw new Error('Invalid consent source outcome')
    return { ...base, cookiebot: data }
  }
  if (value.cookiebot !== undefined) throw new Error('Unexpected provider payload')
  const data = search(value.algolia)
  if (value.status !== (success(data.analytics.status) || data.topSearches?.status === 'connected' || data.noResultTerms?.status === 'connected' ? 'connected' : 'unavailable')) throw new Error('Invalid search source outcome')
  return { ...base, algolia: data }
}
async function request(context: DiscoveryContext, selected: SiteSourceProvider, signal: AbortSignal | undefined, sync: boolean): Promise<SiteSourceReport> {
  const current = validateDiscoveryContext(context)
  if (!provider(selected)) throw new Error('Invalid site-source provider')
  const query = new URLSearchParams({ websiteId: current.websiteId, site: current.site, provider: selected })
  const response = await fetch(sync ? marketingSiteSourcesContracts.sync.path : `${marketingSiteSourcesContracts.read.path}?${query}`, { method: sync ? 'POST' : 'GET', credentials: 'include', redirect: 'error', signal, headers: { Accept: 'application/json', ...(sync ? { 'Content-Type': 'application/json' } : {}) }, ...(sync ? { body: JSON.stringify({ ...current, provider: selected }) } : {}) })
  if (response.status !== 200) throw new Error('site_source_unavailable')
  const body: unknown = await response.json()
  return sanitizeSiteSource(record(body) ? body.data : undefined, current, selected)
}
export const loadSiteSource = (context: DiscoveryContext, selected: SiteSourceProvider, signal?: AbortSignal) => request(context, selected, signal, false)
export const syncSiteSource = (context: DiscoveryContext, selected: SiteSourceProvider, signal?: AbortSignal) => request(context, selected, signal, true)
