// @vitest-environment jsdom

/**
 * WS09 S9: with `VITE_CHAT_PAGED_TRANSCRIPT=true` a thread opens on its newest
 * page and older pages are read on request. Off, the whole thread is read in one
 * call, as before. The local snapshot keeps the turns a page does not reach, and
 * a page that lands after the reader has left the thread changes nothing.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'
import type { RunDetail } from '@/shared/api/runs-client'
import { __resetRetentionForTests, applyServerRetention } from '@/features/chat/lib/chat-retention'

type PageRequest = { limit: number; beforeMessageId?: string }

const mocks = vi.hoisted(() => ({
  streamChat: vi.fn(),
  resumeStream: vi.fn(),
  cancelInvocation: vi.fn(async () => undefined),
  getChatThreadTranscript: vi.fn(async () => null as unknown),
  getRun: vi.fn(async () => null as RunDetail | null),
  listChatThreads: vi.fn(async () => [] as unknown[]),
  getChatThread: vi.fn((threadId: string) =>
    mocks.listChatThreads().then((listed) =>
      (listed as Array<{ threadId: string }>).find((item) => item.threadId === threadId) ?? null)),
  getThreadMessages: vi.fn(async (_threadId: string, _request?: PageRequest): Promise<unknown> => [] as unknown[]),
  listRuns: vi.fn(async () => ({ runs: [] as RunDetail[], hasMore: false })),
  rewindChatThread: vi.fn(async (..._args: unknown[]): Promise<unknown> => ({ activeLeafMessageId: '', activePath: [] })),
}))

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
    getChatThread: mocks.getChatThread,
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
    getThreadMessages: mocks.getThreadMessages,
    rewindChatThread: mocks.rewindChatThread,
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

import { ApiError } from '@/shared/api/http'
import {
  readActiveChatThreadId,
  readChatThreadTranscript,
  setActiveChatThreadId,
  upsertChatThreadTranscript,
} from '@/features/chat/lib/chat-thread-history'
import { useChatController } from './use-chat-controller'

const THREAD = 'thread-paged'
const STAMP = '2026-10-26T10:00:00.000Z'
const SOURCE = { id: 'src-1', title: 'Rapport', url: 'https://example.test/rapport', snippet: 'Shared folder' }

/**
 * A stored message as the messages read returns it, already normalised. The owner read flattens the turn's evidence onto the
 * message, so its citations, artifacts and grounding sit beside the text (F-f01-24).
 */
type Stored = {
  id: string
  role: 'user' | 'assistant'
  content: string
  createdAt: string
  citations?: unknown[]
  artifacts?: unknown[]
  grounding?: unknown
  retracted?: boolean
  version?: unknown
  modelConnectionId?: string
}

/** The thread as the server holds it: 300 messages, oldest first. */
const SERVER: Stored[] = Array.from({ length: 300 }, (_, index) => ({
  id: `m-${index}`,
  role: index % 2 === 0 ? 'user' as const : 'assistant' as const,
  content: `Message ${index}`,
  createdAt: '',
}))

/** Answers as the server does: the newest `limit` messages, or the ones before the cursor. */
function serve(request?: PageRequest, thread: Stored[] = SERVER) {
  if (!request) return thread
  const end = request.beforeMessageId
    ? thread.findIndex((message) => message.id === request.beforeMessageId)
    : thread.length
  const start = Math.max(0, end - request.limit)
  return { messages: thread.slice(start, end), hasMore: start > 0 }
}

const GROUNDING = {
  mode: 'retrieve',
  query: 'Rapport',
  lowConfidence: false,
  factCount: 0,
  sourceCount: 1,
  facts: [],
  sources: [],
}

/** The ids of messages `from` up to, not including, `to`. */
const ids = (from: number, to: number) => Array.from({ length: to - from }, (_, offset) => `m-${from + offset}`)

async function tick(times = 3) {
  for (let i = 0; i < times; i += 1) {
    await Promise.resolve()
    flush()
  }
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

/** Client storage reads fall back to sessionStorage, so both are cleared. */
function forgetStoredThreads() {
  window.localStorage.clear()
  window.sessionStorage.clear()
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  mocks.listChatThreads.mockResolvedValue([{
    threadId: THREAD, title: 'Paged', preview: 'Paged', updatedAt: STAMP, pinned: false,
  }])
  mocks.getThreadMessages.mockImplementation(async (_threadId: string, request?: PageRequest) => serve(request))
})

