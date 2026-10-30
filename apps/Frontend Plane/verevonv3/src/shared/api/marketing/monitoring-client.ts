import { marketingMonitoringContracts } from '../../actions/marketing-monitoring'
import { isAuditId } from './audit-client'
import { validateDiscoveryContext, type DiscoveryContext } from './discovery-client'

export type MonitoringCadence = 'daily' | 'weekly'
export type MonitoringStatus = 'not_configured' | 'active' | 'paused' | 'context_changed' | 'blocked'
export const monitoringReasons = ['not_configured', 'context_changed', 'capacity_deferred', 'interrupted', 'source_unavailable', 'quota_limited', 'timeout', 'access_denied', 'policy_blocked', 'invalid_source', 'unsupported_content', 'storage_unavailable'] as const
export type MonitoringReason = typeof monitoringReasons[number]
export type MonitoringReceipt = { auditId: string; scheduledFor: string; startedAt: string; completedAt: string; status: 'running' | 'complete' | 'failed'; attempt: number; checkedPages: number; findingCount: number; reason?: MonitoringReason }
export type AuditMonitoring = DiscoveryContext & { source: 'marketing_scheduler'; revision: number; configured: boolean; enabled: boolean; cadence: MonitoringCadence; status: MonitoringStatus; nextRunAt: string; updatedAt: string; lastAuditId: string; lastOutcome: 'none' | MonitoringReceipt['status']; failureCount: number; reason?: MonitoringReason; history: MonitoringReceipt[] }
export type ConfigureMonitoringInput = DiscoveryContext & { scheduleRevision: number; enabled: boolean; cadence: MonitoringCadence }

const record = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)
const integer = (v: unknown, max = Number.MAX_SAFE_INTEGER): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 && v <= max
const cadence = (v: unknown): v is MonitoringCadence => v === 'daily' || v === 'weekly'
const outcome = (v: unknown): v is MonitoringReceipt['status'] => v === 'running' || v === 'complete' || v === 'failed'
function timestamp(v: unknown): v is string {
  if (typeof v !== 'string' || v.length > 40 || !/^\d{4}-\d\d-\d\dT(?:[01]\d|2[0-3]):[0-5]\d:[0-5]\d(?:\.\d{1,9})?(?:Z|[+-](?:[01]\d|2[0-3]):[0-5]\d)$/.test(v) || !Number.isFinite(Date.parse(v))) return false
  const day = new Date(`${v.slice(0, 10)}T00:00:00Z`)
  return Number.isFinite(day.getTime()) && day.toISOString().slice(0, 10) === v.slice(0, 10)
}
function reason(v: unknown): { reason?: MonitoringReason } {
  if (v === undefined) return {}
  if (typeof v !== 'string' || !monitoringReasons.includes(v as MonitoringReason)) throw new Error('Invalid monitoring reason')
  return { reason: v as MonitoringReason }
}
function receipt(v: unknown): MonitoringReceipt {
  if (!record(v) || !isAuditId(v.auditId) || !timestamp(v.scheduledFor) || !timestamp(v.startedAt) || !outcome(v.status) || !integer(v.attempt, 3) || v.attempt < 1 || !integer(v.checkedPages, 100) || !integer(v.findingCount, 1400)) throw new Error('Invalid monitoring receipt')
  if (Date.parse(v.startedAt) < Date.parse(v.scheduledFor) || (v.status === 'running' ? v.completedAt !== '' || v.checkedPages !== 0 || v.findingCount !== 0 : !timestamp(v.completedAt) || Date.parse(v.completedAt) < Date.parse(v.startedAt))) throw new Error('Invalid monitoring receipt lifecycle')
  const diagnostic = reason(v.reason)
  if (v.status === 'failed' ? !diagnostic.reason : diagnostic.reason !== undefined) throw new Error('Invalid monitoring receipt reason')
  return { auditId: v.auditId, scheduledFor: v.scheduledFor, startedAt: v.startedAt, completedAt: v.completedAt as string, status: v.status, attempt: v.attempt, checkedPages: v.checkedPages, findingCount: v.findingCount, ...diagnostic }
}

