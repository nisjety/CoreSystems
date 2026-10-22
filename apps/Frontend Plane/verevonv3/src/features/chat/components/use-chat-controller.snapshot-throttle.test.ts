// @vitest-environment jsdom

/**
 * WS09 S3: the local transcript snapshot is throttled while an answer streams,
 * and every trigger that must not lose text still writes it.
 *
 * `use-chat-controller.stream-churn.test.ts` pins the per-second ceiling and the
 * Trace and step numbers. This file drives the SAME real controller and chat
 * client (only the SSE reader is scripted) through the flush triggers the writer
 * has (terminal, `visibilitychange`, `pagehide`, thread switch, cleanup), the ZDR
 * guards at each of them, and the `VITE_CHAT_SNAPSHOT_THROTTLE_MS=0` rollback.
 *
 * Only lengths and presence of marker tokens are read from storage: the tests
 * keep no content.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { RunDetail } from '@/shared/api/runs-client'
import type { SseEvent } from '@/shared/api/sse'
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

type UseChatController = typeof import('./use-chat-controller').useChatController
type Controller = ReturnType<UseChatController>

const THREAD = 'thread-throttle'
const RUN_ID = '01M2ZNAZ0W48WCREHFTGPKPNV2'
const QUESTION = 'Lag en lang oppsummering av kvartalet'

const frame = (event: string, data: Record<string, unknown>, id?: number): Frame => ({
  event,
  data,
  ...(id !== undefined ? { id: String(id) } : {}),
})

/** A marker per token, so a stored value says exactly how far the stream got. */
const token = (index: number) => `tok${index}`

async function tick(times = 3) {
  for (let i = 0; i < times; i += 1) {
    await Promise.resolve()
    flush()
  }
}

/**
 * A fresh copy of the controller and of the retention module it reads. The
 * switch is read once at import, and the ZDR cases flip the posture on the SAME
 * module instance the controller holds, so both come from one load.
 */
async function load(env: Record<string, string> = {}) {
  vi.resetModules()
  for (const [name, value] of Object.entries(env)) vi.stubEnv(name, value)
  const { useChatController } = await import('./use-chat-controller')
  const retention = await import('@/features/chat/lib/chat-retention')
  return { useChatController, applyServerRetention: retention.applyServerRetention, resetRetention: retention.__resetRetentionForTests }
}

function mount(useChatController: UseChatController): { controller: Controller; dispose: () => void } {
  let dispose = () => {}
  let controller!: Controller
  createRoot((rootDispose) => {
    dispose = rootDispose
    controller = useChatController()
  })
  return { controller, dispose }
}

/** `setItem` calls for one key, per storage. Counts only: nothing here keeps content. */
function watchStorage(key: string) {
  const writes = { local: 0, session: 0 }
  const wrap = (storage: Storage, field: 'local' | 'session') => {
    const original = storage.setItem.bind(storage)
    vi.spyOn(storage, 'setItem').mockImplementation((name: string, value: string) => {
      if (name === key) writes[field] += 1
      original(name, value)
    })
  }
  wrap(window.localStorage, 'local')
  wrap(window.sessionStorage, 'session')
  return { writes }
}

/** Whether the stored transcripts hold this marker (read, never kept). */
const stored = (marker: string): boolean =>
  (window.localStorage.getItem(CHAT_THREAD_TRANSCRIPTS_KEY) ?? '').includes(marker)

async function startTurn(
  controller: Controller,
  options: { zdr?: boolean } = {},
): Promise<OpenStream> {
  await tick()
  await controller.handleComposerSubmit({ actions: [], attachments: [], text: QUESTION, tools: [], ...options })
  await tick()
  const stream = sse.streams.at(-1)
  if (!stream) throw new Error('no stream opened')
  stream.emit(frame(
    'connected',
    options.zdr ? { ok: true, request_id: 'req-1' } : { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID },
    1,
  ))
  await tick()
  return stream
}

/** Stream `count` tokens inside one throttle window: no timer is advanced. */
async function streamTokens(live: OpenStream, count: number) {
  for (let index = 0; index < count; index += 1) {
    live.emit(frame('chunk', { delta: `${token(index)} ` }))
    await tick()
  }
}

const setVisibility = (state: 'visible' | 'hidden') => {
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => state })
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  mocks.listChatThreads.mockResolvedValue([
    { threadId: THREAD, title: 'Kvartal', preview: QUESTION, updatedAt: '2026-10-21T08:00:00.000Z', pinned: false },
  ])
})

afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllEnvs()
  vi.restoreAllMocks()
  setVisibility('visible')
  sse.streams.length = 0
  for (const mock of Object.values(mocks)) mock.mockReset()
  mocks.cancelRun.mockImplementation(async (runId: string) => ({ runId, cancelled: true, status: 'cancelled', receiptId: 'receipt-1' }))
  mocks.getChatThreadTranscript.mockResolvedValue(null)
  mocks.getThreadMessages.mockResolvedValue([])
  mocks.getRun.mockResolvedValue(null)
  mocks.listRuns.mockResolvedValue({ runs: [], hasMore: false })
  mocks.saveChatThreadSnapshot.mockResolvedValue(null)
  window.localStorage.clear()
  window.sessionStorage.clear()
})

