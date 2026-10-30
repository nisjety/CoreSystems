import { marketingInsightsContracts } from '../../actions/marketing-insights'
import { isAuditId, isAuditRule, isAuditRunId, isAuditFingerprint, type AuditRule } from './audit-client'
import { isPublicSitePage, validateDiscoveryContext, type DiscoveryContext } from './discovery-client'
import { auditHistoryBase, changeCounts, changeKinds, historyNotAfter, historyRecord, historyReason, historyStamp, readAuditHistory, type AuditChangeKind, type AuditChanges, type AuditHistoryBase } from './audit-history-boundary'

export type AuditInsightItem = { pageUrl: string; ruleId: AuditRule; status: AuditChangeKind; checkStatus: 'passed' | 'failed' | 'review' | 'inconclusive' | 'unavailable'; severity: 'warning' | 'error' | 'review'; proofAuditId: string; observedAt: string; runId: string; fingerprint: string; relatedUrls: string[] }
export type AuditInsightAlert = { kind: 'new_finding' | 'regressed_finding' | 'audit_failed' | 'coverage_gap'; auditId: string; pageUrl: string; ruleId: AuditRule | ''; reason: string; observedAt: string }
export type AuditInsights = AuditHistoryBase & { comparisonAvailable: boolean; latestAuditId: string; latestStatus: 'idle' | 'running' | 'complete' | 'failed'; summary: AuditChanges; items: AuditInsightItem[]; alerts: AuditInsightAlert[] }

function item(value: unknown, site: string): AuditInsightItem {
  if (!historyRecord(value) || !isPublicSitePage(site, value.pageUrl) || !isAuditRule(value.ruleId) || typeof value.status !== 'string' || !changeKinds.includes(value.status as AuditChangeKind) || typeof value.checkStatus !== 'string' || !['passed', 'failed', 'review', 'inconclusive', 'unavailable'].includes(value.checkStatus) || typeof value.severity !== 'string' || !['error', 'warning', 'review'].includes(value.severity) || !isAuditId(value.proofAuditId) || !historyStamp(value.observedAt) || !isAuditRunId(value.runId) || !isAuditFingerprint(value.fingerprint) || !Array.isArray(value.relatedUrls) || value.relatedUrls.length > 100 || !value.relatedUrls.every((url) => isPublicSitePage(site, url)) || new Set(value.relatedUrls).size !== value.relatedUrls.length) throw new Error('Invalid audit insight proof')
  if (value.status === 'resolved' ? value.checkStatus !== 'passed' : value.status === 'unknown' ? value.checkStatus === 'passed' : !['failed', 'review'].includes(value.checkStatus)) throw new Error('Inconsistent audit change evidence')
  return { pageUrl: value.pageUrl, ruleId: value.ruleId, status: value.status as AuditChangeKind, checkStatus: value.checkStatus as AuditInsightItem['checkStatus'], severity: value.severity as AuditInsightItem['severity'], proofAuditId: value.proofAuditId, observedAt: value.observedAt, runId: value.runId, fingerprint: value.fingerprint, relatedUrls: [...value.relatedUrls] as string[] }
}
function alert(value: unknown, site: string): AuditInsightAlert {
  if (!historyRecord(value) || typeof value.kind !== 'string' || !['new_finding', 'regressed_finding', 'audit_failed', 'coverage_gap'].includes(value.kind) || !isAuditId(value.auditId) || !historyStamp(value.observedAt)) throw new Error('Invalid audit alert')
  const finding = value.kind === 'new_finding' || value.kind === 'regressed_finding'
  if (finding ? !isPublicSitePage(site, value.pageUrl) || !isAuditRule(value.ruleId) : value.pageUrl !== '' || value.ruleId !== '') throw new Error('Invalid audit alert scope')
  return { kind: value.kind as AuditInsightAlert['kind'], auditId: value.auditId, observedAt: value.observedAt, pageUrl: value.pageUrl as string, ruleId: value.ruleId as AuditInsightAlert['ruleId'], reason: historyReason(value.reason) }
}
export function sanitizeAuditInsights(value: unknown, expected: DiscoveryContext): AuditInsights {
  const base = auditHistoryBase(value, expected)
  if (!historyRecord(value) || typeof value.comparisonAvailable !== 'boolean' || typeof value.latestStatus !== 'string' || !['idle', 'running', 'complete', 'failed'].includes(value.latestStatus) || (value.latestStatus === 'idle' ? value.latestAuditId !== '' : !isAuditId(value.latestAuditId)) || !Array.isArray(value.items) || value.items.length > 2000 || !Array.isArray(value.alerts) || value.alerts.length > 2002) throw new Error('Invalid audit insights')
  const items = value.items.map((v: unknown) => item(v, base.site)), alerts = value.alerts.map((v: unknown) => alert(v, base.site)), summary = changeCounts(value.summary, 2000)
  if (items.some((v) => !historyNotAfter(v.observedAt, base.asOf)) || alerts.some((v) => !historyNotAfter(v.observedAt, base.asOf) || v.auditId !== value.latestAuditId)) throw new Error('Invalid insight observation boundary')
  for (const alert of alerts) {
    if (alert.kind === 'audit_failed' && (value.latestStatus !== 'failed' || !alert.reason)) throw new Error('Contradictory failed audit alert')
    if (alert.kind === 'new_finding' || alert.kind === 'regressed_finding') {
      const expectedStatus = alert.kind === 'new_finding' ? 'new' : 'regressed'
      if (value.latestStatus !== 'complete' || alert.reason !== '' || !items.some((v) => v.status === expectedStatus && v.proofAuditId === alert.auditId && v.pageUrl === alert.pageUrl && v.ruleId === alert.ruleId && v.observedAt === alert.observedAt)) throw new Error('Alert lacks matching finding proof')
    }
  }
  if (changeKinds.some((key) => summary[key] !== items.filter((v) => v.status === key).length) || new Set(items.map((v) => `${v.pageUrl}|${v.ruleId}`)).size !== items.length || value.latestStatus === 'idle' && (items.length > 0 || alerts.length > 0 || value.comparisonAvailable)) throw new Error('Invalid insight comparison counts')
  return { ...base, comparisonAvailable: value.comparisonAvailable, latestAuditId: value.latestAuditId as string, latestStatus: value.latestStatus as AuditInsights['latestStatus'], summary, items, alerts }
}
export async function loadAuditInsights(input: DiscoveryContext, signal?: AbortSignal): Promise<AuditInsights> {
  const context = validateDiscoveryContext(input), query = new URLSearchParams({ websiteId: context.websiteId, site: context.site })
  return sanitizeAuditInsights(await readAuditHistory(`${marketingInsightsContracts.insights.path}?${query}`, signal), context)
}
