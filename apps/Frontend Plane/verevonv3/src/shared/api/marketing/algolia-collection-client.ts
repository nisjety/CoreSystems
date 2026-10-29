import { marketingSiteSourcesContracts as contracts } from '../../actions/marketing-site-sources'
import { validateDiscoveryContext, type DiscoveryContext } from './discovery-client'
export type AlgoliaCollection = DiscoveryContext & {
  provider: 'algolia'; status: 'idle' | 'queued' | 'syncing' | 'ready' | 'needs_attention' | 'reconnect_required'
  nextRunAt?: string; lastAttemptAt?: string; lastSuccessAt?: string
  reason?: 'partial_report' | 'provider_unavailable' | 'not_configured' | 'reconnect_required' | 'context_changed'
}
const record = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)
const timestamp = (v: unknown): v is string => typeof v === 'string' && v.length <= 40 && /^\d{4}-\d\d-\d\dT(?:[01]\d|2[0-3]):[0-5]\d:[0-5]\d(?:\.\d{1,9})?(?:Z|[+-](?:[01]\d|2[0-3]):[0-5]\d)$/.test(v) && Number.isFinite(Date.parse(v)) && new Date(`${v.slice(0,10)}T00:00:00Z`).toISOString().slice(0,10) === v.slice(0,10)
export function sanitizeAlgoliaCollection(value: unknown, expected: DiscoveryContext): AlgoliaCollection {
  const selected = validateDiscoveryContext(expected), current = validateDiscoveryContext(value)
  if (!record(value) || selected.websiteId !== current.websiteId || selected.site !== current.site || selected.websiteRevision !== current.websiteRevision || value.provider !== 'algolia' || !['idle','queued','syncing','ready','needs_attention','reconnect_required'].includes(String(value.status)) || value.reason !== undefined && !['partial_report','provider_unavailable','not_configured','reconnect_required','context_changed'].includes(String(value.reason))) throw new Error('Invalid Algolia collection receipt')
  const fields = ['nextRunAt','lastAttemptAt','lastSuccessAt'] as const
  if (fields.some(key => value[key] !== undefined && !timestamp(value[key]))) throw new Error('Invalid Algolia collection timestamp')
  return { ...current, provider: 'algolia', status: value.status as AlgoliaCollection['status'], ...Object.fromEntries(fields.filter(key => value[key] !== undefined).map(key => [key,value[key]])), ...(value.reason === undefined ? {} : { reason: value.reason as AlgoliaCollection['reason'] }) }
}
async function request(context: DiscoveryContext, queue: boolean, signal?: AbortSignal) {
  const current = validateDiscoveryContext(context), query = new URLSearchParams({ websiteId: current.websiteId, site: current.site, provider: 'algolia' })
  const response = await fetch(queue ? contracts.queue.path : `${contracts.collection.path}?${query}`, { method: queue ? 'POST' : 'GET', credentials: 'include', redirect: 'error', signal, headers: { Accept: 'application/json', ...(queue ? { 'Content-Type': 'application/json' } : {}) }, ...(queue ? { body: JSON.stringify({ ...current, provider: 'algolia' }) } : {}) })
  if (response.status !== (queue ? 202 : 200)) throw new Error('algolia_collection_unavailable')
  const value: unknown = await response.json()
  return sanitizeAlgoliaCollection(record(value) ? value.data : undefined, current)
}
export const loadAlgoliaCollection = (context: DiscoveryContext, signal?: AbortSignal) => request(context, false, signal)
export const queueAlgoliaCollection = (context: DiscoveryContext, signal?: AbortSignal) => request(context, true, signal)
