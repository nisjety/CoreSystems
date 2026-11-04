/**
 * The goal pump: attended auto-continue of a thread goal (EP-7 S4, owner decisions D4 and D5).
 *
 * With "Keep going" on, after a goal turn ends `completed` and the judge says `not_met` or
 * `unsure`, the goal bar counts down five seconds ("Continuing in 5 s - Stop") and sends the next
 * continuation turn through the ordinary send path. Each turn is a request of its own, made while
 * the person has the thread open (D4); closing the tab stops the goal.
 *
 * This module is the decision only: pure data in, one action out, no timers and no sends. The
 * controller runs the countdown and sends. The pump never approves, answers or resumes anything
 * (D5): any decision that waits on the person stops it, and so does every cap, a met or blocked
 * goal, a failed or parked turn, and Stop. A hidden tab holds the countdown and never sends, so
 * "attended" means seen; the countdown starts again in full when the tab is shown. The server
 * enforces the same rule on its own (`thread_goal_pending_decision`), for a stale or a hostile
 * client, and caps claims at 30 per goal per hour.
 */

import type { ThreadGoal } from '@/shared/api/thread-goal-client'

/** D4 default: the visible countdown before a turn the pump sends. */
export const GOAL_PUMP_COUNTDOWN_SECONDS = 5

/** Why the pump stopped. A closed set: the goal bar names each one. */
export type GoalPumpStopReason =
  | 'pending_approval'
  | 'pending_question'
  | 'plan_proposed'
  | 'parked'
  | 'failed'
  | 'budget_reached'
  | 'turns_reached'
  | 'met'
  | 'blocked'
  | 'user_stopped'
  | 'hidden_tab'

export const GOAL_PUMP_STOP_REASONS: readonly GoalPumpStopReason[] = [
  'pending_approval',
  'pending_question',
  'plan_proposed',
  'parked',
  'failed',
  'budget_reached',
  'turns_reached',
  'met',
  'blocked',
  'user_stopped',
  'hidden_tab',
]

/** How a goal turn ended, as the gateway classifies a finished Do run (EP-7 S2 settle outcomes). */
export type GoalTurnOutcome =
  | 'completed'
  | 'failed'
  | 'awaiting_approval'
  | 'awaiting_input'
  | 'parked'
  | 'cancelled'
  | 'unknown'

/** The last goal turn that ended, and how many turns the goal had used when it was sent. */
export type SettledGoalTurn = {
  /** The assistant turn it ended in: the pump sends at most once per settled turn. */
  turnId: string
  outcome: GoalTurnOutcome
  /** `goal.turnsUsed` when the turn was sent; a verdict counts only once the count moved past it. */
  turnsUsedAtSend: number
}

/** What waits on the person on this thread, as the composer dock shows it. */
export type GoalPumpPending = {
  approvals: number
  questions: number
  plan: boolean
  parked: boolean
}

export type GoalPumpInput = {
  goal: Pick<ThreadGoal, 'status' | 'turnsUsed' | 'maxTurns' | 'lastVerdict'> | null
  settled: SettledGoalTurn | null
  pending: GoalPumpPending
  /** "Keep going" is on. */
  autoOn: boolean
  /** The person pressed Stop since "Keep going" was last turned on. */
  userStopped: boolean
  /** `document.visibilityState` is not `hidden`. */
  visible: boolean
  /** A turn is running on the thread. */
  busy: boolean
  /** The pump already sent the turn that follows `settled`. */
  sentForSettled: boolean
  /** The countdown for `settled` ran to zero while the tab was shown. */
  countdownElapsed: boolean
}

export type GoalPumpAction =
  | { kind: 'wait' }
  | { kind: 'countdown'; seconds: number }
  | { kind: 'send' }
  | { kind: 'stop'; reason: GoalPumpStopReason }

const WAIT: GoalPumpAction = { kind: 'wait' }

function stop(reason: GoalPumpStopReason): GoalPumpAction {
  return { kind: 'stop', reason }
}

/** The stop a goal's own status names, or `null` while it may continue. */
function statusStop(status: ThreadGoal['status']): GoalPumpStopReason | null {
  switch (status) {
    case 'met':
      return 'met'
    case 'blocked':
      return 'blocked'
    case 'budget_reached':
      return 'budget_reached'
    case 'turns_reached':
      return 'turns_reached'
    default:
      return null
  }
}

