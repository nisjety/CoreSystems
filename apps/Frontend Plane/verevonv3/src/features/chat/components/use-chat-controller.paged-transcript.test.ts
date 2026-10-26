// @vitest-environment jsdom

/**
 * WS09 S9: with `VITE_CHAT_PAGED_TRANSCRIPT=true` a thread opens on its newest
 * page and older pages are read on request. Off, the whole thread is read in one
 * call, as before. The local snapshot keeps the turns a page does not reach, and
 * a page that lands after the reader has left the thread changes nothing.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
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

import {
  readChatThreadTranscript,
  setActiveChatThreadId,
  upsertChatThreadTranscript,
} from '@/features/chat/lib/chat-thread-history'
import { useChatController } from './use-chat-controller'

const THREAD = 'thread-paged'
const STAMP = '2026-10-26T10:00:00.000Z'
const SOURCE = { id: 'src-1', title: 'Rapport', url: 'https://example.test/rapport', snippet: 'Shared folder' }

/** The thread as the server holds it: 300 messages, oldest first. */
const SERVER = Array.from({ length: 300 }, (_, index) => ({
  id: `m-${index}`,
  role: index % 2 === 0 ? 'user' as const : 'assistant' as const,
  content: `Message ${index}`,
  createdAt: '',
}))

/** Answers as the server does: the newest `limit` messages, or the ones before the cursor. */
function serve(request?: PageRequest, thread: typeof SERVER = SERVER) {
  if (!request) return thread
  const end = request.beforeMessageId
    ? thread.findIndex((message) => message.id === request.beforeMessageId)
    : thread.length
  const start = Math.max(0, end - request.limit)
  return { messages: thread.slice(start, end), hasMore: start > 0 }
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
  mocks.listChatThreads.mockReset()
  mocks.listChatThreads.mockResolvedValue([])
  mocks.listRuns.mockReset()
  mocks.listRuns.mockResolvedValue({ runs: [], hasMore: false })
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
