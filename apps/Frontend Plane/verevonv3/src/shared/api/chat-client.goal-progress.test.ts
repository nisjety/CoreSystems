import { afterEach, describe, expect, it, vi } from 'vitest'
import { goalProgressEventOf, streamChat } from './chat-client'

// The `goal_progress` frame (EP-7 S2): a settled goal turn's counts and status, read typed, with
// unknown keys ignored and a frame the client cannot read reported rather than guessed at.

function sseResponse(frames: string[]): Response {
  const encoder = new TextEncoder()
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const frame of frames) controller.enqueue(encoder.encode(frame))
      controller.close()
    },
  })
  return new Response(stream, {
    status: 200,
    headers: { 'Content-Type': 'text/event-stream' },
  })
}

const SETTLED = {
  goal_id: 'goal_1',
  turn_no: 3,
  tokens_used: 31_000,
  token_budget: 30_000,
  cost_used_usd_micros: 120_000,
  cost_budget_usd_micros: null,
  turns_used: 3,
  max_turns: 10,
  status: 'budget_reached',
  status_reason: 'token_budget',
  request_id: 'req-1',
}

describe('goalProgressEventOf', () => {
  it('reads the counts and closed tokens, and leaves an unset budget out', () => {
    expect(goalProgressEventOf({ ...SETTLED, extra_key: 'ignored' })).toEqual({
      goalId: 'goal_1',
      turnNo: 3,
      tokensUsed: 31_000,
      tokenBudget: 30_000,
      costUsedUsdMicros: 120_000,
      turnsUsed: 3,
      maxTurns: 10,
      status: 'budget_reached',
      statusReason: 'token_budget',
      requestId: 'req-1',
    })
  })

  it('reads an active goal with no budget and no reason', () => {
    const active = goalProgressEventOf({
      goal_id: 'goal_1',
      turn_no: 1,
      tokens_used: 0,
      token_budget: null,
      cost_used_usd_micros: 0,
      cost_budget_usd_micros: null,
      turns_used: 1,
      max_turns: 10,
      status: 'active',
      status_reason: '',
    })
    expect(active).toEqual({
      goalId: 'goal_1',
      turnNo: 1,
      tokensUsed: 0,
      costUsedUsdMicros: 0,
      turnsUsed: 1,
      maxTurns: 10,
      status: 'active',
    })
  })

  it('refuses a frame without a goal id or with a status this build does not know', () => {
    expect(goalProgressEventOf({ ...SETTLED, goal_id: '' })).toBeUndefined()
    expect(goalProgressEventOf({ ...SETTLED, status: 'exploded' })).toBeUndefined()
  })

  it('never reads a count below zero or a non-number as a count', () => {
    const odd = goalProgressEventOf({ ...SETTLED, tokens_used: -5, turns_used: '3' })
    expect(odd?.tokensUsed).toBe(0)
    expect(odd?.turnsUsed).toBe(0)
  })
})

describe('chat-client goal_progress frame (EP-7 S2)', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('hands a readable frame to onGoalProgress and an unreadable one to onUnknownEvent', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        sseResponse([
          `event: goal_progress\ndata: ${JSON.stringify(SETTLED)}\n\n`,
          `event: goal_progress\ndata: ${JSON.stringify({ ...SETTLED, status: 'exploded' })}\n\n`,
        ]),
      ),
    )
    const onGoalProgress = vi.fn()
    const onUnknownEvent = vi.fn()
    await streamChat({ content: 'Continue toward the goal' }, { onGoalProgress, onUnknownEvent })

    expect(onGoalProgress).toHaveBeenCalledTimes(1)
    expect(onGoalProgress.mock.calls[0]?.[0]).toMatchObject({
      goalId: 'goal_1',
      status: 'budget_reached',
      tokensUsed: 31_000,
      tokenBudget: 30_000,
    })
    expect(onUnknownEvent).toHaveBeenCalledWith(expect.objectContaining({ name: 'goal_progress' }))
  })
})
