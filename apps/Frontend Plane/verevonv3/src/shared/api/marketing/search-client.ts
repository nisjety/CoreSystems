import { normalizePulseSite } from './pulse-site-stack.ts'
import { pulsePeriodDates } from './pulse-timeline.ts'
import type { PulseQuarter } from './pulse-client.ts'
import { isWebsiteId, isGSCReportingProperty } from './website-client.ts'

export type SearchRow = { key: string; clicks: number; impressions: number; ctr: number; position: number }
export type OpportunityType = 'snippet_review' | 'title_meta' | 'title_test' | 'content_expansion'
export type Opportunity = SearchRow & { type: OpportunityType }
export type SearchSnapshot = {
  site: string
  source: 'google_search_console'
  status: 'connected' | 'unavailable'
  period: { startDate: string; endDate: string }
  queries: SearchRow[]
  pages: SearchRow[]
  opportunities: Opportunity[]
  rowLimit: 100
  queriesAtLimit: boolean
  pagesAtLimit: boolean
  propertyScope?: 'domain' | 'url_prefix'
  reportingProperty?: string
  updatedAt?: string
  reason?: 'not_configured' | 'property_unverified' | 'source_unavailable' | 'snapshot_pending' | 'snapshot_expired' | 'authorization_required'
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value)
const isCount = (value: unknown): value is number => typeof value === 'number' && Number.isSafeInteger(value) && value >= 0
const finitePositive = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value) && value >= 0
const encoder = new TextEncoder()

export function isSearchRow(value: unknown, maxBytes: number): value is SearchRow {
  if (!isRecord(value) || typeof value.key !== 'string' || !value.key.trim() || /[\x00-\x1f\x7f]/.test(value.key) || encoder.encode(value.key).length > maxBytes) return false
  if (!isCount(value.clicks) || !isCount(value.impressions) || value.clicks > value.impressions) return false
  if (!finitePositive(value.ctr) || value.ctr > 1 || !finitePositive(value.position)) return false
  if (value.impressions > 0 && value.position < 1) return false
  return Math.abs(value.ctr - (value.impressions === 0 ? 0 : value.clicks / value.impressions)) <= 0.000001
}

function isRows(value: unknown, maxBytes: number): value is SearchRow[] {
  return Array.isArray(value) && value.length <= 100 && value.every((row) => isSearchRow(row, maxBytes)) &&
    new Set(value.map((row) => row.key)).size === value.length
}

function opportunityType(row: SearchRow): OpportunityType | null {
  if (row.impressions < 100 || row.ctr >= 0.05 || row.position <= 0 || row.position > 20) return null
  if (row.position <= 3) return row.ctr < 0.03 ? 'snippet_review' : null
  if (row.position <= 10) return row.ctr < 0.03 ? 'title_meta' : 'title_test'
  return 'content_expansion'
}

/** Validate the narrow, public read-only projection; provider payloads never belong here. */
export function isSearchSnapshot(value: unknown): value is SearchSnapshot {
  if (!isRecord(value) || typeof value.site !== 'string' || value.source !== 'google_search_console') return false
  try { if (normalizePulseSite(value.site) !== value.site) return false } catch { return false }
  if (value.status !== 'connected' && value.status !== 'unavailable') return false
  if (!isRecord(value.period) || typeof value.period.startDate !== 'string' || typeof value.period.endDate !== 'string') return false
  if (pulsePeriodDates({ startDate: value.period.startDate, endDate: value.period.endDate }).length === 0) return false
  if (!isRows(value.queries, 512) || !isRows(value.pages, 2048) || !isRows(value.opportunities, 512)) return false
  if (value.rowLimit !== 100 || typeof value.queriesAtLimit !== 'boolean' || typeof value.pagesAtLimit !== 'boolean') return false
  if (value.queriesAtLimit !== (value.queries.length === 100) || value.pagesAtLimit !== (value.pages.length === 100)) return false
  if (value.propertyScope !== undefined && value.propertyScope !== 'domain' && value.propertyScope !== 'url_prefix') return false
  if (value.reportingProperty !== undefined && (value.status !== 'connected' || !isGSCReportingProperty(value.site,value.reportingProperty))) return false
  if (value.reason !== undefined && (typeof value.reason !== 'string' || !['not_configured', 'property_unverified', 'source_unavailable', 'snapshot_pending', 'snapshot_expired', 'authorization_required'].includes(value.reason))) return false
  if (value.updatedAt !== undefined && (typeof value.updatedAt !== 'string' || !Number.isFinite(Date.parse(value.updatedAt)))) return false
  if (value.status === 'unavailable') return value.queries.length === 0 && value.pages.length === 0 && value.opportunities.length === 0
  if (value.reason !== undefined) return false
  const queries = value.queries
  return value.opportunities.every((opportunity) => {
    const query = queries.find((row) => row.key === opportunity.key)
    const candidate: unknown = opportunity
    return query !== undefined && isRecord(candidate) && candidate.type === opportunityType(query) && candidate.type !== null &&
      query.clicks === opportunity.clicks && query.impressions === opportunity.impressions && query.ctr === opportunity.ctr && query.position === opportunity.position
  })
}

const projectRow = (row: SearchRow): SearchRow => ({ key: row.key, clicks: row.clicks, impressions: row.impressions, ctr: row.ctr, position: row.position })

/** Reconstruct every nested object so unknown upstream fields cannot cross the BFF. */
export function sanitizeSearchSnapshot(value: unknown): SearchSnapshot {
  if (!isSearchSnapshot(value)) throw new Error('Invalid Search response')
  return {
    site: value.site, source: value.source, status: value.status,
    period: { startDate: value.period.startDate, endDate: value.period.endDate },
    queries: value.queries.map(projectRow), pages: value.pages.map(projectRow),
    opportunities: value.opportunities.map((row) => ({ ...projectRow(row), type: row.type })),
    rowLimit: 100, queriesAtLimit: value.queriesAtLimit, pagesAtLimit: value.pagesAtLimit,
    ...(value.propertyScope === undefined ? {} : { propertyScope: value.propertyScope }),
    ...(value.reportingProperty === undefined ? {} : { reportingProperty: value.reportingProperty }),
    ...(value.updatedAt === undefined ? {} : { updatedAt: value.updatedAt }),
    ...(value.reason === undefined ? {} : { reason: value.reason }),
  }
}

export async function loadSearch(quarter: PulseQuarter, site: string, signal?: AbortSignal, websiteId?: string): Promise<SearchSnapshot> {
  try { if (normalizePulseSite(site) !== site) throw new Error('Invalid Search site') } catch { throw new Error('Invalid Search site') }
  if (websiteId !== undefined && !isWebsiteId(websiteId)) throw new Error('Invalid website identity')
  const response = await fetch(`/api/v1/visibility/search?quarter=${quarter}&site=${encodeURIComponent(site)}${websiteId ? `&websiteId=${websiteId}` : ''}`, {
    credentials: 'include', headers: { Accept: 'application/json' }, signal,
  })
  if (!response.ok) throw new Error('Search data is unavailable')
  const payload: unknown = await response.json()
  const data = sanitizeSearchSnapshot(isRecord(payload) ? payload.data : undefined)
  if (data.site !== site) throw new Error('Invalid Search response')
  return data
}
