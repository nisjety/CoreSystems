import { marketingOutcomesContract } from '@/shared/actions/marketing-outcomes'
import { validateDiscoveryContext, type DiscoveryContext } from './discovery-client'
import type { PulseQuarter } from './pulse-client'
export type OutcomeContext = DiscoveryContext & { quarter: PulseQuarter; environment: 'development' | 'production' }
export type OutcomeSnapshot = OutcomeContext & { status: 'connected' | 'unavailable'; reason?: 'not_connected' | 'source_unavailable'; metrics: { inquiries: number; leads: number; bookings: null } | null; period: { startDate: string; endDate: string }; timeZone: 'Europe/Oslo'; asOf?: string; lastReceivedAt: string | null }
const record = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)
const date = (v: unknown): v is string => typeof v === 'string' && /^\d{4}-\d\d-\d\d$/.test(v) && new Date(`${v}T00:00:00Z`).toISOString().slice(0, 10) === v
const timestamp = (v: unknown): v is string => typeof v === 'string' && v.length < 40 && /^\d{4}-\d\d-\d\dT/.test(v) && Number.isFinite(Date.parse(v))
function validPeriodForQuarter(start: string, end: string, quarter: PulseQuarter, asOf?: string): boolean {
  const year = Number(start.slice(0, 4)), month = quarter === 'FY' ? 1 : (Number(quarter[1]) - 1) * 3 + 1
  if (start !== `${year}-${String(month).padStart(2, '0')}-01` || end.slice(0, 4) !== start.slice(0, 4)) return false
  const maxEnd = new Date(Date.UTC(year, quarter === 'FY' ? 12 : month + 2, 0)).toISOString().slice(0, 10)
  if (end > maxEnd) return false
  if (asOf) {
    const today = new Intl.DateTimeFormat('sv-SE', { year: 'numeric', month: '2-digit', day: '2-digit', timeZone: 'Europe/Oslo' }).format(new Date(asOf))
    if (today.slice(0, 4) !== start.slice(0, 4) || end !== (today < maxEnd ? today : maxEnd)) return false
  }
  return true
}
export function sanitizeOutcomes(raw: unknown, expected: OutcomeContext): OutcomeSnapshot {
  const context = validateDiscoveryContext(raw), selected = validateDiscoveryContext(expected)
  if (!record(raw) || context.websiteId !== selected.websiteId || context.site !== selected.site || context.websiteRevision !== selected.websiteRevision || raw.source !== 'leads_core' || raw.quarter !== expected.quarter || raw.environment !== expected.environment || raw.timeZone !== 'Europe/Oslo' || !record(raw.period) || !date(raw.period.startDate) || !date(raw.period.endDate) || raw.period.endDate < raw.period.startDate) throw new Error('Invalid outcomes scope')
  if (raw.status !== 'connected' && raw.status !== 'unavailable') throw new Error('Invalid outcomes status')
  if (!validPeriodForQuarter(raw.period.startDate, raw.period.endDate, expected.quarter)) throw new Error('Invalid outcomes period')
  let metrics: OutcomeSnapshot['metrics'] = null
  if (raw.status === 'connected') {
    if (raw.reason !== undefined || !record(raw.metrics) || !Number.isSafeInteger(raw.metrics.inquiries) || (raw.metrics.inquiries as number) < 0 || raw.metrics.leads !== raw.metrics.inquiries || raw.metrics.bookings !== null || !timestamp(raw.asOf) || raw.lastReceivedAt !== null && (!timestamp(raw.lastReceivedAt) || Date.parse(raw.lastReceivedAt) > Date.parse(raw.asOf))) throw new Error('Invalid outcomes counts')
    if (!validPeriodForQuarter(raw.period.startDate, raw.period.endDate, expected.quarter, raw.asOf as string)) throw new Error('Invalid outcomes period')
    metrics = { inquiries: raw.metrics.inquiries as number, leads: raw.metrics.leads as number, bookings: null }
  } else if (raw.metrics !== null || raw.lastReceivedAt !== null || !['not_connected', 'source_unavailable'].includes(String(raw.reason))) throw new Error('Invalid unavailable outcomes')
  return { ...selected, quarter: expected.quarter, environment: expected.environment, status: raw.status, ...(raw.reason === undefined ? {} : { reason: raw.reason as OutcomeSnapshot['reason'] }), metrics, period: { startDate: raw.period.startDate, endDate: raw.period.endDate }, timeZone: 'Europe/Oslo', lastReceivedAt: raw.lastReceivedAt as string | null, ...(raw.asOf === undefined ? {} : { asOf: raw.asOf as string }) }
}
export async function loadOutcomes(context: OutcomeContext, signal?: AbortSignal) {
  const selected = validateDiscoveryContext(context)
  if (!['Q1', 'Q2', 'Q3', 'Q4', 'FY'].includes(context.quarter) || !['development', 'production'].includes(context.environment)) throw new Error('Invalid outcomes context')
  const query = new URLSearchParams({ websiteId: selected.websiteId, site: selected.site, quarter: context.quarter, environment: context.environment })
  const response = await fetch(`${marketingOutcomesContract.path}?${query}`, { method: 'GET', credentials: 'include', redirect: 'error', signal, headers: { Accept: 'application/json' } })
  if (!response.ok) throw new Error('outcomes_unavailable')
  const payload: unknown = await response.json()
  return sanitizeOutcomes(record(payload) ? payload.data : undefined, context)
}
