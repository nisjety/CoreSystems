import { marketingAnalyticsSyncContracts } from '../../actions/marketing-analytics-sync'
import { validateDiscoveryContext, type DiscoveryContext } from './discovery-client'
import type { PulseQuarter } from './pulse-client'

export type AnalyticsSyncContext = DiscoveryContext & { quarter: PulseQuarter }
export const analyticsSyncReasons = ['not_configured', 'not_connected', 'requires_consent', 'needs_selection', 'not_found', 'source_unavailable', 'quota_limited', 'authorization_required', 'property_unverified', 'pending_verification', 'no_data', 'timeout', 'interrupted', 'context_changed'] as const
export type AnalyticsSyncReason = typeof analyticsSyncReasons[number]
export type AnalyticsSyncStep = { key: 'gsc' | 'ga4' | 'clarity'; status: 'queued' | 'running' | 'connected' | 'unavailable'; reason?: AnalyticsSyncReason; observedAt?: string; lastSuccessAt?: string }
export type AnalyticsSyncState = AnalyticsSyncContext & { source: 'marketing_analytics_sync'; enabled: boolean; status: 'idle' | 'queued' | 'running' | 'complete' | 'partial' | 'failed'; jobId?: string; queuedAt?: string; startedAt?: string; completedAt?: string; nextSyncAt?: string; expiresAt?: string; steps: AnalyticsSyncStep[] }
const record = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value)
function member<T extends string>(value: unknown, choices: readonly T[]): value is T { return typeof value === 'string' && choices.includes(value as T) }
function timestamp(value: unknown): value is string {
  if (typeof value !== 'string' || value.length > 40 || !/^\d{4}-\d\d-\d\dT(?:[01]\d|2[0-3]):[0-5]\d:[0-5]\d(?:\.\d{1,9})?(?:Z|[+-](?:[01]\d|2[0-3]):[0-5]\d)$/.test(value) || !Number.isFinite(Date.parse(value))) return false
  return new Date(`${value.slice(0, 10)}T00:00:00Z`).toISOString().slice(0, 10) === value.slice(0, 10)
}
function validateContext(value: unknown): AnalyticsSyncContext {
  const selected = validateDiscoveryContext(value)
  if (!record(value) || !member(value.quarter, ['Q1', 'Q2', 'Q3', 'Q4', 'FY'])) throw new Error('Invalid analytics sync quarter')
  return { ...selected, quarter: value.quarter }
}
function step(value: unknown): AnalyticsSyncStep {
  if (!record(value) || !member(value.key, ['gsc', 'ga4', 'clarity']) || !member(value.status, ['queued', 'running', 'connected', 'unavailable'])) throw new Error('Invalid analytics sync step')
  if (value.status === 'unavailable' ? !member(value.reason, analyticsSyncReasons) : value.reason !== undefined) throw new Error('Invalid analytics sync reason')
  const terminal = value.status === 'connected' || value.status === 'unavailable'
  if (terminal ? !timestamp(value.observedAt) : value.observedAt !== undefined) throw new Error('Invalid analytics sync observation')
  if (value.lastSuccessAt !== undefined && !timestamp(value.lastSuccessAt)) throw new Error('Invalid analytics success timestamp')
  return { key: value.key, status: value.status, ...(value.lastSuccessAt === undefined ? {} : { lastSuccessAt: value.lastSuccessAt as string }), ...(value.reason === undefined ? {} : { reason: value.reason as AnalyticsSyncReason }), ...(value.observedAt === undefined ? {} : { observedAt: value.observedAt as string }) }
}