describe('what a throttled snapshot still guarantees', () => {
  const lastToken = 7

  it('leaves the newest tokens out of storage inside the window (the precondition of every trigger below)', async () => {
    const { controller, dispose } = mount((await load()).useChatController)
    const live = await startTurn(controller)
    await streamTokens(live, lastToken + 1)
    expect(stored(token(lastToken))).toBe(false)
    dispose()
  })

  it('a terminal frame writes the full answer at once, without waiting for the window', async () => {
    const { controller, dispose } = mount((await load()).useChatController)
    const live = await startTurn(controller)
    await streamTokens(live, lastToken + 1)
    expect(stored(token(lastToken))).toBe(false)

    live.emit(frame('done', { request_id: 'req-1' }))
    await tick()
    expect(stored(token(lastToken))).toBe(true)
    dispose()
  })

  it('the window closing writes the latest state once', async () => {
    const { controller, dispose } = mount((await load()).useChatController)
    const live = await startTurn(controller)
    await streamTokens(live, lastToken + 1)
    vi.advanceTimersByTime(1000)
    expect(stored(token(lastToken))).toBe(true)
    dispose()
  })

  it('a page hidden mid-stream flushes the pending snapshot', async () => {
    const { controller, dispose } = mount((await load()).useChatController)
    const live = await startTurn(controller)
    await streamTokens(live, lastToken + 1)
    expect(stored(token(lastToken))).toBe(false)

    setVisibility('visible')
    document.dispatchEvent(new Event('visibilitychange'))
    expect(stored(token(lastToken))).toBe(false)
    setVisibility('hidden')
    document.dispatchEvent(new Event('visibilitychange'))
    expect(stored(token(lastToken))).toBe(true)
    dispose()
  })

  it('pagehide flushes the pending snapshot', async () => {
    const { controller, dispose } = mount((await load()).useChatController)
    const live = await startTurn(controller)
    await streamTokens(live, lastToken + 1)
    expect(stored(token(lastToken))).toBe(false)

    window.dispatchEvent(new Event('pagehide'))
    expect(stored(token(lastToken))).toBe(true)
    dispose()
  })

  it('switching to a new chat flushes the thread being left', async () => {
    const { controller, dispose } = mount((await load()).useChatController)
    const live = await startTurn(controller)
    await streamTokens(live, lastToken + 1)
    expect(stored(token(lastToken))).toBe(false)

    controller.startNewChat()
    expect(stored(token(lastToken))).toBe(true)
    // The new, empty chat wrote nothing over it.
    await tick()
    vi.advanceTimersByTime(2000)
    expect(stored(token(lastToken))).toBe(true)
    dispose()
  })

  it('unmounting flushes the pending snapshot before the controller refuses late writes', async () => {
    const { controller, dispose } = mount((await load()).useChatController)
    const live = await startTurn(controller)
    await streamTokens(live, lastToken + 1)
    expect(stored(token(lastToken))).toBe(false)

    dispose()
    expect(stored(token(lastToken))).toBe(true)
  })

  it('stops listening to the page after unmount', async () => {
    const { controller, dispose } = mount((await load()).useChatController)
    const live = await startTurn(controller)
    await streamTokens(live, lastToken + 1)
    dispose()
    const storage = watchStorage(CHAT_THREAD_TRANSCRIPTS_KEY)

    setVisibility('hidden')
    document.dispatchEvent(new Event('visibilitychange'))
    window.dispatchEvent(new Event('pagehide'))
    vi.advanceTimersByTime(5000)
    expect(storage.writes.local).toBe(0)
  })
})

