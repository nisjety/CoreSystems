import { describe, expect, it } from 'vitest'
import {
  GOAL_PUMP_COUNTDOWN_SECONDS,
  GOAL_PUMP_STOP_REASONS,
  nextGoalAction,
  settledGoalOutcome,
  type GoalPumpInput,
  type GoalPumpStopReason,
} from './goal-pump'

/** A goal turn that completed with `not_met`: the one case in which the pump counts down. */
function input(overrides: Partial<GoalPumpInput> = {}): GoalPumpInput {
  return {
    goal: { status: 'active', turnsUsed: 2, maxTurns: 10, lastVerdict: 'not_met' },
    settled: { turnId: 'asst-2', outcome: 'completed', turnsUsedAtSend: 1 },
    pending: { approvals: 0, questions: 0, plan: false, parked: false },
    autoOn: true,
    userStopped: false,
    visible: true,
    busy: false,
    sentForSettled: false,
    countdownElapsed: false,
    ...overrides,
  }
}

const NONE = { approvals: 0, questions: 0, plan: false, parked: false }

describe('nextGoalAction', () => {
  it('counts down after a completed turn the judge did not find met, then sends once', () => {
    expect(nextGoalAction(input())).toEqual({ kind: 'countdown', seconds: GOAL_PUMP_COUNTDOWN_SECONDS })
    expect(nextGoalAction(input({ goal: { status: 'active', turnsUsed: 2, maxTurns: 10, lastVerdict: 'unsure' } }))).toEqual({
      kind: 'countdown',
      seconds: 5,
    })
    expect(nextGoalAction(input({ countdownElapsed: true }))).toEqual({ kind: 'send' })
    // One send per settled turn: once it went, the pump waits for the next turn to settle.
    expect(nextGoalAction(input({ countdownElapsed: true, sentForSettled: true }))).toEqual({ kind: 'wait' })
  })

  const stops: [GoalPumpStopReason, Partial<GoalPumpInput>][] = [
    ['pending_approval', { pending: { ...NONE, approvals: 1 } }],
    ['pending_question', { pending: { ...NONE, questions: 1 } }],
    ['plan_proposed', { pending: { ...NONE, plan: true } }],
    ['parked', { pending: { ...NONE, parked: true } }],
    ['pending_approval', { settled: { turnId: 'asst-2', outcome: 'awaiting_approval', turnsUsedAtSend: 1 } }],
    ['pending_question', { settled: { turnId: 'asst-2', outcome: 'awaiting_input', turnsUsedAtSend: 1 } }],
    ['parked', { settled: { turnId: 'asst-2', outcome: 'parked', turnsUsedAtSend: 1 } }],
    ['failed', { settled: { turnId: 'asst-2', outcome: 'failed', turnsUsedAtSend: 1 } }],
    ['failed', { settled: { turnId: 'asst-2', outcome: 'unknown', turnsUsedAtSend: 1 } }],
    ['user_stopped', { settled: { turnId: 'asst-2', outcome: 'cancelled', turnsUsedAtSend: 1 } }],
    ['budget_reached', { goal: { status: 'budget_reached', turnsUsed: 2, maxTurns: 10, lastVerdict: 'not_met' } }],
    ['turns_reached', { goal: { status: 'turns_reached', turnsUsed: 10, maxTurns: 10, lastVerdict: 'not_met' } }],
    ['turns_reached', { goal: { status: 'active', turnsUsed: 10, maxTurns: 10, lastVerdict: 'not_met' } }],
    ['met', { goal: { status: 'met', turnsUsed: 2, maxTurns: 10, lastVerdict: 'met' } }],
    ['met', { goal: { status: 'active', turnsUsed: 2, maxTurns: 10, lastVerdict: 'met' } }],
    ['blocked', { goal: { status: 'blocked', turnsUsed: 2, maxTurns: 10, lastVerdict: 'blocked' } }],
    ['blocked', { goal: { status: 'active', turnsUsed: 2, maxTurns: 10, lastVerdict: 'blocked' } }],
    ['user_stopped', { userStopped: true }],
    ['hidden_tab', { visible: false }],
  ]

  it.each(stops)('stops with %s', (reason, overrides) => {
    expect(nextGoalAction(input(overrides))).toEqual({ kind: 'stop', reason })
    // A stop never turns into a send when the countdown would have ended.
    expect(nextGoalAction(input({ ...overrides, countdownElapsed: true })).kind).toBe('stop')
  })

  it('names every stop reason in the table', () => {
    expect(new Set(stops.map(([reason]) => reason))).toEqual(new Set(GOAL_PUMP_STOP_REASONS))
  })

  it('stops for a decision that waits on the person before anything else the turn says', () => {
    // A stale `met` from an earlier turn never outranks an approval that waits now.
    const waiting = input({
      pending: { ...NONE, approvals: 1 },
      goal: { status: 'active', turnsUsed: 2, maxTurns: 10, lastVerdict: 'met' },
    })
    expect(nextGoalAction(waiting)).toEqual({ kind: 'stop', reason: 'pending_approval' })
  })

  it('reads a takeover that waits as a pending approval, and one that expired as failed, never met (round B C-B5)', () => {
    // The turn waits on browser_human_assist: an approval row the person decides.
    expect(
      nextGoalAction(input({ settled: { turnId: 'asst-2', outcome: 'awaiting_approval', turnsUsedAtSend: 1 } })),
    ).toEqual({ kind: 'stop', reason: 'pending_approval' })
    // The takeover expired: the turn failed, and a `met` left on the goal by an earlier turn is not read.
    const expired = input({
      settled: { turnId: 'asst-3', outcome: 'failed', turnsUsedAtSend: 2 },
      goal: { status: 'active', turnsUsed: 3, maxTurns: 10, lastVerdict: 'met' },
    })
    expect(nextGoalAction(expired)).toEqual({ kind: 'stop', reason: 'failed' })
  })

  it('pauses on a hidden tab and counts down again in full when it is shown', () => {
    expect(nextGoalAction(input({ visible: false, countdownElapsed: true }))).toEqual({ kind: 'stop', reason: 'hidden_tab' })
    expect(nextGoalAction(input({ visible: true }))).toEqual({ kind: 'countdown', seconds: GOAL_PUMP_COUNTDOWN_SECONDS })
  })

  it('waits while it has nothing to act on', () => {
    expect(nextGoalAction(input({ autoOn: false }))).toEqual({ kind: 'wait' })
    expect(nextGoalAction(input({ goal: null }))).toEqual({ kind: 'wait' })
    expect(nextGoalAction(input({ busy: true }))).toEqual({ kind: 'wait' })
    // The first goal turn is the person's Continue.
    expect(nextGoalAction(input({ settled: null }))).toEqual({ kind: 'wait' })
    // A paused goal is the person's own pause.
    expect(nextGoalAction(input({ goal: { status: 'paused', turnsUsed: 2, maxTurns: 10, lastVerdict: 'not_met' } }))).toEqual({
      kind: 'wait',
    })
  })

  it('never continues without a fresh verdict', () => {
    // The judge is off or has not answered.
    expect(nextGoalAction(input({ goal: { status: 'active', turnsUsed: 2, maxTurns: 10 } }))).toEqual({ kind: 'wait' })
    // The goal still shows the previous turn's verdict: its turn count has not moved past the send.
    expect(
      nextGoalAction(input({ settled: { turnId: 'asst-2', outcome: 'completed', turnsUsedAtSend: 2 } })),
    ).toEqual({ kind: 'wait' })
  })

  it('lets the person stop a countdown', () => {
    expect(nextGoalAction(input()).kind).toBe('countdown')
    expect(nextGoalAction(input({ userStopped: true, autoOn: false }))).toEqual({ kind: 'stop', reason: 'user_stopped' })
  })
})

describe('settledGoalOutcome', () => {
  const base = { questionPending: false, parked: false }

  it('reads how the turn ended from what the client saw', () => {
    expect(settledGoalOutcome(base)).toBe('completed')
    expect(settledGoalOutcome({ ...base, parked: true, pendingApprovals: [{}] })).toBe('parked')
    expect(settledGoalOutcome({ ...base, pendingApprovals: [{}] })).toBe('awaiting_approval')
    expect(settledGoalOutcome({ ...base, questionPending: true })).toBe('awaiting_input')
    expect(settledGoalOutcome({ ...base, status: 'stopped' })).toBe('cancelled')
    expect(settledGoalOutcome({ ...base, status: 'error' })).toBe('failed')
    expect(settledGoalOutcome({ ...base, status: 'waiting' })).toBe('unknown')
  })
})
