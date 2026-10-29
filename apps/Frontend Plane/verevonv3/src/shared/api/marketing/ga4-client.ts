import type { PulseQuarter } from './pulse-client.ts'
import { pulsePeriodDates } from './pulse-timeline.ts'
import { analyticsRecord as record, analyticsCount as count, analyticsFinite as number, analyticsSite, analyticsUpdatedAt, validateAnalyticsContext } from './analytics-boundary.ts'
import { isGA4ReportingSite } from './website-client.ts'

export type GA4Metrics = { users: number; sessions: number; engagedSessions: number; pageViews: number; keyEvents: number; engagementSeconds: number }
export type GA4Quality = { thresholded: boolean; sampled: boolean; dataLoss: boolean }
export type GA4Reason = 'not_connected' | 'requires_consent' | 'needs_selection' | 'not_found' | 'source_unavailable' | 'not_configured' | 'snapshot_pending' | 'snapshot_expired' | 'authorization_required'
export type GA4Snapshot = {
  site: string; source: 'google_analytics'; status: 'connected' | 'unavailable'; reason?: GA4Reason
  period: { startDate: string; endDate: string }; timeZone?: string
  totals: GA4Metrics | null; days: (GA4Metrics & { date: string })[]; reportingSite?: string; updatedAt?: string; quality?: GA4Quality
}
const reasons: readonly GA4Reason[] = ['not_connected', 'requires_consent', 'needs_selection', 'not_found', 'source_unavailable', 'not_configured', 'snapshot_pending', 'snapshot_expired', 'authorization_required']

function metrics(value: unknown): GA4Metrics {
  if (!record(value) || !count(value.users) || !count(value.sessions) || !count(value.engagedSessions) || !count(value.pageViews) || !number(value.keyEvents) || !number(value.engagementSeconds)) throw new Error('Invalid Analytics metrics')
  return { users: value.users, sessions: value.sessions, engagedSessions: value.engagedSessions, pageViews: value.pageViews, keyEvents: value.keyEvents, engagementSeconds: value.engagementSeconds }
}
function quality(value: unknown): GA4Quality {
  if (!record(value) || typeof value.thresholded !== 'boolean' || typeof value.sampled !== 'boolean' || typeof value.dataLoss !== 'boolean') throw new Error('Invalid Analytics quality')
  return { thresholded: value.thresholded, sampled: value.sampled, dataLoss: value.dataLoss }
}
function timeZone(value: unknown): string {
  if (typeof value !== 'string' || !/^[A-Za-z0-9_+/-]{1,64}$/.test(value)) throw new Error('Invalid Analytics timezone')
  try { new Intl.DateTimeFormat('en', { timeZone: value }).format(0) } catch { throw new Error('Invalid Analytics timezone') }
  return value
}

/** The period-wide user total is provider-issued, never a sum of daily users. */
export function sanitizeGA4Snapshot(value: unknown): GA4Snapshot {
  if (!record(value) || value.source !== 'google_analytics' || !['connected', 'unavailable'].includes(String(value.status)) || !record(value.period) || typeof value.period.startDate !== 'string' || typeof value.period.endDate !== 'string') throw new Error('Invalid Analytics response')
  const site = analyticsSite(value.site)
  const period = { startDate: value.period.startDate, endDate: value.period.endDate }
  const dates = pulsePeriodDates(period)
  if (dates.length === 0 || dates.length > 366 || !Array.isArray(value.days) || value.days.length > dates.length) throw new Error('Invalid Analytics period')
  const dateSet = new Set(dates)
  const days = value.days.map((day: unknown) => {
    if (!record(day) || typeof day.date !== 'string' || !dateSet.has(day.date)) throw new Error('Invalid Analytics day')
    return { date: day.date, ...metrics(day) }
  })
  if (new Set(days.map((day) => day.date)).size !== days.length || days.some((day, index) => index > 0 && day.date <= days[index - 1]!.date)) throw new Error('Invalid Analytics calendar')
  if (value.status === 'unavailable' ? !reasons.includes(value.reason as GA4Reason) || value.totals !== null || days.length !== 0 : value.reason !== undefined || value.totals === null && days.length !== 0) throw new Error('Invalid Analytics state')
  if (value.reportingSite !== undefined && (value.status !== 'connected' || typeof value.reportingSite !== 'string' || !isGA4ReportingSite(site, value.reportingSite))) throw new Error('Invalid Analytics reporting scope')
  return {
    site, source: 'google_analytics', status: value.status as GA4Snapshot['status'], period,
    totals: value.totals === null ? null : metrics(value.totals), days,
    ...(value.timeZone === undefined ? {} : { timeZone: timeZone(value.timeZone) }),
    ...(value.reason === undefined ? {} : { reason: value.reason as GA4Reason }),
    ...(value.quality === undefined ? {} : { quality: quality(value.quality) }),
    ...(value.reportingSite === undefined ? {} : { reportingSite: value.reportingSite }),
    ...analyticsUpdatedAt(value),
  }
}
export async function loadGA4(quarter: PulseQuarter, site: string, websiteId: string, signal?: AbortSignal): Promise<GA4Snapshot> {
  validateAnalyticsContext(site, websiteId)
  if (!/^Q[1-4]$/.test(quarter) && quarter !== 'FY') throw new Error('Invalid Analytics quarter')
  const response = await fetch(`/api/v1/visibility/ga4?quarter=${quarter}&site=${encodeURIComponent(site)}&websiteId=${websiteId}`, { credentials: 'include', signal, headers: { Accept: 'application/json' } })
  if (!response.ok) throw new Error('Analytics is unavailable')
  const payload: unknown = await response.json()
  const data = sanitizeGA4Snapshot(record(payload) ? payload.data : undefined)
  if (data.site !== site) throw new Error('Unexpected Analytics website')
  return data
}
