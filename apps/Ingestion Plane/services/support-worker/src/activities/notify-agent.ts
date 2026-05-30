import { config } from '../config';

export interface NotifyAgentInput {
  /** Notification event type, e.g. 'ticket.triaged', 'sla.warning', 'sla.breach'. */
  type: string;
  ticketId: number;
  /** Zammad owner user ID, or 'broadcast' for un-assigned tickets. */
  recipientId: number | 'broadcast';
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
  const url = `${config.NOTIFICATION_CORE_URL}/v1/notifications`;

  const body = {
    recipient_id: input.recipientId,
    type: input.type,
    payload: input.payload,
  };

  const response = await fetch(url, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-Internal-Api-Key': config.INTERNAL_API_KEY,
    },
    body: JSON.stringify(body),
  });

  if (!response.ok) {
    const text = await response.text().catch(() => '');
    throw new Error(
      `notification-core POST /v1/notifications failed: ${response.status} ${response.statusText} — ${text}`,
    );
  }
}