/** Rebuild bounded job metadata only. A connected step is not permission to read metrics. */
export function sanitizeAnalyticsSync(value: unknown, expected: AnalyticsSyncContext): AnalyticsSyncState {
  const context = validateContext(value), selected = validateContext(expected)
  if (context.websiteId !== selected.websiteId || context.site !== selected.site || context.websiteRevision !== selected.websiteRevision || context.quarter !== selected.quarter || !record(value) || value.source !== 'marketing_analytics_sync' || typeof value.enabled !== 'boolean' || !member(value.status, ['idle', 'queued', 'running', 'complete', 'partial', 'failed']) || !Array.isArray(value.steps) || value.steps.length !== 3) throw new Error('Invalid analytics sync response')
  const steps = value.steps.map(step)
  if (new Set(steps.map((item) => item.key)).size !== 3) throw new Error('Invalid analytics sync sources')
  if (steps.some((item) => item.lastSuccessAt !== undefined && (value.status === 'idle' || Date.parse(item.lastSuccessAt) > Date.parse((value.completedAt ?? value.queuedAt) as string)))) throw new Error('Invalid analytics success history')
  const times = ['queuedAt', 'startedAt', 'completedAt', 'nextSyncAt', 'expiresAt'] as const
  if (times.some((key) => value[key] !== undefined && !timestamp(value[key]))) throw new Error('Invalid analytics sync timestamp')
  if (value.status === 'idle') {
    if (value.enabled || value.jobId !== undefined || times.some((key) => value[key] !== undefined) || steps.some((item) => item.status !== 'queued')) throw new Error('Invalid idle analytics sync')
  } else {
    if (typeof value.jobId !== 'string' || !/^sync_[a-f0-9]{32}$/.test(value.jobId) || !timestamp(value.queuedAt)) throw new Error('Invalid analytics sync job')
    if (value.status === 'queued') {
      if (times.slice(1).some((key) => value[key] !== undefined) || steps.some((item) => item.status !== 'queued')) throw new Error('Invalid queued analytics sync')
    } else {
      if (!timestamp(value.startedAt) || Date.parse(value.startedAt) < Date.parse(value.queuedAt)) throw new Error('Invalid analytics sync start')
      if (value.status === 'running') {
        if (value.completedAt !== undefined || value.nextSyncAt !== undefined || value.expiresAt !== undefined || steps.some((item) => item.status !== 'running')) throw new Error('Invalid running analytics sync')
      } else {
        if (!timestamp(value.completedAt) || Date.parse(value.completedAt) < Date.parse(value.startedAt) || steps.some((item) => !['connected', 'unavailable'].includes(item.status) || Date.parse(item.observedAt!) < Date.parse(value.startedAt as string) || Date.parse(item.observedAt!) > Date.parse(value.completedAt as string))) throw new Error('Invalid terminal analytics sync')
        const connected = steps.filter((item) => item.status === 'connected').length
        if (value.status === 'complete' ? connected !== 3 : value.status === 'failed' ? connected !== 0 : connected === 0 || connected === 3) throw new Error('Invalid analytics sync outcome')
        if (value.enabled ? !timestamp(value.nextSyncAt) : value.nextSyncAt !== undefined) throw new Error('Invalid analytics sync schedule')
        if (value.nextSyncAt !== undefined && Date.parse(value.nextSyncAt as string) < Date.parse(value.completedAt)) throw new Error('Invalid analytics sync schedule order')
        if (connected > 0 ? !timestamp(value.expiresAt) : value.expiresAt !== undefined) throw new Error('Invalid analytics snapshot expiry')
        if (value.expiresAt !== undefined && (Date.parse(value.expiresAt as string) <= Date.parse(value.completedAt) || Date.parse(value.expiresAt as string) > Date.parse(value.completedAt) + 6 * 3600000)) throw new Error('Invalid analytics snapshot freshness')
      }
    }
  }
  return { ...context, source: 'marketing_analytics_sync', enabled: value.enabled, status: value.status, steps, ...(value.jobId === undefined ? {} : { jobId: value.jobId as string }), ...Object.fromEntries(times.filter((key) => value[key] !== undefined).map((key) => [key, value[key]])) }
}
async function request(context: AnalyticsSyncContext, method: 'GET' | 'POST' | 'PATCH', signal?: AbortSignal, enabled?: boolean): Promise<AnalyticsSyncState> {
  const selected = validateContext(context), query = new URLSearchParams({ websiteId: selected.websiteId, site: selected.site, quarter: selected.quarter })
  if (method === 'PATCH' && typeof enabled !== 'boolean') throw new Error('Invalid analytics sync configuration')
  const path = marketingAnalyticsSyncContracts.read.path
  const response = await fetch(method === 'GET' ? `${path}?${query}` : path, { method, signal, credentials: 'include', redirect: 'error', headers: { Accept: 'application/json', ...(method === 'GET' ? {} : { 'Content-Type': 'application/json' }) }, ...(method === 'GET' ? {} : { body: JSON.stringify({ ...selected, ...(method === 'PATCH' ? { enabled } : {}) }) }) })
  if (response.status !== (method === 'POST' ? 202 : 200)) throw new Error(response.status === 409 ? 'analytics_sync_conflict' : 'analytics_sync_unavailable')
  const body: unknown = await response.json()
  return sanitizeAnalyticsSync(record(body) ? body.data : undefined, selected)
}
export const loadAnalyticsSync = (context: AnalyticsSyncContext, signal?: AbortSignal) => request(context, 'GET', signal)
export const requestAnalyticsSync = (context: AnalyticsSyncContext, signal?: AbortSignal) => request(context, 'POST', signal)
export const setAnalyticsSyncEnabled = (context: AnalyticsSyncContext, enabled: boolean, signal?: AbortSignal) => request(context, 'PATCH', signal, enabled)
