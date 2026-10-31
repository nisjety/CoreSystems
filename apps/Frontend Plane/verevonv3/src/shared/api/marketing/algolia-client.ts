import { marketingAlgoliaContracts as contracts } from '../../actions/marketing-algolia'
import { validateDiscoveryContext, type DiscoveryContext } from './discovery-client'
export type AlgoliaMapping = DiscoveryContext & { status: 'not_mapped' | 'mapped' | 'reconnect_required' | 'access_check_required'; connectionId?: string; applicationId?: string; indexName?: string }
export type AlgoliaSelection = { connectionId: string; applicationId: string; indexName: string }
export type AlgoliaGrant = { id: string; status: 'active' | 'reconnect_required' | 'inactive'; displayName?: string }
export type AlgoliaSession = { id: string; authorizationUrl: string; expiresAt: string }
export type AlgoliaSessionStatus = { id: string; status: 'pending' | 'completed' | 'failed' | 'expired'; expiresAt: string }
const record = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value)
const connection = (value: unknown): value is string => typeof value === 'string' && /^conn_[A-Za-z0-9_-]{1,100}$/.test(value)
const application = (value: unknown): value is string => typeof value === 'string' && /^[A-Za-z0-9]{1,64}$/.test(value)
const index = (value: unknown): value is string => typeof value === 'string' && /^[A-Za-z0-9_.-]{1,128}$/.test(value)
const sessionId = (value: unknown): value is string => typeof value === 'string' && /^cs_[A-Za-z0-9_-]{1,100}$/.test(value)
const stamp = (value: unknown): value is string => typeof value === 'string' && value.length <= 40 && /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d{1,9})?(?:Z|[+-]\d\d:\d\d)$/.test(value) && Number.isFinite(Date.parse(value)) && new Date(`${value.slice(0, 10)}T00:00:00Z`).toISOString().slice(0, 10) === value.slice(0, 10)
const text = (value: unknown): value is string => typeof value === 'string' && value.length > 0 && value.length <= 200 && !/[\x00-\x1f\x7f-\x9f]/.test(value)
export function algoliaAuthorizationUrl(value: unknown): string {
  if (typeof value !== 'string' || value.length > 4096 || /[\x00-\x20\x7f]/.test(value)) throw new Error('Invalid Algolia authorization URL')
  const url = new URL(value), q = url.searchParams
  const allowed = ['response_type', 'client_id', 'redirect_uri', 'scope', 'state', 'code_challenge', 'code_challenge_method', 'resource']
  if (url.origin !== 'https://dashboard.algolia.com' || url.pathname !== '/2/oauth/authorize' || url.username || url.password || url.hash || [...q.keys()].some((key) => !allowed.includes(key)) || allowed.some((key) => q.getAll(key).length !== 1) || q.get('response_type') !== 'code' || q.get('scope') !== 'public' || q.get('resource') !== 'https://mcp.algolia.com/mcp' || q.get('redirect_uri') !== 'http://localhost:3026/api/v1/providers/algolia/callback' || q.get('code_challenge_method') !== 'S256' || !/^[A-Za-z0-9_-]{43,128}$/.test(q.get('code_challenge') ?? '') || !/^[A-Za-z0-9._~-]{1,256}$/.test(q.get('state') ?? '') || !text(q.get('client_id'))) throw new Error('Invalid Algolia authorization contract')
  return value
}
export function sanitizeAlgoliaMapping(value: unknown, expected: DiscoveryContext): AlgoliaMapping {
  const selected = validateDiscoveryContext(expected), current = validateDiscoveryContext(value)
  if (!record(value) || current.websiteId !== selected.websiteId || current.site !== selected.site || current.websiteRevision !== selected.websiteRevision || typeof value.status !== 'string' || !['not_mapped', 'mapped', 'reconnect_required', 'access_check_required'].includes(value.status)) throw new Error('Invalid Algolia site mapping')
  if (value.status === 'not_mapped') {
    if (['connectionId', 'applicationId', 'indexName'].some((key) => value[key] !== undefined)) throw new Error('Invalid unmapped Algolia site')
    return { ...current, status: 'not_mapped' }
  }
  if (!connection(value.connectionId) || !application(value.applicationId) || !index(value.indexName)) throw new Error('Invalid Algolia mapping identity')
  return { ...current, status: value.status as AlgoliaMapping['status'], connectionId: value.connectionId, applicationId: value.applicationId, indexName: value.indexName }
}
async function request(path: string, method: 'GET' | 'POST' | 'PUT' | 'DELETE', signal?: AbortSignal, body?: unknown): Promise<unknown> {
  const response = await fetch(path, { method, credentials: 'include', redirect: 'error', signal, headers: { Accept: 'application/json', ...(body === undefined ? {} : { 'Content-Type': 'application/json' }) }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) })
  if (response.status !== 200) throw new Error('algolia_unavailable')
  const value: unknown = await response.json()
  if (!record(value)) throw new Error('Invalid Algolia envelope')
  return value.data
}
export async function loadAlgoliaMapping(context: DiscoveryContext, signal?: AbortSignal) {
  const selected = validateDiscoveryContext(context), query = new URLSearchParams({ websiteId: selected.websiteId, site: selected.site })
  return sanitizeAlgoliaMapping(await request(`${contracts.mapping.path}?${query}`, 'GET', signal), selected)
}
export async function saveAlgoliaMapping(context: DiscoveryContext, selected: AlgoliaSelection, signal?: AbortSignal) {
  const current = validateDiscoveryContext(context)
  if (!connection(selected.connectionId) || !application(selected.applicationId) || !index(selected.indexName)) throw new Error('Invalid Algolia mapping choice')
  return sanitizeAlgoliaMapping(await request(contracts.save.path, 'PUT', signal, { ...current, connectionId: selected.connectionId, applicationId: selected.applicationId, indexName: selected.indexName }), current)
}
export async function disconnectAlgoliaSite(context: DiscoveryContext, signal?: AbortSignal) {
  const current = validateDiscoveryContext(context)
  return sanitizeAlgoliaMapping(await request(contracts.disconnectSite.path, 'DELETE', signal, current), current)
}
export async function loadAlgoliaConnections(signal?: AbortSignal): Promise<AlgoliaGrant[]> {
  const value = await request(contracts.connections.path, 'GET', signal)
  if (!record(value) || !Array.isArray(value.connections) || value.connections.length > 64) throw new Error('Invalid Algolia grants')
  const grants = value.connections.map((row: unknown): AlgoliaGrant => {
    if (!record(row) || !connection(row.id) || !['active', 'reconnect_required', 'inactive'].includes(String(row.status)) || typeof row.status !== 'string' || row.displayName !== undefined && !text(row.displayName)) throw new Error('Invalid Algolia grant')
    return { id: row.id, status: row.status as AlgoliaGrant['status'], ...(row.displayName === undefined ? {} : { displayName: row.displayName as string }) }
  })
  if (new Set(grants.map((row) => row.id)).size !== grants.length) throw new Error('Duplicate Algolia grant')
  return grants
}
async function choices(id: string, app: string | undefined, signal?: AbortSignal): Promise<string[]> {
  if (!connection(id) || app !== undefined && !application(app)) throw new Error('Invalid Algolia discovery choice')
  const key = app === undefined ? 'applications' : 'indices', field = app === undefined ? 'id' : 'name'
  const value = await request(`/api/v1/integrations/algolia/connections/${encodeURIComponent(id)}/${key}${app === undefined ? '' : `?${new URLSearchParams({ applicationId: app })}`}`, 'GET', signal)
  if (!record(value) || !Array.isArray(value[key]) || value[key].length > (app === undefined ? 128 : 5000)) throw new Error('Invalid Algolia discovery metadata')
  const result = (value[key] as unknown[]).map((row) => {
    if (!record(row) || !(app === undefined ? application(row[field]) : index(row[field]))) throw new Error('Invalid Algolia resource')
    return row[field] as string
  })
  if (new Set(result).size !== result.length) throw new Error('Duplicate Algolia resource')
  return result
}
export const loadAlgoliaApplications = (id: string, signal?: AbortSignal) => choices(id, undefined, signal)
export const loadAlgoliaIndices = (id: string, app: string, signal?: AbortSignal) => choices(id, app, signal)
export async function startAlgoliaSession(returnUrl: string, signal?: AbortSignal): Promise<AlgoliaSession> {
  const value = await request(contracts.connect.path, 'POST', signal, { returnUrl })
  if (!record(value) || !sessionId(value.id) || !stamp(value.expiresAt)) throw new Error('Invalid Algolia consent session')
  return { id: value.id, authorizationUrl: algoliaAuthorizationUrl(value.authorizationUrl), expiresAt: value.expiresAt }
}
export async function loadAlgoliaSession(id: string, signal?: AbortSignal): Promise<AlgoliaSessionStatus> {
  if (!sessionId(id)) throw new Error('Invalid Algolia session identity')
  const value = await request(`/api/v1/integrations/algolia/connect-sessions/${encodeURIComponent(id)}/status`, 'GET', signal)
  if (!record(value) || value.id !== id || typeof value.status !== 'string' || !['pending', 'completed', 'failed', 'expired'].includes(value.status) || !stamp(value.expiresAt)) throw new Error('Invalid Algolia consent status')
  return { id, status: value.status as AlgoliaSessionStatus['status'], expiresAt: value.expiresAt }
}
