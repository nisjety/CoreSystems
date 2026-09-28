// @vitest-environment jsdom

/**
 * Wave-2 recovery, driven through the real chat client.
 *
 * `streamChat` and `resumeStream` are the real ones; only the SSE reader
 * underneath is scripted, so every frame goes through the same filter, cursor
 * and dispatch a browser uses. The gateway now buffers the tool phase (tool
 * calls and results, steps, citations, verification, queued input) under SSE
 * ids, and a resume replays it, so:
 *
 *  - a resume after a reload (no cursor, replay from the start) must land on
 *    what the turn already holds, never beside it;
 *  - a resume with a cursor must refuse a frame it already applied, even if
 *    the server sends it again;
 *  - failures the server reports are shown for what they are: an unavailable
 *    model service is not a failed check, and answer text the server did not
 *    save must not look saved;
 *  - a stream that already delivered its outcome is not reattached, and a
 *    resume that closes with nothing is settled from the run, not as a lost
 *    connection;
 *  - Stop cancels the durable run of every turn that has one.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { RunDetail } from '@/shared/api/runs-client'
import type { SseEvent } from '@/shared/api/sse'
import { ApiError } from '@/shared/api/http'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'

type Frame = { event: string; data: Record<string, unknown>; id?: string }

type OpenStream = {
  path: string
  lastEventId?: string
  emit: (frame: Frame) => void
  fail: (error?: unknown) => void
  end: () => void
}

type ResumeScript = { frames: Frame[]; fail?: unknown }

const sse = vi.hoisted(() => ({
  streams: [] as OpenStream[],
  resumeScripts: [] as ResumeScript[],
}))

vi.mock('@/shared/api/sse', () => ({
  readSseStream: vi.fn((
    path: string,
    options: { lastEventId?: string; signal?: AbortSignal },
    onEvent: (event: SseEvent) => void,
    onError?: (error: unknown) => void,
  ) => new Promise<void>((resolve) => {
    let open = true
    const close = () => {
      if (!open) return
      open = false
      resolve()
    }
    const stream: OpenStream = {
      path,
      lastEventId: options.lastEventId,
      emit: (frame) => {
        if (!open) return
        onEvent({ event: frame.event, data: JSON.stringify(frame.data), ...(frame.id ? { id: frame.id } : {}) })
      },
      fail: (error = new Error('network down')) => {
        if (!open) return
        onError?.(error)
        close()
      },
      end: close,
    }
    options.signal?.addEventListener('abort', close, { once: true })
    sse.streams.push(stream)
    if (path.includes('/resume/')) {
      const script = sse.resumeScripts.shift() ?? { frames: [] }
      for (const frame of script.frames) stream.emit(frame)
      if (script.fail) stream.fail(script.fail)
      else close()
    }
  })),
}))

const mocks = vi.hoisted(() => ({
  cancelInvocation: vi.fn(async () => undefined),
  cancelRun: vi.fn(async (runId: string) => ({ runId, cancelled: true, status: 'cancelled', receiptId: 'receipt-1' })),
  getChatThreadTranscript: vi.fn(async () => null as unknown),
  listChatThreads: vi.fn(async () => [] as unknown[]),
  getThreadMessages: vi.fn(async () => [] as unknown[]),
  getRun: vi.fn(async () => null as RunDetail | null),
  listRuns: vi.fn(async () => ({ runs: [] as RunDetail[], hasMore: false })),
}))

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    cancelInvocation: mocks.cancelInvocation,
    getChatThreadTranscript: mocks.getChatThreadTranscript,
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: mocks.listChatThreads,
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
    getThreadMessages: mocks.getThreadMessages,
  }
})

vi.mock('@/shared/api/orchestration-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/orchestration-client')>()
  return { ...actual, cancelRun: mocks.cancelRun, listApprovals: vi.fn(async () => []) }
})

vi.mock('@/shared/api/runs-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/runs-client')>()
  return { ...actual, getRun: mocks.getRun, listRuns: mocks.listRuns }
})

import { readChatThreadTranscript } from '@/features/chat/lib/chat-thread-history'
import { deriveConversationNodes } from '@/shared/chat-nodes'
import { useChatController } from './use-chat-controller'

const THREAD = 'thread-replay'
const RUN_ID = '01M2ZNAZ0W48WCREHFTGPKPNV2'
const QUESTION = 'Lag et utkast fra kildene'

const frame = (event: string, data: Record<string, unknown>, id?: number): Frame => ({
  event,
  data,
  ...(id !== undefined ? { id: String(id) } : {}),
})

/** The live tool phase of one turn, with the ids the gateway gives it. */
const toolPhase: Frame[] = [
  frame('connected', { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID }, 1),
  frame('reasoning_delta', { delta: 'Leser kildene.' }, 2),
  frame('tool_call', { id: 'call-1', name: 'web_search', args: { query: 'laks' } }, 3),
  frame('tool_result', { id: 'call-1', output: 'ok', status: 'done' }, 4),
  // A step the server sent without an id of its own.
  frame('step_update', { title: 'Kontrollerer kildene', status: 'running' }, 5),
  frame('citation', { id: 'cit-1', title: 'Kilde', url: 'https://example.com/laks' }, 6),
  frame('verification', { verdict: 'supports', kb_citations: 1, web_citations: 1, web_allowed: true }, 7),
  frame('queued_input', { messages: ['bruk EUR'] }, 8),
  frame('chunk', { delta: 'Delvis' }, 9),
]