/** The stop a settled turn's outcome names, or `null` for a turn that completed. */
function outcomeStop(outcome: GoalTurnOutcome): GoalPumpStopReason | null {
  switch (outcome) {
    case 'completed':
      return null
    case 'awaiting_approval':
      return 'pending_approval'
    case 'awaiting_input':
      return 'pending_question'
    case 'parked':
      return 'parked'
    case 'cancelled':
      return 'user_stopped'
    // A failed run, a takeover that expired (round B C-B5) and anything this build does not know
    // read as failed: never as met, and never continued.
    default:
      return 'failed'
  }
}

/**
 * The pump's next action. The order is fixed: the person's own Stop first, then the goal's status,
 * then anything waiting on the person, then how the last turn ended, the turn cap, the verdict, and
 * only then the countdown and the send.
 */
export function nextGoalAction(input: GoalPumpInput): GoalPumpAction {
  if (input.userStopped) return stop('user_stopped')
  if (!input.autoOn || !input.goal || input.busy) return WAIT
  const closed = statusStop(input.goal.status)
  if (closed) return stop(closed)
  // A paused goal is the person's own pause: nothing to stop, nothing to send.
  if (input.goal.status !== 'active') return WAIT
  if (input.pending.approvals > 0) return stop('pending_approval')
  if (input.pending.questions > 0) return stop('pending_question')
  if (input.pending.plan) return stop('plan_proposed')
  if (input.pending.parked) return stop('parked')
  // The first goal turn is always the person's Continue; the pump only follows a settled one.
  if (!input.settled) return WAIT
  const ended = outcomeStop(input.settled.outcome)
  if (ended) return stop(ended)
  const maxTurns = input.goal.maxTurns ?? 0
  if (maxTurns > 0 && input.goal.turnsUsed >= maxTurns) return stop('turns_reached')
  // The verdict must be the one for the settled turn: until the goal's turn count moved past the
  // count at send, the verdict on the goal is the previous turn's, so the pump waits for the read.
  if (input.goal.turnsUsed <= input.settled.turnsUsedAtSend) return WAIT
  switch (input.goal.lastVerdict) {
    case 'met':
      return stop('met')
    case 'blocked':
      return stop('blocked')
    case 'not_met':
    case 'unsure':
      break
    // No verdict (the judge is off or has not answered): nothing to continue on.
    default:
      return WAIT
  }
  if (input.sentForSettled) return WAIT
  if (!input.visible) return stop('hidden_tab')
  if (input.countdownElapsed) return { kind: 'send' }
  return { kind: 'countdown', seconds: GOAL_PUMP_COUNTDOWN_SECONDS }
}

/**
 * How a finished goal turn ended, from what the client saw of it. A turn that left an approval or a
 * question open stopped for the person; a parked run stopped on a recoverable failure; a turn the
 * person stopped was cancelled. The server's claim is the authority either way.
 */
export function settledGoalOutcome(turn: {
  status?: 'waiting' | 'stopped' | 'error'
  pendingApprovals?: readonly unknown[]
  questionPending: boolean
  parked: boolean
}): GoalTurnOutcome {
  if (turn.parked) return 'parked'
  if ((turn.pendingApprovals ?? []).length > 0) return 'awaiting_approval'
  if (turn.questionPending) return 'awaiting_input'
  if (turn.status === 'stopped') return 'cancelled'
  if (turn.status === 'error') return 'failed'
  // A turn still waiting for its first frame did not end in a way the client can read.
  if (turn.status === 'waiting') return 'unknown'
  return 'completed'
}

/**
 * What the goal bar shows of the pump and the two things it can do there, read as accessors so the
 * bar follows the controller. `available` is the server's word (`autoContinueAvailable` on the goal
 * read): the toggle is hidden without it.
 */
export type GoalAutoContinueView = {
  available: () => boolean
  on: () => boolean
  /** Seconds left before the pump sends, while it counts down. */
  countdown: () => number | null
  /** Why the pump stopped last, until "Keep going" is turned on again or a turn settles. */
  stopReason: () => GoalPumpStopReason | null
  setOn: (on: boolean) => void
  /** Cancels a countdown and turns "Keep going" off. */
  stop: () => void
}