afterEach(() => {
  vi.unstubAllEnvs()
  vi.useRealTimers()
  mocks.getChatThreadTranscript.mockReset()
  mocks.getChatThreadTranscript.mockResolvedValue(null)
  mocks.getThreadMessages.mockReset()
  mocks.rewindChatThread.mockReset()
  mocks.streamChat.mockReset()
  mocks.listChatThreads.mockReset()
  mocks.listChatThreads.mockResolvedValue([])
  mocks.listRuns.mockReset()
  mocks.listRuns.mockResolvedValue({ runs: [], hasMore: false })
  mocks.getRun.mockResolvedValue(null)
  __resetRetentionForTests()
  forgetStoredThreads()
})

describe('WS09 S9 paged transcript', () => {
  it('reads the whole thread in one call, and offers no older turns, with the flag off', async () => {
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      expect(mocks.getThreadMessages).toHaveBeenCalledWith(THREAD)
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(ids(0, 300))
      expect(controller.hasEarlierTurns()).toBe(false)
    })
  })

  it('opens on the newest page, and says that older turns remain', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      expect(mocks.getThreadMessages).toHaveBeenCalledWith(THREAD, { limit: 100 })
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(ids(200, 300))
      expect(controller.hasEarlierTurns()).toBe(true)
    })
  })

  it('prepends each older page in turn, anchored on the oldest message held, until the start', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      await controller.loadEarlier()
      expect(mocks.getThreadMessages).toHaveBeenLastCalledWith(THREAD, { limit: 100, beforeMessageId: 'm-200' })
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(ids(100, 300))
      expect(controller.hasEarlierTurns()).toBe(true)

      await controller.loadEarlier()
      expect(mocks.getThreadMessages).toHaveBeenLastCalledWith(THREAD, { limit: 100, beforeMessageId: 'm-100' })
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(ids(0, 300))
      expect(controller.hasEarlierTurns()).toBe(false)
    })
  })

  it('ignores a request for an older page while one is already loading', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      const first = controller.loadEarlier()
      await controller.loadEarlier()
      await first
      const pageRequests = mocks.getThreadMessages.mock.calls.filter((call) => (call[1] as PageRequest | undefined)?.beforeMessageId)
      expect(pageRequests).toHaveLength(1)
      expect(controller.loadingEarlierTurns()).toBe(false)
    })
  })

  it('does not apply an older page that lands after the reader has started a new chat', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      let release: (page: unknown) => void = () => undefined
      mocks.getThreadMessages.mockImplementationOnce(() => new Promise((resolve) => { release = resolve }))
      const pending = controller.loadEarlier()
      controller.startNewChat()
      release(serve({ limit: 100, beforeMessageId: 'm-200' }))
      await pending
      expect(controller.state.turns).toEqual([])
      expect(controller.hasEarlierTurns()).toBe(false)
      expect(controller.loadingEarlierTurns()).toBe(false)
    })
  })

  it('keeps the snapshot turns older than the page until an older page reaches them, and carries their citations', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    // The local snapshot holds the newest 120 turns, one of them with a citation
    // the messages read does not carry.
    upsertChatThreadTranscript({
      threadId: THREAD,
      updatedAt: STAMP,
      turns: SERVER.slice(180).map((message) => ({
        id: message.id,
        role: message.role,
        content: message.content,
        createdAt: '',
        citations: message.id === 'm-190' ? [SOURCE] : undefined,
      })),
    })
    mocks.getChatThreadTranscript.mockClear()
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      // The snapshot supplies the turns, so the transcript is not read again.
      expect(mocks.getChatThreadTranscript).not.toHaveBeenCalled()
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(ids(180, 300))
      expect(controller.hasEarlierTurns()).toBe(true)

      await controller.loadEarlier()
      expect(mocks.getThreadMessages).toHaveBeenLastCalledWith(THREAD, { limit: 100, beforeMessageId: 'm-200' })
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(ids(100, 300))
      expect(controller.state.turns.find((turn) => turn.id === 'm-190')?.citations).toMatchObject([
        { title: SOURCE.title, url: SOURCE.url },
      ])
    })
    // The snapshot still holds the same turns it did before the older page.
    expect(readChatThreadTranscript(THREAD)?.turns.map((turn) => turn.id)).toEqual(ids(180, 300))
  })

  it('writes no message text to client storage while older pages load under a zero-retention posture', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    applyServerRetention(true)
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      await controller.loadEarlier()
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(ids(100, 300))
    })
    const stored = [window.localStorage, window.sessionStorage]
      .flatMap((store) => Array.from({ length: store.length }, (_, index) => store.getItem(store.key(index) ?? '') ?? ''))
      .join('\n')
    expect(stored).not.toContain('Message 250')
    expect(stored).not.toContain('Message 150')
  })

  it('stores the newest page on the device when retention allows it, so the zero-retention test above can fail', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    setActiveChatThreadId(THREAD)
    await withController(async () => {
      await tick(30)
    })
    const stored = [window.localStorage, window.sessionStorage]
      .flatMap((store) => Array.from({ length: store.length }, (_, index) => store.getItem(store.key(index) ?? '') ?? ''))
      .join('\n')
    expect(stored).toContain('Message 250')
  })

  it('keeps the snapshot turns that reach past one page until a later page covers them', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    // A 500-message thread. The snapshot holds 281 to 400, so it reaches past the
    // first older page (300 to 399) and overlaps the newest page (400 to 499) by one message.
    const long = Array.from({ length: 500 }, (_, index) => ({
      id: `m-${index}`,
      role: index % 2 === 0 ? 'user' as const : 'assistant' as const,
      content: `Message ${index}`,
      createdAt: '',
    }))
    mocks.getThreadMessages.mockImplementation(async (_threadId: string, request?: PageRequest) => serve(request, long))
    upsertChatThreadTranscript({
      threadId: THREAD,
      updatedAt: STAMP,
      turns: long.slice(281, 401).map((message) => ({
        id: message.id,
        role: message.role,
        content: message.content,
        createdAt: '',
      })),
    })
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(ids(281, 500))
      expect(controller.hasEarlierTurns()).toBe(true)

      // The first older page covers 300 to 399; 281 to 299 are older than it and stay.
      await controller.loadEarlier()
      expect(mocks.getThreadMessages).toHaveBeenLastCalledWith(THREAD, { limit: 100, beforeMessageId: 'm-400' })
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(ids(281, 500))

      // The next page covers 200 to 299, which reaches the 281 to 299 that were kept.
      await controller.loadEarlier()
      expect(mocks.getThreadMessages).toHaveBeenLastCalledWith(THREAD, { limit: 100, beforeMessageId: 'm-300' })
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(ids(200, 500))
    })
  })
})

