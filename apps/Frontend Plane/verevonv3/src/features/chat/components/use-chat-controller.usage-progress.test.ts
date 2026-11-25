// @vitest-environment jsdom

/**
 * A running cost total is not a Trace row (WS27-S4, D2-660, D2-662).
 *
 * The chat client projects every SSE frame to a Verevon UI event before its focused handler runs, so a
 * `usage_progress` frame reaches `appendUiEvent` as `usage.progress`. The Trace and the work feed read that
 * record, so the controller keeps the frame out of it. The cost meter reads the same frame through
 * `onUsageProgress`, and that path is what this file pins. It drives the real controller and the real chat
 * client; only the SSE reader underneath is scripted, as in `use-chat-controller.stream-churn`.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { RunDetail } from '@/shared/api/runs-client'
import type { SseEvent } from '@/shared/api/sse'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'

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

const THREAD = 'thread-usage-progress'
const RUN_ID = '01M2ZNAZ0W48WCREHFTGPKPNV3'
const QUESTION = 'Hva koster kvartalsrapporten?'

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

async function startTurn(controller: ReturnType<typeof useChatController>): Promise<OpenStream> {
  await tick()
  await controller.handleComposerSubmit({ actions: [], attachments: [], text: QUESTION, tools: [] })
  await tick()
  const stream = sse.streams.at(-1)
  if (!stream) throw new Error('no stream opened')
  stream.emit(frame('connected', { ok: true, request_id: 'req-1', thread_id: THREAD, run_id: RUN_ID }, 1))
  await tick()
  return stream
}

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
})

describe('usage_progress: the cost meter moves and the Trace does not', () => {
  it('keeps every progress frame out of the Trace while the run total still moves', async () => {
    const { controller, dispose } = mount()
    const live = await startTurn(controller)
    const before = controller.uiEvents().length

    live.emit(frame('usage_progress', { calls: 1, phase: 'agent_round', cost_usd: 0.01, cost_basis: 'metered' }))
    await tick()
    live.emit(frame('chunk', { delta: 'Kvartalsrapporten koster ' }))
    await tick()
    live.emit(frame('usage_progress', { calls: 2, phase: 'answer', cost_usd: 0.025, cost_basis: 'metered' }))
    await tick()

    const rows = controller.uiEvents().slice(before)
    // The frames never become a row, so the Trace and the work feed cannot show them as events.
    expect(rows.some((row) => row.type === 'usage.progress')).toBe(false)
    // The rest of the stream is still recorded: only the progress frames are kept out.
    expect(rows.some((row) => row.type === 'message.delta')).toBe(true)
    // The meter reads the same frames through onUsageProgress, so its total is the latest frame's.
    expect(controller.state.turns.at(-1)?.meter).toMatchObject({ calls: 2, costUsd: 0.025, costBasis: 'metered' })
    dispose()
  })
})
