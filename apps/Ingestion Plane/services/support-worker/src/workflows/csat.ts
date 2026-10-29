import { proxyActivities, sleep } from '@temporalio/workflow';
import type * as activities from '../activities';

const { sendCsatActivity } = proxyActivities<typeof activities>({
  startToCloseTimeout: '30s',
  retry: {
    maximumAttempts: 3,
  },
});

/**
 * Waits 2 hours after ticket close, then fires a CSAT survey to the customer.
 *
 * @param ticketId       Zammad ticket ID.
 * @param customerEmail  Requester e-mail address.
 */
export async function csatSurvey(
  ticketId: number,
  customerEmail: string,
): Promise<void> {
  // Durable 2-hour delay — survives worker restarts.
  await sleep('2h');

  await sendCsatActivity({ ticketId, customerEmail });
}
