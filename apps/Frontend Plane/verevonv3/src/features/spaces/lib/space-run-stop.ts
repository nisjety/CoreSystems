// The Stop control's state on a live run card (WS09-S8). The card reads the typed answer of the stop call and
// decides whether Stop may be pressed again. The rules live here so they can be read and tested without a DOM.

/** Where one stop attempt stands on the card. One attempt is one idempotency key (see SpaceRunCard). */
export type RunStopState = 'idle' | 'pending' | 'sent' | 'forbidden' | 'zdr' | 'timeout' | 'failed'

/** How long a stop waits for its answer before the card says it is not confirmed yet. */
export const RUN_STOP_TIMEOUT_MS = 15_000

/** The rejection a stop gets when it has not answered within {@link RUN_STOP_TIMEOUT_MS}. */
export class RunStopTimeout extends Error {}

/**
 * Stop may be pressed while no attempt is in flight or sent, and no refusal has closed it. A timeout or a
 * failure leaves it open: a retry reuses the key, so the server de-duplicates. A refusal that a retry cannot
 * change (the organisation has not allowed stopping, or the server refuses the posture) closes it.
 */
export function runStopCanBePressed(state: RunStopState): boolean {
  return state === 'idle' || state === 'timeout' || state === 'failed'
}

/** The state a typed refusal of the stop call leaves the card in. Any other answer is a failure, which can be retried. */
export function runStopStateForCode(code: string | null | undefined): RunStopState {
  if (code === 'space_run_control_not_authorized') return 'forbidden'
  if (code === 'zdr_durable_mutation_forbidden') return 'zdr'
  return 'failed'
}
