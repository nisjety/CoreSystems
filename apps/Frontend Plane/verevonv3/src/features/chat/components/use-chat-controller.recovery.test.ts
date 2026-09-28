// @vitest-environment jsdom

/**
 * Q15 recovery, the browser half: a turn the browser could not settle itself
 * is settled from the server's record, never from the transport's.
 *
 *  - A resume that fails (404, lost connection) no longer means "stopped":
 *    the durable run is asked, a running run is followed until its saved
 *    answer lands, and a failed run shows its failure.
 *  - A Stop still answers at once, but a cancel the server did not confirm is
 *    recorded, and an answer the server saved anyway replaces "Stoppet".
 *  - A Stop before `connected` waits for the request id instead of detaching.
 *  - Timing, cost basis and the publication receipt land on the turn.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'
import type { RunDetail } from '@/shared/api/runs-client'
import { ApiError } from '@/shared/api/http'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'

type CapturedStream = {
  handlers: ChatStreamHandlers
  signal?: AbortSignal
  resolve: () => void
}

const mocks = vi.hoisted(() => {
  const streams: CapturedStream[] = []
  return {
    streams,
    streamChat: vi.fn((_request: unknown, handlers: ChatStreamHandlers, signal?: AbortSignal) => new Promise<void>((resolve) => {
      streams.push({ handlers, signal, resolve })
    })),
    resumeStream: vi.fn(async (_id: string, handlers: ChatStreamHandlers) => handlers.onError?.({ code: 'connection_error', message: 'Request failed (404)' })),
    cancelInvocation: vi.fn(async () => undefined),
    getChatThreadTranscript: vi.fn(async () => null as unknown),
    getRun: vi.fn(async () => null as RunDetail | null),
    listChatThreads: vi.fn(async () => [] as unknown[]),
    getThreadMessages: vi.fn(async () => [] as unknown[]),
    listRuns: vi.fn(async () => ({ runs: [] as RunDetail[], hasMore: false })),
  }
})

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    streamChat: mocks.streamChat,
    resumeStream: mocks.resumeStream,
    cancelInvocation: mocks.cancelInvocation,
    getChatThreadTranscript: mocks.getChatThreadTranscript,
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: mocks.listChatThreads,
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
    getThreadMessages: mocks.getThreadMessages,
  }
})

vi.mock('@/shared/api/runs-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/runs-client')>()
  return {
    ...actual,
    getRun: mocks.getRun,
    listRuns: mocks.listRuns,
  }
})

// Stop now cancels the durable run of every turn that has one; keep that
// request offline and confirmed, so the cancel records below stay about the
// invocation cancel each test exercises.
vi.mock('@/shared/api/orchestration-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/orchestration-client')>()
  return {
    ...actual,
    cancelRun: vi.fn(async (runId: string) => ({ runId, cancelled: true, status: 'cancelled' })),
  }
})

import { CHAT_THREAD_TRANSCRIPTS_KEY, setActiveChatThreadId } from '@/features/chat/lib/chat-thread-history'
import { useChatController } from './use-chat-controller'

const RUN_ID = '01M2ZNAZ0W48WCREHFTGPKPNV2'

const run = (status: string, overrides: Partial<RunDetail> = {}): RunDetail => ({
  runId: RUN_ID,
  status,
  goal: 'Read this',
  checkpointIndex: 0,
  stepsCompleted: 0,
  inputTokens: 0,
  outputTokens: 0,
  ...overrides,
})

const transcript = (...turns: Array<{ role: 'user' | 'assistant'; content: string; artifacts?: unknown[] }>) => ({
  threadId: 'thread',
  updatedAt: '2026-09-23T10:00:00.000Z',
  turns: turns.map((turn, index) => ({ id: `msg-${index}`, createdAt: '2026-09-23T10:00:00.000Z', ...turn })),
})

async function tick(times = 3) {
  for (let i = 0; i < times; i += 1) {
    await Promise.resolve()
    flush()
  }
}

/** Let the reconciliation poll run, on fake timers. */
async function advance(ms: number) {
  await vi.advanceTimersByTimeAsync(ms)
  await tick(8)
}

