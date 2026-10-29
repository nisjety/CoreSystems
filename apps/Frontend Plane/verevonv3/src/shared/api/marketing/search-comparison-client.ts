import { normalizePulseSite } from './pulse-site-stack.ts'
import { isWebsiteId, isGSCReportingProperty } from './website-client.ts'
import { isSearchRow, type SearchRow } from './search-client.ts'
import { marketingSearchComparisonContract } from '../../actions/marketing-search.ts'

type Period = { startDate: string; endDate: string }
export type ComparisonWindows = { currentStart: string; currentEnd: string; previousStart: string; previousEnd: string }
export type ComparisonInput = ComparisonWindows & { site: string; websiteId: string; dimension: 'query' | 'page' }
export type ComparisonRow = {
  key: string; state: 'compared' | 'not_comparable'; current?: SearchRow; previous?: SearchRow
  clicksChange?: number; impressionsChange?: number; positionChange?: number; clicksChangePercent?: number
}
export type SearchComparison = {
  site: string; source: 'google_search_console'; status: 'connected' | 'unavailable'; dimension: 'query' | 'page'
  currentPeriod: Period; previousPeriod: Period; rows: ComparisonRow[]; rowLimit: number; rowsAtLimit: boolean
  comparableRows: number; unmatchedRows: number; propertyScope?: 'domain' | 'url_prefix'; reportingProperty?: string; updatedAt?: string
  reason?: 'not_configured' | 'property_unverified' | 'source_unavailable'
}
export const COMPARISON_PATH = marketingSearchComparisonContract.path
export const COMPARISON_ROW_LIMIT = 100
const DAY = 86_400_000
const record = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value)
const count = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) >= 0
const near = (value: unknown, expected: number) => typeof value === 'number' && Number.isFinite(value) && Math.abs(value - expected) <= 0.000001
const iso = (date: Date) => date.toISOString().slice(0, 10)

function dateValue(value: unknown): number {
  if (typeof value !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(value)) return NaN
  const time = Date.parse(`${value}T00:00:00Z`)
  return Number.isFinite(time) && iso(new Date(time)) === value ? time : NaN
}

function validPeriods(current: unknown, previous: unknown): current is Period {
  if (!record(current) || !record(previous)) return false
  const start = dateValue(current.startDate), end = dateValue(current.endDate)
  const priorStart = dateValue(previous.startDate), priorEnd = dateValue(previous.endDate)
  const days = (end - start) / DAY + 1
  return Number.isInteger(days) && days >= 1 && days <= 90 && priorEnd + DAY === start && end - start === priorEnd - priorStart
}

/** Calendar windows are independent of Visibility's quarter; allow three days for finalized GSC reporting. */
export function comparisonWindows(days: 28 | 90, now = new Date()): ComparisonWindows {
  if (![28, 90].includes(days) || !Number.isFinite(now.getTime())) throw new Error('Invalid comparison window')
  const end = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate()) - 3 * DAY
  const start = end - (days - 1) * DAY
  return { currentStart: iso(new Date(start)), currentEnd: iso(new Date(end)), previousStart: iso(new Date(start - days * DAY)), previousEnd: iso(new Date(start - DAY)) }
}

export function comparisonQuery(input: ComparisonInput): string {
  if (normalizePulseSite(input.site) !== input.site || !isWebsiteId(input.websiteId) || !['query', 'page'].includes(input.dimension) ||
    !validPeriods({ startDate: input.currentStart, endDate: input.currentEnd }, { startDate: input.previousStart, endDate: input.previousEnd })) throw new Error('Invalid comparison request')
  return new URLSearchParams({ site: input.site, websiteId: input.websiteId, dimension: input.dimension, currentStart: input.currentStart,
    currentEnd: input.currentEnd, previousStart: input.previousStart, previousEnd: input.previousEnd, rowLimit: String(COMPARISON_ROW_LIMIT) }).toString()
}

function validRow(value: unknown, dimension: string): value is ComparisonRow {
  if (!record(value) || typeof value.key !== 'string') return false
  const bytes = dimension === 'page' ? 2048 : 512
  const current = value.current, previous = value.previous
  if (current !== undefined && (!isSearchRow(current, bytes) || current.key !== value.key)) return false
  if (previous !== undefined && (!isSearchRow(previous, bytes) || previous.key !== value.key)) return false
  if (value.state === 'not_comparable') return Boolean(current) !== Boolean(previous) &&
    [value.clicksChange, value.impressionsChange, value.positionChange, value.clicksChangePercent].every((v) => v === undefined)
  if (value.state !== 'compared' || !isSearchRow(current, bytes) || !isSearchRow(previous, bytes)) return false
  return value.clicksChange === current.clicks - previous.clicks && value.impressionsChange === current.impressions - previous.impressions &&
    near(value.positionChange, current.position - previous.position) && (previous.clicks === 0 ? value.clicksChangePercent === undefined : near(value.clicksChangePercent, (current.clicks - previous.clicks) / previous.clicks * 100))
}

