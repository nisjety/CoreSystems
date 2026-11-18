import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  BUSY_RETRIES,
  BUSY_RETRY_MS,
  createWakeBoard,
  createWakeSequencer,
  type WakeBoard,
  type WakeEntry,
  type WakePlan,
  type WakePlanEntry,
  type WakeSequencer,
  type WakeStreamResult,
} from './wake-sequencer'

const MESSAGE = 'post-1'

type Stream = (entry: WakeEntry, signal: AbortSignal) => Promise<WakeStreamResult>

/** A board, a sequencer and a mocked wake stream; ids are minted as wake-1, wake-2, ... */
function harness(stream: Stream) {
  const board = createWakeBoard()
  let minted = 0
  const streamWake = vi.fn(stream)
  const newWakeId = vi.fn(() => {
    minted += 1
    return `wake-${minted}`
  })
  const sequencer = createWakeSequencer({ board, streamWake, newWakeId })
  return { board, streamWake, newWakeId, sequencer }
}

function plan(agents: WakePlanEntry[], messageId = MESSAGE): WakePlan {
  return { messageId, threadId: 'thread-1', spaceRef: 'room-a', content: 'Hei @Lager', agents }
}

function agent(agentSubjectId: string, ordinal: number, refusedReason?: string): WakePlanEntry {
  return { agentSubjectId, agentName: agentSubjectId, ordinal, ...(refusedReason ? { refusedReason } : {}) }
}

function stateOf(board: WakeBoard, agentSubjectId: string, messageId = MESSAGE) {
  return board.forMessage(messageId).find((entry) => entry.agentSubjectId === agentSubjectId)
}

function callOrder(calls: ReadonlyArray<readonly [WakeEntry, AbortSignal]>): string[] {
  return calls.map(([entry]) => entry.agentSubjectId)
}

/**
 * Replaces AbortController with a subclass that remembers each instance, so a test can abort the
 * controller the runner made for a run. Nothing in the module aborts it on its own.
 */
function captureAbortControllers(): AbortController[] {
  const RealAbortController = globalThis.AbortController
  const created: AbortController[] = []
  class CapturingAbortController extends RealAbortController {
    constructor() {
      super()
      created.push(this)
    }
  }
  vi.stubGlobal('AbortController', CapturingAbortController)
  return created
}

