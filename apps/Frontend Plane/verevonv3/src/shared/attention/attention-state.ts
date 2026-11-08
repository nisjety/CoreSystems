/**
 * The attention vocabulary the Frontend Plane shares (build plan WS16 S2). Session Core
 * derives the state of a thread; the rail, the tab badge and the thread chip all read it
 * through this module, so a run-status string decides a section in one place only. A value
 * outside the closed set is dropped, never guessed at.
 */

export const ATTENTION_STATES = [
  'needs_approval',
  'needs_input',
  'plan_ready',
  'working',
  'completed',
  'failed',
  'archived',
  'idle',
] as const

export type AttentionState = (typeof ATTENTION_STATES)[number]

/** Most urgent first. The same order Session Core's precedence uses (DEC-11). */
export const ATTENTION_PRECEDENCE: readonly AttentionState[] = [
  'needs_approval',
  'needs_input',
  'plan_ready',
  'working',
  'failed',
  'completed',
  'archived',
  'idle',
]

/** The rail sections. A thread in none of them is not shown in the rail. */
export type AttentionSection = 'waiting' | 'working' | 'unseen'

const WAITING_STATES: ReadonlySet<AttentionState> = new Set(['needs_approval', 'needs_input', 'plan_ready'])

export function isAttentionState(value: unknown): value is AttentionState {
  return typeof value === 'string' && (ATTENTION_STATES as readonly string[]).includes(value)
}

/** Whether a state waits on the person. Only these count toward the tab badge. */
export function isWaitingState(state: AttentionState | undefined): boolean {
  return state !== undefined && WAITING_STATES.has(state)
}

/**
 * The rail section a thread belongs to, or null. Unseen is a finished run (completed or
 * failed) the person has not opened yet; a working or waiting thread is shown by its state.
 */
export function attentionSection(state: AttentionState | undefined, unseen: boolean): AttentionSection | null {
  if (isWaitingState(state)) return 'waiting'
  if (state === 'working') return 'working'
  if (unseen && (state === 'completed' || state === 'failed')) return 'unseen'
  return null
}

/**
 * The closed state a latest run status reads as, for the thread rows that still show the
 * run-status chip (retired with that chip in WS16 S7). This is the only place the legacy run
 * statuses are named. The server derives its state from the approval and plan rows, so an
 * `awaiting_approval` run can read as waiting here and as working on the server until S7.
 */
export function legacyStateFromRunStatus(value: string | undefined): AttentionState | undefined {
  switch (value?.trim().toLowerCase()) {
    case 'queued':
    case 'pending':
    case 'running':
    case 'in_progress':
      return 'working'
    case 'awaiting_approval':
    case 'waiting_approval':
      return 'needs_approval'
    case 'paused':
    case 'blocked':
    case 'ambiguous':
      return 'needs_input'
    case 'failed':
    case 'timed_out':
    case 'denied':
    case 'rejected':
      return 'failed'
    case 'completed':
      return 'completed'
    case 'cancelled':
    case 'canceled':
      return 'idle'
    default:
      return undefined
  }
}
