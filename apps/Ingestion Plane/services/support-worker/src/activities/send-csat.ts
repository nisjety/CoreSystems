import { config } from '../config';

export interface SendCsatInput {
  ticketId: number;
  customerEmail: string;
}

/**
 * Sends a CSAT survey to the customer via notification-core.
 */
export async function sendCsatActivity(input: SendCsatInput): Promise<void> {
  if (config.SUPPORT_NOTIFICATION_MODE === 'disabled') {
    console.warn('[support-worker] CSAT notification skipped: integration intentionally disabled');
    return;
  }
  void input;
  throw new Error(
    'CSAT notification requires a separate consent-aware external-contact contract; email is not a Control user recipient',
  );
}
