// @vitest-environment jsdom

/**
 * What one streamed token costs the controller outside the renderer.
 *
 * WS09 S1 ("measure first"), the half that `ChatMessages.render-budget.test.tsx`
 * cannot see. The transcript audit named the renderer; it missed three costs
 * that sit in `use-chat-controller.ts`, and whether they dominate was never
 * measured. This drives the real controller and the real chat client (only the
 * SSE reader underneath is scripted, as in `use-chat-controller.resume-replay`)
 * through a long answer at 40 frames per second and counts, per source:
 *
 *  - the local snapshot: `setItem` calls and bytes for
 *    `verevon.chat.threadTranscripts.v1`, per storage (the persistence helper
 *    writes localStorage AND sessionStorage), and the debounced server snapshot;
 *  - the Trace record: how many rows `uiEvents()` holds after the stream;
 *  - the Steps panel: how often the `taskSteps` array and the "Compose
 *    response" step are replaced by an equal copy.
 *
 * Only lengths are recorded, never the stored text: nothing here keeps content.
 *
 * WHAT IS ASSERTED. Structural counts (never milliseconds). S1 pinned the
 * per-token behaviour; WS09 S3 flipped exactly those lines to their targets
 * (transcript writes at most one per second, steps replaced once, one Trace row
 * per stream for `message.delta`). The ZDR cases at the bottom never changed: a
 * temporary thread writes nothing. `VITE_CHAT_SNAPSHOT_THROTTLE_MS=0` restores
 * the per-token numbers and is covered in `use-chat-controller.snapshot-throttle`.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { RunDetail } from '@/shared/api/runs-client'
import type { SseEvent } from '@/shared/api/sse'
import { __resetRetentionForTests, applyServerRetention } from '@/features/chat/lib/chat-retention'
import { CHAT_THREAD_TRANSCRIPTS_KEY } from '@/features/chat/lib/chat-thread-history'

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

const THREAD = 'thread-churn'
const RUN_ID = '01M2ZNAZ0W48WCREHFTGPKPNV2'
const QUESTION = 'Lag en lang oppsummering av kvartalet'

/** 40 frames per second: the cadence the streaming budget (plan section 21) is written for. */
const FRAME_MS = 25

const frame = (event: string, data: Record<string, unknown>, id?: number): Frame => ({
  event,
  data,
  ...(id !== undefined ? { id: String(id) } : {}),
})

/** About 20 paragraphs of ~8 deltas each: a long, real-shaped answer. */
const PARAGRAPHS = 20
const DELTAS_PER_PARAGRAPH = 8
const deltas = (): string[] => {
  const out: string[] = []
  for (let paragraph = 0; paragraph < PARAGRAPHS; paragraph += 1) {
    for (let index = 0; index < DELTAS_PER_PARAGRAPH; index += 1) {
      const tail = index === DELTAS_PER_PARAGRAPH - 1 ? '\n\n' : ' '
      out.push(`ord${paragraph}-${index} om omsetning og margin i kvartalet${tail}`)
    }
  }
  return out
}

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

/**
 * `setItem` calls and bytes for one key, per storage, plus how many of those
 * writes carried the user's text. Only lengths and that one boolean are kept.
 */
function watchStorage(key: string) {
  const writes = { local: { count: 0, bytes: 0 }, session: { count: 0, bytes: 0 }, withContent: 0 }
  const wrap = (storage: Storage, into: { count: number; bytes: number }) => {
    const original = storage.setItem.bind(storage)
    vi.spyOn(storage, 'setItem').mockImplementation((name: string, value: string) => {
      if (name === key) {
        into.count += 1
        into.bytes += value.length
        if (value.includes(QUESTION) || value.includes('ord0-0')) writes.withContent += 1
      }
      original(name, value)
    })
  }
  wrap(window.localStorage, writes.local)
  wrap(window.sessionStorage, writes.session)
  return { writes }
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

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  mocks.listChatThreads.mockResolvedValue([
    { threadId: THREAD, title: 'Kvartal', preview: QUESTION, updatedAt: '2026-10-21T08:00:00.000Z', pinned: false },
  ])
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
})

