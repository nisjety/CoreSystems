// @vitest-environment jsdom

/**
 * A customer-gateway turn replays on its own connection (W4-BYO-6, V3-spa-1-rel2). A regenerate, an edit and a continuation of a
 * turn written under a customer gateway send that connection with the gateway's model id. Without it the request names the model
 * alone, and the platform chain can serve a model under that id, which takes the answer off the person's own gateway.
 *
 * Driven through the real controller, with the request `streamChat` was called with captured, as the continue-generation test
 * does. A platform turn sends no connection, as before.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'

type CapturedStream = {
  request: { content: string; model?: string; modelConnectionId?: string; regenerated?: boolean; editResubmit?: boolean }
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

const GATEWAY_MODEL = 'gateway-a'
const GATEWAY_CONNECTION = 'conn-gw-1'

/** Flush pending microtasks (SSE-handler continuations, chained awaits) and Solid's batched store writes. */
async function tick(times = 3) {
  for (let i = 0; i < times; i += 1) {
    await Promise.resolve()
    flush()
  }
}

function withController<T>(
  fn: (controller: ReturnType<typeof useChatController>) => Promise<T>,
): Promise<T> {
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

/** Sends one question on `connection` (none for a platform turn) and lets its answer finish. */
async function answered(controller: ReturnType<typeof useChatController>, connection?: string) {
  await tick()
  await controller.handleComposerSubmit({
    actions: [],
    attachments: [],
    text: 'Hei',
    tools: [],
    model: GATEWAY_MODEL,
    modelConnectionId: connection,
  })
  await tick()
  const stream = streams.at(-1)!
  stream.handlers.onMessage?.({ content: 'Svar', requestId: 'req-1' })
  stream.handlers.onDone?.({ requestId: 'req-1', modelUsed: GATEWAY_MODEL, outputTokens: 4, stopReason: 'stop' })
  stream.resolve()
  await tick()
}

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

describe('replaying a customer-gateway turn keeps the connection it was written under', () => {
  it('a regenerate names the connection the answer was written under', async () => {
    await withController(async (controller) => {
      await answered(controller, GATEWAY_CONNECTION)
      controller.regenerateLatest()
      await tick()
      expect(streams).toHaveLength(2)
      expect(streams[1]!.request).toMatchObject({ modelConnectionId: GATEWAY_CONNECTION, regenerated: true })
    })
  })

  it('an edit of the question names the connection that question was sent on', async () => {
    await withController(async (controller) => {
      await answered(controller, GATEWAY_CONNECTION)
      const question = controller.state.turns[0]!
      void controller.editAndResubmit(question.id, 'Hei igjen')
      await tick(6)
      expect(streams).toHaveLength(2)
      expect(streams[1]!.request).toMatchObject({ modelConnectionId: GATEWAY_CONNECTION, editResubmit: true })
    })
  })

  it('a continuation of a stopped answer names the connection the answer was written under', async () => {
    await withController(async (controller) => {
      await tick()
      await controller.handleComposerSubmit({
        actions: [],
        attachments: [],
        text: 'Skriv litt om fiskeoppdrett',
        tools: [],
        model: GATEWAY_MODEL,
        modelConnectionId: GATEWAY_CONNECTION,
      })
      await tick()
      const first = streams[0]!
      const assistantId = controller.state.turns[1]!.id
      first.handlers.onMessage?.({ content: 'Fiskeoppdrett startet', requestId: 'req-1' })
      flush()
      controller.handleStop()
      flush()
      expect(controller.state.turns[1]?.status).toBe('stopped')
      first.resolve()
      await tick()

      controller.continueGeneration(assistantId)
      flush()
      await tick()
      expect(streams).toHaveLength(2)
      expect(streams[1]!.request).toMatchObject({ modelConnectionId: GATEWAY_CONNECTION })
    })
  })

  it('a platform answer is regenerated without a connection, as before', async () => {
    await withController(async (controller) => {
      await answered(controller)
      controller.regenerateLatest()
      await tick()
      expect(streams).toHaveLength(2)
      expect(streams[1]!.request.modelConnectionId).toBeUndefined()
    })
  })
})
