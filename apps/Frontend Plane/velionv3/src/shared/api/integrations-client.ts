import { requestJson } from './http'

export interface IntegrationProvider {
  id: string
  name: string
  type: string
  description?: string
  logoUrl?: string
  requiresPlan?: string
  /** integration-core groups providers by category (e.g. "source", "social"). */
  category?: string
  /** Declared capabilities, used as a permissions fallback when a connection
   * exposes no granted scopes. integration-core returns objects keyed by `key`;
   * `direction` marks read (into Velion) vs write (out to the provider). */
  capabilities?: Array<{
    key: string
    label?: string
    description?: string
    direction?: 'read' | 'write'
    scopes?: string[]
    sensitive?: boolean
  }>
}

export interface IntegrationConnection {
  id: string
  userId?: string
  providerId: string
  /** integration-core's canonical provider key. `providerId` is retained as
   * the normalized frontend alias for older consumers. */
  providerKey?: string
  connectorType?: string
  providerName?: string
  displayName?: string
  status: string
  lastSyncAt?: string
  lastSyncStatus?: string
  createdAt: string
  deletedAt?: string
  metadata?: Record<string, unknown>
  /** OAuth scopes granted on this connection (what the app is permitted to access). */
  scopes?: string[]
  /** Number of granted scopes, when the upstream summarizes rather than enumerates. */
  scopeCount?: number
  /** Capability labels active on this connection. */
  capabilities?: string[]
  /** Data classification for retention/handling (e.g. "public" | "organization" | "customer"). */
  dataClass?: string
  /** Retention label/policy for cached data, when the provider declares one. */
  retention?: string
}

export interface ConnectSession {
  id: string
  providerId: string
  redirectUrl: string
  expiresAt: string
}

export interface ConnectSessionStatus {
  id: string
  status: 'pending' | 'completed' | 'failed' | 'expired'
  connectionId?: string
  error?: string
}

export interface SyncJob {
  id: string
  connectionId: string
  status: string
  startedAt?: string
  completedAt?: string
  recordsProcessed?: number
  error?: string
}

export interface InboxHistoryRequest {
  channel: 'teams'
  historyDays: number
  queued: boolean
}

export interface IntegrationProfile {
  orgId: string
  connectedProviders: string[]
  lastSyncAt?: string
  totalDocuments?: number
}

export function listProviders(orgId: string): Promise<IntegrationProvider[]> {
  return requestJson<IntegrationProvider[]>('/api/v1/integrations/providers', {
    headers: { 'x-velion-org-id': orgId },
  })
}

export function startConnectSession(
  orgId: string,
  provider: string,
  body?: Record<string, unknown>,
): Promise<ConnectSession> {
  return requestJson<ConnectSession>(
    `/api/v1/integrations/providers/${encodeURIComponent(provider)}/connect-session`,
    {
      method: 'POST',
      body: JSON.stringify(body ?? {}),
      headers: { 'x-velion-org-id': orgId },
    },
  )
}

export function getConnectSessionStatus(
  orgId: string,
  id: string,
): Promise<ConnectSessionStatus> {
  return requestJson<ConnectSessionStatus>(
    `/api/v1/integrations/connect-sessions/${encodeURIComponent(id)}/status`,
    { headers: { 'x-velion-org-id': orgId } },
  )
}

export async function listConnections(orgId: string): Promise<IntegrationConnection[]> {
  const payload = await requestJson<unknown>('/api/v1/integrations/connections', {
    headers: { 'x-velion-org-id': orgId },
  })
  const rows = Array.isArray(payload)
    ? payload
    : payload && typeof payload === 'object' && Array.isArray((payload as { connections?: unknown }).connections)
      ? (payload as { connections: unknown[] }).connections
      : []

  return rows.flatMap((value) => {
    if (!value || typeof value !== 'object') return []
    const row = value as Record<string, unknown>
    const id = stringField(row.id)
    const providerKey = stringField(row.providerKey ?? row.provider_key ?? row.providerId)
    if (!id || !providerKey) return []
    const displayName = stringField(row.displayName ?? row.display_name)
    const metadata = objectField(row.metadata) ?? objectField(row.providerContext ?? row.provider_context)
    return [{
      id,
      userId: stringField(row.userId ?? row.user_id) || undefined,
      providerId: providerKey,
      providerKey,
      connectorType: stringField(row.connectorType ?? row.connector_type) || undefined,
      providerName: stringField(row.providerName ?? row.provider_name) || providerKey,
      displayName: displayName || undefined,
      status: stringField(row.status) || 'unknown',
      lastSyncAt: stringField(row.lastSyncAt ?? row.last_sync_at) || undefined,
      lastSyncStatus: stringField(row.lastSyncStatus ?? row.last_sync_status) || undefined,
      createdAt: stringField(row.createdAt ?? row.created_at),
      deletedAt: stringField(row.deletedAt ?? row.deleted_at) || undefined,
      metadata: metadata ?? undefined,
      scopes: stringArray(row.scopes),
      scopeCount: typeof row.scopeCount === 'number' ? row.scopeCount : stringArray(row.scopes).length,
      capabilities: stringArray(row.capabilities),
      dataClass: stringField(row.dataClass ?? row.data_class) || undefined,
      retention: stringField(row.retention) || undefined,
    }]
  })
}

function stringField(value: unknown): string {
  return typeof value === 'string' ? value.trim() : ''
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === 'string' && item.trim().length > 0)
    : []
}

function objectField(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null
}

export function getConnection(orgId: string, id: string): Promise<IntegrationConnection> {
  return requestJson<IntegrationConnection>(
    `/api/v1/integrations/connections/${encodeURIComponent(id)}`,
    { headers: { 'x-velion-org-id': orgId } },
  )
}

export function disconnectConnection(orgId: string, id: string): Promise<void> {
  return requestJson<void>(`/api/v1/integrations/connections/${encodeURIComponent(id)}`, {
    method: 'DELETE',
    headers: { 'x-velion-org-id': orgId },
  })
}

export function triggerSync(orgId: string, id: string): Promise<SyncJob> {
  return requestJson<SyncJob>(
    `/api/v1/integrations/connections/${encodeURIComponent(id)}/sync`,
    {
      method: 'POST',
      headers: { 'x-velion-org-id': orgId },
    },
  )
}

export async function extendInboxHistory(orgId: string, id: string): Promise<InboxHistoryRequest> {
  const payload = await requestJson<InboxHistoryRequest | { history: InboxHistoryRequest }>(
    `/api/v1/integrations/connections/${encodeURIComponent(id)}/inbox-history`,
    {
      method: 'POST',
      headers: { 'x-velion-org-id': orgId },
    },
  )
  return 'history' in payload ? payload.history : payload
}

export function listSyncJobs(orgId: string): Promise<SyncJob[]> {
  return requestJson<SyncJob[]>('/api/v1/integrations/sync-jobs', {
    headers: { 'x-velion-org-id': orgId },
  })
}

export function getSyncJob(orgId: string, id: string): Promise<SyncJob> {
  return requestJson<SyncJob>(
    `/api/v1/integrations/sync-jobs/${encodeURIComponent(id)}`,
    { headers: { 'x-velion-org-id': orgId } },
  )
}

export function getIntegrationProfile(orgId: string): Promise<IntegrationProfile> {
  return requestJson<IntegrationProfile>('/api/v1/integrations/profile', {
    headers: { 'x-velion-org-id': orgId },
  })
}
