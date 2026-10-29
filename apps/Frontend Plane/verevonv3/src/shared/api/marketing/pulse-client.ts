import { normalizePulseSite } from './pulse-site-stack.ts'
import { pulsePeriodDates } from './pulse-timeline.ts'
import { isWebsiteId, isGSCReportingProperty } from './website-client.ts'

export type PulseQuarter = 'Q1' | 'Q2' | 'Q3' | 'Q4' | 'FY'
export type PulseChartMetric = 'clicks' | 'impressions'

export type PulseDay = {
  date: string
  clicks: number
  impressions: number
}

export type PulseSnapshot = {
  site: string
  source: 'google_search_console'
  status: 'connected' | 'unavailable'
  propertyScope?: 'domain' | 'url_prefix'
  reportingProperty?: string
  period: { startDate: string; endDate: string }
  totals: { clicks: number; impressions: number } | null
  days: PulseDay[]
  updatedAt?: string
  reason?: 'not_configured' | 'property_unverified' | 'source_unavailable' | 'snapshot_pending' | 'snapshot_expired' | 'authorization_required'
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

const isCount = (value: unknown): value is number =>
  typeof value === 'number' && Number.isFinite(value) && value >= 0

const isDate = (value: unknown): value is string =>
  typeof value === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(value)

export function isPulseSnapshot(value: unknown): value is PulseSnapshot {
  if (!isRecord(value) || typeof value.site !== 'string' || value.source !== 'google_search_console') return false
  try { if (normalizePulseSite(value.site) !== value.site) return false } catch { return false }
  if (value.status !== 'connected' && value.status !== 'unavailable') return false
  if (value.reason !== undefined && !['not_configured', 'property_unverified', 'source_unavailable', 'snapshot_pending', 'snapshot_expired', 'authorization_required'].includes(String(value.reason))) return false
  if (value.status === 'connected' && value.reason !== undefined) return false
  if (value.propertyScope !== undefined && value.propertyScope !== 'domain' && value.propertyScope !== 'url_prefix') return false
  if (value.reportingProperty !== undefined && (value.status !== 'connected' || !isGSCReportingProperty(value.site,value.reportingProperty))) return false
  if (!isRecord(value.period) || !isDate(value.period.startDate) || !isDate(value.period.endDate)) return false
  if (!Array.isArray(value.days) || value.days.length > 366) return false
  if (!value.days.every((day) => isRecord(day) && isDate(day.date) && isCount(day.clicks) && isCount(day.impressions))) return false
  if (value.status === 'unavailable') return value.totals === null && value.days.length === 0
  return isRecord(value.totals) && isCount(value.totals.clicks) && isCount(value.totals.impressions)
}

export async function loadPulse(quarter: PulseQuarter, site: string, signal?: AbortSignal, websiteId?: string): Promise<PulseSnapshot> {
  const canonicalSite = normalizePulseSite(site)
  if (canonicalSite !== site) throw new Error('Invalid Visibility site')
  if (websiteId !== undefined && !isWebsiteId(websiteId)) throw new Error('Invalid website identity')
  const response = await fetch(`/api/v1/visibility/pulse?quarter=${quarter}&site=${encodeURIComponent(site)}${websiteId ? `&websiteId=${websiteId}` : ''}`, {
    credentials: 'include',
    headers: { Accept: 'application/json' },
    signal,
  })
  if (!response.ok) throw new Error('Visibility data is unavailable')
  const payload: unknown = await response.json()
  const data = isRecord(payload) ? payload.data : undefined
  if (!isPulseSnapshot(data) || data.site !== site) throw new Error('Invalid Visibility response')
  return data
}

export function scalePulseBars(days: readonly PulseDay[], metric: PulseChartMetric): number[] {
  const max = Math.max(0, ...days.map((day) => day[metric]))
  if (max === 0) return days.map(() => 0)
  return days.map((day) => Math.round((day[metric] / max) * 100))
}

export function buildPulseChart(
  period: PulseSnapshot['period'],
  days: readonly PulseDay[],
  metric: PulseChartMetric,
): { date: string; height: number; hasValue: boolean }[] {
  const dates = pulsePeriodDates(period)
  const dateSet = new Set(dates)
  const visibleDays = days.filter((day) => dateSet.has(day.date))
  const heights = scalePulseBars(visibleDays, metric)
  const byDate = new Map(visibleDays.map((day, index) => [day.date, heights[index] ?? 0]))
  return dates.map((date) => ({ date, height: byDate.get(date) ?? 0, hasValue: byDate.has(date) }))
}
