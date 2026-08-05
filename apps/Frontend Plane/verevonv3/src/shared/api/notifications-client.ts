import { requestJson } from './http'

export interface Notification {
  id: string
  userId: string
  type: string
  title: string
  body?: string
  read: boolean
  createdAt: string
  metadata?: Record<string, unknown>
}

export interface UnreadCount {
  count: number
}

export interface NotificationPreference {
  eventType: string
  channel: string
  enabled: boolean
}

export function listNotifications(): Promise<Notification[]> {
  return requestJson<Notification[]>('/api/v1/notifications')
}

export function getUnreadCount(): Promise<UnreadCount> {
  return requestJson<UnreadCount>('/api/v1/notifications/unread/count')
}

export function markRead(id: string): Promise<void> {
  return requestJson<void>(`/api/v1/notifications/${encodeURIComponent(id)}/read`, {
    method: 'POST',
  })
}

export function markAllRead(): Promise<void> {
  return requestJson<void>('/api/v1/notifications/mark-all-read', { method: 'POST' })
}

export function deleteNotification(id: string): Promise<void> {
  return requestJson<void>(`/api/v1/notifications/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  })
}

export function getNotificationPreferences(): Promise<NotificationPreference[]> {
  return requestJson<NotificationPreference[]>('/api/v1/notifications/preferences')
}

export function updateNotificationPreference(
  eventType: string,
  channel: string,
  enabled: boolean,
): Promise<NotificationPreference> {
  return requestJson<NotificationPreference>(
    `/api/v1/notifications/preferences/${encodeURIComponent(eventType)}/${encodeURIComponent(channel)}`,
    {
      method: 'PUT',
      body: JSON.stringify({ enabled }),
    },
  )
}
