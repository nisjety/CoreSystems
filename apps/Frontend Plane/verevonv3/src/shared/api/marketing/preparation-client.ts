import { marketingPreparationContracts } from '../../actions/marketing-preparation'
import { validateDiscoveryContext, type DiscoveryContext } from './discovery-client'

export const preparationStepKeys = ['discovery', 'pagespeed_mobile', 'pagespeed_desktop'] as const
export const preparationReasons = ['not_configured', 'source_unavailable', 'quota_limited', 'capacity_deferred', 'timeout', 'interrupted', 'context_changed'] as const
export type PreparationReason = typeof preparationReasons[number]
export type PreparationStep = { key: typeof preparationStepKeys[number]; status: 'queued' | 'running' | 'complete' | 'failed' | 'unavailable'; reason?: PreparationReason; observedAt?: string }
export type WebsitePreparation = DiscoveryContext & { source: 'marketing_preparation'; status: 'idle' | 'queued' | 'running' | 'complete' | 'failed' | 'partial'; jobId?: string; queuedAt?: string; startedAt?: string; completedAt?: string; steps: PreparationStep[] }
export const sourceKeys = ['quarry', 'pagespeed', 'search_console', 'ga4', 'clarity', 'google_ads', 'bing_webmaster', 'bing_ai', 'crux', 'web_vitals', 'google_trends', 'common_crawl', 'cookiebot', 'algolia'] as const
const modes = ['public_check', 'oauth', 'project_token', 'installation', 'import', 'public_archive', 'api_key'] as const
const sourceStatuses = ['available', 'connected', 'needs_connection', 'requires_consent', 'needs_selection', 'not_found', 'source_unavailable', 'pending_verification', 'not_connected', 'not_implemented', 'authorization_required'] as const
const refreshes = ['preparation_job', 'dashboard_read', 'snapshot_read', 'public_signal_job', 'trends_job', 'manual', 'not_available'] as const
export type MarketingSource = { key: typeof sourceKeys[number]; implemented: boolean; mode: typeof modes[number]; status: typeof sourceStatuses[number]; refresh: typeof refreshes[number] }
export type SourceCatalog = DiscoveryContext & { source: 'marketing_source_catalog'; items: MarketingSource[] }
const record = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)
function member<T extends string>(v: unknown, options: readonly T[]): v is T { return typeof v === 'string' && options.includes(v as T) }
function timestamp(v: unknown): v is string {
  if (typeof v !== 'string' || v.length > 40 || !/^\d{4}-\d\d-\d\dT(?:[01]\d|2[0-3]):[0-5]\d:[0-5]\d(?:\.\d{1,9})?(?:Z|[+-](?:[01]\d|2[0-3]):[0-5]\d)$/.test(v) || !Number.isFinite(Date.parse(v))) return false
  const day = new Date(`${v.slice(0, 10)}T00:00:00Z`)
  return Number.isFinite(day.getTime()) && day.toISOString().slice(0, 10) === v.slice(0, 10)
}
function scoped(value: unknown, expected: DiscoveryContext) {
  const current = validateDiscoveryContext(value), selected = validateDiscoveryContext(expected)
  if (current.websiteId !== selected.websiteId || current.site !== selected.site || current.websiteRevision !== selected.websiteRevision) throw new Error('Invalid preparation context')
  return current
}
function step(v: unknown): PreparationStep {
  if (!record(v) || !member(v.key, preparationStepKeys) || !member(v.status, ['queued', 'running', 'complete', 'failed', 'unavailable'])) throw new Error('Invalid preparation step')
  const unsuccessful = v.status === 'failed' || v.status === 'unavailable'
  if (unsuccessful ? !member(v.reason, preparationReasons) : v.reason !== undefined) throw new Error('Invalid preparation reason')
  if (v.observedAt !== undefined && !timestamp(v.observedAt) || (v.status === 'complete' || unsuccessful) && !timestamp(v.observedAt) || (v.status === 'queued' || v.status === 'running') && v.observedAt !== undefined) throw new Error('Invalid preparation observation')
  return { key: v.key, status: v.status, ...(v.reason === undefined ? {} : { reason: v.reason as PreparationReason }), ...(v.observedAt === undefined ? {} : { observedAt: v.observedAt as string }) }
}

