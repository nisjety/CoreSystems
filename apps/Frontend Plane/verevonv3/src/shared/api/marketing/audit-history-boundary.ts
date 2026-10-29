import { validateDiscoveryContext, type DiscoveryContext } from './discovery-client'
import { monitoringReasons } from './monitoring-client'
import { currentUTCWeek, isUTCWeekStart } from './report-week'

export const historyRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value)
export const historyCount = (value: unknown, max = Number.MAX_SAFE_INTEGER): value is number => typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 && value <= max
export function historyStamp(value: unknown): value is string {
  if (typeof value !== 'string' || value.length > 40 || !/^\d{4}-\d\d-\d\dT(?:[01]\d|2[0-3]):[0-5]\d:[0-5]\d(?:\.\d{1,9})?(?:Z|[+-](?:[01]\d|2[0-3]):[0-5]\d)$/.test(value) || !Number.isFinite(Date.parse(value))) return false
  const day = new Date(`${value.slice(0, 10)}T00:00:00Z`)
  return Number.isFinite(day.getTime()) && day.toISOString().slice(0, 10) === value.slice(0, 10)
}
export function historyNotAfter(value: string, maximum: string): boolean {
  const nanos = (stamp: string) => BigInt(Date.parse(stamp)) * 1_000_000n + BigInt((stamp.match(/\.(\d+)/)?.[1] ?? '').padEnd(9, '0').slice(3))
  return nanos(value) <= nanos(maximum)
}
export const changeKinds = ['new', 'unchanged', 'resolved', 'regressed', 'unknown'] as const
export type AuditChangeKind = typeof changeKinds[number]
export type AuditChanges = Record<AuditChangeKind, number>
export function changeCounts(value: unknown, max = Number.MAX_SAFE_INTEGER): AuditChanges {
  if (!historyRecord(value) || !changeKinds.every((key) => historyCount(value[key], max))) throw new Error('Invalid change counts')
  return { new: Number(value.new), unchanged: Number(value.unchanged), resolved: Number(value.resolved), regressed: Number(value.regressed), unknown: Number(value.unknown) }
}
export function historyReason(value: unknown): string {
  if (typeof value !== 'string' || value !== '' && !monitoringReasons.includes(value as typeof monitoringReasons[number])) throw new Error('Invalid audit history reason')
  return value
}
export type AuditHistoryBase = DiscoveryContext & { source: 'marketing_audit_history'; asOf: string; trackingSince: string; partial: true; historyLimited: boolean; truncated: boolean; availableWeeks: string[] }
export function auditHistoryBase(value: unknown, expected: DiscoveryContext): AuditHistoryBase {
  const context = validateDiscoveryContext(value), selected = validateDiscoveryContext(expected)
  if (!historyRecord(value) || context.websiteId !== selected.websiteId || context.site !== selected.site || context.websiteRevision !== selected.websiteRevision || value.source !== 'marketing_audit_history' || !historyStamp(value.asOf) || value.trackingSince !== '' && !historyStamp(value.trackingSince) || value.partial !== true || typeof value.historyLimited !== 'boolean' || typeof value.truncated !== 'boolean' || !Array.isArray(value.availableWeeks) || value.availableWeeks.length > 8) throw new Error('Invalid audit history response')
  const weeks = value.availableWeeks
  if (value.trackingSince !== '' && !historyNotAfter(value.trackingSince as string, value.asOf)) throw new Error('Invalid recording boundary')
  if (!weeks.every((week) => isUTCWeekStart(week) && week <= currentUTCWeek(new Date(value.asOf as string)))) throw new Error('Invalid report weeks')
  if (weeks.some((week, index) => index > 0 && week >= weeks[index - 1]!)) throw new Error('Unordered report weeks')
  return { ...context, source: 'marketing_audit_history', asOf: value.asOf, trackingSince: value.trackingSince as string, partial: true, historyLimited: value.historyLimited, truncated: value.truncated, availableWeeks: [...weeks] as string[] }
}
export async function readAuditHistory(path: string, signal?: AbortSignal): Promise<unknown> {
  const response = await fetch(path, { method: 'GET', credentials: 'include', redirect: 'error', signal, headers: { Accept: 'application/json' } })
  if (!response.ok) throw new Error(response.status === 409 ? 'history_context_changed' : 'Audit history is unavailable')
  const body: unknown = await response.json()
  return historyRecord(body) ? body.data : undefined
}