describe('stream churn: what one token costs outside the renderer', () => {
  it('writes the transcript to both storages at most once per second, not once per delta', async () => {
    const { controller, dispose } = mount()
    const live = await startTurn(controller)
    // The send itself queues one server snapshot (500 ms debounce); let it land
    // so the stream below is measured on its own.
    vi.advanceTimersByTime(600)
    await tick()
    mocks.saveChatThreadSnapshot.mockClear()
    const storage = watchStorage(CHAT_THREAD_TRANSCRIPTS_KEY)

    const all = deltas()
    const stepRef: unknown[] = []
    const stepsArrayRef: unknown[] = []
    const perDeltaBytes: number[] = []
    for (const delta of all) {
      const before = storage.writes.local.bytes
      live.emit(frame('chunk', { delta }))
      await tick()
      vi.advanceTimersByTime(FRAME_MS)
      perDeltaBytes.push(storage.writes.local.bytes - before)
      stepsArrayRef.push(controller.state.taskSteps)
      stepRef.push(controller.state.taskSteps.find((step) => step.title === 'Compose response'))
    }
    const streamWrites = { local: { ...storage.writes.local }, session: { ...storage.writes.session } }
    const serverSnapshotsMidStream = mocks.saveChatThreadSnapshot.mock.calls.length

    // The persistence effect still re-runs on every token, but the snapshot
    // writer lets one write through per second: at most ceil(duration) + 1
    // (the first call of the stream, then one per window), to EACH storage.
    const streamSeconds = (all.length * FRAME_MS) / 1000
    expect(streamWrites.local.count).toBeGreaterThanOrEqual(1)
    expect(streamWrites.local.count).toBeLessThanOrEqual(Math.ceil(streamSeconds) + 1)
    expect(streamWrites.session.count).toBe(streamWrites.local.count)
    // Total bytes are bounded by the seconds of stream, not the delta count: far
    // below the old "about deltas/2 times the final value".
    const finalValueBytes = Math.max(...perDeltaBytes, 1)
    expect(streamWrites.local.bytes / finalValueBytes).toBeLessThan(all.length / 10)
    // The per-delta writes pass `persistServer: false`, so the server copy is
    // saved only at submit and at the terminal event: nothing mid-stream, and the
    // local snapshot is the only copy of a long answer until it ends.
    expect(serverSnapshotsMidStream).toBe(0)
    // Steps: the unchanged "Compose response" step is no longer replaced by an
    // equal copy on every delta, so the array and the step keep their identity.
    expect(new Set(stepsArrayRef).size).toBeLessThanOrEqual(2)
    expect(new Set(stepRef).size).toBeLessThanOrEqual(2)
    expect(controller.state.taskSteps.filter((step) => step.title === 'Compose response')).toHaveLength(1)

    live.emit(frame('done', { request_id: 'req-1' }))
    await tick()
    vi.advanceTimersByTime(600)
    await tick()
    expect(mocks.saveChatThreadSnapshot).toHaveBeenCalledTimes(1)

    console.info('[stream-churn] writes', JSON.stringify({
      deltas: all.length,
      streamSeconds: (all.length * FRAME_MS) / 1000,
      localWrites: streamWrites.local.count,
      localBytes: streamWrites.local.bytes,
      sessionWrites: streamWrites.session.count,
      finalValueBytes,
      stepsArrayIdentities: new Set(stepsArrayRef).size,
      composeStepIdentities: new Set(stepRef).size,
    }))
    dispose()
  })

  it('records the first message.delta of a stream in the Trace, not one row per delta', async () => {
    const { controller, dispose } = mount()
    const live = await startTurn(controller)
    const before = controller.uiEvents().length
    const all = deltas()
    for (const delta of all) {
      live.emit(frame('chunk', { delta }))
      await tick()
    }
    const rows = controller.uiEvents().slice(before)
    const deltaRows = rows.filter((row) => row.type === 'message.delta')

    // The Trace used to get a row for every token, so a long answer was thousands
    // of text-fragment rows against a 5,000-row cap. It now keeps the first
    // `message.delta` of the stream only (lifecycle rows are untouched).
    expect(deltaRows).toHaveLength(1)
    expect(rows).toHaveLength(1)
    console.info('[stream-churn] trace', JSON.stringify({ deltas: all.length, uiEventRows: rows.length }))
    dispose()
  })

  // ZDR across the whole stream and every flush trigger the controller has (each
  // delta, the terminal event, the 500 ms server debounce). The trigger cases of
  // the snapshot writer (visibility, pagehide, thread switch, cleanup) are in
  // `use-chat-controller.snapshot-throttle.test.ts`.
  const zdrCases: Array<[string, () => { zdr?: boolean }]> = [
    ['a temporary (ZDR) thread', () => ({ zdr: true })],
    ['an org whose ZDR posture is on', () => {
      applyServerRetention(true)
      return {}
    }],
  ]

  async function streamToTerminal(live: OpenStream) {
    for (const delta of deltas()) {
      live.emit(frame('chunk', { delta }))
      await tick()
      vi.advanceTimersByTime(FRAME_MS)
    }
    live.emit(frame('done', { request_id: 'req-1' }))
    await tick()
    vi.advanceTimersByTime(600)
    await tick()
  }

  it.each(zdrCases)('writes nothing from the connected frame to the terminal for %s', async (_name, arrange) => {
    // A new thread: the temporary marking is made at the first send of a thread.
    mocks.listChatThreads.mockResolvedValue([])
    const { controller, dispose } = mount()
    const options = arrange()
    const live = await startTurn(controller, options)
    // Watched once the send is connected: this is the stream the budget governs.
    const storage = watchStorage(CHAT_THREAD_TRANSCRIPTS_KEY)
    await streamToTerminal(live)

    expect(storage.writes.local.count).toBe(0)
    expect(storage.writes.session.count).toBe(0)
    expect(mocks.saveChatThreadSnapshot).not.toHaveBeenCalled()
    dispose()
  })

  it('an org whose ZDR posture is on keeps the first send out of local storage too', async () => {
    mocks.listChatThreads.mockResolvedValue([])
    const { controller, dispose } = mount()
    applyServerRetention(true)
    const storage = watchStorage(CHAT_THREAD_TRANSCRIPTS_KEY)
    await startTurn(controller)
    expect(storage.writes.withContent).toBe(0)
    dispose()
  })

  // KNOWN GAP, found by this slice's measurement and not fixed here (the fix is
  // in `use-chat-controller.ts`, outside S1). `sendContent` marks a new
  // temporary thread (`markThreadTemporary`, a signal write) and then, with no
  // flush between, writes the first-turn snapshot (`writeThreadSnapshot`), whose
  // `isTemporaryThread` guard still reads the unmarked set. The user's first
  // message is stored under the provisional thread id in both storages. When the
  // fix lands this test passes, vitest reports it as an unexpected pass, and the
  // `.fails` marker is deleted.
  it.fails('a temporary thread keeps the first send out of local storage too', async () => {
    mocks.listChatThreads.mockResolvedValue([])
    const { controller, dispose } = mount()
    const storage = watchStorage(CHAT_THREAD_TRANSCRIPTS_KEY)
    await startTurn(controller, { zdr: true })
    expect(storage.writes.withContent).toBe(0)
    dispose()
  })
})