describe('ZDR across every flush trigger', () => {
  /** Every way a pending write can be released, run after tokens streamed. */
  async function fireEveryTrigger(controller: Controller, dispose: () => void) {
    vi.advanceTimersByTime(1000)
    setVisibility('hidden')
    document.dispatchEvent(new Event('visibilitychange'))
    window.dispatchEvent(new Event('pagehide'))
    controller.startNewChat()
    await tick()
    dispose()
    await tick()
  }

  it('a temporary thread writes nothing at any trigger', async () => {
    mocks.listChatThreads.mockResolvedValue([])
    const { controller, dispose } = mount((await load()).useChatController)
    const live = await startTurn(controller, { zdr: true })
    const storage = watchStorage(CHAT_THREAD_TRANSCRIPTS_KEY)
    await streamTokens(live, 8)
    await fireEveryTrigger(controller, dispose)

    expect(storage.writes.local).toBe(0)
    expect(storage.writes.session).toBe(0)
    expect(stored(token(0))).toBe(false)
    expect(mocks.saveChatThreadSnapshot).not.toHaveBeenCalled()
  })

  it('a pending write is dropped, not flushed, when the org turns ZDR before the window closes', async () => {
    const loaded = await load()
    const { controller, dispose } = mount(loaded.useChatController)
    const live = await startTurn(controller)
    await streamTokens(live, 8)
    expect(stored(token(7))).toBe(false)

    // The org posture arrives with a listing mid-stream: everything local is purged
    // at once, and the write that was waiting must not bring any of it back.
    loaded.applyServerRetention(true)
    const storage = watchStorage(CHAT_THREAD_TRANSCRIPTS_KEY)
    live.emit(frame('chunk', { delta: `${token(8)} ` }))
    await tick()
    await fireEveryTrigger(controller, dispose)

    expect(storage.writes.local).toBe(0)
    expect(storage.writes.session).toBe(0)
    expect(stored(token(0))).toBe(false)
    expect(stored(token(7))).toBe(false)
  })
})

describe('VITE_CHAT_SNAPSHOT_THROTTLE_MS=0 restores the per-token behaviour', () => {
  const deltaCount = 40

  it('writes once per delta, records every delta in the Trace and replaces the step each time', async () => {
    const { controller, dispose } = mount((await load({ VITE_CHAT_SNAPSHOT_THROTTLE_MS: '0' })).useChatController)
    const live = await startTurn(controller)
    const traceBefore = controller.uiEvents().length
    const storage = watchStorage(CHAT_THREAD_TRANSCRIPTS_KEY)
    const stepRefs = new Set<unknown>()
    for (let index = 0; index < deltaCount; index += 1) {
      live.emit(frame('chunk', { delta: `${token(index)} ` }))
      await tick()
      stepRefs.add(controller.state.taskSteps.find((step) => step.title === 'Compose response'))
    }

    expect(storage.writes.local).toBe(deltaCount)
    expect(storage.writes.session).toBe(deltaCount)
    expect(controller.uiEvents().slice(traceBefore).filter((row) => row.type === 'message.delta')).toHaveLength(deltaCount)
    expect(stepRefs.size).toBeGreaterThan(deltaCount * 0.9)
    expect(stored(token(deltaCount - 1))).toBe(true)
    dispose()
  })

  it('anything but whole milliseconds keeps the one-second default', async () => {
    const { controller, dispose } = mount((await load({ VITE_CHAT_SNAPSHOT_THROTTLE_MS: 'fast' })).useChatController)
    const live = await startTurn(controller)
    const storage = watchStorage(CHAT_THREAD_TRANSCRIPTS_KEY)
    await streamTokens(live, deltaCount)
    expect(storage.writes.local).toBeLessThanOrEqual(1)
    dispose()
  })
})

describe('Trace and steps', () => {
  it('keeps the lifecycle rows next to the single message.delta row', async () => {
    const { controller, dispose } = mount((await load()).useChatController)
    const live = await startTurn(controller)
    const before = controller.uiEvents().length
    await streamTokens(live, 30)
    live.emit(frame('done', { request_id: 'req-1' }))
    await tick()

    const types = controller.uiEvents().slice(before).map((row) => row.type)
    expect(types.filter((type) => type === 'message.delta')).toHaveLength(1)
    expect(types).toContain('message.done')
    dispose()
  })

  it('records the first delta of the next turn, whose send starts a new Trace', async () => {
    const { controller, dispose } = mount((await load()).useChatController)
    const first = await startTurn(controller)
    await streamTokens(first, 5)
    first.emit(frame('done', { request_id: 'req-1' }))
    await tick()

    await controller.handleComposerSubmit({ actions: [], attachments: [], text: 'Og neste kvartal?', tools: [], })
    await tick()
    const second = sse.streams.at(-1)
    if (!second || second === first) throw new Error('no second stream opened')
    second.emit(frame('connected', { ok: true, request_id: 'req-2', thread_id: THREAD, run_id: RUN_ID }, 1))
    await tick()
    await streamTokens(second, 5)

    // A send starts a new Trace, and its first delta is recorded like the last one's.
    const deltaRows = controller.uiEvents().filter((row) => row.type === 'message.delta')
    expect(deltaRows).toHaveLength(1)
    dispose()
  })

  it('replaces the answer step once, however many tokens arrive', async () => {
    const { controller, dispose } = mount((await load()).useChatController)
    const live = await startTurn(controller)
    const seen = new Set<unknown>()
    for (let index = 0; index < 30; index += 1) {
      live.emit(frame('chunk', { delta: `${token(index)} ` }))
      await tick()
      seen.add(controller.state.taskSteps.find((step) => step.title === 'Compose response'))
    }
    expect(seen.size).toBeLessThanOrEqual(2)
    expect(controller.state.taskSteps.filter((step) => step.title === 'Compose response')).toHaveLength(1)
    dispose()
  })
})
