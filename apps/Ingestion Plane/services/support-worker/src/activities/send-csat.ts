import { config } from '../config';

export interface SendCsatInput {
  ticketId: number;
  customerEmail: string;
}

/**
 * Sends a CSAT survey to the customer via notification-core.
 */
export async function sendCsatActivity(input: SendCsatInput): Promise<void> {
  const url = `${config.NOTIFICATION_CORE_URL}/v1/notifications`;

  const body = {
    recipient_id: input.customerEmail,
    type: 'csat.survey',
    payload: {
      ticketId: input.ticketId,
      customerEmail: input.customerEmail,
      surveyUrl: `/support/csat/${input.ticketId}`,
    },
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
      `notification-core CSAT survey POST failed: ${response.status} ${response.statusText} — ${text}`,
    );
  }
}
