// @vitest-environment jsdom

/**
 * The run lifecycle notices a turn collects from its stream (WS15 S5). The frames go through the real chat client;
 * only the SSE reader underneath is scripted, as in `use-chat-controller.stream-churn`. What is pinned here:
 *
 *  - each notice lands on the turn in the order the stream sends it, with counts and closed tokens only;
 *  - a retry is matched to the newest call of its tool that is still out, never to one that already finished;
 *  - an exact replay is not added twice, and a turn keeps its newest twenty notices;
 *  - the notices are session-only: an ordinary thread's local transcript carries none of them, and a temporary (ZDR)
 *    thread writes no transcript and no server snapshot at all.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { RunDetail } from '@/shared/api/runs-client'
import type { SseEvent } from '@/shared/api/sse'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { CHAT_THREAD_TRANSCRIPTS_KEY } from '@/features/chat/lib/chat-thread-history'
import { LIFECYCLE_NOTICE_CAP } from './chat-types'

type Frame = { event: string; data: Record<string, unknown>; id?: string }

type OpenStream = {
  emit: (frame: Frame) => void
  end: () => void
}

const sse = vi.hoisted(() => ({ streams: [] as OpenStream[] }))

vi.mock('@/shared/api/sse', () => ({
  readSseStream: vi.fn((
    _path: string,
    options: { signal?: AbortSignal },
    onEvent: (event: SseEvent) => void,
  ) => new Promise<void>((resolve) => {
    let open = true
    const close = () => {
      if (!open) return
      open = false
      resolve()
    }
    sse.streams.push({
      emit: (frame) => {
        if (!open) return
        onEvent({ event: frame.event, data: JSON.stringify(frame.data), ...(frame.id ? { id: frame.id } : {}) })
      },
      end: close,
    })
    options.signal?.addEventListener('abort', close, { once: true })
  })),
}))

const mocks = vi.hoisted(() => ({
  cancelRun: vi.fn(async (runId: string) => ({ runId, cancelled: true, status: 'cancelled', receiptId: 'receipt-1' })),
  getChatThreadTranscript: vi.fn(async () => null as unknown),
  listChatThreads: vi.fn(async () => [] as unknown[]),
  getThreadMessages: vi.fn(async () => [] as unknown[]),
  getRun: vi.fn(async () => null as RunDetail | null),
  listRuns: vi.fn(async () => ({ runs: [] as RunDetail[], hasMore: false })),
  saveChatThreadSnapshot: vi.fn(async () => null as unknown),
}))

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    cancelInvocation: vi.fn(async () => undefined),
    getChatThreadTranscript: mocks.getChatThreadTranscript,
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: mocks.listChatThreads,
    saveChatThreadSnapshot: mocks.saveChatThreadSnapshot,
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

import { useChatController } from './use-chat-controller'

const THREAD = 'thread-lifecycle'
const RUN_ID = '01M2ZNAZ0W48WCREHFTGPKPNV3'
const QUESTION = 'Hva skjedde med leveransene i uke 42?'
const ANSWER = 'Leveransene ble forsinket to dager.'

const RETRY = { scope: 'tool', tool: 'web_search', attempt: 2, max_attempts: 3, wait_ms: 500, reason: 'timeout' }

const frame = (event: string, data: Record<string, unknown>, id?: number): Frame => ({
  event,
  data,
  ...(id !== undefined ? { id: String(id) } : {}),
})

async function tick(times = 3) {
  for (let i = 0; i < times; i += 1) {
    await Promise.resolve()
    flush()
  }
}

function mount(): { controller: ReturnType<typeof useChatController>; dispose: () => void } {
  let dispose = () => {}
  let controller!: ReturnType<typeof useChatController>
  createRoot((rootDispose) => {
    dispose = rootDispose
    controller = useChatController()
  })
  return { controller, dispose }
}

async function startTurn(
  controller: ReturnType<typeof useChatController>,
  options: { zdr?: boolean } = {},
): Promise<OpenStream> {
  await tick()
  await controller.handleComposerSubmit({ actions: [], attachments: [], text: QUESTION, tools: [], ...options })
  await tick()
  const stream = sse.streams.at(-1)
  if (!stream) throw new Error('no stream opened')
  // A ZDR turn creates no thread or run server-side, so its `connected` frame names neither.
  stream.emit(frame(
    'connected',
    options.zdr ? { ok: true, request_id: 'req-1' } : { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID },
    1,
  ))
  await tick()
  return stream
}

/** The assistant turn the stream is writing into is the last turn. */
const streamingTurn = (controller: ReturnType<typeof useChatController>) => controller.state.turns.at(-1)

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  mocks.listChatThreads.mockResolvedValue([])
})

afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
  sse.streams.length = 0
  for (const mock of Object.values(mocks)) mock.mockReset()
  mocks.cancelRun.mockImplementation(async (runId: string) => ({ runId, cancelled: true, status: 'cancelled', receiptId: 'receipt-1' }))
  mocks.getChatThreadTranscript.mockResolvedValue(null)
  mocks.getThreadMessages.mockResolvedValue([])
  mocks.getRun.mockResolvedValue(null)
  mocks.listRuns.mockResolvedValue({ runs: [], hasMore: false })
  mocks.saveChatThreadSnapshot.mockResolvedValue(null)
  __resetRetentionForTests()
  window.localStorage.clear()
})

describe('run lifecycle notices on a turn (WS15)', () => {
  it('collects each notice in the order the stream sends it', async () => {
    const { controller, dispose } = mount()
    const live = await startTurn(controller)
    live.emit(frame('tool_call', { id: 'tc-1', name: 'web_search', args: {} }))
    live.emit(frame('retry_scheduled', RETRY))
    live.emit(frame('compaction', { scope: 'history', outcome: 'summarized', messages_compacted: 12, messages_kept: 4 }))
    live.emit(frame('context_change', { change: 'history_shed', messages_dropped: 3 }))
    live.emit(frame('retrieval_notice', { code: 'zdr_dense_unavailable', wording_key: 'retrieval.zdr_dense_unavailable' }))
    await tick()

    expect(streamingTurn(controller)?.lifecycle).toEqual([
      { kind: 'retry', tool: 'web_search', attempt: 2, maxAttempts: 3, reason: 'timeout', callId: 'tc-1' },
      { kind: 'compaction', scope: 'history', outcome: 'summarized' },
      { kind: 'context', change: 'history_shed', messagesDropped: 3 },
      { kind: 'retrieval', wordingKey: 'retrieval.zdr_dense_unavailable' },
    ])
    dispose()
  })

  it('matches a retry to the call still out, not to one that already finished', async () => {
    const { controller, dispose } = mount()
    const live = await startTurn(controller)
    live.emit(frame('tool_call', { id: 'tc-1', name: 'web_search' }))
    live.emit(frame('tool_result', { id: 'tc-1', output: '{}', status: 'done' }))
    live.emit(frame('tool_call', { id: 'tc-2', name: 'web_search' }))
    live.emit(frame('retry_scheduled', RETRY))
    await tick()

    expect(streamingTurn(controller)?.lifecycle).toEqual([
      { kind: 'retry', tool: 'web_search', attempt: 2, maxAttempts: 3, reason: 'timeout', callId: 'tc-2' },
    ])
    dispose()
  })

  it('does not add an exact replay twice, and keeps the newest twenty', async () => {
    const { controller, dispose } = mount()
    const live = await startTurn(controller)
    live.emit(frame('retry_scheduled', RETRY))
    live.emit(frame('retry_scheduled', RETRY))
    await tick()
    expect(streamingTurn(controller)?.lifecycle).toHaveLength(1)

    for (let dropped = 1; dropped <= LIFECYCLE_NOTICE_CAP + 5; dropped += 1) {
      live.emit(frame('context_change', { change: 'history_shed', messages_dropped: dropped }))
    }
    await tick()

    const kept = streamingTurn(controller)?.lifecycle ?? []
    expect(kept).toHaveLength(LIFECYCLE_NOTICE_CAP)
    expect(kept[0]).toEqual({ kind: 'context', change: 'history_shed', messagesDropped: 6 })
    expect(kept.at(-1)).toEqual({ kind: 'context', change: 'history_shed', messagesDropped: LIFECYCLE_NOTICE_CAP + 5 })
    dispose()
  })

  it('keeps the notices out of the local transcript of an ordinary thread', async () => {
    const { controller, dispose } = mount()
    const live = await startTurn(controller)
    live.emit(frame('tool_call', { id: 'tc-1', name: 'web_search' }))
    live.emit(frame('retry_scheduled', RETRY))
    live.emit(frame('chunk', { delta: ANSWER }))
    live.emit(frame('done', { request_id: 'req-1' }))
    await tick()
    vi.advanceTimersByTime(600)
    await tick()

    const stored = window.localStorage.getItem(CHAT_THREAD_TRANSCRIPTS_KEY) ?? ''
    // The answer was written, so the check below reads a real transcript and not an absent one.
    expect(stored).toContain(ANSWER)
    expect(stored).not.toContain('"lifecycle"')
    expect(stored).not.toContain('"kind":"retry"')
    dispose()
  })

  it('writes no transcript and no server snapshot for a temporary thread, and keeps its notices for the turn', async () => {
    const { controller, dispose } = mount()
    const live = await startTurn(controller, { zdr: true })
    live.emit(frame('retry_scheduled', RETRY))
    live.emit(frame('chunk', { delta: ANSWER }))
    live.emit(frame('done', { request_id: 'req-1' }))
    await tick()
    vi.advanceTimersByTime(600)
    await tick()

    expect(streamingTurn(controller)?.lifecycle).toHaveLength(1)
    expect(window.localStorage.getItem(CHAT_THREAD_TRANSCRIPTS_KEY) ?? '').not.toContain(ANSWER)
    expect(mocks.saveChatThreadSnapshot).not.toHaveBeenCalled()
    dispose()
  })
})
