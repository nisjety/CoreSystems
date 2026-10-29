import { pageSpeedCheckIds, type PageSpeedAuditCommand, type PageSpeedStrategy, type PageSpeedCheck } from '../../actions/marketing-pagespeed.ts'
import { normalizePulseSite } from './pulse-site-stack.ts'
import { isWebsiteId, isGA4ReportingSite } from './website-client.ts'

export type { PageSpeedStrategy }
export type { PageSpeedCheck }
export type PageSpeedReport = {
  reportingOrigin?: string
  performance: number | null; accessibility: number | null; bestPractices: number | null; seo: number | null
  lcpMs: number | null; cls: number | null; tbtMs: number | null; fcpMs: number | null; speedIndexMs: number | null
  fetchTime: string; lighthouseVersion: string
  checks?: PageSpeedCheck[]
}
export type PageSpeedCache = {
 stale: boolean; refreshStatus: 'queued' | 'running' | 'complete' | 'failed'
 lastSuccessAt?: string; lastAttemptAt?: string; nextRefreshAt?: string
 refreshReason?: 'quota_limited' | 'not_configured' | 'source_unavailable' | 'audit_failed'
}
export type PageSpeedSnapshot = {
 cache?: PageSpeedCache
  site: string; source: 'google_pagespeed'; strategy: PageSpeedStrategy
  status: 'idle' | 'running' | 'complete' | 'failed'
  requestedAt?: string; completedAt?: string
  reason?: 'quota_limited' | 'not_configured' | 'source_unavailable' | 'audit_failed'
  report?: PageSpeedReport
}
const record = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value)
const timestamp = (value: unknown): value is string => typeof value === 'string' && value.length <= 40 && /^\d{4}-\d{2}-\d{2}T/.test(value) && Number.isFinite(Date.parse(value))
const metric = (value: unknown, max = Number.MAX_VALUE): value is number | null => value === null || typeof value === 'number' && Number.isFinite(value) && value >= 0 && value <= max

function projectChecks(value: unknown): PageSpeedCheck[] {
  if (!Array.isArray(value) || value.length > pageSpeedCheckIds.length) throw new Error('Invalid lab checks')
  const ids = value.map((check: unknown) => record(check) ? check.id : undefined)
  if (new Set(ids).size !== ids.length) throw new Error('Duplicate lab check')
  return value.map((check: unknown) => {
    if (!record(check) || typeof check.id !== 'string' || typeof check.status !== 'string' || !pageSpeedCheckIds.includes(check.id as PageSpeedCheck['id'])) throw new Error('Invalid lab check')
    const validScore = check.status === 'passed' ? check.score === 1 : check.status === 'failed' ? check.score === 0
      : ['not_applicable', 'manual', 'informative', 'error', 'not_reported'].includes(check.status) && check.score === null
    if (!validScore) throw new Error('Invalid lab check status')
    return { id: check.id as PageSpeedCheck['id'], status: check.status as PageSpeedCheck['status'], score: check.score as number | null }
  })
}

function projectReport(value: unknown,site:string): PageSpeedReport {
  if (!record(value) || !timestamp(value.fetchTime) || typeof value.lighthouseVersion !== 'string' || !/^[a-zA-Z0-9.+-]{1,64}$/.test(value.lighthouseVersion)) throw new Error('Invalid lab report')
  if(value.reportingOrigin!==undefined&&(typeof value.reportingOrigin!=='string'||!isGA4ReportingSite(site,`${value.reportingOrigin}/`)))throw new Error('Invalid PageSpeed origin')
  if (![value.performance, value.accessibility, value.bestPractices, value.seo].every((score) => metric(score, 100)) || ![value.lcpMs, value.cls, value.tbtMs, value.fcpMs, value.speedIndexMs].every((score) => metric(score))) throw new Error('Invalid lab metric')
  return {
    performance: value.performance as number | null, accessibility: value.accessibility as number | null,
    bestPractices: value.bestPractices as number | null, seo: value.seo as number | null,
    lcpMs: value.lcpMs as number | null, cls: value.cls as number | null, tbtMs: value.tbtMs as number | null,
    fcpMs: value.fcpMs as number | null, speedIndexMs: value.speedIndexMs as number | null,
    fetchTime: value.fetchTime, lighthouseVersion: value.lighthouseVersion,
    ...(value.reportingOrigin===undefined?{}:{reportingOrigin:value.reportingOrigin as string}),
    ...(value.checks === undefined ? {} : { checks: projectChecks(value.checks) }),
  }
}

