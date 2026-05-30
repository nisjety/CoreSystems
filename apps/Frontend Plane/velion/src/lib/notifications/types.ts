// ── Domain types for Aqencia's first-party notification system ────────────────
// No Novu vocabulary. These types match the notification-core Go service API.

export type NotificationChannel = 'in_app' | 'email'

export type NotificationEventType =
  | 'team_invite_sent'
  | 'user_mentioned'
  | 'crawl_completed'
  | 'document_indexed'
  // G44 (velion-gap.md §8.30): notification-core's control-session subscriber
  // dispatches this type when CP session-core publishes
  // `app.session.entitlements_changed` (plan upgrade, org switch, billing
  // webhook ack). The Go constant for the same value is
  // `subscribers.NotificationTypeEntitlementsChanged` in
  // `Application Plane/notification-core/internal/subscribers/control_session.go`.
  | 'control_session.entitlements_changed'

export type DeliveryStatus = 'pending' | 'sent' | 'failed'

export interface Notification {
  id: string
  recipient_id: string
  event_type: NotificationEventType
  channel: NotificationChannel
  title: string
  body: string
  action_url?: string
  payload?: Record<string, unknown>
  read: boolean
  read_at?: string
  seen: boolean
  seen_at?: string
  feed: string
  archived: boolean
  archived_at?: string
  delivery_status: DeliveryStatus
  delivery_attempts: number
  actor_id?: string
  actor_name?: string
  actor_avatar?: string
  created_at: string
}

export interface NotificationFeed {
  notifications: Notification[]
  total_count: number
  has_more: boolean
}

export interface UnreadCount {
  count: number
}

export interface UnseenCount {
  count: number
}

export interface ChannelConfig {
  event_type: NotificationEventType
  channel: NotificationChannel
  enabled: boolean
}

export interface ChannelConfigList {
  configs: ChannelConfig[]
}

export interface Preference {
  recipient_id: string
  event_type: NotificationEventType
  channel: NotificationChannel
  enabled: boolean
}