async function tick(times = 3) {
  for (let i = 0; i < times; i += 1) {
    await Promise.resolve()
    flush()
  }
}

/** Mount a controller; the returned dispose is the page unloading. */
function mount(): { controller: ReturnType<typeof useChatController>; dispose: () => void } {
  let dispose = () => {}
  let controller!: ReturnType<typeof useChatController>
  createRoot((rootDispose) => {
    dispose = rootDispose
    controller = useChatController()
  })
  return { controller, dispose }
}

/**
 * Deliver live frames one network read at a time, as a browser receives them:
 * each lands after the previous one's store writes are committed.
 */
async function deliver(stream: OpenStream, ...frames: Frame[]) {
  for (const next of frames) {
    stream.emit(next)
    await tick()
  }
}

async function send(controller: ReturnType<typeof useChatController>): Promise<OpenStream> {
  await tick()
  await controller.handleComposerSubmit({ actions: [], attachments: [], text: QUESTION, tools: [] })
  await tick()
  const stream = sse.streams.at(-1)
  if (!stream) throw new Error('no stream opened')
  return stream
}

const resumes = () => sse.streams.filter((stream) => stream.path.includes('/resume/'))
const stepsNamed = (controller: ReturnType<typeof useChatController>, title: string) =>
  controller.state.taskSteps.filter((step) => step.title === title)

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  mocks.listChatThreads.mockResolvedValue([
    { threadId: THREAD, title: 'Utkast', preview: QUESTION, updatedAt: '2026-09-24T08:00:00.000Z', pinned: false },
  ])
})