function projectCache(value: unknown, snapshot: Record<string, unknown>): PageSpeedCache {
 if (!record(value) || typeof value.stale !== 'boolean' || !['queued', 'running', 'complete', 'failed'].includes(String(value.refreshStatus))) throw new Error('Invalid audit cache')
 for (const key of ['lastSuccessAt', 'lastAttemptAt', 'nextRefreshAt']) if (value[key] !== undefined && !timestamp(value[key])) throw new Error('Invalid cache date')
 if (value.refreshStatus === 'failed' ? !['quota_limited', 'not_configured', 'source_unavailable', 'audit_failed'].includes(String(value.refreshReason)) : value.refreshReason !== undefined) throw new Error('Invalid refresh reason')
 if (snapshot.status === 'complete') {
  if (!record(snapshot.report) || value.lastSuccessAt !== snapshot.report.fetchTime) throw new Error('Invalid saved measurement')
 } else if (value.lastSuccessAt !== undefined || !value.stale || value.refreshStatus === 'complete' || snapshot.status !== (value.refreshStatus === 'failed' ? 'failed' : 'running')) throw new Error('Invalid empty cache')
 return {
  stale: value.stale, refreshStatus: value.refreshStatus as PageSpeedCache['refreshStatus'],
  ...(value.lastSuccessAt === undefined ? {} : { lastSuccessAt: value.lastSuccessAt as string }),
  ...(value.lastAttemptAt === undefined ? {} : { lastAttemptAt: value.lastAttemptAt as string }),
  ...(value.nextRefreshAt === undefined ? {} : { nextRefreshAt: value.nextRefreshAt as string }),
  ...(value.refreshReason === undefined ? {} : { refreshReason: value.refreshReason as PageSpeedCache['refreshReason'] }),
 }
}

/** Rebuild every nested field; raw Lighthouse/provider objects cannot cross BFF. */
export function sanitizePageSpeedSnapshot(value: unknown): PageSpeedSnapshot {
  if (!record(value) || typeof value.site !== 'string' || normalizePulseSite(value.site) !== value.site || value.source !== 'google_pagespeed' || !['mobile', 'desktop'].includes(String(value.strategy)) || !['idle', 'running', 'complete', 'failed'].includes(String(value.status))) throw new Error('Invalid PageSpeed state')
  if (value.requestedAt !== undefined && !timestamp(value.requestedAt) || value.completedAt !== undefined && !timestamp(value.completedAt)) throw new Error('Invalid audit date')
  if (value.status === 'failed' ? !['quota_limited', 'not_configured', 'source_unavailable', 'audit_failed'].includes(String(value.reason)) : value.reason !== undefined) throw new Error('Invalid audit reason')
  if (value.status !== 'complete' && value.report !== undefined) throw new Error('Unexpected lab report')
  return {
    ...(value.cache === undefined ? {} : { cache: projectCache(value.cache, value) }),
    site: value.site, source: 'google_pagespeed', strategy: value.strategy as PageSpeedStrategy, status: value.status as PageSpeedSnapshot['status'],
    ...(value.requestedAt === undefined ? {} : { requestedAt: value.requestedAt as string }),
    ...(value.completedAt === undefined ? {} : { completedAt: value.completedAt as string }),
    ...(value.reason === undefined ? {} : { reason: value.reason as PageSpeedSnapshot['reason'] }),
    ...(value.status === 'complete' ? { report: projectReport(value.report,value.site) } : {}),
  }
}

async function pageSpeedRequest(method: 'GET' | 'POST', command: PageSpeedAuditCommand, signal?: AbortSignal): Promise<PageSpeedSnapshot> {
  if (normalizePulseSite(command.site) !== command.site || !['mobile', 'desktop'].includes(command.strategy)) throw new Error('Invalid PageSpeed command')
  if (command.websiteId !== undefined && !isWebsiteId(command.websiteId)) throw new Error('Invalid website identity')
  const path = '/api/v1/visibility/pagespeed'
  const response = await fetch(method === 'POST' ? path : `${path}?site=${encodeURIComponent(command.site)}&strategy=${command.strategy}${command.websiteId ? `&websiteId=${command.websiteId}` : ''}`, {
    method, credentials: 'include', signal,
    headers: { Accept: 'application/json', ...(method === 'POST' ? { 'Content-Type': 'application/json' } : {}) },
    ...(method === 'POST' ? { body: JSON.stringify(command) } : {}),
  })
  if (!response.ok) throw new Error('PageSpeed is unavailable')
  const payload: unknown = await response.json()
  const snapshot = sanitizePageSpeedSnapshot(record(payload) ? payload.data : undefined)
  if (snapshot.site !== command.site || snapshot.strategy !== command.strategy) throw new Error('Unexpected PageSpeed result')
  return snapshot
}
export const loadPageSpeed = (site: string, strategy: PageSpeedStrategy, signal?: AbortSignal, websiteId?: string) => pageSpeedRequest('GET', { site, strategy, ...(websiteId ? { websiteId } : {}) }, signal)
export const startPageSpeed = (site: string, strategy: PageSpeedStrategy, signal?: AbortSignal, websiteId?: string) => pageSpeedRequest('POST', { site, strategy, ...(websiteId ? { websiteId } : {}) }, signal)
