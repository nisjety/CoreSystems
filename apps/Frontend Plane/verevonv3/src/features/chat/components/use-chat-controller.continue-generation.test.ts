// @vitest-environment jsdom

/**
 * "Fortsett" (continue-after-stop; chat-parity §0.1 — the cheapest of the
 * three parity gaps the audit left open after F-12, and table stakes in
 * ChatGPT/Claude/Perplexity). See `continueGeneration` in
 * use-chat-controller.ts for the design: mechanically a `regenerateLatest`
 * with the outgoing turn's partial text carried forward as the new turn's
 * seed, and folded into the model-facing request as a continuation
 * instruction — never a parallel mechanism.
 *
 * Mirrors use-chat-controller.f12.test.ts's mocking shape (same
 * `streamChat` stub, same `withController`/`tick` helpers), plus captures the
 * REQUEST object `streamChat` was called with, which the F-12 test does not
 * need but this one does — the whole point here is what actually goes out on
 * the wire, not just what lands in `state.turns`.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'

type CapturedStream = {
  request: { content: string; regenerated?: boolean; editResubmit?: boolean }
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

describe('continueGeneration ("Fortsett")', () => {
  it('seeds the new turn with the stopped partial answer and tells the model to continue it, without a regenerate signal', async () => {
    await withController(async (controller) => {
      await tick()

      await controller.handleComposerSubmit({
        actions: [],
        attachments: [],
        text: 'Skriv en lang tekst om norsk fiskeoppdrett',
        tools: [],
      })
      await tick()

      expect(streams).toHaveLength(1)
      const first = streams[0]!
      const firstAssistantId = controller.state.turns[1]?.id
      expect(firstAssistantId).toBeTruthy()

      // Partial text arrives, then the user stops mid-answer.
      first.handlers.onMessage?.({ content: 'Norsk fiskeoppdrett startet på', requestId: 'req-1' })
      flush()
      controller.handleStop()
      flush()
      expect(controller.state.turns[1]?.status).toBe('stopped')
      expect(controller.state.turns[1]?.content).toBe('Norsk fiskeoppdrett startet på')

      // The aborted fetch's read loop settles asynchronously, same as every
      // other stop in this codebase's tests.
      first.resolve()
      await tick()

      controller.continueGeneration(firstAssistantId!)
      flush()

      // Truncated + replaced synchronously — the same F-12 flush() fix
      // `regenerateLatest` needed, reused here rather than re-broken.
      expect(controller.state.turns).toHaveLength(2)
      await tick()
      expect(streams).toHaveLength(2)
      const second = streams[1]!

      const secondTurn = controller.state.turns[1]
      expect(secondTurn?.id).not.toBe(firstAssistantId)
      // Seeded: the new turn starts with the OLD partial text already in
      // place — nothing the model already wrote is lost or shown as blank
      // even for an instant.
      expect(secondTurn?.content).toBe('Norsk fiskeoppdrett startet på')
      expect(secondTurn?.status).toBe('waiting')

      // The transcript still shows the ORIGINAL question, unchanged — the
      // continuation instruction is model-facing only, never displayed.
      expect(controller.state.turns[0]?.content).toBe('Skriv en lang tekst om norsk fiskeoppdrett')

      // The model is told, on the wire, to continue that exact partial text.
      expect(second.request.content).toContain('Norsk fiskeoppdrett startet på')
      expect(second.request.content).toContain('Fortsett svaret nøyaktig der det slapp')

      // Never the dissatisfaction signal a real regenerate sends — continuing
      // is the opposite signal (chat-types.ts `SendOptions.regenerated`).
      expect(second.request.regenerated).not.toBe(true)
      expect(second.request.editResubmit).not.toBe(true)

      // Streamed continuation deltas append after the seed, splicing rather
      // than overwriting or duplicating it.
      second.handlers.onMessage?.({ content: ' 1970-tallet, med de første anleggene i Hardanger.', requestId: 'req-2' })
      flush()
      expect(controller.state.turns[1]?.content).toBe(
        'Norsk fiskeoppdrett startet på 1970-tallet, med de første anleggene i Hardanger.',
      )

      second.handlers.onDone?.({ requestId: 'req-2', modelUsed: 'verevon-balance', outputTokens: 40, stopReason: 'stop' })
      second.resolve()
      await tick()
      expect(controller.state.turns[1]?.status).toBeUndefined()
    })
  })

  it('does nothing for a stopped turn with no partial content — "Generer på nytt" already covers it', async () => {
    await withController(async (controller) => {
      await tick()

      await controller.handleComposerSubmit({
        actions: [],
        attachments: [],
        text: 'Hei',
        tools: [],
      })
      await tick()

      const stream = streams[0]!
      const assistantId = controller.state.turns[1]?.id
      expect(assistantId).toBeTruthy()

      // Stopped before any content arrived at all.
      controller.handleStop()
      flush()
      expect(controller.state.turns[1]?.status).toBe('stopped')
      expect(controller.state.turns[1]?.content).toBe('')

      controller.continueGeneration(assistantId!)
      flush()
      await tick()

      // No second stream opened, and the turn is untouched — there is
      // nothing to continue from, so this is a deliberate no-op rather than
      // a regenerate wearing the wrong label.
      expect(streams).toHaveLength(1)
      expect(controller.state.turns).toHaveLength(2)
      expect(controller.state.turns[1]?.status).toBe('stopped')

      stream.resolve()
      await tick()
    })
  })

  it('is a no-op for a turn that is not the trailing stopped turn (a stale click target)', async () => {
    await withController(async (controller) => {
      await tick()

      await controller.handleComposerSubmit({
        actions: [],
        attachments: [],
        text: 'Skriv om laks',
        tools: [],
      })
      await tick()

      const stream = streams[0]!
      const assistantId = controller.state.turns[1]?.id

      stream.handlers.onMessage?.({ content: 'Laks er en fisk', requestId: 'req-1' })
      flush()
      stream.handlers.onDone?.({ requestId: 'req-1', modelUsed: 'verevon-balance', outputTokens: 10, stopReason: 'stop' })
      stream.resolve()
      await tick()

      // The turn finished normally — never stopped — so continuing it must
      // do nothing, even though it still has content and even though a stale
      // caller might pass its id.
      expect(controller.state.turns[1]?.status).toBeUndefined()
      controller.continueGeneration(assistantId!)
      flush()
      await tick()

      expect(streams).toHaveLength(1)
      expect(controller.state.turns[1]?.content).toBe('Laks er en fisk')
    })
  })
})