/** Rebuild receipts only. Provider payloads, credentials and analytics are not part of preparation. */
export function sanitizeWebsitePreparation(value: unknown, expected: DiscoveryContext): WebsitePreparation {
  const context = scoped(value, expected)
  if (!record(value) || value.source !== 'marketing_preparation' || !member(value.status, ['idle', 'queued', 'running', 'complete', 'failed', 'partial']) || !Array.isArray(value.steps) || value.steps.length !== 3) throw new Error('Invalid preparation response')
  const steps = value.steps.map(step)
  if (new Set(steps.map((item) => item.key)).size !== 3) throw new Error('Invalid preparation steps')
  if (steps.filter((item) => item.status === 'running').length > 1) throw new Error('Invalid simultaneous preparation steps')
  if (value.status === 'idle') {
    if (value.jobId !== undefined || value.queuedAt !== undefined || value.startedAt !== undefined || value.completedAt !== undefined || steps.some((item) => item.status !== 'queued' || item.observedAt !== undefined)) throw new Error('Invalid idle preparation')
  } else {
    if (typeof value.jobId !== 'string' || !/^prep_[a-f0-9]{32}$/.test(value.jobId) || !timestamp(value.queuedAt)) throw new Error('Invalid preparation job')
    if (steps.some((item) => item.observedAt && Date.parse(item.observedAt) < Date.parse(value.queuedAt as string))) throw new Error('Invalid preparation observation order')
    if (value.status === 'queued') {
      if (value.startedAt !== undefined || value.completedAt !== undefined || steps.some((item) => item.status === 'running')) throw new Error('Invalid queued preparation')
    } else {
      if (!timestamp(value.startedAt) || Date.parse(value.startedAt) < Date.parse(value.queuedAt)) throw new Error('Invalid preparation start')
      if (value.status === 'running') { if (value.completedAt !== undefined) throw new Error('Invalid running preparation') }
      else {
        if (!timestamp(value.completedAt) || Date.parse(value.completedAt) < Date.parse(value.startedAt) || steps.some((item) => item.status === 'queued' || item.status === 'running')) throw new Error('Invalid terminal preparation')
        if (steps.some((item) => item.observedAt && Date.parse(item.observedAt) > Date.parse(value.completedAt as string))) throw new Error('Invalid preparation observation order')
        const completed = steps.filter((item) => item.status === 'complete').length
        if (value.status === 'complete' ? completed !== 3 : value.status === 'failed' ? completed !== 0 : completed === 0 || completed === 3) throw new Error('Invalid preparation outcome')
      }
    }
  }
  return { ...context, source: 'marketing_preparation', status: value.status, steps, ...(value.jobId === undefined ? {} : { jobId: value.jobId as string }), ...(value.queuedAt === undefined ? {} : { queuedAt: value.queuedAt as string }), ...(value.startedAt === undefined ? {} : { startedAt: value.startedAt as string }), ...(value.completedAt === undefined ? {} : { completedAt: value.completedAt as string }) }
}
export function sanitizeSourceCatalog(value: unknown, expected: DiscoveryContext): SourceCatalog {
  const context = scoped(value, expected)
  if (!record(value) || value.source !== 'marketing_source_catalog' || !Array.isArray(value.items) || value.items.length !== sourceKeys.length) throw new Error('Invalid source catalogue')
  const items = value.items.map((item): MarketingSource => {
    if (!record(item) || !member(item.key, sourceKeys) || typeof item.implemented !== 'boolean' || !member(item.mode, modes) || !member(item.status, sourceStatuses) || !member(item.refresh, refreshes)) throw new Error('Invalid marketing source')
    if (item.implemented ? item.status === 'not_implemented' || item.refresh === 'not_available' : item.status !== 'not_implemented' || item.refresh !== 'not_available') throw new Error('Invalid source readiness')
    if (item.refresh === 'snapshot_read' && !['search_console', 'ga4', 'clarity', 'algolia'].includes(item.key)) throw new Error('Invalid snapshot source')
    if (item.refresh === 'public_signal_job' && !['crux', 'common_crawl'].includes(item.key)) throw new Error('Invalid public signal source')
    if (item.refresh === 'trends_job' && item.key !== 'google_trends') throw new Error('Invalid automatic Trends source')
    return { key: item.key, implemented: item.implemented, mode: item.mode, status: item.status, refresh: item.refresh }
  })
  if (new Set(items.map((item) => item.key)).size !== sourceKeys.length) throw new Error('Invalid source set')
  return { ...context, source: 'marketing_source_catalog', items }
}
async function request<T>(path: string, context: DiscoveryContext, sanitize: (value: unknown, context: DiscoveryContext) => T, method: 'GET' | 'POST', signal?: AbortSignal): Promise<T> {
  const selected = validateDiscoveryContext(context), query = new URLSearchParams({ websiteId: selected.websiteId, site: selected.site })
  const response = await fetch(method === 'GET' ? `${path}?${query}` : path, { method, credentials: 'include', redirect: 'error', signal, headers: { Accept: 'application/json', ...(method === 'POST' ? { 'Content-Type': 'application/json' } : {}) }, ...(method === 'POST' ? { body: JSON.stringify(selected) } : {}) })
  if (response.status !== (method === 'POST' ? 202 : 200)) throw new Error(response.status === 409 ? 'preparation_conflict' : 'preparation_unavailable')
  const body: unknown = await response.json()
  return sanitize(record(body) ? body.data : undefined, selected)
}
export const loadWebsitePreparation = (context: DiscoveryContext, signal?: AbortSignal) => request(marketingPreparationContracts.read.path, context, sanitizeWebsitePreparation, 'GET', signal)
export const prepareWebsite = (context: DiscoveryContext, signal?: AbortSignal) => request(marketingPreparationContracts.prepare.path, context, sanitizeWebsitePreparation, 'POST', signal)
export const loadSourceCatalog = (context: DiscoveryContext, signal?: AbortSignal) => request(marketingPreparationContracts.sources.path, context, sanitizeSourceCatalog, 'GET', signal)
