import 'server-only'
import type {
  ChannelConfig,
  ChannelConfigList,
  Notification,
  NotificationChannel,
  NotificationEventType,
  NotificationFeed,
  Preference,
  UnreadCount,
  UnseenCount,
} from './types'

// ── Transport ─────────────────────────────────────────────────────────────────

function getServiceURL(): string {
  const url = process.env.NOTIFICATION_SERVICE_URL ?? 'http://notification-core:3140'
  return url.endsWith('/') ? url.slice(0, -1) : url
}

function getInternalKey(): string {
  // NOTIFICATION_INTERNAL_KEY takes precedence — notification-core is provisioned
  // with its own key, separate from the auth/org/user control-plane services.
  const key =
    process.env.NOTIFICATION_INTERNAL_KEY ??
    process.env.INTERNAL_API_KEY ??
    process.env.INTERNAL_SERVICE_SECRET
  if (!key) throw new Error('NOTIFICATION_INTERNAL_KEY (or INTERNAL_API_KEY) is not configured')
  return key
}

async function request<T>(
  path: string,
  userID: string,
  init: RequestInit = {},
): Promise<T> {
  const url = `${getServiceURL()}${path}`
  const headers = new Headers(init.headers)
  headers.set('x-internal-api-key', getInternalKey())
  headers.set('x-user-id', userID)
  headers.set('Content-Type', 'application/json')

  const res = await fetch(url, { ...init, headers, cache: 'no-store' })
  if (!res.ok) {
    const text = await res.text().catch(() => '')
    throw new Error(`notification-core ${res.status}: ${text}`)
  }
  const text = await res.text()
  return text ? (JSON.parse(text) as T) : (null as T)
}

async function internalRequest<T>(path: string, init: RequestInit = {}): Promise<T> {
  const url = `${getServiceURL()}${path}`
  const headers = new Headers(init.headers)
  headers.set('x-internal-api-key', getInternalKey())
  headers.set('Content-Type', 'application/json')

  const res = await fetch(url, { ...init, headers, cache: 'no-store' })
  if (!res.ok) {
    const text = await res.text().catch(() => '')
    throw new Error(`notification-core ${res.status}: ${text}`)
  }
  const text = await res.text()
  return text ? (JSON.parse(text) as T) : (null as T)
}

// ── Feed API ──────────────────────────────────────────────────────────────────

export async function listNotifications(
  userID: string,
  opts: { page?: number; limit?: number; read?: boolean; feed?: string; showArchived?: boolean } = {},
): Promise<NotificationFeed> {
  const params = new URLSearchParams()
  if (opts.page !== undefined) params.set('page', String(opts.page))
  if (opts.limit !== undefined) params.set('limit', String(opts.limit))
  if (opts.read !== undefined) params.set('read', String(opts.read))
  if (opts.feed) params.set('feed', opts.feed)
  if (opts.showArchived) params.set('showArchived', 'true')
  const qs = params.toString() ? `?${params.toString()}` : ''
  return request<NotificationFeed>(`/notifications${qs}`, userID)
}

export async function countUnread(userID: string): Promise<UnreadCount> {
  return request<UnreadCount>('/notifications/unread/count', userID)
}

export async function countUnseen(userID: string): Promise<UnseenCount> {
  return request<UnseenCount>('/notifications/unseen/count', userID)
}

export async function markRead(userID: string, notificationID: string): Promise<void> {
  await request(`/notifications/${encodeURIComponent(notificationID)}/read`, userID, {
    method: 'POST',
  })
}

export async function markAllRead(userID: string): Promise<void> {
  await request('/notifications/mark-all-read', userID, { method: 'POST' })
}

export async function markSeen(userID: string, notificationID: string): Promise<void> {
  await request(`/notifications/${encodeURIComponent(notificationID)}/seen`, userID, {
    method: 'POST',
  })
}

export async function markAllSeen(userID: string): Promise<void> {
  await request('/notifications/mark-all-seen', userID, { method: 'POST' })
}

export async function deleteNotification(
  userID: string,
  notificationID: string,
  permanent = false,
): Promise<void> {
  const qs = permanent ? '?permanent=true' : ''
  await request(`/notifications/${encodeURIComponent(notificationID)}${qs}`, userID, {
    method: 'DELETE',
  })
}

// ── Preferences API ───────────────────────────────────────────────────────────

export async function listPreferences(userID: string): Promise<Preference[]> {
  const data = await request<{ preferences: Preference[] }>('/preferences', userID)
  return data?.preferences ?? []
}

export async function setPreference(
  userID: string,
  eventType: NotificationEventType,
  channel: NotificationChannel,
  enabled: boolean,
): Promise<void> {
  await request(
    `/preferences/${encodeURIComponent(eventType)}/${encodeURIComponent(channel)}`,
    userID,
    { method: 'PUT', body: JSON.stringify({ enabled }) },
  )
}

// ── Channel config API ────────────────────────────────────────────────────────

export async function listChannelConfigs(): Promise<ChannelConfig[]> {
  const data = await internalRequest<ChannelConfigList>('/channels/config')
  return data.configs ?? []
}

export async function setChannelEnabled(
  eventType: NotificationEventType,
  channel: NotificationChannel,
  enabled: boolean,
): Promise<void> {
  await internalRequest(
    `/channels/config/${encodeURIComponent(eventType)}/${encodeURIComponent(channel)}`,
    { method: 'PATCH', body: JSON.stringify({ enabled }) },
  )
}

// ── Recipient management ──────────────────────────────────────────────────────

export async function upsertRecipient(params: {
  userId: string
  email?: string
  name?: string
}): Promise<void> {
  await internalRequest('/internal/recipients/upsert', {
    method: 'POST',
    body: JSON.stringify({
      user_id: params.userId,
      email: params.email ?? '',
      name: params.name ?? '',
    }),
  })
}

// ── Unused re-export kept for unused-import safety ────────────────────────────
export type { Notification, NotificationFeed, UnreadCount, UnseenCount, Preference }
