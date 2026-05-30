/**
 * NATS subject naming convention:
 *   Cross-plane (velion-nats): velion.<plane>.<domain>.<action>
 *   Intra-plane (local NATS):  ingestion.<domain>.<action>
 */
export const integrationSubjects = {
  connectionCreated: 'velion.ingestion.integration.connection_created',
  connectionUpdated: 'velion.ingestion.integration.connection_updated',
  connectionDeleted: 'velion.ingestion.integration.connection_deleted',
  connectionAuthFailed: 'velion.ingestion.integration.connection_auth_failed',
  syncStarted: 'velion.ingestion.integration.sync_started',
  syncCompleted: 'velion.ingestion.integration.sync_completed',
  syncFailed: 'velion.ingestion.integration.sync_failed'
} as const;

/** Ticket lifecycle events published after Zammad webhook ingestion. */
export const supportSubjects = {
  ticketCreated:  'velion.support.ticket.created',
  ticketUpdated:  'velion.support.ticket.updated',
  ticketAssigned: 'velion.support.ticket.assigned',
  articleAdded:   'velion.support.article.added',
  slaBreach:      'velion.support.sla.breach',
} as const;

/** Novu notification lifecycle events published after webhook intake. */
export const notificationSubjects = {
  novuEvent: 'velion.notifications.novu.event',
  novuSent: 'velion.notifications.novu.sent',
  novuFailed: 'velion.notifications.novu.failed',
  novuRead: 'velion.notifications.novu.read',
  novuSubscriberCreated: 'velion.notifications.novu.subscriber_created',
  novuSubscriberUpdated: 'velion.notifications.novu.subscriber_updated',
} as const;

export type IntegrationSubject = (typeof integrationSubjects)[keyof typeof integrationSubjects];
export type SupportSubject = (typeof supportSubjects)[keyof typeof supportSubjects];
export type NotificationSubject = (typeof notificationSubjects)[keyof typeof notificationSubjects];

/** Union of all subjects this service may publish to velion-nats. */
export type VelionSubject = IntegrationSubject | SupportSubject | NotificationSubject;