function withController<T>(fn: (controller: ReturnType<typeof useChatController>) => Promise<T>): Promise<T> {
  return new Promise((resolvePromise, rejectPromise) => {
    createRoot((dispose) => {
      const controller = useChatController()
      fn(controller).then(
        (value) => { dispose(); resolvePromise(value) },
        (error) => { dispose(); rejectPromise(error) },
      )
    })
  })
}

async function startTurn(controller: ReturnType<typeof useChatController>, connected: { requestId?: string; runId?: string } | null = { requestId: 'req-1', runId: RUN_ID }) {
  await tick()
  await controller.handleComposerSubmit({ actions: [], attachments: [], text: 'Read this', tools: [] })
  await tick()
  const stream = mocks.streams.at(-1)!
  if (connected) stream.handlers.onConnected?.({ ok: true, ...connected })
  return stream
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
})

afterEach(() => {
  vi.useRealTimers()
  mocks.streams.length = 0
  mocks.streamChat.mockClear()
  mocks.resumeStream.mockClear()
  mocks.cancelInvocation.mockReset()
  mocks.cancelInvocation.mockResolvedValue(undefined)
  mocks.getChatThreadTranscript.mockReset()
  mocks.getChatThreadTranscript.mockResolvedValue(null)
  mocks.getRun.mockReset()
  mocks.getRun.mockResolvedValue(null)
  mocks.listChatThreads.mockReset()
  mocks.listChatThreads.mockResolvedValue([])
  mocks.getThreadMessages.mockReset()
  mocks.getThreadMessages.mockResolvedValue([])
  mocks.listRuns.mockReset()
  mocks.listRuns.mockResolvedValue({ runs: [], hasMore: false })
  __resetRetentionForTests()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('Q15 reconcile after a failed resume', () => {
  it('a resume 404 while the run is running adopts the server answer', async () => {
    mocks.getRun
      .mockResolvedValueOnce(run('running'))
      .mockResolvedValueOnce(run('completed'))
    mocks.getChatThreadTranscript
      // Still working: only the question is saved so far.
      .mockResolvedValueOnce(transcript({ role: 'user', content: 'Read this' }))
      .mockResolvedValueOnce(transcript(
        { role: 'user', content: 'Read this' },
        { role: 'assistant', content: 'The saved answer.', artifacts: [{ id: 'doc', kind: 'document', title: 'Plan', content: '# Plan', version: 1 }] },
      ))
    await withController(async (controller) => {
      const stream = await startTurn(controller)
      stream.handlers.onMessage?.({ content: 'Partial' })
      await tick()
      stream.resolve()
      await tick(8)
      expect(mocks.resumeStream).toHaveBeenCalledTimes(1)
      expect(mocks.getRun).toHaveBeenCalledWith(RUN_ID, expect.any(AbortSignal))
      // Still running: held pending, not settled as a lost connection.
      expect(controller.state.turns.at(-1)).toMatchObject({ status: 'waiting', content: 'Partial' })
      expect(controller.state.status).toBe('streaming')

      await advance(1_000)
      const turn = controller.state.turns.at(-1)
      expect(turn?.content).toBe('The saved answer.')
      expect(turn?.status).toBeUndefined()
      expect(turn?.streaming).toBe(false)
      expect(turn?.artifacts?.map((artifact) => artifact.id)).toEqual(['doc'])
      expect(controller.state.status).toBe('idle')
      expect(controller.state.error).toBeNull()
      // The run was reattached to, never re-sent.
      expect(mocks.streamChat).toHaveBeenCalledTimes(1)
    })
  })

  it('shows the failure when the run failed while the stream was gone', async () => {
    mocks.getRun.mockResolvedValue(run('failed', { error: 'provider_timeout' }))
    mocks.getChatThreadTranscript.mockResolvedValue(transcript({ role: 'user', content: 'Read this' }))
    await withController(async (controller) => {
      const stream = await startTurn(controller)
      stream.resolve()
      await tick(12)
      expect(controller.state.turns.at(-1)).toMatchObject({
        status: 'error',
        content: 'Modelltjenesten svarte ikke i tide, så svaret ble ikke fullført.',
      })
      expect(controller.state.status).toBe('error')
      expect(controller.state.taskSteps.filter((step) => step.status === 'active')).toHaveLength(0)
    })
  })

  it('still settles as a lost connection when the run cannot be read', async () => {
    mocks.getRun.mockRejectedValue(new ApiError('not found', 404, 'run_not_found'))
    await withController(async (controller) => {
      const stream = await startTurn(controller)
      stream.handlers.onMessage?.({ content: 'Partial' })
      stream.resolve()
      await tick(12)
      expect(controller.state.turns.at(-1)).toMatchObject({ status: 'stopped', content: 'Partial' })
      expect(controller.state.status).toBe('idle')
    })
  })

  it('tells the user to sign in again when the resume is refused', async () => {
    mocks.resumeStream.mockImplementationOnce(async (_id, handlers) => handlers.onError?.({
      code: 'unauthorized',
      message: 'Økten din er utløpt. Logg inn på nytt for å se svaret.',
    }))
    await withController(async (controller) => {
      const stream = await startTurn(controller)
      stream.resolve()
      await tick(12)
      expect(controller.state.turns.at(-1)?.status).toBe('error')
      expect(controller.state.error).toContain('Logg inn på nytt')
      expect(mocks.getRun).not.toHaveBeenCalled()
    })
  })
})

describe('Q15 reload reconciliation beyond cancelled runs', () => {
  const THREAD = 'thread-reload'
  const QUESTION = { id: '01M2ZNAY0W7NFJY543JZ2DJZTB', role: 'user' as const, content: 'Draft from sources.', createdAt: '' }

  function reloadInto(status: string, error?: string) {
    mocks.listChatThreads.mockResolvedValue([{
      threadId: THREAD, title: 'Draft', preview: 'Draft', updatedAt: '2026-09-23T10:00:00.000Z', pinned: false,
      latestRunId: RUN_ID, latestRunStatus: status,
    }])
    mocks.getThreadMessages.mockResolvedValue([QUESTION])
    mocks.listRuns.mockResolvedValue({ runs: [run(status, { threadId: THREAD, goal: QUESTION.content, error })], hasMore: false })
    setActiveChatThreadId(THREAD)
  }

  it('follows a run a reload finds still running, and adopts its answer when it lands', async () => {
    reloadInto('running')
    mocks.getRun
      .mockResolvedValueOnce(run('running', { threadId: THREAD }))
      .mockResolvedValueOnce(run('completed', { threadId: THREAD }))
    mocks.getChatThreadTranscript
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(transcript({ role: 'user', content: QUESTION.content }))
      .mockResolvedValueOnce(transcript({ role: 'user', content: QUESTION.content }, { role: 'assistant', content: 'The durable draft.' }))
    await withController(async (controller) => {
      await tick(30)
      expect(controller.state.threadId).toBe(THREAD)
      expect(controller.state.turns.at(-1)).toMatchObject({ role: 'assistant', status: 'waiting', runId: RUN_ID })
      expect(controller.state.status).toBe('streaming')
      await advance(1_000)
      expect(controller.state.turns.at(-1)).toMatchObject({ role: 'assistant', content: 'The durable draft.', status: undefined })
      expect(controller.state.status).toBe('idle')
      expect(mocks.streamChat).not.toHaveBeenCalled()
    })
  })

  it('shows a failed run a reload finds behind the trailing question', async () => {
    reloadInto('failed', 'provider_unavailable')
    await withController(async (controller) => {
      await tick(30)
      expect(controller.state.turns.at(-1)).toMatchObject({
        role: 'assistant',
        status: 'error',
        content: 'Modelltjenesten var utilgjengelig, så svaret ble ikke fullført.',
      })
      expect(mocks.getRun).not.toHaveBeenCalled()
    })
  })
})

describe('Q15 a Stop that tells the truth', () => {
  it('shows the saved answer when the server published it despite the Stop', async () => {
    mocks.getRun.mockResolvedValue(run('completed'))
    mocks.getChatThreadTranscript.mockResolvedValue(transcript(
      { role: 'user', content: 'Read this' },
      { role: 'assistant', content: 'The complete saved answer.' },
    ))
    await withController(async (controller) => {
      const stream = await startTurn(controller)
      stream.handlers.onMessage?.({ content: 'The comp' })
      await tick()
      controller.handleStop()
      flush()
      // The immediate response is unchanged.
      expect(controller.state.turns.at(-1)?.status).toBe('stopped')
      expect(controller.state.status).toBe('idle')
      expect(mocks.cancelInvocation).toHaveBeenCalledWith('req-1')
      stream.resolve()
      await tick(12)
      const turn = controller.state.turns.at(-1)
      expect(turn?.content).toBe('The complete saved answer.')
      expect(turn?.status).toBeUndefined()
      expect(controller.state.taskSteps.some((step) => step.id.endsWith(':saved-after-stop'))).toBe(true)
    })
  })

  it('keeps the stop, and records the unconfirmed cancel without content, when the run was cancelled', async () => {
    mocks.cancelInvocation.mockRejectedValue(new ApiError('gone', 404, 'not_found'))
    mocks.getRun.mockResolvedValue(run('cancelled'))
    await withController(async (controller) => {
      const stream = await startTurn(controller)
      stream.handlers.onMessage?.({ content: 'Partial' })
      await tick()
      controller.handleStop()
      stream.resolve()
      await tick(12)
      const turn = controller.state.turns.at(-1)
      expect(turn).toMatchObject({ status: 'stopped', content: 'Partial' })
      expect(turn?.cancelFailures).toEqual([{ target: 'invocation', status: 404, code: 'not_found', at: expect.any(String) }])
      expect(mocks.getChatThreadTranscript).not.toHaveBeenCalled()
    })
  })

  it('leaves a Stop as a stop when the run then failed, even if the thread holds its explanation', async () => {
    mocks.getRun.mockResolvedValue(run('failed', { error: 'inference_failed' }))
    mocks.getChatThreadTranscript.mockResolvedValue(transcript(
      { role: 'user', content: 'Read this' },
      { role: 'assistant', content: 'Et utkast kunne ikke kontrolleres mot kildene.' },
    ))
    await withController(async (controller) => {
      const stream = await startTurn(controller)
      stream.handlers.onMessage?.({ content: 'Partial' })
      await tick()
      controller.handleStop()
      stream.resolve()
      await tick(12)
      expect(mocks.getRun).toHaveBeenCalled()
      expect(controller.state.turns.at(-1)).toMatchObject({ status: 'stopped', content: 'Partial' })
      expect(controller.state.error).toBeNull()
    })
  })

  it('cancels a Stop pressed before `connected` with the id that frame brings, instead of detaching', async () => {
    await withController(async (controller) => {
      const stream = await startTurn(controller, null)
      controller.handleStop()
      flush()
      expect(controller.state.turns.at(-1)?.status).toBe('stopped')
      expect(controller.state.status).toBe('idle')
      // No id yet, so nothing was cancelled — and the stream is kept open.
      expect(mocks.cancelInvocation).not.toHaveBeenCalled()
      expect(stream.signal?.aborted).toBe(false)

      stream.handlers.onConnected?.({ ok: true, requestId: 'req-late', runId: RUN_ID })
      await tick()
      expect(mocks.cancelInvocation).toHaveBeenCalledWith('req-late')
      expect(stream.signal?.aborted).toBe(true)
      // Frames that were already on their way cannot reopen the turn.
      stream.handlers.onMessage?.({ content: 'late text' })
      stream.handlers.onDone?.({})
      stream.resolve()
      await tick(12)
      expect(controller.state.turns.at(-1)).toMatchObject({ status: 'stopped', content: '', requestId: 'req-late' })
      expect(mocks.resumeStream).not.toHaveBeenCalled()
    })
  })

  it('lets a deferred Stop go after the bound when `connected` never arrives', async () => {
    await withController(async (controller) => {
      const stream = await startTurn(controller, null)
      controller.handleStop()
      await advance(20_000)
      expect(stream.signal?.aborted).toBe(true)
      stream.resolve()
      await tick(8)
      expect(controller.state.turns.at(-1)?.status).toBe('stopped')
      expect(mocks.cancelInvocation).not.toHaveBeenCalled()
    })
  })
})

describe('turn timing, cost basis and receipt', () => {
  it('stores turn_timing and the receipt on the turn, and never keeps a subscription zero cost', async () => {
    await withController(async (controller) => {
      const stream = await startTurn(controller)
      stream.handlers.onUsage?.({ inputTokens: 1200, outputTokens: 300, costUsd: 0, costBasis: 'subscription', latencyMs: 43_000 })
      stream.handlers.onTurnTiming?.({ serverMs: 43_100, providerMs: 42_900, orchestrationMs: 200, phases: [] })
      stream.handlers.onResultReceipt?.({ receipt: { scope: 'direct_answer_word_range', words: 110 } })
      stream.handlers.onDone?.({})
      stream.resolve()
      await tick(8)
      const turn = controller.state.turns.at(-1)
      expect(turn?.costUsd).toBeUndefined()
      expect(turn?.costBasis).toBe('subscription')
      expect(turn?.timing).toMatchObject({ providerMs: 42_900, orchestrationMs: 200 })
      expect(turn?.resultReceipt).toEqual({ scope: 'direct_answer_word_range', words: 110 })
      const usageStep = controller.state.taskSteps.find((step) => step.id.endsWith(':usage'))
      expect(usageStep?.detail).toContain('included in subscription')
      expect(usageStep?.detail).not.toContain('$')
    })
  })

  it('keeps a metered cost as reported', async () => {
    await withController(async (controller) => {
      const stream = await startTurn(controller)
      stream.handlers.onUsage?.({ costUsd: 0.0031, costBasis: 'metered' })
      stream.handlers.onDone?.({})
      stream.resolve()
      await tick(8)
      expect(controller.state.turns.at(-1)).toMatchObject({ costUsd: 0.0031, costBasis: 'metered' })
    })
  })
})

describe('unmounting the chat', () => {
  /** Mount a controller whose disposal the test controls. */
  function mount() {
    let dispose!: () => void
    let controller!: ReturnType<typeof useChatController>
    createRoot((disposeRoot) => {
      dispose = disposeRoot
      controller = useChatController()
    })
    return { controller, dispose }
  }

  /** The assistant turns the local transcript store holds, across threads. */
  const storedAssistantStatuses = () => {
    const raw = window.localStorage.getItem(CHAT_THREAD_TRANSCRIPTS_KEY)
    const transcripts = raw ? JSON.parse(raw) as Array<{ turns: Array<{ role: string; status?: string }> }> : []
    return transcripts.flatMap((item) => item.turns.filter((turn) => turn.role === 'assistant').map((turn) => turn.status))
  }

  it('aborts the live stream, and neither follows the run nor rewrites the thread afterwards', async () => {
    const { controller, dispose } = mount()
    const stream = await startTurn(controller)
    stream.handlers.onMessage?.({ content: 'Partial' })
    await tick()
    dispose()
    expect(stream.signal?.aborted).toBe(true)
    stream.resolve()
    await tick(12)
    await advance(20_000)
    // No reattach and no durable wait from a controller nobody sees: the
    // remounted chat resumes the turn itself.
    expect(mocks.resumeStream).not.toHaveBeenCalled()
    expect(mocks.getRun).not.toHaveBeenCalled()
    // The abort is not the user's Stop, so the stored turn must not say so.
    expect(storedAssistantStatuses()).not.toContain('stopped')
  })

  it('starts no durable wait when a resume ends after the chat unmounted', async () => {
    let resume: { handlers: ChatStreamHandlers; signal?: AbortSignal; resolve: () => void } | undefined
    mocks.resumeStream.mockImplementationOnce((_id: string, handlers: ChatStreamHandlers, signal?: AbortSignal) =>
      new Promise<void>((resolve) => { resume = { handlers, signal, resolve } }))
    const { controller, dispose } = mount()
    const stream = await startTurn(controller)
    // Closed without an outcome: the controller reattaches.
    stream.resolve()
    await tick(8)
    expect(mocks.resumeStream).toHaveBeenCalledTimes(1)
    dispose()
    expect(resume?.signal?.aborted).toBe(true)
    resume?.handlers.onError?.({ code: 'connection_error', message: 'Request failed (404)' })
    resume?.resolve()
    await tick(12)
    await advance(20_000)
    expect(mocks.getRun).not.toHaveBeenCalled()
    expect(mocks.getChatThreadTranscript).not.toHaveBeenCalled()
  })
})

describe('a ChatGPT-subscription refusal is never retried by the chat', () => {
  it('holds a usage limit until its reset, and sends nothing when the reset passes', async () => {
    const resetsAt = new Date(Date.now() + 3_600_000).toISOString()
    await withController(async (controller) => {
      const stream = await startTurn(controller)
      stream.handlers.onMessage?.({ content: 'Delvis' })
      await tick()
      stream.handlers.onError?.({
        code: 'subscription_usage_limit',
        message: 'Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex.',
        retryable: false,
        resetsAt,
        retryAfterMs: 3_600_000,
      })
      stream.resolve()
      await tick(8)

      const turn = controller.state.turns.at(-1)
      expect(turn).toMatchObject({ status: 'error', failureAction: 'wait', retryAt: resetsAt, unsavedContent: 'Delvis' })
      expect(turn?.content).toContain('Den tilbakestilles')
      expect(controller.state.error).toBe(turn?.content)
      // A refusal, not a failed check: nothing in the Work feed is failed.
      expect(controller.state.taskSteps.filter((step) => step.status === 'error')).toHaveLength(0)

      await advance(3_600_000 + 60_000)
      expect(mocks.streamChat).toHaveBeenCalledTimes(1)
      expect(mocks.resumeStream).not.toHaveBeenCalled()
      expect(controller.state.turns.at(-1)).toMatchObject({ status: 'error', failureAction: 'wait', retryAt: resetsAt })
    })
  })

  it('waits out a throttle delay the same way, without a resend', async () => {
    await withController(async (controller) => {
      const stream = await startTurn(controller)
      stream.handlers.onError?.({ code: 'rate_limited', message: 'x', retryable: true, retryAfterMs: 2_000 })
      stream.resolve()
      await tick(8)
      const turn = controller.state.turns.at(-1)
      expect(turn).toMatchObject({ status: 'error', failureAction: 'wait' })
      expect(Date.parse(turn?.retryAt ?? '')).toBeGreaterThan(Date.now())
      expect(turn?.content).toContain('Prøv igjen om 2 sekunder')
      await advance(10_000)
      expect(mocks.streamChat).toHaveBeenCalledTimes(1)
      expect(mocks.resumeStream).not.toHaveBeenCalled()
    })
  })
})
