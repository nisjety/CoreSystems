// @vitest-environment jsdom

/**
 * `followUpPending`: a message sent while a reply streams, that the running
 * turn has not taken.
 *
 * Voice mode sends a spoken message through the composer like a typed one, so
 * it can land mid-run. The chat then either queues it into the run (which goes
 * on to answer it) or defers it: the stream had no request id yet, or the run
 * ended before the message landed, and it goes out as the next turn once the
 * run is over. Voice mode used to take the streaming turn as the answer in
 * both cases, announcing "Svar mottatt" for, and reading aloud, the earlier
 * reply while the real one streamed in unseen. `followUpPending` is how the
 * chat tells it to keep waiting; `latestVoiceReply` folds it into the reply.
 *
 * Mirrors the `streamChat` stub and `withController`/`tick` helpers of
 * use-chat-controller.continue-generation.test.ts.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers, QueuedInputResult } from '@/shared/api/chat-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { latestVoiceReply } from '@/features/voice/voice-reply'

type CapturedStream = {
  request: { content: string }
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

/** What Voice mode is handed on the chat page. */
const voiceReply = (controller: ReturnType<typeof useChatController>) =>
  latestVoiceReply(controller.state.turns, controller.followUpPending())

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

describe('followUpPending', () => {
  it('holds while a message waits for a run that is still opening, until it goes out as the next turn', async () => {
    await withController(async (controller) => {
      await tick()
      await send(controller, 'Første spørsmål')
      await tick()
      expect(controller.followUpPending()).toBe(false)

      // No request id yet: nothing to deliver to, so the message is deferred.
      await send(controller, 'Og hvem eier planen?')
      await tick()
      expect(controller.followUpPending()).toBe(true)
      expect(mockQueueInvocationInput).not.toHaveBeenCalled()

      streams[0]!.handlers.onMessage?.({ content: 'Første svar.', requestId: 'req-1' })
      flush()
      await finishStream(streams[0]!, 'req-1')

      // The first reply has settled, but it does not answer the spoken message.
      // It went out as the next turn in the same pass that cleared the flag.
      expect(streams).toHaveLength(2)
      expect(streams[1]!.request.content).toContain('Og hvem eier planen?')
      expect(controller.followUpPending()).toBe(false)
      const reply = voiceReply(controller)
      expect(reply?.id).toBe(controller.state.turns.at(-1)?.id)
      expect(reply?.streaming).toBe(true)

      streams[1]!.handlers.onMessage?.({ content: 'Eva eier planen.', requestId: 'req-2' })
      flush()
      await finishStream(streams[1]!, 'req-2')
      expect(voiceReply(controller)).toMatchObject({ text: 'Eva eier planen.', streaming: false })
    })
  })

  it('holds through a delivery the run ended too early to take, and through the turn that replaces it', async () => {
    let answer!: (result: QueuedInputResult) => void
    mockQueueInvocationInput.mockImplementationOnce(() => new Promise<QueuedInputResult>((resolve) => { answer = resolve }))
    await withController(async (controller) => {
      await tick()
      await send(controller, 'Første spørsmål')
      await tick()
      streams[0]!.handlers.onMessage?.({ content: 'Første svar', requestId: 'req-1' })
      flush()

      await send(controller, 'Og hvem eier planen?')
      await tick()
      expect(mockQueueInvocationInput).toHaveBeenCalledWith('req-1', 'Og hvem eier planen?', expect.anything())
      expect(controller.followUpPending()).toBe(true)

      // The run finishes while the delivery is still in flight.
      await finishStream(streams[0]!, 'req-1')
      expect(controller.followUpPending()).toBe(true)
      expect(voiceReply(controller)?.streaming).toBe(true)

      answer({ outcome: 'run_ended' })
      await tick()
      expect(streams).toHaveLength(2)
      expect(controller.followUpPending()).toBe(false)
      expect(voiceReply(controller)?.streaming).toBe(true)
    })
  })

  it('clears once the running turn takes the message, which then answers it', async () => {
    mockQueueInvocationInput.mockResolvedValueOnce({ outcome: 'queued', pending: 1, persisted: true })
    await withController(async (controller) => {
      await tick()
      await send(controller, 'Første spørsmål')
      await tick()
      streams[0]!.handlers.onMessage?.({ content: 'Første svar', requestId: 'req-1' })
      flush()

      await send(controller, 'Og hvem eier planen?')
      await tick()
      expect(controller.followUpPending()).toBe(false)

      await finishStream(streams[0]!, 'req-1')
      expect(streams).toHaveLength(1)
      expect(voiceReply(controller)).toMatchObject({ text: 'Første svar', streaming: false })
    })
  })
})