afterEach(() => {
  vi.useRealTimers()
  sse.streams.length = 0
  sse.resumeScripts.length = 0
  for (const mock of Object.values(mocks)) mock.mockReset()
  mocks.cancelInvocation.mockResolvedValue(undefined)
  mocks.cancelRun.mockImplementation(async (runId: string) => ({ runId, cancelled: true, status: 'cancelled', receiptId: 'receipt-1' }))
  mocks.getChatThreadTranscript.mockResolvedValue(null)
  mocks.getThreadMessages.mockResolvedValue([])
  mocks.getRun.mockResolvedValue(null)
  mocks.listRuns.mockResolvedValue({ runs: [], hasMore: false })
  __resetRetentionForTests()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('a resume never duplicates what the turn already holds', () => {
  it('after a reload, replays the tool phase from the start onto the same tool call, source and step', async () => {
    // First page: the tool phase streams, then the page goes away mid-answer.
    const first = mount()
    const live = await send(first.controller)
    for (const f of toolPhase) live.emit(f)
    await tick(6)
    expect(first.controller.state.turns.at(-1)).toMatchObject({ status: 'waiting', content: 'Delvis', requestId: 'req-1' })
    expect(stepsNamed(first.controller, 'Kontrollerer kildene')).toHaveLength(1)
    first.dispose()

    // Second page: the server holds only the question; the buffered stream is
    // replayed from its start (the cursor is not persisted), then continues.
    mocks.getThreadMessages.mockResolvedValue([{ id: 'msg-user', role: 'user', content: QUESTION, createdAt: '' }])
    sse.resumeScripts.push({
      frames: [
        ...toolPhase,
        frame('chunk', { delta: ' og resten.' }, 10),
        frame('done', { request_id: 'req-1' }, 11),
      ],
    })
    const second = mount()
    await tick(30)

    expect(resumes()).toHaveLength(1)
    expect(resumes()[0]?.lastEventId).toBeUndefined()
    const turn = second.controller.state.turns.at(-1)
    expect(turn?.content).toBe('Delvis og resten.')
    expect(turn?.status).toBeUndefined()
    // Reasoning is append-only too: replayed once, not appended to the copy.
    expect(turn?.reasoning).toBe('Leser kildene.')
    expect(turn?.toolCalls?.map((call) => call.id)).toEqual(['call-1'])
    expect(turn?.citations?.map((citation) => citation.id)).toEqual(['cit-1'])
    expect(turn?.verification).toMatchObject({ verdict: 'supports' })
    // The step that had no id of its own is keyed on its frame, so the
    // replay updated it instead of adding a twin.
    expect(stepsNamed(second.controller, 'Kontrollerer kildene')).toHaveLength(1)
    const ids = second.controller.state.taskSteps.map((step) => step.id)
    expect(new Set(ids).size).toBe(ids.length)
    second.dispose()
  })

  /**
   * chat-wave2 citations spec §2.4/§3: the post-stream anchor pass re-emits a
   * `citation` frame with the SAME id, now carrying `claim_id`, once the
   * answer is complete. `SinkResume`'s buffer is generic over any frame and
   * keys purely on the stream's own sequence number — never on a citation's
   * logical id — so this update must survive a reload/resume exactly like
   * the original citation event does, landing merged onto the one row
   * (`upsertCitation`'s merge-on-duplicate-id change), never as a second row
   * and never dropped.
   */
  it('an anchor-pass citation update (same id, now with claimId) survives a reload and resume', async () => {
    const first = mount()
    const live = await send(first.controller)
    live.emit(frame('connected', { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID }, 1))
    live.emit(frame('chunk', { delta: 'Delvis' }, 2))
    live.emit(frame('citation', { id: 'cit-1', title: 'Kilde', url: 'https://example.com/laks' }, 3))
    await tick(6)
    const liveCitations = first.controller.state.turns.at(-1)?.citations
    expect(liveCitations).toHaveLength(1)
    expect(liveCitations?.[0]).toMatchObject({ id: 'cit-1', url: 'https://example.com/laks' })
    first.dispose()

    // Second page: the server holds only the question; the buffered stream
    // replays BOTH citation frames — the original and the anchor-pass update.
    mocks.getThreadMessages.mockResolvedValue([{ id: 'msg-user', role: 'user', content: QUESTION, createdAt: '' }])
    sse.resumeScripts.push({
      frames: [
        frame('connected', { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID }, 1),
        frame('chunk', { delta: 'Delvis' }, 2),
        frame('citation', { id: 'cit-1', title: 'Kilde', url: 'https://example.com/laks' }, 3),
        frame('citation', {
          id: 'cit-1',
          title: 'Kilde',
          url: 'https://example.com/laks',
          claim_id: 'req-1:claim-0',
          start: 4,
          end: 7,
        }, 4),
        frame('done', { request_id: 'req-1' }, 5),
      ],
    })
    const second = mount()
    await tick(30)

    const turn = second.controller.state.turns.at(-1)
    // ONE row, not two — the second frame merged onto the first rather than
    // adding a twin or being dropped.
    expect(turn?.citations).toHaveLength(1)
    expect(turn?.citations?.[0]).toMatchObject({
      id: 'cit-1',
      url: 'https://example.com/laks',
      claimId: 'req-1:claim-0',
      start: 4,
      end: 7,
    })
    second.dispose()
  })

  it('with a cursor, refuses frames it already applied even when the server sends them again', async () => {
    const { controller, dispose } = mount()
    const live = await send(controller)
    await deliver(live, frame('connected', { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID }, 1))
    await deliver(live, frame('tool_call', { id: 'call-1', name: 'web_search', args: { query: 'laks' } }, 2))
    await deliver(live, frame('step_update', { title: 'Søker', status: 'running' }, 3))
    await deliver(live, frame('chunk', { delta: 'Hei' }, 4))
    await tick()
    // The server replays from the start although we sent Last-Event-ID 4 (an
    // overlap between its replay and its live tail looks exactly like this).
    sse.resumeScripts.push({
      frames: [
        frame('tool_call', { id: 'call-1', name: 'web_search', args: { query: 'laks' } }, 2),
        frame('step_update', { title: 'Søker', status: 'running' }, 3),
        frame('chunk', { delta: 'Hei' }, 4),
        frame('tool_result', { id: 'call-1', output: 'ok', status: 'done' }, 5),
        frame('citation', { id: 'cit-1', title: 'Kilde', url: 'https://example.com/laks' }, 6),
        frame('chunk', { delta: ' der' }, 7),
        frame('done', { request_id: 'req-1' }, 8),
      ],
    })
    const eventsBefore = controller.uiEvents().length
    live.fail(new Error('network down'))
    await tick(12)

    expect(resumes()).toHaveLength(1)
    expect(resumes()[0]?.lastEventId).toBe('4')
    const turn = controller.state.turns.at(-1)
    expect(turn?.content).toBe('Hei der')
    expect(turn?.status).toBeUndefined()
    expect(turn?.toolCalls).toHaveLength(1)
    expect(turn?.toolCalls?.[0]).toMatchObject({ id: 'call-1', output: 'ok' })
    expect(turn?.citations).toHaveLength(1)
    expect(stepsNamed(controller, 'Søker')).toHaveLength(1)
    // Only the four new frames reached the Trace record.
    expect(controller.uiEvents().length - eventsBefore).toBe(4)
    expect(turn?.lastFrameId).toBe('8')
    dispose()
  })
})

describe('server failures are shown for what they are', () => {
  const SUBSCRIPTION_MESSAGE = 'Den valgte ChatGPT-abonnementstilkoblingen svarte ikke mens utkastet ble kontrollert mot kildene. Dette utkastet er ikke publisert. Prøv igjen om litt. Ingen annen modell er brukt.'

  it('shows response_validation_unavailable as an unavailable service, not as a failed check', async () => {
    const { controller, dispose } = mount()
    const live = await send(controller)
    await deliver(live, frame('connected', { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID }, 1))
    await deliver(live, frame('step_update', { id: 'draft-1:source-check', title: 'Kontrollerer utkastet mot kildene', status: 'running' }, 2))
    await deliver(live, frame('error', { code: 'response_validation_unavailable', message: SUBSCRIPTION_MESSAGE, retryable: true }, 3))
    live.end()
    await tick(8)

    const turn = controller.state.turns.at(-1)
    expect(turn).toMatchObject({ status: 'error', content: SUBSCRIPTION_MESSAGE })
    expect(controller.state.error).toBe(SUBSCRIPTION_MESSAGE)
    // No check failed: nothing in the Work feed is marked failed...
    expect(controller.state.taskSteps.filter((step) => step.status === 'error')).toHaveLength(0)
    expect(controller.state.taskSteps.find((step) => step.id.endsWith(':event-draft-1:source-check'))?.status).toBe('stopped')
    // ...and a service step says what happened instead.
    expect(controller.state.taskSteps.find((step) => step.id.endsWith(':service-unavailable'))).toMatchObject({
      status: 'stopped',
      detail: SUBSCRIPTION_MESSAGE,
    })
    expect(resumes()).toHaveLength(0)
    dispose()
  })

  it('does not keep streamed text as the answer after inference_stream_ended_without_terminal', async () => {
    const { controller, dispose } = mount()
    const live = await send(controller)
    await deliver(live, frame('connected', { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID }, 1))
    await deliver(live, frame('chunk', { delta: 'Første del av svaret' }, 2))
    await deliver(live, frame('error', {
      code: 'inference_stream_ended_without_terminal',
      message: 'The answer stream ended before it was complete. Please try again.',
      retryable: true,
    }, 3))
    live.end()
    await tick(8)

    const turn = controller.state.turns.at(-1)
    expect(turn?.status).toBe('error')
    expect(turn?.content).toContain('ingenting ble lagret')
    expect(turn?.unsavedContent).toBe('Første del av svaret')
    // A server outcome, not a lost connection: nothing is reattached.
    expect(resumes()).toHaveLength(0)
    dispose()
  })

  it('does not let a turn look saved when session_liveness_failed follows the whole answer', async () => {
    const { controller, dispose } = mount()
    const live = await send(controller)
    await deliver(live, frame('connected', { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID }, 1))
    await deliver(live, frame('chunk', { delta: 'Hele svaret er her.' }, 2))
    await deliver(live, frame('error', { code: 'session_liveness_failed', message: 'The chat run is no longer active. Please retry.' }, 3))
    live.end()
    await tick(8)

    const turn = controller.state.turns.at(-1)
    expect(turn?.status).toBe('error')
    expect(turn?.content).toBe('Svaret ble ikke lagret, fordi kjøringen ikke lenger var aktiv. Prøv igjen.')
    expect(turn?.unsavedContent).toBe('Hele svaret er her.')
    // Rendered as a failure; the text sits behind the "not saved" disclosure.
    const answer = deriveConversationNodes(turn!).find((node) => node.kind === 'answer')
    expect(answer).toMatchObject({ answer: { state: 'failed', unsaved: 'Hele svaret er her.' } })
    // And nothing persisted says otherwise: the unsaved text is not stored.
    const stored = readChatThreadTranscript(THREAD)?.turns.at(-1)
    expect(stored).toMatchObject({ role: 'assistant', status: 'error' })
    expect(JSON.stringify(readChatThreadTranscript(THREAD))).not.toContain('Hele svaret er her.')
    dispose()
  })
})

describe('a ChatGPT usage limit is shown with its reset, and nothing retries it', () => {
  // Whole seconds, as the broker writes it; two hours from now (the clock is real here).
  const RESET = new Date(Math.ceil((Date.now() + 2 * 3_600_000) / 1000) * 1000).toISOString().replace('.000Z', 'Z')
  const USAGE_FRAME = {
    code: 'subscription_usage_limit',
    message: 'Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex. Den tilbakestilles 26.09.2026 kl. 01:21 UTC. Ingen annen modell ble brukt.',
    retryable: false,
    resets_at: RESET,
    retry_after_ms: 7_200_000,
  }

  it('on the live stream: waits for the reset, and opens no new request past it', async () => {
    const { controller, dispose } = mount()
    const live = await send(controller)
    await deliver(live, frame('connected', { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID }, 1))
    await deliver(live, frame('error', USAGE_FRAME, 2))
    live.end()
    await tick(8)

    const turn = controller.state.turns.at(-1)
    expect(turn).toMatchObject({ status: 'error', failureAction: 'wait', retryAt: RESET })
    // The local wording, never the broker's UTC text.
    expect(turn?.content).toMatch(/^Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex\. Den tilbakestilles (i dag|i morgen) kl\. \d{2}:\d{2}\. Ingen annen modell ble brukt\.$/)
    expect(turn?.content).not.toContain('UTC')
    expect(deriveConversationNodes(turn!).find((node) => node.kind === 'answer'))
      .toMatchObject({ answer: { state: 'failed', action: 'wait', retryAt: RESET } })
    expect(controller.state.taskSteps.find((step) => step.id.endsWith(':service-unavailable')))
      .toMatchObject({ title: 'ChatGPT-planen har nådd bruksgrensen', status: 'stopped' })

    const opened = sse.streams.length
    await vi.advanceTimersByTimeAsync(2 * 3_600_000 + 60_000)
    await tick(8)
    // No resend, no resume: the retry is the reader's to press.
    expect(sse.streams).toHaveLength(opened)
    expect(resumes()).toHaveLength(0)
    expect(controller.state.turns.at(-1)).toMatchObject({ status: 'error', failureAction: 'wait' })
    dispose()
  })

  it('in a resume after a reload: presents the replayed refusal exactly as the live stream would', async () => {
    const first = mount()
    const live = await send(first.controller)
    live.emit(frame('connected', { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID }, 1))
    live.emit(frame('chunk', { delta: 'Delvis' }, 2))
    await tick(6)
    first.dispose()

    mocks.getThreadMessages.mockResolvedValue([{ id: 'msg-user', role: 'user', content: QUESTION, createdAt: '' }])
    sse.resumeScripts.push({
      frames: [
        frame('connected', { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID }, 1),
        frame('chunk', { delta: 'Delvis' }, 2),
        frame('error', USAGE_FRAME, 3),
      ],
    })
    const second = mount()
    await tick(30)

    expect(resumes()).toHaveLength(1)
    const turn = second.controller.state.turns.at(-1)
    expect(turn).toMatchObject({ status: 'error', failureAction: 'wait', retryAt: RESET, unsavedContent: 'Delvis' })
    expect(turn?.content).toContain('Den tilbakestilles')
    // Session-only: nothing about the wait is stored for a later reload.
    const stored = readChatThreadTranscript(THREAD)?.turns.at(-1)
    expect(stored).toMatchObject({ role: 'assistant', status: 'error' })
    expect(stored).not.toHaveProperty('failureAction')
    expect(stored).not.toHaveProperty('retryAt')
    second.dispose()
  })

  it('sends the reader to reconnect when the ChatGPT sign-in must be renewed', async () => {
    const { controller, dispose } = mount()
    const live = await send(controller)
    await deliver(live, frame('connected', { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID }, 1))
    await deliver(live, frame('error', { code: 'subscription_reauth_required', message: 'x', retryable: false }, 2))
    live.end()
    await tick(8)

    const turn = controller.state.turns.at(-1)
    expect(turn).toMatchObject({ status: 'error', failureAction: 'reconnect' })
    expect(turn?.retryAt).toBeUndefined()
    expect(turn?.content).toContain('Koble til ChatGPT på nytt under Profil → ChatGPT-abonnement')
    expect(controller.state.taskSteps.find((step) => step.id.endsWith(':service-unavailable'))?.title)
      .toBe('ChatGPT-tilkoblingen må fornyes')
    expect(resumes()).toHaveLength(0)
    dispose()
  })

  it('offers no retry for a request ChatGPT rejected, and does not report it as an outage', async () => {
    const { controller, dispose } = mount()
    const live = await send(controller)
    await deliver(live, frame('connected', { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID }, 1))
    await deliver(live, frame('error', { code: 'subscription_bad_request', message: 'x', retryable: false }, 2))
    live.end()
    await tick(8)

    expect(controller.state.turns.at(-1)).toMatchObject({ status: 'error', failureAction: 'none' })
    expect(controller.state.taskSteps.find((step) => step.id.endsWith(':service-unavailable'))).toBeUndefined()
    dispose()
  })
})

describe('an early failure in the same network read as `connected`', () => {
  it('is still applied, instead of leaving the turn waiting', async () => {
    const { controller, dispose } = mount()
    const live = await send(controller)
    // One read, three frames, dispatched back to back with no flush between.
    live.emit(frame('connected', { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID }, 1))
    live.emit(frame('turn_timing', { server_ms: 40, provider_ms: 0, orchestration_ms: 40, outcome: 'failed' }, 2))
    live.emit(frame('error', { code: 'response_validation_failed', message: 'Utkastet er ikke publisert.' }, 3))
    live.end()
    await tick(8)

    expect(controller.state.turns.at(-1)).toMatchObject({ status: 'error', content: 'Utkastet er ikke publisert.' })
    expect(controller.state.status).toBe('error')
    expect(resumes()).toHaveLength(0)
    dispose()
  })
})

describe('a stream that already ended is settled from its end', () => {
  it('does not reattach when the connection breaks after the terminal error arrived', async () => {
    mocks.getRun.mockRejectedValue(new ApiError('not found', 404, 'run_not_found'))
    const { controller, dispose } = mount()
    const live = await send(controller)
    await deliver(live, frame('connected', { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID }, 1))
    await deliver(live, frame('chunk', { delta: 'Svar' }, 2))
    await deliver(live, frame('error', { code: 'response_validation_failed', message: 'Utkastet er ikke publisert.' }, 3))
    live.fail(new Error('network down'))
    await tick(12)

    expect(resumes()).toHaveLength(0)
    expect(controller.state.turns.at(-1)).toMatchObject({ status: 'error', content: 'Svar' })
    expect(controller.state.error).toBe('Utkastet er ikke publisert.')
    dispose()
  })

  it('does not reattach when the connection breaks after done', async () => {
    const { controller, dispose } = mount()
    const live = await send(controller)
    await deliver(live, frame('connected', { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID }, 1))
    await deliver(live, frame('chunk', { delta: 'Ferdig svar.' }, 2))
    await deliver(live, frame('done', { request_id: 'req-1' }, 3))
    live.fail(new Error('network down'))
    await tick(12)

    expect(resumes()).toHaveLength(0)
    expect(controller.state.turns.at(-1)).toMatchObject({ content: 'Ferdig svar.', status: undefined, streaming: false })
    expect(controller.state.status).toBe('idle')
    dispose()
  })

  it('settles an empty resume from the run status, never as a lost connection', async () => {
    mocks.getRun.mockResolvedValue({
      runId: RUN_ID, status: 'failed', goal: QUESTION, error: 'provider_unavailable',
      checkpointIndex: 0, stepsCompleted: 0, inputTokens: 0, outputTokens: 0,
    })
    mocks.getChatThreadTranscript.mockResolvedValue({ threadId: THREAD, updatedAt: '', turns: [{ id: 'm1', role: 'user', content: QUESTION, createdAt: '' }] })
    const { controller, dispose } = mount()
    const live = await send(controller)
    await deliver(live, frame('connected', { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID }, 1))
    await deliver(live, frame('chunk', { delta: 'Delvis' }, 2))
    // The stream closes with no terminal frame; the resume that follows has
    // nothing after the cursor (the gateway closes it right after its buffer).
    live.end()
    await tick(16)

    expect(resumes()).toHaveLength(1)
    const turn = controller.state.turns.at(-1)
    expect(turn?.status).toBe('error')
    expect(turn?.content).toBe('Delvis')
    expect(controller.state.error).toBe('Modelltjenesten var utilgjengelig, så svaret ble ikke fullført.')
    const details = controller.state.taskSteps.map((step) => step.detail).join(' | ')
    expect(details).not.toMatch(/connection|forbindelse/i)
    dispose()
  })

  it('leaves an empty resume the server cannot explain stopped, without blaming the connection', async () => {
    mocks.getRun.mockRejectedValue(new ApiError('not found', 404, 'run_not_found'))
    const { controller, dispose } = mount()
    const live = await send(controller)
    await deliver(live, frame('connected', { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID }, 1))
    await deliver(live, frame('chunk', { delta: 'Delvis' }, 2))
    live.end()
    await tick(16)

    expect(resumes()).toHaveLength(1)
    expect(controller.state.turns.at(-1)).toMatchObject({ status: 'stopped', content: 'Delvis' })
    const details = controller.state.taskSteps.map((step) => step.detail).join(' | ')
    expect(details).not.toMatch(/connection|forbindelse/i)
    expect(details).toContain('tjenesten kunne ikke si hvordan kjøringen endte')
    dispose()
  })
})

describe('Stop cancels the durable run of every turn that has one', () => {
  it('cancels the run of a plain Ask turn, not only of a Work turn', async () => {
    mocks.getRun.mockResolvedValue({
      runId: RUN_ID, status: 'cancelled', goal: QUESTION,
      checkpointIndex: 0, stepsCompleted: 0, inputTokens: 0, outputTokens: 0,
    })
    const { controller, dispose } = mount()
    const live = await send(controller)
    await deliver(live, frame('connected', { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID }, 1))
    await deliver(live, frame('chunk', { delta: 'Delvis' }, 2))
    await tick()
    expect(controller.liveRunId()).toBeNull()
    controller.handleStop()
    await tick(8)

    expect(mocks.cancelInvocation).toHaveBeenCalledWith('req-1')
    expect(mocks.cancelRun).toHaveBeenCalledWith(RUN_ID)
    // The wave-1 reconciliation still runs and keeps the honoured stop.
    expect(mocks.getRun).toHaveBeenCalledWith(RUN_ID, expect.any(AbortSignal))
    expect(controller.state.turns.at(-1)).toMatchObject({ status: 'stopped', content: 'Delvis' })
    dispose()
  })

  it('cancels the run a deferred Stop learns from `connected`', async () => {
    const { controller, dispose } = mount()
    const live = await send(controller)
    controller.handleStop()
    flush()
    expect(mocks.cancelRun).not.toHaveBeenCalled()
    await deliver(live, frame('connected', { ok: true, request_id: 'req-late', thread_id: THREAD, run_id: RUN_ID }, 1))
    await tick(8)

    expect(mocks.cancelInvocation).toHaveBeenCalledWith('req-late')
    expect(mocks.cancelRun).toHaveBeenCalledWith(RUN_ID)
    dispose()
  })

  it('records a run cancel the server refused, without content', async () => {
    mocks.cancelRun.mockRejectedValue(new ApiError('gone', 404, 'run_not_found'))
    mocks.getRun.mockResolvedValue({
      runId: RUN_ID, status: 'cancelled', goal: QUESTION,
      checkpointIndex: 0, stepsCompleted: 0, inputTokens: 0, outputTokens: 0,
    })
    const { controller, dispose } = mount()
    const live = await send(controller)
    await deliver(live, frame('connected', { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID }, 1))
    await tick()
    controller.handleStop()
    await tick(8)

    expect(controller.state.turns.at(-1)?.cancelFailures).toEqual([
      { target: 'run', status: 404, code: 'run_not_found', at: expect.any(String) },
    ])
    dispose()
  })
})