describe('F-f01-24 paged reads keep the evidence and the state the owner read carries', () => {
  it('issues exactly one messages request on a cold open, for the newest page, and reads no whole thread', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      expect(mocks.getThreadMessages).toHaveBeenCalledTimes(1)
      expect(mocks.getThreadMessages).toHaveBeenCalledWith(THREAD, { limit: 100 })
      expect(mocks.getChatThreadTranscript).not.toHaveBeenCalled()
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(ids(200, 300))
    })
  })

  it('shows the citations of the newest page, which the messages read carries', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    const cited = SERVER.map((message) => (message.id === 'm-299' ? { ...message, citations: [SOURCE] } : message))
    mocks.getThreadMessages.mockImplementation(async (_threadId: string, request?: PageRequest) => serve(request, cited))
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      expect(controller.state.turns.find((turn) => turn.id === 'm-299')?.citations).toMatchObject([
        { title: SOURCE.title, url: SOURCE.url },
      ])
    })
  })

  it('keeps the citations and grounding of an older page that loadEarlier adds', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    const grounded = SERVER.map((message) => (message.id === 'm-150' ? { ...message, citations: [SOURCE], grounding: GROUNDING } : message))
    mocks.getThreadMessages.mockImplementation(async (_threadId: string, request?: PageRequest) => serve(request, grounded))
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      await controller.loadEarlier()
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(ids(100, 300))
      const older = controller.state.turns.find((turn) => turn.id === 'm-150')
      expect(older?.citations).toMatchObject([{ title: SOURCE.title, url: SOURCE.url }])
      expect(older?.grounding).toMatchObject({ mode: 'retrieve', sourceCount: 1 })
    })
  })

  it('adopts the saved answer of an active run from the messages route, and keeps its citations', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    // The thread ends with the question of a run that is still active, so the browser holds a pending answer for it. The
    // question's id is a ULID and the run is newer, as the saved-answer match requires.
    const question = '01ARZ3NDEKTSV4RRFFQ69G5FAV'
    const runId = '01BX5ZZKBKACTAV9WEVGEMMVRZ'
    const goal = 'Summarise the Rapport'
    const asked = [
      { id: 'm-0', role: 'user' as const, content: 'Earlier question', createdAt: '' },
      { id: 'm-1', role: 'assistant' as const, content: 'Earlier answer', createdAt: '' },
      { id: question, role: 'user' as const, content: goal, createdAt: '' },
    ]
    // Once the run has finished the server holds its answer after the question. The adoption reads the newest four turns.
    const answered = [
      ...asked,
      { id: 'a-1', role: 'assistant' as const, content: 'Saved answer', createdAt: '', citations: [SOURCE], grounding: GROUNDING },
    ]
    mocks.getThreadMessages.mockImplementation(async (_threadId: string, request?: PageRequest) => (
      serve(request, request?.limit === 4 ? answered : asked)
    ))
    mocks.listChatThreads.mockResolvedValue([{
      threadId: THREAD, title: 'Paged', preview: 'Paged', updatedAt: STAMP, pinned: false,
      latestRunId: runId, latestRunStatus: 'running',
    }])
    mocks.listRuns.mockResolvedValue({
      runs: [{ runId, threadId: THREAD, status: 'running', goal, createdAt: STAMP, updatedAt: STAMP } as unknown as RunDetail],
      hasMore: false,
    })
    mocks.getRun.mockResolvedValue({
      runId, threadId: THREAD, status: 'completed', goal, createdAt: STAMP, updatedAt: STAMP,
    } as unknown as RunDetail)
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(60)
      // The adoption reads the newest four messages, which carry the citations the saved answer was written with.
      expect(mocks.getThreadMessages).toHaveBeenCalledWith(THREAD, { limit: 4 })
      const adopted = controller.state.turns.at(-1)
      expect(adopted?.id).toBe(`active-run-${runId}`)
      expect(adopted?.content).toBe('Saved answer')
      expect(adopted?.status).not.toBe('waiting')
      expect(adopted?.citations).toMatchObject([{ title: SOURCE.title, url: SOURCE.url }])
    })
  })

  it('reads a foreign-origin thread in pages from the messages route, and keeps it open', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    // A cross-surface link to a thread that Chat's own listing does not hold.
    const FOREIGN = 'thread-foreign'
    mocks.listChatThreads.mockResolvedValue([])
    window.history.replaceState({}, '', `/?thread_id=${FOREIGN}`)
    try {
      await withController(async (controller) => {
        await tick(30)
        expect(controller.isForeignOriginThread()).toBe(true)
        // The newest page is the window; older pages come from the same route.
        expect(mocks.getThreadMessages).toHaveBeenCalledWith(FOREIGN, { limit: 100 })
        expect(controller.state.threadId).toBe(FOREIGN)
        expect(controller.state.turns.map((turn) => turn.id)).toEqual(ids(200, 300))
        expect(controller.hasEarlierTurns()).toBe(true)

        await controller.loadEarlier()
        expect(mocks.getThreadMessages).toHaveBeenLastCalledWith(FOREIGN, { limit: 100, beforeMessageId: 'm-200' })
        expect(controller.state.turns.map((turn) => turn.id)).toEqual(ids(100, 300))
      })
    } finally {
      window.history.replaceState({}, '', '/')
    }
  })

  it('evicts a thread that is gone: the page read answers 404, and the open forgets the thread', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    mocks.getThreadMessages.mockRejectedValue(new ApiError('not found', 404, 'thread_not_found'))
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      expect(mocks.getThreadMessages).toHaveBeenCalledWith(THREAD, { limit: 100 })
      expect(controller.state.turns).toEqual([])
    })
    expect(readActiveChatThreadId()).toBeNull()
  })

  it('shows a retracted note as retracted, and regenerates the question before it rather than the note (O-01)', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    mocks.getThreadMessages.mockImplementation(async (_threadId: string, request?: PageRequest) => serve(request, [
      { id: 'srv-q1', role: 'user', content: 'Hva er lakselus?', createdAt: '' },
      { id: 'srv-n1', role: 'user', content: '', createdAt: '', retracted: true },
      { id: 'srv-a1', role: 'assistant', content: 'Lakselus er en parasitt.', createdAt: '' },
    ]))
    mocks.streamChat.mockImplementation(() => new Promise<void>(() => undefined))
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(['srv-q1', 'srv-n1', 'srv-a1'])
      expect(controller.state.turns[1]?.retracted).toBe(true)

      controller.regenerateLatest()
      await tick(10)
      expect(mocks.streamChat).toHaveBeenCalledTimes(1)
      expect(mocks.streamChat.mock.calls[0]?.[0]).toMatchObject({ content: 'Hva er lakselus?', regenerated: true })
    })
  })

  it('records the server id of the question a versioned send wrote, not the retracted note stored after it (O-01)', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    const earlier: Stored[] = [
      { id: 'srv-q0', role: 'user', content: 'Tidligere', createdAt: '' },
      {
        id: 'srv-a0',
        role: 'assistant',
        content: 'Tidligere svar',
        createdAt: '',
        version: { parentMessageId: 'srv-q0', siblingOrdinal: 1, siblingCount: 1, onActivePath: true, siblings: ['srv-a0'] },
      },
    ]
    // After the send the server holds the question, the note the person took back before its run read it, and the answer.
    const sent: Stored[] = [
      ...earlier,
      {
        id: 'srv-q1',
        role: 'user',
        content: 'Hei',
        createdAt: '',
        version: { parentMessageId: 'srv-a0', siblingOrdinal: 1, siblingCount: 1, onActivePath: true, siblings: ['srv-q1'] },
      },
      { id: 'srv-n1', role: 'user', content: '', createdAt: '', retracted: true },
      {
        id: 'srv-a1',
        role: 'assistant',
        content: 'Svar',
        createdAt: '',
        version: { parentMessageId: 'srv-q1', siblingOrdinal: 1, siblingCount: 1, onActivePath: true, siblings: ['srv-a1'] },
      },
    ]
    let current: Stored[] = earlier
    mocks.getThreadMessages.mockImplementation(async (_threadId: string, request?: PageRequest) => serve(request, current))
    const streams: Array<{ request: Record<string, unknown>; handlers: ChatStreamHandlers; resolve: () => void }> = []
    mocks.streamChat.mockImplementation((request: Record<string, unknown>, handlers: ChatStreamHandlers) => new Promise<void>((resolve) => {
      streams.push({ request, handlers, resolve })
    }))
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      await controller.handleComposerSubmit({ actions: [], attachments: [], text: 'Hei', tools: [], model: 'verevon-balance' })
      await tick(10)
      current = sent
      const stream = streams.at(-1)!
      stream.handlers.onMessage?.({ content: 'Svar', requestId: 'req-1' })
      stream.handlers.onDone?.({ requestId: 'req-1', modelUsed: 'verevon-balance', outputTokens: 4, stopReason: 'stop' })
      stream.resolve()
      await tick(10)

      const question = controller.state.turns.find((turn) => turn.role === 'user' && turn.content === 'Hei')
      expect(question).toBeDefined()
      void controller.editAndResubmit(question!.id, 'Hei igjen')
      await tick(10)
      expect(streams).toHaveLength(2)
      expect(streams[1]!.request).toMatchObject({ editOfMessageId: 'srv-q1' })
    })
  })

  it('keeps the attachment of a paged user turn, which the messages read carries as the inline suffix', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    mocks.getThreadMessages.mockImplementation(async (_threadId: string, request?: PageRequest) => serve(request, [
      {
        id: 'srv-q1',
        role: 'user',
        content: 'Les notatet\n\n--- VEDLEGG: notat.txt ---\nInnhold i notatet\n--- SLUTT PÅ VEDLEGG: notat.txt ---',
        createdAt: '',
      },
      { id: 'srv-a1', role: 'assistant', content: 'Ferdig', createdAt: '' },
    ]))
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      const question = controller.state.turns[0]
      expect(question?.content).toBe('Les notatet')
      expect(question?.attachments.map((file) => file.name)).toEqual(['notat.txt'])
      expect(question?.attachments[0]?.extractedText).toBe('Innhold i notatet')
    })
  })

  it('a regenerate names the customer-gateway connection a reopened answer was written under, with the switch on', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    // A turn keeps its connection when its module loads with the customer-gateway switch on, so the controller is loaded again
    // under it, with the solid runtime it renders in.
    vi.resetModules()
    const solid = await import('solid-js')
    const { useChatController: loadedController } = await import('./use-chat-controller')
    const { setActiveChatThreadId: setActiveThread } = await import('@/features/chat/lib/chat-thread-history')
    mocks.getThreadMessages.mockImplementation(async (_threadId: string, request?: PageRequest) => serve(request, [
      { id: 'srv-q1', role: 'user', content: 'Hei', createdAt: '', modelConnectionId: 'conn-gw-1' },
      { id: 'srv-a1', role: 'assistant', content: 'Svar', createdAt: '', modelConnectionId: 'conn-gw-1' },
    ]))
    const sent: Array<Record<string, unknown>> = []
    mocks.streamChat.mockImplementation((request: Record<string, unknown>) => {
      sent.push(request)
      return new Promise<void>(() => undefined)
    })
    setActiveThread(THREAD)
    await new Promise<void>((resolvePromise, rejectPromise) => {
      solid.createRoot((dispose) => {
        const controller = loadedController()
        void (async () => {
          for (let i = 0; i < 30; i += 1) {
            await Promise.resolve()
            solid.flush()
          }
          controller.regenerateLatest()
          for (let i = 0; i < 10; i += 1) {
            await Promise.resolve()
            solid.flush()
          }
          dispose()
        })().then(resolvePromise, rejectPromise)
      })
    })
    expect(sent).toHaveLength(1)
    expect(sent[0]).toMatchObject({ modelConnectionId: 'conn-gw-1', regenerated: true })
  })
})

