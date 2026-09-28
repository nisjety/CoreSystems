// @vitest-environment jsdom

/**
 * chat-wave2 scope spec — the crux of the per-turn design (§2.1/§6.1): unlike
 * `sourceScope`, which is pinned to the thread's first user turn once one
 * exists, `retrievalScope` must be freely changeable EVERY send within the
 * same thread ("for a chat turn", Perplexity-Focus style). This file asserts
 * that difference directly, plus the composer-submit passthrough and the
 * server's `source_scope_applied` receipt landing on the right turn.
 *
 * Mirrors the `streamChat` stub and `withController`/`tick` helpers of
 * use-chat-controller.follow-up.test.ts.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers, QueuedInputResult } from '@/shared/api/chat-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'

type CapturedStream = {
  request: { content: string; sourceScope?: string; retrievalScope?: string }
  handlers: ChatStreamHandlers
  resolve: () => void
}

const { streams, mockStreamChat, mockQueueInvocationInput } = vi.hoisted(() => {
  const streams: CapturedStream[] = []
  const mockStreamChat = vi.fn((request: CapturedStream['request'], handlers: ChatStreamHandlers) => {
    return new Promise<void>((resolve) => {
      streams.push({ request, handlers, resolve })
    })
  })
  const mockQueueInvocationInput = vi.fn<(requestId: string, content: string) => Promise<QueuedInputResult>>()
  return { streams, mockStreamChat, mockQueueInvocationInput }
})

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    streamChat: mockStreamChat,
    queueInvocationInput: mockQueueInvocationInput,
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

async function finishStream(stream: CapturedStream, requestId: string) {
  stream.handlers.onDone?.({ requestId, modelUsed: 'verevon-balance', outputTokens: 4, stopReason: 'stop' })
  stream.resolve()
  await tick()
}

afterEach(() => {
  streams.length = 0
  mockStreamChat.mockClear()
  mockQueueInvocationInput.mockReset()
  __resetRetentionForTests()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('retrievalScope is per-turn, unlike sourceScope', () => {
  it('changes the wire value turn to turn within the same thread, never pinned to the first turn', async () => {
    await withController(async (controller) => {
      await tick()
      controller.setRetrievalScope('knowledge_only')
      await send(controller, 'Hva sier retningslinjene?')
      await tick()
      expect(streams[0]!.request.retrievalScope).toBe('knowledge_only')
      await finishStream(streams[0]!, 'req-1')

      // A later turn in the SAME thread picks a different scope — this is the
      // entire point of the design (§2.1): sourceScope pins to the thread's
      // first user turn once one exists, retrievalScope must not.
      controller.setRetrievalScope('web_only')
      await send(controller, 'Hva skjer i nyhetene i dag?')
      await tick()
      expect(streams[1]!.request.retrievalScope).toBe('web_only')

      // Contrast: sourceScope genuinely IS pinned — asserting the two behave
      // differently is the crux of this design, not incidental coverage.
      expect(streams[1]!.request.sourceScope).toBe(streams[0]!.request.sourceScope)
    })
  })

  it("sends the picker's default ('auto') when it was never changed — buildChatWireBody is what omits it from the actual HTTP body (see chat-client.test.ts)", async () => {
    await withController(async (controller) => {
      await tick()
      await send(controller, 'Vanlig spørsmål')
      await tick()
      expect(streams[0]!.request.retrievalScope).toBe('auto')
    })
  })

  it("captures the turn's own scope at send time, unaffected by a later picker change", async () => {
    await withController(async (controller) => {
      await tick()
      controller.setRetrievalScope('knowledge_only')
      await send(controller, 'Første melding')
      await tick()
      const firstUserTurn = controller.state.turns.find((turn) => turn.role === 'user')
      expect(firstUserTurn?.retrievalScope).toBe('knowledge_only')

      // Flipping the picker afterward must not retroactively relabel the
      // turn that already sent — same guarantee `planMode` already has.
      controller.setRetrievalScope('web_only')
      await tick()
      expect(firstUserTurn?.retrievalScope).toBe('knowledge_only')
    })
  })

  it("lands the server's source_scope_applied receipt on the turn that requested it", async () => {
    await withController(async (controller) => {
      await tick()
      controller.setRetrievalScope('web_only')
      await send(controller, 'Hva skjer i nyhetene?')
      await tick()
      const assistantId = controller.state.turns.find((turn) => turn.role === 'assistant')?.id
      streams[0]!.handlers.onSourceScopeApplied?.({ scope: 'web_only', excludedTools: ['knowledge_search', 'inbox_search'] })
      await tick()
      const assistantTurn = controller.state.turns.find((turn) => turn.id === assistantId)
      expect(assistantTurn?.scopeApplied).toEqual({ scope: 'web_only', excludedTools: ['knowledge_search', 'inbox_search'] })
    })
  })
})
