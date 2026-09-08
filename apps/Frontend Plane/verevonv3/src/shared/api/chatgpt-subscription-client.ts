import { listConnections, type IntegrationConnection } from './integrations-client'
import { requestJson } from './http'

/** Canonical key persisted by Integration Core. Keep it out of generic source
 * integration controls: this connection gives Model Plane inference access,
 * it does not ingest a knowledge source. */
export const OPENAI_CODEX_SUBSCRIPTION_PROVIDER = 'openai-codex-subscription'

export type ChatGptSubscriptionConnection = {
  id: string
  providerKey: typeof OPENAI_CODEX_SUBSCRIPTION_PROVIDER
  displayName?: string
  status: string
  createdAt?: string
}

export type ChatGptDeviceLogin = {
  loginId: string
  connectionId: string
  verificationUrl: string
  userCode: string
  expiresAt?: string
  status?: 'pending' | 'connected' | 'failed' | 'expired'
}

export type ChatGptSubscriptionStart = {
  connection: ChatGptSubscriptionConnection
  login: ChatGptDeviceLogin
}

export type ChatGptSubscriptionStatus = {
  connection: ChatGptSubscriptionConnection
  // Integration Core does not repeat the device code or URL on status polls.
  login: Pick<ChatGptDeviceLogin, 'loginId' | 'connectionId'> & {
    status: NonNullable<ChatGptDeviceLogin['status']>
  }
}

function headers(orgId: string): HeadersInit | undefined {
  const trimmed = orgId.trim()
  return trimmed ? { 'x-verevon-org-id': trimmed } : undefined
}

export function startChatGptSubscription(orgId: string): Promise<ChatGptSubscriptionStart> {
  return requestJson<ChatGptSubscriptionStart>('/api/v1/model-subscriptions/openai-codex/connect', {
    method: 'POST',
    body: JSON.stringify({}),
    headers: headers(orgId),
  })
}

export function getChatGptSubscriptionStatus(
  orgId: string,
  connectionId: string,
  loginId: string,
): Promise<ChatGptSubscriptionStatus> {
  return requestJson<ChatGptSubscriptionStatus>(
    `/api/v1/model-subscriptions/openai-codex/connect/${encodeURIComponent(connectionId)}/${encodeURIComponent(loginId)}`,
    { headers: headers(orgId) },
  )
}

export function disconnectChatGptSubscription(orgId: string, connectionId: string): Promise<void> {
  return requestJson<void>(
    `/api/v1/model-subscriptions/openai-codex/connections/${encodeURIComponent(connectionId)}`,
    { method: 'DELETE', headers: headers(orgId) },
  )
}

export async function listChatGptSubscriptions(orgId: string): Promise<ChatGptSubscriptionConnection[]> {
  const connections = await listConnections(orgId)
  return connections.flatMap(toChatGptSubscription)
}

function toChatGptSubscription(connection: IntegrationConnection): ChatGptSubscriptionConnection[] {
  if (connection.providerKey !== OPENAI_CODEX_SUBSCRIPTION_PROVIDER || connection.deletedAt) return []
  return [{
    id: connection.id,
    providerKey: OPENAI_CODEX_SUBSCRIPTION_PROVIDER,
    displayName: connection.displayName,
    status: connection.status,
    createdAt: connection.createdAt || undefined,
  }]
}
