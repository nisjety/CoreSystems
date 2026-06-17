import type { NotificationEventType } from './types'

/** Human-readable labels for each notification event type. */
export const EVENT_TYPE_LABELS: Record<NotificationEventType, string> = {
  team_invite_sent: 'Team invitation',
  user_mentioned: 'Mention',
  crawl_completed: 'Crawl completed',
  document_indexed: 'Document indexed',
  'control_session.entitlements_changed': 'Entitlements changed',
}

/** Maps event types to their NATS publishing subjects.
 *  Used by any CoreSystem service that needs to publish a notification. */
const EVENT_NATS_SUBJECTS: Record<NotificationEventType, string> = {
  team_invite_sent: 'notifications.team.invite.sent',
  user_mentioned: 'notifications.user.mentioned',
  crawl_completed: 'notifications.crawl.completed',
  document_indexed: 'notifications.document.indexed',
  'control_session.entitlements_changed': 'notifications.control.session.entitlements.changed',
}

export const ALL_EVENT_TYPES: NotificationEventType[] = [
  'team_invite_sent',
  'user_mentioned',
  'crawl_completed',
  'document_indexed',
  'control_session.entitlements_changed',
]
