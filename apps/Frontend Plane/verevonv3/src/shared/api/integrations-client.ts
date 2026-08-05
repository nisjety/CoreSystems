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
   * `direction` marks read (into Verevon) vs write (out to the provider). */
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
  /** The provider-confirmed mailbox identity. It is used only to label the
   * operator's own connected account; customer addresses never enter sidebar
   * navigation. */
  userEmail?: string
  providerId: string
  /** integration-core's canonical provider key. `providerId` is retained as
   * the normalized frontend alias for older consumers. */
  providerKey?: string
  connectorType?: string
  providerName?: string
  displayName?: string
  providerAccountId?: string
  /** Provider-confirmed account email, when the provider grants it. */
  providerEmail?: string
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
  /** Newer integration-core sessions expose the COOP-safe direct OAuth
   * contract. Keep the older fields above for onboarding callers. */
  authMode?: 'direct-oauth'
  connectUrl?: string
  sessionToken?: string
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
    headers: { 'x-verevon-org-id': orgId },
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
      headers: { 'x-verevon-org-id': orgId },
    },
  )
}

export function getConnectSessionStatus(
  orgId: string,
  id: string,
): Promise<ConnectSessionStatus> {
  return requestJson<ConnectSessionStatus>(
    `/api/v1/integrations/connect-sessions/${encodeURIComponent(id)}/status`,
    { headers: { 'x-verevon-org-id': orgId } },
  )
}

export async function listConnections(orgId: string): Promise<IntegrationConnection[]> {
  const payload = await requestJson<unknown>('/api/v1/integrations/connections', {
    headers: { 'x-verevon-org-id': orgId },
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
    const providerEmail = stringField(row.providerEmail ?? row.provider_email)
      || stringField(metadata?.mailbox_address ?? metadata?.email)
    return [{
      id,
      userId: stringField(row.userId ?? row.user_id) || undefined,
      userEmail: stringField(row.userEmail ?? row.user_email) || undefined,
      providerId: providerKey,
      providerKey,
      connectorType: stringField(row.connectorType ?? row.connector_type) || undefined,
      providerName: stringField(row.providerName ?? row.provider_name) || providerKey,
      displayName: displayName || undefined,
      providerAccountId: stringField(row.providerAccountId ?? row.provider_account_id) || undefined,
      providerEmail: providerEmail || undefined,
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
    { headers: { 'x-verevon-org-id': orgId } },
  )
}

export function disconnectConnection(orgId: string, id: string): Promise<void> {
  return requestJson<void>(`/api/v1/integrations/connections/${encodeURIComponent(id)}`, {
    method: 'DELETE',
    headers: { 'x-verevon-org-id': orgId },
  })
}

export function triggerSync(orgId: string, id: string): Promise<{ syncJob: SyncJob }> {
  return requestJson<{ syncJob: SyncJob }>(
    `/api/v1/integrations/connections/${encodeURIComponent(id)}/sync`,
    {
      method: 'POST',
      headers: { 'x-verevon-org-id': orgId },
    },
  )
}

/** Queues a bounded provider inbox fetch. This is distinct from the generic
 * integration sync route: the resulting job is claimed by the inbox worker
 * and only its terminal receipt can prove the Support queue was refreshed. */
export function triggerInboxSync(
  orgId: string,
  id: string,
  channel: 'email' | 'teams' | 'slack',
): Promise<{ syncJob: SyncJob }> {
  return requestJson<{ syncJob: SyncJob }>(
    `/api/v1/integrations/connections/${encodeURIComponent(id)}/inbox-sync`,
    {
      method: 'POST',
      body: JSON.stringify({ channel }),
      headers: { 'x-verevon-org-id': orgId },
    },
  )
}

export async function extendInboxHistory(orgId: string, id: string): Promise<InboxHistoryRequest> {
  const payload = await requestJson<InboxHistoryRequest | { history: InboxHistoryRequest }>(
    `/api/v1/integrations/connections/${encodeURIComponent(id)}/inbox-history`,
    {
      method: 'POST',
      headers: { 'x-verevon-org-id': orgId },
    },
  )
  return 'history' in payload ? payload.history : payload
}

export function listSyncJobs(orgId: string): Promise<SyncJob[]> {
  return requestJson<SyncJob[]>('/api/v1/integrations/sync-jobs', {
    headers: { 'x-verevon-org-id': orgId },
  })
}

export async function getSyncJob(orgId: string, id: string): Promise<SyncJob> {
  const payload = await requestJson<SyncJob | { syncJob: SyncJob }>(
    `/api/v1/integrations/sync-jobs/${encodeURIComponent(id)}`,
    { headers: { 'x-verevon-org-id': orgId } },
  )
  return 'syncJob' in payload ? payload.syncJob : payload
}

export function getIntegrationProfile(orgId: string): Promise<IntegrationProfile> {
  return requestJson<IntegrationProfile>('/api/v1/integrations/profile', {
    headers: { 'x-verevon-org-id': orgId },
  })
}
