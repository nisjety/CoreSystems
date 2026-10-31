/**
 * The thread status the Frontend Plane reads (build plan WS15 S6, amended by K3-8).
 *
 * WS16 owns the closed attention vocabulary, its precedence and the unseen marker, in
 * `shared/attention/attention-state.ts`. This module re-exports it so the thread surfaces read
 * one path and cannot grow a second vocabulary. It adds only the run tokens that a surface labels
 * apart although the attention vocabulary reads them alike: a queued run is working, but the room
 * says "queued"; a cancelled run is idle, but the console and the room say "stopped". It decides no
 * precedence (DEC-11).
 */
export * from '@/shared/attention/attention-state'

/** The token Session Core writes for a run paused on a human decision. Named once, so the pulse, the approval surface and the composer guard agree on it. */
export const AWAITING_APPROVAL_RUN_STATUS = 'awaiting_approval'

/** Statuses under which a run has not finished: queued, running, or waiting on a human decision. */
export const ACTIVE_RUN_STATUSES = new Set(['queued', 'running', AWAITING_APPROVAL_RUN_STATUS])

/**
 * Statuses under which an agent is producing output right now.
 *
 * Distinct from `ACTIVE_RUN_STATUSES` on purpose: a run paused for approval is active in the sense
 * that it has not finished, but nobody is working. The "working" indicator must say the first thing
 * and not the second, or a room stalled on a decision would look busy.
 */
export const LIVE_RUN_STATUSES = new Set(['queued', 'running'])

export const FAILED_RUN_STATUSES = new Set(['failed'])

/** A run a member stopped. Session Core writes `cancelled`; the other spelling is read the same way. */
export function isStoppedRunStatus(value: string | undefined): boolean {
  const token = value?.trim().toLowerCase()
  return token === 'cancelled' || token === 'canceled'
}

/** A run waiting for a worker. The attention vocabulary reads it as working; the room names it as queued. */
export function isQueuedRunStatus(value: string | undefined): boolean {
  return value?.trim().toLowerCase() === 'queued'
}