function validSnapshot(value: unknown): value is SearchComparison {
  if (!record(value) || value.source !== 'google_search_console' || typeof value.site !== 'string') return false
  try { if (normalizePulseSite(value.site) !== value.site) return false } catch { return false }
  if (!['query', 'page'].includes(String(value.dimension)) || !['connected', 'unavailable'].includes(String(value.status))) return false
  if (!validPeriods(value.currentPeriod, value.previousPeriod) || !count(value.rowLimit) || value.rowLimit < 1 || value.rowLimit > 1000 || typeof value.rowsAtLimit !== 'boolean') return false
  if (!Array.isArray(value.rows) || value.rows.length > value.rowLimit * 2 || !value.rows.every((row) => validRow(row, String(value.dimension)))) return false
  const rows = value.rows as ComparisonRow[]
  if (new Set(rows.map((row) => row.key)).size !== rows.length || rows.filter((row) => row.current).length > value.rowLimit || rows.filter((row) => row.previous).length > value.rowLimit) return false
  if (value.comparableRows !== rows.filter((row) => row.state === 'compared').length || value.unmatchedRows !== rows.filter((row) => row.state === 'not_comparable').length) return false
  if (value.propertyScope !== undefined && !['domain', 'url_prefix'].includes(String(value.propertyScope))) return false
  if (value.reportingProperty !== undefined && (value.status !== 'connected' || !isGSCReportingProperty(value.site,value.reportingProperty))) return false
  if (value.updatedAt !== undefined && (typeof value.updatedAt !== 'string' || !Number.isFinite(Date.parse(value.updatedAt)))) return false
  if (value.status === 'unavailable') return rows.length === 0 && value.rowsAtLimit === false && ['not_configured', 'property_unverified', 'source_unavailable'].includes(String(value.reason))
  return value.reason === undefined && value.rowsAtLimit === (rows.filter((row) => row.current).length === value.rowLimit || rows.filter((row) => row.previous).length === value.rowLimit)
}

const projectMetrics = (value: SearchRow): SearchRow => ({ key: value.key, clicks: value.clicks, impressions: value.impressions, ctr: value.ctr, position: value.position })

/** Reconstruct public fields: upstream credentials and unknown nested metadata never reach the browser. */
export function sanitizeSearchComparison(value: unknown): SearchComparison {
  if (!validSnapshot(value)) throw new Error('Invalid comparison response')
  return {
    site: value.site, source: value.source, status: value.status, dimension: value.dimension,
    currentPeriod: { startDate: value.currentPeriod.startDate, endDate: value.currentPeriod.endDate },
    previousPeriod: { startDate: value.previousPeriod.startDate, endDate: value.previousPeriod.endDate },
    rowLimit: value.rowLimit, rowsAtLimit: value.rowsAtLimit, comparableRows: value.comparableRows, unmatchedRows: value.unmatchedRows,
    rows: value.rows.map((row) => ({ key: row.key, state: row.state,
      ...(row.current === undefined ? {} : { current: projectMetrics(row.current) }), ...(row.previous === undefined ? {} : { previous: projectMetrics(row.previous) }),
      ...(row.clicksChange === undefined ? {} : { clicksChange: row.clicksChange }), ...(row.impressionsChange === undefined ? {} : { impressionsChange: row.impressionsChange }),
      ...(row.positionChange === undefined ? {} : { positionChange: row.positionChange }), ...(row.clicksChangePercent === undefined ? {} : { clicksChangePercent: row.clicksChangePercent }),
    })),
    ...(value.reason === undefined ? {} : { reason: value.reason }), ...(value.propertyScope === undefined ? {} : { propertyScope: value.propertyScope }),
    ...(value.reportingProperty === undefined ? {} : { reportingProperty: value.reportingProperty }),
    ...(value.updatedAt === undefined ? {} : { updatedAt: value.updatedAt }),
  }
}

export function assertComparisonContext(data: SearchComparison, input: ComparisonInput): void {
  if (data.site !== input.site || data.dimension !== input.dimension || data.rowLimit !== COMPARISON_ROW_LIMIT ||
    data.currentPeriod.startDate !== input.currentStart || data.currentPeriod.endDate !== input.currentEnd ||
    data.previousPeriod.startDate !== input.previousStart || data.previousPeriod.endDate !== input.previousEnd) throw new Error('Unexpected comparison context')
}

export async function loadSearchComparison(input: ComparisonInput, signal?: AbortSignal): Promise<SearchComparison> {
  const response = await fetch(`${COMPARISON_PATH}?${comparisonQuery(input)}`, { credentials: 'include', headers: { Accept: 'application/json' }, signal })
  if (!response.ok) throw new Error('Comparison is unavailable')
  const payload: unknown = await response.json()
  const data = sanitizeSearchComparison(record(payload) ? payload.data : undefined)
  assertComparisonContext(data, input)
  return data
}
