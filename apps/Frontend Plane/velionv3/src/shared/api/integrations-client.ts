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
  providerId: string
  providerName?: string
  status: string
  lastSyncAt?: string
  createdAt: string
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

export function listConnections(orgId: string): Promise<IntegrationConnection[]> {
  return requestJson<IntegrationConnection[]>('/api/v1/integrations/connections', {
    headers: { 'x-velion-org-id': orgId },
  })
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
