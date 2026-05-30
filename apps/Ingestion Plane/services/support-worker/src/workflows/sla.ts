import { proxyActivities, sleep } from '@temporalio/workflow';
import type * as activities from '../activities';

const THIRTY_MINUTES_MS = 30 * 60 * 1000;

const { patchZammadActivity, notifyAgentActivity } =
  proxyActivities<typeof activities>({
    startToCloseTimeout: '30s',
    retry: {
      maximumAttempts: 3,
    },
  });

/**
 * Runs a two-stage SLA countdown:
 *   1. Warning notification 30 minutes before breach deadline.
 *   2. Breach: tag the ticket and fire a breach notification.
 *
 * @param ticketId   Zammad ticket ID.
 * @param deadlineMs Unix timestamp (ms) of the SLA hard deadline.
 */
export async function slaCountdown(
  ticketId: number,
  deadlineMs: number,
): Promise<void> {
  const now = Date.now();

  // ── Warning stage ──────────────────────────────────────────────────────────
  const warningAt = deadlineMs - THIRTY_MINUTES_MS;
  const msUntilWarning = warningAt - now;

  if (msUntilWarning > 0) {
    await sleep(msUntilWarning);
  }

  await notifyAgentActivity({
    type: 'sla.warning',
    ticketId,
    recipientId: 'broadcast',
    payload: {
      ticketId,
      message: `SLA warning: ticket #${ticketId} will breach in 30 minutes.`,
    },
  });

  // ── Breach stage ───────────────────────────────────────────────────────────
  const msUntilBreach = deadlineMs - Date.now();

  if (msUntilBreach > 0) {
    await sleep(msUntilBreach);
  }

  await patchZammadActivity(ticketId, { tags: ['sla:breached'] });

  await notifyAgentActivity({
    type: 'sla.breach',
    ticketId,
    recipientId: 'broadcast',
    payload: {
      ticketId,
      message: `SLA breached for ticket #${ticketId}.`,
    },
  });
}