beforeEach(() => {
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('createWakeSequencer order and concurrency (WS08-S6)', () => {
  it('runs the wakes in ordinal order, one request in flight at a time', async () => {
    const resolvers: Array<(result: WakeStreamResult) => void> = []
    const { board, streamWake, sequencer } = harness(
      () =>
        new Promise<WakeStreamResult>((resolve) => {
          resolvers.push(resolve)
        }),
    )
    sequencer.start(plan([agent('agent-c', 2), agent('agent-a', 0), agent('agent-b', 1)]))
    await vi.advanceTimersByTimeAsync(0)
    expect(callOrder(streamWake.mock.calls)).toEqual(['agent-a'])
    expect(resolvers).toHaveLength(1)

    resolvers.shift()?.({})
    await vi.advanceTimersByTimeAsync(0)
    expect(callOrder(streamWake.mock.calls)).toEqual(['agent-a', 'agent-b'])
    expect(resolvers).toHaveLength(1)

    resolvers.shift()?.({})
    await vi.advanceTimersByTimeAsync(0)
    expect(callOrder(streamWake.mock.calls)).toEqual(['agent-a', 'agent-b', 'agent-c'])
    expect(resolvers).toHaveLength(1)

    resolvers.shift()?.({})
    await vi.advanceTimersByTimeAsync(0)
    expect(resolvers).toHaveLength(0)
    expect(['agent-a', 'agent-b', 'agent-c'].map((id) => stateOf(board, id)?.state)).toEqual(['done', 'done', 'done'])
  })

  it('waits 3 s between busy answers, asks ten more times, then fails', async () => {
    expect(BUSY_RETRY_MS).toBe(3000)
    expect(BUSY_RETRIES).toBe(10)
    const { board, streamWake, sequencer } = harness(async () => ({ code: 'thread_run_active' }))
    sequencer.start(plan([agent('agent-a', 0)]))
    await vi.advanceTimersByTimeAsync(0)
    expect(streamWake).toHaveBeenCalledTimes(1)
    expect(stateOf(board, 'agent-a')).toMatchObject({ state: 'waiting', code: 'thread_run_active' })

    await vi.advanceTimersByTimeAsync(BUSY_RETRY_MS - 1)
    expect(streamWake).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(1)
    expect(streamWake).toHaveBeenCalledTimes(2)

    // Each later ask is 3 s after the one before: the tenth retry is at 27 s, the last ask at 30 s.
    await vi.advanceTimersByTimeAsync(BUSY_RETRY_MS * (BUSY_RETRIES - 1) - 1)
    expect(streamWake).toHaveBeenCalledTimes(BUSY_RETRIES)
    expect(stateOf(board, 'agent-a')?.state).toBe('waiting')
    await vi.advanceTimersByTimeAsync(1)
    expect(streamWake).toHaveBeenCalledTimes(BUSY_RETRIES + 1)
    expect(stateOf(board, 'agent-a')).toMatchObject({ state: 'failed', code: 'thread_run_active' })

    await vi.advanceTimersByTimeAsync(BUSY_RETRY_MS * 5)
    expect(streamWake).toHaveBeenCalledTimes(BUSY_RETRIES + 1)
    expect(new Set(streamWake.mock.calls.map(([entry]) => entry.clientWakeId)).size).toBe(1)
  })

  it('treats managed_run_already_started as done and carries on with the next agent', async () => {
    const { board, streamWake, sequencer } = harness(async (entry) =>
      entry.agentSubjectId === 'agent-a' ? { code: 'managed_run_already_started' } : {},
    )
    sequencer.start(plan([agent('agent-a', 0), agent('agent-b', 1)]))
    await vi.advanceTimersByTimeAsync(0)
    expect(stateOf(board, 'agent-a')?.state).toBe('done')
    expect(stateOf(board, 'agent-a')?.code).toBeUndefined()
    expect(stateOf(board, 'agent-b')?.state).toBe('done')
    expect(callOrder(streamWake.mock.calls)).toEqual(['agent-a', 'agent-b'])
  })
})

describe('createWakeSequencer outcomes (WS08-S6)', () => {
  const refusals = [
    'agent_respond_to_denied',
    'agent_mention_not_enabled',
    'agent_invocation_blocked',
    'mentioned_agent_not_bound',
    'wake_owner_required',
  ]

  it.each(refusals)('shows %s as refused, never asks again, and moves on to the next agent', async (code) => {
    const { board, streamWake, sequencer } = harness(async (entry) =>
      entry.agentSubjectId === 'agent-a' ? { code } : {},
    )
    sequencer.start(plan([agent('agent-a', 0), agent('agent-b', 1)]))
    await vi.advanceTimersByTimeAsync(0)
    expect(stateOf(board, 'agent-a')).toMatchObject({ state: 'refused', code })
    expect(stateOf(board, 'agent-b')?.state).toBe('done')
    expect(callOrder(streamWake.mock.calls)).toEqual(['agent-a', 'agent-b'])
  })

  it('a failure that is not a refusal is failed, not refused, and is not asked again by itself', async () => {
    const { board, streamWake, sequencer } = harness(async () => ({ code: 'connection_error' }))
    sequencer.start(plan([agent('agent-a', 0)]))
    await vi.advanceTimersByTimeAsync(BUSY_RETRY_MS * 5)
    expect(stateOf(board, 'agent-a')).toMatchObject({ state: 'failed', code: 'connection_error' })
    expect(streamWake).toHaveBeenCalledTimes(1)
  })

  it('a refused plan entry is never streamed, and a plan of only refused entries sends nothing', async () => {
    const { board, streamWake, sequencer } = harness(async () => ({}))
    sequencer.start(plan([agent('agent-a', 0, 'agent_mention_not_enabled'), agent('agent-b', 1)]))
    expect(stateOf(board, 'agent-a')).toMatchObject({ state: 'refused', code: 'agent_mention_not_enabled' })
    await vi.advanceTimersByTimeAsync(0)
    expect(callOrder(streamWake.mock.calls)).toEqual(['agent-b'])
    expect(stateOf(board, 'agent-a')?.state).toBe('refused')

    const onlyRefused = harness(async () => ({}))
    onlyRefused.sequencer.start(plan([agent('agent-x', 0, 'agent_respond_to_denied')], 'post-2'))
    await vi.advanceTimersByTimeAsync(0)
    expect(onlyRefused.streamWake).not.toHaveBeenCalled()
    expect(onlyRefused.board.forMessage('post-2')).toEqual([expect.objectContaining({ state: 'refused' })])
  })
})

describe('createWakeSequencer abort, retry and Wake now (WS08-S6)', () => {
  it('an abort interrupts the wake in flight and the ones behind it, and Wake now runs only those', async () => {
    const controllers = captureAbortControllers()
    let bAttempts = 0
    const { board, streamWake, sequencer } = harness((entry, signal) => {
      if (entry.agentSubjectId !== 'agent-b') return Promise.resolve({})
      bAttempts += 1
      if (bAttempts > 1) return Promise.resolve({})
      // The first ask for agent-b hangs until this run is aborted.
      return new Promise<WakeStreamResult>((resolve) => {
        signal.addEventListener('abort', () => resolve({}))
      })
    })

    sequencer.start(plan([agent('agent-a', 0), agent('agent-b', 1), agent('agent-c', 2)]))
    await vi.advanceTimersByTimeAsync(0)
    expect(callOrder(streamWake.mock.calls)).toEqual(['agent-a', 'agent-b'])
    expect(stateOf(board, 'agent-b')?.state).toBe('answering')

    controllers[controllers.length - 1]?.abort()
    await vi.advanceTimersByTimeAsync(0)
    expect(['agent-a', 'agent-b', 'agent-c'].map((id) => stateOf(board, id)?.state)).toEqual([
      'done',
      'interrupted',
      'interrupted',
    ])
    expect(streamWake).toHaveBeenCalledTimes(2)

    sequencer.wakeNow(MESSAGE)
    await vi.advanceTimersByTimeAsync(0)
    // agent-a is done and is not asked again; the interrupted two run again, in order.
    expect(callOrder(streamWake.mock.calls)).toEqual(['agent-a', 'agent-b', 'agent-b', 'agent-c'])
    expect(['agent-a', 'agent-b', 'agent-c'].map((id) => stateOf(board, id)?.state)).toEqual(['done', 'done', 'done'])
  })

  it('retry runs a failed wake again under the same clientWakeId, and a finished wake is not retried', async () => {
    let attempts = 0
    const { board, streamWake, newWakeId, sequencer } = harness(async () => {
      attempts += 1
      return attempts === 1 ? { code: 'session_unavailable' } : {}
    })
    sequencer.start(plan([agent('agent-a', 0)]))
    await vi.advanceTimersByTimeAsync(0)
    expect(stateOf(board, 'agent-a')).toMatchObject({ state: 'failed', code: 'session_unavailable' })

    sequencer.retry(MESSAGE, 'agent-a')
    await vi.advanceTimersByTimeAsync(0)
    expect(streamWake).toHaveBeenCalledTimes(2)
    expect(streamWake.mock.calls.map(([entry]) => entry.clientWakeId)).toEqual(['wake-1', 'wake-1'])
    expect(newWakeId).toHaveBeenCalledTimes(1)
    expect(stateOf(board, 'agent-a')?.state).toBe('done')

    sequencer.retry(MESSAGE, 'agent-a')
    await vi.advanceTimersByTimeAsync(0)
    expect(streamWake).toHaveBeenCalledTimes(2)
  })
})

describe('createWakeSequencer interrupt (WS08-S6)', () => {
  /** The first ask hangs until its run is aborted, the way a stream does when the session lets go of it. */
  function hangingFirstAsk() {
    let asks = 0
    return (_entry: WakeEntry, signal: AbortSignal): Promise<WakeStreamResult> => {
      asks += 1
      if (asks > 1) return Promise.resolve({})
      return new Promise<WakeStreamResult>((resolve) => {
        signal.addEventListener('abort', () => resolve({}))
      })
    }
  }

  it.each([
    ['interrupt()', (sequencer: WakeSequencer) => sequencer.interrupt()],
    ['the page hiding (pagehide)', () => window.dispatchEvent(new Event('pagehide'))],
  ])('%s interrupts the wake in flight and the ones queued behind it; Wake now runs them again', async (_how, leave) => {
    const { board, streamWake, sequencer } = harness(hangingFirstAsk())
    sequencer.start(plan([agent('agent-a', 0), agent('agent-b', 1), agent('agent-c', 2)]))
    await vi.advanceTimersByTimeAsync(0)
    expect(stateOf(board, 'agent-a')?.state).toBe('answering')

    leave(sequencer)
    await vi.advanceTimersByTimeAsync(0)
    expect(['agent-a', 'agent-b', 'agent-c'].map((id) => stateOf(board, id)?.state)).toEqual([
      'interrupted',
      'interrupted',
      'interrupted',
    ])
    expect(streamWake).toHaveBeenCalledTimes(1)

    sequencer.wakeNow(MESSAGE)
    await vi.advanceTimersByTimeAsync(0)
    expect(callOrder(streamWake.mock.calls)).toEqual(['agent-a', 'agent-a', 'agent-b', 'agent-c'])
    expect(['agent-a', 'agent-b', 'agent-c'].map((id) => stateOf(board, id)?.state)).toEqual(['done', 'done', 'done'])
  })

  it('interrupting a session with nothing in flight changes nothing, and later wakes still run', async () => {
    const { board, streamWake, sequencer } = harness(async () => ({}))
    sequencer.interrupt()
    window.dispatchEvent(new Event('pagehide'))
    sequencer.start(plan([agent('agent-a', 0)]))
    await vi.advanceTimersByTimeAsync(0)
    expect(stateOf(board, 'agent-a')?.state).toBe('done')
    expect(streamWake).toHaveBeenCalledTimes(1)
  })
})
