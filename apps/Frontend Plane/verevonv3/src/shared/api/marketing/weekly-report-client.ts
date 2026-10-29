import { marketingInsightsContracts } from '../../actions/marketing-insights'
import { isAuditId } from './audit-client'
import { validateDiscoveryContext, type DiscoveryContext } from './discovery-client'
import { auditHistoryBase, changeCounts, historyCount, historyRecord, historyReason, historyStamp, readAuditHistory, type AuditChanges, type AuditHistoryBase } from './audit-history-boundary'
import { currentUTCWeek, isUTCWeekStart, shiftUTCWeek } from './report-week'

export type WeeklyAuditReceipt = { auditId: string; status: 'complete' | 'failed'; startedAt: string; completedAt: string; checkedPages: number; failedPages: number; findingCount: number; reason?: string }
export type ReportWorkSummary = { open: number; inProgress: number; done: number; dismissed: number; total: number; completionPercent: number | null }
export type WeeklyAuditReport = AuditHistoryBase & { weekStart: string; endExclusive: string; status: 'open' | 'closed'; observed: boolean; audits: { total: number; complete: number; failed: number }; pages: { attempted: number; checked: number; failed: number }; failedChecks: number; changes: AuditChanges; receipts: WeeklyAuditReceipt[]; tasksAsOf: string; work: ReportWorkSummary | null }
function work(value: unknown, tasksAsOf: unknown): ReportWorkSummary | null {
  if (value === null) { if (tasksAsOf !== '') throw new Error('Invalid current work timestamp'); return null }
  if (!historyRecord(value) || !historyStamp(tasksAsOf) || !['open', 'inProgress', 'done', 'dismissed', 'total'].every((key) => historyCount(value[key]))) throw new Error('Invalid current work summary')
  const v = value as unknown as ReportWorkSummary, completion = v.total === v.dismissed ? null : Math.round(v.done / (v.total - v.dismissed) * 1000) / 10
  if (v.open + v.done + v.dismissed !== v.total || v.inProgress > v.open || v.completionPercent !== completion) throw new Error('Inconsistent current work summary')
  return { open: v.open, inProgress: v.inProgress, done: v.done, dismissed: v.dismissed, total: v.total, completionPercent: v.completionPercent }
}
function receipt(value: unknown): WeeklyAuditReceipt {
  if (!historyRecord(value) || !isAuditId(value.auditId) || value.status !== 'complete' && value.status !== 'failed' || !historyStamp(value.startedAt) || !historyStamp(value.completedAt) || Date.parse(value.completedAt) < Date.parse(value.startedAt) || !historyCount(value.checkedPages, 100) || !historyCount(value.failedPages, 100) || value.checkedPages + value.failedPages > 100 || !historyCount(value.findingCount, 1400)) throw new Error('Invalid weekly audit receipt')
  const reason = value.reason === undefined ? undefined : historyReason(value.reason)
  if (value.status === 'failed' ? !reason : reason !== undefined) throw new Error('Invalid weekly audit outcome')
  return { auditId: value.auditId, status: value.status, startedAt: value.startedAt, completedAt: value.completedAt, checkedPages: value.checkedPages, failedPages: value.failedPages, findingCount: value.findingCount, ...(reason === undefined ? {} : { reason }) }
}
export function sanitizeWeeklyReport(value: unknown, expected: DiscoveryContext, expectedWeek?: string): WeeklyAuditReport {
  const base = auditHistoryBase(value, expected)
  if (!historyRecord(value) || !isUTCWeekStart(value.weekStart) || value.endExclusive !== shiftUTCWeek(value.weekStart, 1) || value.weekStart > currentUTCWeek(new Date(base.asOf)) || value.status !== (value.weekStart === currentUTCWeek(new Date(base.asOf)) ? 'open' : 'closed') || expectedWeek !== undefined && value.weekStart !== expectedWeek || typeof value.observed !== 'boolean' || !historyRecord(value.audits) || !historyRecord(value.pages) || !['total', 'complete', 'failed'].every((key) => historyCount((value.audits as Record<string, unknown>)[key])) || !['attempted', 'checked', 'failed'].every((key) => historyCount((value.pages as Record<string, unknown>)[key])) || !historyCount(value.failedChecks) || !Array.isArray(value.receipts) || value.receipts.length > 20) throw new Error('Invalid weekly audit report')
  const audits = { total: Number(value.audits.total), complete: Number(value.audits.complete), failed: Number(value.audits.failed) }, pages = { attempted: Number(value.pages.attempted), checked: Number(value.pages.checked), failed: Number(value.pages.failed) }, receipts = value.receipts.map(receipt), changes = changeCounts(value.changes)
  if (audits.total !== audits.complete + audits.failed || pages.attempted !== pages.checked + pages.failed || value.observed !== (audits.total > 0) || new Set(receipts.map((v) => v.auditId)).size !== receipts.length) throw new Error('Inconsistent weekly totals')
  if (receipts.some((v) => Date.parse(v.completedAt) < Date.parse(`${value.weekStart}T00:00:00Z`) || Date.parse(v.completedAt) >= Date.parse(`${value.endExclusive}T00:00:00Z`)) || !value.observed && (pages.attempted > 0 || value.failedChecks > 0 || Object.values(changes).some((count) => count > 0))) throw new Error('Invalid weekly observation scope')
  const proofCounts = { total: receipts.length, complete: receipts.filter((v) => v.status === 'complete').length, failed: receipts.filter((v) => v.status === 'failed').length, checked: receipts.reduce((sum, v) => sum + v.checkedPages, 0), failedPages: receipts.reduce((sum, v) => sum + v.failedPages, 0) }
  if (base.truncated ? proofCounts.total > audits.total || proofCounts.complete > audits.complete || proofCounts.failed > audits.failed || proofCounts.checked > pages.checked || proofCounts.failedPages > pages.failed : proofCounts.total !== audits.total || proofCounts.complete !== audits.complete || proofCounts.failed !== audits.failed || proofCounts.checked !== pages.checked || proofCounts.failedPages !== pages.failed) throw new Error('Weekly receipt coverage mismatch')
  return { ...base, weekStart: value.weekStart, endExclusive: value.endExclusive as string, status: value.status as WeeklyAuditReport['status'], observed: value.observed, audits, pages, failedChecks: value.failedChecks, changes, receipts, tasksAsOf: value.tasksAsOf as string, work: work(value.work, value.tasksAsOf) }
}
export async function loadWeeklyReport(input: DiscoveryContext, weekStart?: string, signal?: AbortSignal): Promise<WeeklyAuditReport> {
  const context = validateDiscoveryContext(input)
  if (weekStart !== undefined && !isUTCWeekStart(weekStart)) throw new Error('Choose a UTC Monday')
  const query = new URLSearchParams({ websiteId: context.websiteId, site: context.site, ...(weekStart === undefined ? {} : { weekStart }) })
  return sanitizeWeeklyReport(await readAuditHistory(`${marketingInsightsContracts.weekly.path}?${query}`, signal), context, weekStart)
}