describe('F-f01-25 turns outside the loaded window', () => {
  /** The newest 120 turns in the local snapshot: the 20 before the server's newest page of 100 are local-only. */
  function snapshotOfNewest120() {
    upsertChatThreadTranscript({
      threadId: THREAD,
      updatedAt: STAMP,
      turns: SERVER.slice(180).map((message) => ({
        id: message.id,
        role: message.role,
        content: message.content,
        createdAt: '',
      })),
    })
  }

  it('lists the local turns before the window as outside it, until the older pages reach them', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    snapshotOfNewest120()
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(ids(180, 300))
      expect([...controller.unloadedTurnIds()]).toEqual(ids(180, 200))

      await controller.loadEarlier()
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(ids(100, 300))
      expect([...controller.unloadedTurnIds()]).toEqual([])
    })
  })

  it('refuses an edit on a turn outside the window, with the notice, and leaves the list as it was', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    snapshotOfNewest120()
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      await controller.editAndResubmit('m-190', 'Changed question')
      expect(controller.feedbackNotice()).not.toBeNull()
      expect(mocks.streamChat).not.toHaveBeenCalled()
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(ids(180, 300))
    })
  })

  // Continue, rerun, branch and rewind each name the turn they act on, so each is refused on a turn outside the window
  // before any of its own checks run. Regenerate and the version switch act on the final exchange, which always lies in
  // the newest page (the leading turns are older than it), so their refusals guard a state the open does not build.
  it.each([
    ['continue', (controller: ReturnType<typeof useChatController>) => controller.continueGeneration('m-191')],
    ['rerun', (controller: ReturnType<typeof useChatController>) => controller.rerunAsNewTurn('m-191')],
    ['branch', (controller: ReturnType<typeof useChatController>) => controller.branchAt('m-191')],
    ['rewind', (controller: ReturnType<typeof useChatController>) => controller.rewindTo('m-191')],
  ])('refuses %s on a turn outside the window, with the notice, and leaves the thread and its window as they were', async (_name, act) => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    snapshotOfNewest120()
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      expect(controller.feedbackNotice()).toBeNull()
      await act(controller)
      flush()
      expect(controller.feedbackNotice()).toMatch(/eldre meldinger|earlier messages/)
      expect(mocks.streamChat).not.toHaveBeenCalled()
      expect(controller.state.threadId).toBe(THREAD)
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(ids(180, 300))
      expect(controller.hasEarlierTurns()).toBe(true)
      expect([...controller.unloadedTurnIds()]).toEqual(ids(180, 200))
    })
  })

  it('an edit on a turn inside the window cuts the list there, and keeps the window and its cursor', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    snapshotOfNewest120()
    // The resend never settles here: only the cut is under test.
    mocks.streamChat.mockImplementation(() => new Promise<void>(() => undefined))
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      void controller.editAndResubmit('m-250', 'Changed question')
      await tick(30)
      expect(controller.state.turns.slice(0, 70).map((turn) => turn.id)).toEqual(ids(180, 250))
      expect([...controller.unloadedTurnIds()]).toEqual(ids(180, 200))
      expect(controller.hasEarlierTurns()).toBe(true)

      await controller.loadEarlier()
      expect(mocks.getThreadMessages).toHaveBeenLastCalledWith(THREAD, { limit: 100, beforeMessageId: 'm-200' })
    })
  })

  it('a branch is a thread of its own, so no older page is offered from the source window', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    snapshotOfNewest120()
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      expect(controller.hasEarlierTurns()).toBe(true)
      controller.branchAt('m-250')
      // A signal write reaches a plain read only once flushed (Solid 2), as the controller's own truncations rely on.
      flush()
      expect(controller.hasEarlierTurns()).toBe(false)
      expect(controller.unloadedTurnIds().size).toBe(0)
    })
  })

  it('a cut into the turns before the window resets the window, so no older page is offered', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    snapshotOfNewest120()
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      expect([...controller.unloadedTurnIds()]).toEqual(ids(180, 200))
      // A cut that keeps 10 turns removes the 20 local turns before the window.
      controller.resetPagingWindowAfterTruncation(10)
      flush()
      expect(controller.hasEarlierTurns()).toBe(false)
      expect(controller.unloadedTurnIds().size).toBe(0)
      const pageReads = mocks.getThreadMessages.mock.calls.length
      await controller.loadEarlier()
      expect(mocks.getThreadMessages.mock.calls.length).toBe(pageReads)
    })
  })

  it('a cut that keeps the whole head before the window leaves the window and its cursor as they were', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    snapshotOfNewest120()
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      controller.resetPagingWindowAfterTruncation(20)
      flush()
      expect(controller.hasEarlierTurns()).toBe(true)
      expect([...controller.unloadedTurnIds()]).toEqual(ids(180, 200))
      await controller.loadEarlier()
      expect(mocks.getThreadMessages).toHaveBeenLastCalledWith(THREAD, { limit: 100, beforeMessageId: 'm-200' })
    })
  })

  it('keeps the window of the open thread while a reload of it is in flight, and asks for no older page meanwhile', async () => {
    vi.stubEnv('VITE_CHAT_PAGED_TRANSCRIPT', 'true')
    snapshotOfNewest120()
    // A versioned thread, so a rewind is offered: each answer carries the place it has in the message tree.
    const versioned: Stored[] = SERVER.map((message, index) => (message.role === 'assistant'
      ? {
        ...message,
        version: {
          parentMessageId: SERVER[index - 1]?.id ?? '',
          siblingOrdinal: 1,
          siblingCount: 1,
          onActivePath: true,
          siblings: [message.id],
        },
      }
      : message))
    mocks.getThreadMessages.mockImplementation(async (_threadId: string, request?: PageRequest) => serve(request, versioned))
    // The server's path differs from the one on screen, so the rewind reloads the thread.
    mocks.rewindChatThread.mockResolvedValue({ activeLeafMessageId: 'm-251', activePath: ['m-250', 'm-251'] })
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      expect([...controller.unloadedTurnIds()]).toEqual(ids(180, 200))

      let release: (page: unknown) => void = () => undefined
      mocks.getThreadMessages.mockImplementationOnce(() => new Promise((resolve) => { release = resolve }))
      await controller.rewindTo('m-251')
      await tick(10)
      expect(mocks.rewindChatThread).toHaveBeenCalledWith(THREAD, 'm-251', 'm-299')
      // The reload is in flight: the turns before the window stay refused, and no older page is read into a list that
      // is about to be replaced.
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(ids(180, 252))
      expect([...controller.unloadedTurnIds()]).toEqual(ids(180, 200))
      const readsBefore = mocks.getThreadMessages.mock.calls.length
      await controller.loadEarlier()
      expect(mocks.getThreadMessages.mock.calls.length).toBe(readsBefore)

      // The page lands and the window is rebuilt from it; older pages are offered again.
      release(serve({ limit: 100 }, versioned))
      await tick(30)
      expect(controller.hasEarlierTurns()).toBe(true)
      await controller.loadEarlier()
      expect(mocks.getThreadMessages).toHaveBeenLastCalledWith(THREAD, { limit: 100, beforeMessageId: 'm-200' })
    })
  })

  it('marks no turn as outside the window with paging off', async () => {
    snapshotOfNewest120()
    setActiveChatThreadId(THREAD)
    await withController(async (controller) => {
      await tick(30)
      expect(controller.unloadedTurnIds().size).toBe(0)
    })
  })
})
