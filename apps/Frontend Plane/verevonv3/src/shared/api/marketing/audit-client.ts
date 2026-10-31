import { sanitizeSEO, type SEOEvidence } from '../visibility/seo-evidence'
import { marketingAuditContracts } from '../../actions/marketing-audit'
import { analyticsRecord as record, analyticsTimestamp } from './analytics-boundary'
import { isPublicSitePage, validateDiscoveryContext,sanitizeSitemapCoverage,type SitemapCoverage, type DiscoveryContext } from './discovery-client'

export const auditRules = ['http_error', 'missing_title_signal', 'missing_description_signal', 'duplicate_title_signal', 'canonical_target_review', 'broken_checked_link', 'h1_structure', 'heading_sequence', 'noindex_directive', 'redirect_chain_review', 'structured_data_parse', 'hreflang_syntax', 'canonical_outside_site', 'hreflang_reciprocity'] as const
export type AuditRule = typeof auditRules[number]
export type AuditCheckStatus = 'passed' | 'failed' | 'review' | 'inconclusive'
export type AuditFinding = { id: string; ruleId: AuditRule; severity: 'warning' | 'error' | 'review'; pageUrl: string; observedAt: string; runId: string; fingerprint: string; relatedUrls: string[] }
export type AuditPage = { url: string; finalUrl: string; status: 'checked' | 'failed'; httpStatus: number; title: string; description: string; canonical: string; observedAt: string; runId: string; fingerprint: string; links: string[]; checks: { ruleId: AuditRule; status: AuditCheckStatus }[]; reason?: string; seo?: SEOEvidence; extractionPartial?: boolean }
export type AuditReceipt = { id: string; status: 'running' | 'complete' | 'failed'; startedAt: string; completedAt: string; pageCount: number; findingCount: number }
export type SiteAudit = DiscoveryContext & { id: string; source: 'quarry_capture'; ruleVersion: 'bounded_rules_v1'; status: 'idle' | 'running' | 'complete' | 'failed'; limit: 10 | 100; partial: true; coverage?: AuditCoverage; startedAt: string; completedAt: string; pages: AuditPage[]; findings: AuditFinding[]; history: AuditReceipt[]; reason?: string }
export type AuditCoverage = {sitemap?:SitemapCoverage;discovered:number;attempted:number;checked:number;failed:number;pending:number;limitReached:boolean;inventoryTruncated:boolean;extractionPartial:boolean;stopReason?:string}
function coverage(value:unknown,pages:AuditPage[],limit:number):AuditCoverage {
 if(!record(value))throw Error('Invalid audit coverage')
 for(const field of ['discovered','attempted','checked','failed','pending'])if(!count(value[field],1000))throw Error('Invalid crawl count')
 if(value.attempted!==pages.length||Number(value.checked)+Number(value.failed)!==value.attempted||Number(value.discovered)-Number(value.attempted)!==value.pending||value.checked!==pages.filter(p=>p.status==='checked').length||value.limitReached!==(pages.length>=limit&&Number(value.pending)>0)||typeof value.inventoryTruncated!=='boolean'||typeof value.extractionPartial!=='boolean'||value.stopReason!==undefined&&(typeof value.stopReason!=='string'||!reasons.includes(value.stopReason)))throw Error('Inconsistent crawl coverage')
 return {...(value.sitemap===undefined?{}:{sitemap:sanitizeSitemapCoverage(value.sitemap)}),discovered:Number(value.discovered),attempted:Number(value.attempted),checked:Number(value.checked),failed:Number(value.failed),pending:Number(value.pending),limitReached:value.limitReached,inventoryTruncated:value.inventoryTruncated,extractionPartial:value.extractionPartial,...(value.stopReason===undefined?{}:{stopReason:value.stopReason as string})}
}
export const isAuditId = (value: unknown): value is string => typeof value === 'string' && /^audit_[a-f0-9]{32}$/.test(value)
export const isAuditFindingId = (value: unknown): value is string => typeof value === 'string' && /^finding_[a-f0-9]{32}$/.test(value)
export const isAuditRule = (value: unknown): value is AuditRule => auditRules.includes(value as AuditRule)
export const isAuditRunId = (value: unknown): value is string => typeof value === 'string' && /^run_[0-7][0-9A-HJKMNP-TV-Z]{25}$/.test(value)
export const isAuditFingerprint = (value: unknown): value is string => typeof value === 'string' && /^blake3:[a-f0-9]{64}$/.test(value)
export const isAuditCheckStatus = (value: unknown): value is AuditCheckStatus => ['passed', 'failed', 'review', 'inconclusive'].includes(String(value)) && typeof value === 'string'
const text = (value: unknown, max: number): value is string => typeof value === 'string' && value.length <= max && !/[\x00-\x1f\x7f]/.test(value)
const count = (value: unknown, max: number): value is number => Number.isSafeInteger(value) && Number(value) >= 0 && Number(value) <= max
const statuses = ['running', 'complete', 'failed']
const reasons = ['not_configured', 'source_unavailable', 'policy_blocked', 'access_denied', 'quota_limited', 'invalid_source', 'timeout', 'unsupported_content', 'interrupted', 'context_changed', 'storage_unavailable']
function timestamp(value: unknown): value is string {
  if (!analyticsTimestamp(value) || !/^\d{4}-\d\d-\d\dT(?:[01]\d|2[0-3]):[0-5]\d:[0-5]\d(?:\.\d{1,9})?(?:Z|[+-](?:[01]\d|2[0-3]):[0-5]\d)$/.test(value)) return false
  const day = new Date(`${value.slice(0, 10)}T00:00:00Z`)
  return Number.isFinite(day.getTime()) && day.toISOString().slice(0, 10) === value.slice(0, 10)
}
function urls(value: unknown, site: string): string[] {
  if (!Array.isArray(value) || value.length > 100 || !value.every((url) => isPublicSitePage(site, url)) || new Set(value).size !== value.length) throw new Error('Invalid audit URL evidence')
  return [...value] as string[]
}
function page(value: unknown, site: string): AuditPage {
  if (!record(value) || !isPublicSitePage(site, value.url) || typeof value.status !== 'string' || !['checked', 'failed'].includes(value.status) || !count(value.httpStatus, 599) || !text(value.title, 512) || !text(value.description, 2048) || !text(value.finalUrl, 2048) || !text(value.canonical, 2048) || !Array.isArray(value.checks) || value.checks.length > auditRules.length) throw new Error('Invalid audit page')
  if(value.extractionPartial!==undefined&&typeof value.extractionPartial!=='boolean'||value.status==='failed'&&value.seo!==undefined)throw Error('Invalid optional page evidence')
  const checked = value.status === 'checked'
  if (checked ? !isPublicSitePage(site, value.finalUrl) || Number(value.httpStatus) < 100 || !timestamp(value.observedAt) || !isAuditRunId(value.runId) || !isAuditFingerprint(value.fingerprint) : value.checks.length !== 0 || value.httpStatus !== 0 || value.observedAt !== '' || value.runId !== '' || value.fingerprint !== '') throw new Error('Invalid audit coverage')
  if (value.canonical !== '' && !isPublicSitePage(site, value.canonical)) throw new Error('Invalid audit canonical')
  const checks = value.checks.map((check: unknown) => {
    if (!record(check) || !isAuditRule(check.ruleId) || !isAuditCheckStatus(check.status)) throw new Error('Invalid audit check')
    return { ruleId: check.ruleId, status: check.status }
  })
  if (new Set(checks.map((check) => check.ruleId)).size !== checks.length || value.reason !== undefined && (typeof value.reason !== 'string' || !reasons.includes(value.reason))) throw new Error('Invalid audit checks')
  return { url: value.url, finalUrl: value.finalUrl, status: value.status as AuditPage['status'], httpStatus: Number(value.httpStatus), title: value.title, description: value.description, canonical: value.canonical, observedAt: value.observedAt as string, runId: value.runId as string, fingerprint: value.fingerprint as string, links: urls(value.links, site), checks, ...(value.seo===undefined?{}:{seo:sanitizeSEO(value.seo,site)}),...(value.extractionPartial===undefined?{}:{extractionPartial:value.extractionPartial===true}), ...(value.reason === undefined ? {} : { reason: value.reason as string }) }
}
function finding(value: unknown, pages: AuditPage[], site: string): AuditFinding {
  if (!record(value) || !isAuditFindingId(value.id) || !isAuditRule(value.ruleId) || typeof value.severity !== 'string' || !['error', 'warning', 'review'].includes(value.severity)) throw new Error('Invalid audit finding')
  const source = pages.find((page) => page.status === 'checked' && page.url === value.pageUrl)
  if (!source || !source.checks.some((check) => check.ruleId === value.ruleId && ['failed', 'review'].includes(check.status)) || source.observedAt !== value.observedAt || source.runId !== value.runId || source.fingerprint !== value.fingerprint) throw new Error('Audit finding lacks checked evidence')
  return { id: value.id, ruleId: value.ruleId, severity: value.severity as AuditFinding['severity'], pageUrl: source.url, observedAt: source.observedAt, runId: source.runId, fingerprint: source.fingerprint, relatedUrls: urls(value.relatedUrls, site) }
}
function receipt(value: unknown): AuditReceipt {
  if (!record(value) || !isAuditId(value.id) || typeof value.status !== 'string' || !statuses.includes(value.status) || !timestamp(value.startedAt) || (value.status === 'running' ? value.completedAt !== '' : !timestamp(value.completedAt)) || !count(value.pageCount, 100) || !count(value.findingCount, 1400)) throw new Error('Invalid audit receipt')
  return { id: value.id, status: value.status as AuditReceipt['status'], startedAt: value.startedAt, completedAt: value.completedAt as string, pageCount: value.pageCount, findingCount: value.findingCount }
}
/** Normalize public evidence only; raw HTML, artifacts, credentials and unknown fields stay out. */
export function sanitizeSiteAudit(value: unknown, expected: DiscoveryContext): SiteAudit {
  const context = validateDiscoveryContext(value), selected = validateDiscoveryContext(expected)
  if (!record(value) || context.websiteId !== selected.websiteId || context.site !== selected.site || context.websiteRevision !== selected.websiteRevision || value.source !== 'quarry_capture' || value.ruleVersion !== 'bounded_rules_v1' || typeof value.status !== 'string' || !['idle', ...statuses].includes(value.status) || (value.limit !== 10 && value.limit !== 100) || value.partial !== true || !Array.isArray(value.pages) || value.pages.length > Number(value.limit) || !Array.isArray(value.findings) || value.findings.length > Number(value.limit)*auditRules.length || !Array.isArray(value.history) || value.history.length > 5) throw new Error('Unexpected audit response')
  const unstarted = value.status === 'idle' || value.status === 'failed' && value.reason === 'not_configured' && value.id === ''
  if (unstarted ? value.id !== '' || value.startedAt !== '' || value.completedAt !== '' : !isAuditId(value.id) || !timestamp(value.startedAt) || (value.status === 'running' ? value.completedAt !== '' : !timestamp(value.completedAt))) throw new Error('Invalid audit lifecycle')
  if ((unstarted || value.status === 'running') && (value.pages.length || value.findings.length)) throw new Error('Audit is not completed')
  const pages = value.pages.map((item: unknown) => page(item, context.site))
  const findings = value.findings.map((item: unknown) => finding(item, pages, context.site))
  const history = value.history.map(receipt)
  if (new Set(pages.map((p) => p.url)).size !== pages.length || new Set(findings.map((f) => f.id)).size !== findings.length || new Set(history.map((r) => r.id)).size !== history.length || value.reason !== undefined && (typeof value.reason !== 'string' || !reasons.includes(value.reason))) throw new Error('Invalid audit evidence')
  return { ...context, id: value.id as string, source: 'quarry_capture', ruleVersion: 'bounded_rules_v1', status: value.status as SiteAudit['status'], limit: value.limit as SiteAudit['limit'], partial: true, ...(value.coverage===undefined?{}:{coverage:coverage(value.coverage,pages,Number(value.limit))}), startedAt: value.startedAt as string, completedAt: value.completedAt as string, pages, findings, history, ...(value.reason === undefined ? {} : { reason: value.reason as string }) }
}
async function request(method: 'GET' | 'POST', input: DiscoveryContext, signal?: AbortSignal, auditId?: string): Promise<SiteAudit> {
  const context = validateDiscoveryContext(input)
  if (auditId !== undefined && !isAuditId(auditId)) throw new Error('Invalid audit identity')
  const query = new URLSearchParams({ websiteId: context.websiteId, site: context.site, ...(auditId ? { auditId } : {}) })
  const response = await fetch(method === 'GET' ? `${marketingAuditContracts.read.path}?${query}` : marketingAuditContracts.start.path, { method, credentials: 'include', signal, redirect: 'error', headers: { Accept: 'application/json', ...(method === 'POST' ? { 'Content-Type': 'application/json' } : {}) }, ...(method === 'POST' ? { body: JSON.stringify(context) } : {}) })
  if (!response.ok) throw new Error(response.status === 409 ? 'audit_conflict' : 'Site audit is unavailable')
  const body: unknown = await response.json()
  const result = sanitizeSiteAudit(record(body) ? body.data : undefined, context)
  if (auditId && result.id !== auditId) throw new Error('Unexpected audit receipt')
  return result
}
export const loadSiteAudit = (context: DiscoveryContext, signal?: AbortSignal, auditId?: string) => request('GET', context, signal, auditId)
export const startSiteAudit = (context: DiscoveryContext, signal?: AbortSignal) => request('POST', context, signal)
