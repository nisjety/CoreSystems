import { requestJson } from './http'

/**
 * Webhook subscriptions for change alerts (Quarry INNH-5), through the gateway's
 * `/api/ingestions/webhooks*` routes. When a monitored page changes, Quarry POSTs a
 * signed body to the subscription's URL. The signing secret comes back from create and
 * from rotate only: the caller shows it once and must not store it.
 */

/** The one event a subscription can name today. Quarry's list is closed. */
export const WEBHOOK_EVENT = 'change_detected'

export interface WebhookLastDelivery {
  status: string
  at: string
  eventId: string
}

export interface WebhookSubscription {
  id: string
  url: string
  events: string[]
  active: boolean
  description: string | null
  secretHint: string
  createdAt: string
  lastDelivery: WebhookLastDelivery | null
}

/** A subscription just created. `secret` is shown once and never read back. */
export interface WebhookCreated extends WebhookSubscription {
  secret: string
}

/** A new secret after a rotation. The previous secret stops verifying at once. */
export interface WebhookSecret {
  id: string
  secret: string
  secretHint: string
}

/** One attempt to deliver an event to a subscription. There is no payload in it. */
export interface WebhookDelivery {
  id: string
  eventId: string
  eventType: string
  status: string
  attempt: number
  nextAttemptAt: string | null
  lastError: string | null
  createdAt: string
}

interface WebhookPage<T> {
  items: T[]
  nextCursor: string | null
}

const base = '/api/ingestions/webhooks'

function at(id: string): string {
  return `${base}/${encodeURIComponent(id)}`
}

/** The organization's subscriptions. Quarry allows at most ten, so one page holds them all. */
export async function listWebhooks(signal?: AbortSignal): Promise<WebhookSubscription[]> {
  const page = await requestJson<WebhookPage<WebhookSubscription>>(base, { signal })
  return Array.isArray(page?.items) ? page.items : []
}

/** Subscribe `url` to change alerts. The returned `secret` is the only copy there will be. */
export function createWebhook(input: { url: string; description?: string }, signal?: AbortSignal): Promise<WebhookCreated> {
  return requestJson<WebhookCreated>(base, {
    method: 'POST',
    body: JSON.stringify(input),
    signal,
  })
}

/** Pause or resume a subscription, or change its description. The URL and the event cannot change. */
export function updateWebhook(
  id: string,
  patch: { active?: boolean; description?: string },
  signal?: AbortSignal,
): Promise<WebhookSubscription> {
  return requestJson<WebhookSubscription>(at(id), {
    method: 'PATCH',
    body: JSON.stringify(patch),
    signal,
  })
}

/** Remove a subscription and its delivery history. */
export async function deleteWebhook(id: string, signal?: AbortSignal): Promise<void> {
  await requestJson<unknown>(at(id), { method: 'DELETE', signal })
}

/** A new signing secret for the subscription. The old one stops verifying at once. */
export function rotateWebhookSecret(id: string, signal?: AbortSignal): Promise<WebhookSecret> {
  return requestJson<WebhookSecret>(`${at(id)}/rotate-secret`, { method: 'POST', signal })
}

/** The newest deliveries of one subscription, newest first. */
export async function listWebhookDeliveries(id: string, signal?: AbortSignal): Promise<WebhookDelivery[]> {
  const page = await requestJson<WebhookPage<WebhookDelivery>>(`${at(id)}/deliveries?limit=25`, { signal })
  return Array.isArray(page?.items) ? page.items : []
}

/** Queue a failed or dead-lettered delivery to be sent again. */
export function retryWebhookDelivery(id: string, deliveryId: string, signal?: AbortSignal): Promise<WebhookDelivery> {
  return requestJson<WebhookDelivery>(`${at(id)}/deliveries/${encodeURIComponent(deliveryId)}/retry`, {
    method: 'POST',
    signal,
  })
}
