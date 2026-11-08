// @vitest-environment jsdom

/**
 * `settledAt`: the time this client saw a turn's terminal frame (EP-14 S1). The
 * settled-turn fold measures a duration from the message that started the turn to
 * this stamp. It is stamped on every terminal path a turn can take, and it is never
 * written into the stored transcript.
 *
 * Mirrors the `streamChat` stub and `withController`/`tick` helpers of
 * use-chat-controller.follow-up.test.ts.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { CHAT_THREAD_TRANSCRIPTS_KEY } from '@/features/chat/lib/chat-thread-history'

type CapturedStream = {
  request: { content: string }
  handlers: ChatStreamHandlers
  resolve: () => void
}

const { streams, mockStreamChat } = vi.hoisted(() => {
  const streams: CapturedStream[] = []
  const mockStreamChat = vi.fn((request: CapturedStream['request'], handlers: ChatStreamHandlers) => {
    return new Promise<void>((resolve) => {
      streams.push({ request, handlers, resolve })
    })
  })
  return { streams, mockStreamChat }
})

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    streamChat: mockStreamChat,
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
    getChatThreadTranscript: vi.fn().mockResolvedValue(null),
    getThreadMessages: vi.fn().mockResolvedValue([]),
    cancelInvocation: vi.fn().mockResolvedValue(undefined),
  }
})

import { useChatController } from './use-chat-controller'

/** Flush pending microtasks (SSE-handler continuations, chained awaits) and Solid's batched store writes. */
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
        (value) => {
          dispose()
          resolvePromise(value)
        },
        (error) => {
          dispose()
          rejectPromise(error)
        },
      )
    })
  })
}

const send = (controller: ReturnType<typeof useChatController>, text: string) =>
  controller.handleComposerSubmit({ actions: [], attachments: [], text, tools: [] })

const SENT_TEXT = 'Hva er planen for uken?'

afterEach(() => {
  streams.length = 0
  mockStreamChat.mockClear()
  __resetRetentionForTests()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('settledAt on the turn that answered', () => {
  it('is stamped when the run is done, after the message that started it', async () => {
    await withController(async (controller) => {
      await tick()
      await send(controller, SENT_TEXT)
      await tick()
      streams[0]!.handlers.onDone?.({ requestId: 'req-1', modelUsed: 'verevon-balance', outputTokens: 4, stopReason: 'stop' })
      streams[0]!.resolve()
      await tick()

      const user = controller.state.turns.find((turn) => turn.role === 'user')
      const answer = controller.state.turns.find((turn) => turn.role === 'assistant')
      expect(answer?.streaming).toBe(false)
      expect(answer?.settledAt).toEqual(expect.any(String))
      expect(Date.parse(answer!.settledAt!)).toBeGreaterThanOrEqual(Date.parse(user!.createdAt))
    })
  })

  it('is stamped when the server stops the run', async () => {
    await withController(async (controller) => {
      await tick()
      await send(controller, SENT_TEXT)
      await tick()
      streams[0]!.handlers.onStopped?.({ requestId: 'req-1', reason: 'cancelled' })
      streams[0]!.resolve()
      await tick()

      const answer = controller.state.turns.find((turn) => turn.role === 'assistant')
      expect(answer?.streaming).toBe(false)
      expect(answer?.settledAt).toEqual(expect.any(String))
    })
  })

  it('is stamped when the server reports a failure', async () => {
    await withController(async (controller) => {
      await tick()
      await send(controller, SENT_TEXT)
      await tick()
      streams[0]!.handlers.onError?.({ code: 'invalid_request', message: 'Ugyldig forespørsel' })
      streams[0]!.resolve()
      await tick()

      const answer = controller.state.turns.find((turn) => turn.role === 'assistant')
      expect(answer?.status).toBe('error')
      expect(answer?.settledAt).toEqual(expect.any(String))
    })
  })

  it('is stamped when the person stops the run', async () => {
    await withController(async (controller) => {
      await tick()
      await send(controller, SENT_TEXT)
      await tick()
      controller.handleStop()
      await tick()

      const answer = controller.state.turns.find((turn) => turn.role === 'assistant')
      expect(answer?.streaming).toBe(false)
      expect(answer?.settledAt).toEqual(expect.any(String))
    })
  })

  it('is not stamped while the run is still streaming', async () => {
    await withController(async (controller) => {
      await tick()
      await send(controller, SENT_TEXT)
      await tick()

      const answer = controller.state.turns.find((turn) => turn.role === 'assistant')
      expect(answer?.streaming).toBe(true)
      expect(answer?.settledAt).toBeUndefined()
    })
  })

  it('is not written into the stored transcript, so a reloaded turn carries no terminal time', async () => {
    await withController(async (controller) => {
      await tick()
      await send(controller, SENT_TEXT)
      await tick()
      streams[0]!.handlers.onDone?.({ requestId: 'req-1', modelUsed: 'verevon-balance', outputTokens: 4, stopReason: 'stop' })
      streams[0]!.resolve()
      await tick()

      const stored = window.localStorage.getItem(CHAT_THREAD_TRANSCRIPTS_KEY) ?? ''
      expect(stored).toContain(SENT_TEXT)
      expect(stored).not.toContain('settledAt')
    })
  })
})