/** Rebuild the public schedule; deployment authority and raw audit/provider data stay server-side. */
export function sanitizeAuditMonitoring(value: unknown, expected: DiscoveryContext): AuditMonitoring {
  const context = validateDiscoveryContext(value), selected = validateDiscoveryContext(expected)
  if (!record(value) || context.websiteId !== selected.websiteId || context.site !== selected.site || context.websiteRevision !== selected.websiteRevision || value.source !== 'marketing_scheduler' || !integer(value.revision) || typeof value.configured !== 'boolean' || typeof value.enabled !== 'boolean' || !cadence(value.cadence) || typeof value.status !== 'string' || !['not_configured', 'active', 'paused', 'context_changed', 'blocked'].includes(value.status) || !integer(value.failureCount, 3) || !Array.isArray(value.history) || value.history.length > 10) throw new Error('Invalid monitoring response')
  const history = value.history.map(receipt), diagnostic = reason(value.reason)
  if (!value.configured) {
    if (value.revision !== 0 || value.enabled || value.cadence !== 'daily' || value.status !== 'not_configured' || value.nextRunAt !== '' || value.updatedAt !== '' || value.lastAuditId !== '' || value.lastOutcome !== 'none' || value.failureCount !== 0 || history.length || diagnostic.reason !== undefined && diagnostic.reason !== 'not_configured') throw new Error('Invalid unconfigured monitoring state')
  } else {
    if (value.revision < 1 || value.status === 'not_configured' || !timestamp(value.updatedAt) || (value.enabled ? value.status !== 'active' || !timestamp(value.nextRunAt) : value.status === 'active' || value.nextRunAt !== '')) throw new Error('Invalid monitoring lifecycle')
    if (value.lastOutcome === 'none' ? value.lastAuditId !== '' || history.length !== 0 : !outcome(value.lastOutcome) || !isAuditId(value.lastAuditId) || history[0]?.auditId !== value.lastAuditId || history[0]?.status !== value.lastOutcome) throw new Error('Invalid latest monitoring result')
  }
  if (new Set(history.map((item) => item.auditId)).size !== history.length || history.slice(1).some((item) => item.status === 'running')) throw new Error('Invalid monitoring history order')
  return { ...context, source: 'marketing_scheduler', revision: value.revision, configured: value.configured, enabled: value.enabled, cadence: value.cadence, status: value.status as MonitoringStatus, nextRunAt: value.nextRunAt as string, updatedAt: value.updatedAt as string, lastAuditId: value.lastAuditId as string, lastOutcome: value.lastOutcome as AuditMonitoring['lastOutcome'], failureCount: value.failureCount, ...diagnostic, history }
}

async function responseValue(response: Response, context: DiscoveryContext): Promise<AuditMonitoring> {
  if (!response.ok) throw new Error(response.status === 409 ? 'monitoring_conflict' : 'Monitoring is unavailable')
  const body: unknown = await response.json()
  return sanitizeAuditMonitoring(record(body) ? body.data : undefined, context)
}
export async function loadAuditMonitoring(input: DiscoveryContext, signal?: AbortSignal): Promise<AuditMonitoring> {
  const context = validateDiscoveryContext(input), query = new URLSearchParams({ websiteId: context.websiteId, site: context.site })
  return responseValue(await fetch(`${marketingMonitoringContracts.read.path}?${query}`, { method: 'GET', credentials: 'include', redirect: 'error', signal, headers: { Accept: 'application/json' } }), context)
}
export async function configureAuditMonitoring(input: ConfigureMonitoringInput, signal?: AbortSignal): Promise<AuditMonitoring> {
  const context = validateDiscoveryContext(input)
  if (!integer(input.scheduleRevision) || typeof input.enabled !== 'boolean' || !cadence(input.cadence)) throw new Error('Invalid monitoring configuration')
  const body = { ...context, scheduleRevision: input.scheduleRevision, enabled: input.enabled, cadence: input.cadence }
  return responseValue(await fetch(marketingMonitoringContracts.configure.path, { method: 'PATCH', credentials: 'include', redirect: 'error', signal, headers: { Accept: 'application/json', 'Content-Type': 'application/json' }, body: JSON.stringify(body) }), context)
}
