import { config } from '../config';
import {
  buildNotificationIdempotencyKey,
  postNotification,
} from './notification-client';

export interface NotifyAgentInput {
  /** Notification event type, e.g. 'ticket.triaged', 'sla.warning', 'sla.breach'. */
  type: string;
  ticketId: number;
  /** Zammad owner user ID, or 'broadcast' for un-assigned tickets. */
  recipientId: number | 'broadcast';
  /** Control Plane scope resolved by an authoritative Zammad→Control mapping. */
  organizationId?: string;
  controlUserId?: string;
  payload: {
    ticketId: number;
    message: string;
  };
}

/**
 * Posts an internal notification to notification-core.
 */
export async function notifyAgentActivity(
  input: NotifyAgentInput,
): Promise<void> {
  if (config.SUPPORT_NOTIFICATION_MODE === 'disabled') {
    console.warn('[support-worker] agent notification skipped: integration intentionally disabled');
    return;
  }
  if (!input.organizationId?.trim() || !input.controlUserId?.trim()) {
    throw new Error('support notification requires authoritative organizationId and controlUserId');
  }
  await postNotification({
    baseUrl: config.NOTIFICATION_CORE_URL,
    serviceToken: config.NOTIFICATION_SUPPORT_WORKER_SERVICE_TOKEN,
    request: {
      organization_id: input.organizationId,
      idempotency_key: buildNotificationIdempotencyKey({
        type: input.type,
        ticketId: input.ticketId,
        controlUserId: input.controlUserId,
      }),
      retention_mode: 'zdr',
      recipient: { kind: 'user', id: input.controlUserId },
      type: input.type,
      payload: input.payload,
    },
  });
}
