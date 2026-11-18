// @vitest-environment jsdom

/**
 * W3-FIX-14: a private-panel thread (SD-22 v1) is locked to conversation scope by Session Core. The chat controller
 * therefore sends every turn on a thread this tab remembers with conversation scope, whatever the composer passed.
 * A thread the tab never learned of keeps the ordinary workspace body.
 *
 * Mirrors the `streamChat` stub and `withController`/`tick` helpers of use-chat-controller.retrieval-scope.test.ts.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'
import { getSpacePrivatePanel } from '@/shared/api/spaces-client/private-panel'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { buildChatWireBody } from '@/shared/api/chat-client'

type WireRequest = Parameters<typeof buildChatWireBody>[0]
type CapturedStream = {
  request: WireRequest
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
    queueInvocationInput: vi.fn(),
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
    getChatThreadTranscript: vi.fn().mockResolvedValue(null),
    getThreadMessages: vi.fn().mockResolvedValue([]),
    cancelInvocation: vi.fn().mockResolvedValue(undefined),
  }
})

import { useChatController } from './use-chat-controller'

type Controller = ReturnType<typeof useChatController>

/** Flush pending microtasks (SSE-handler continuations, chained awaits) and Solid's batched store writes. */
async function tick(times = 3) {
  for (let i = 0; i < times; i += 1) {
    await Promise.resolve()
    flush()
  }
}

function withController<T>(fn: (controller: Controller) => Promise<T>): Promise<T> {
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

const sendFrom = (controller: Controller, text: string, sourceScope: 'workspace' | 'conversation') =>
  controller.handleComposerSubmit({ actions: [], attachments: [], text, tools: [], sourceScope })

async function finishStream(stream: CapturedStream, requestId: string) {
  stream.handlers.onDone?.({ requestId, modelUsed: 'test-model', outputTokens: 4, stopReason: 'stop' })
  stream.resolve()
  await tick()
}

/** Reads the owner's private-panel pointer the way the room does, so this tab remembers the thread. */
async function rememberPrivatePanel(threadId: string) {
  vi.stubGlobal(
    'fetch',
    vi.fn(
      async () =>
        new Response(
          JSON.stringify({ data: { space_ref: 'room-1', thread_id: threadId, agent_subject_id: null } }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
    ),
  )
  try {
    await getSpacePrivatePanel('room-1')
  } finally {
    vi.unstubAllGlobals()
  }
}

afterEach(() => {
  streams.length = 0
  mockStreamChat.mockClear()
  __resetRetentionForTests()
})

describe('a send on a remembered private-panel thread', () => {
  it('carries conversation_only and conversation scope even when the composer passed workspace', async () => {
    await rememberPrivatePanel('private-panel-1')
    await withController(async (controller) => {
      await tick()
      await sendFrom(controller, 'Første melding', 'workspace')
      await tick()
      streams[0]!.handlers.onConnected?.({ requestId: 'req-1', threadId: 'private-panel-1' })
      await finishStream(streams[0]!, 'req-1')
      expect(controller.state.threadId).toBe('private-panel-1')

      await sendFrom(controller, 'Andre melding', 'workspace')
      await tick()
      const second = streams[1]!.request
      expect(second.sourceScope).toBe('conversation')
      expect(buildChatWireBody(second).features).toContain('conversation_only')
      const lastUserTurn = controller.state.turns.filter((turn) => turn.role === 'user').at(-1)
      expect(lastUserTurn?.sourceScope).toBe('conversation')
    })
  })
})

describe('a send on a thread this tab never remembered', () => {
  it('sends the same workspace body as before, with no conversation_only feature', async () => {
    await withController(async (controller) => {
      await tick()
      await sendFrom(controller, 'Første melding', 'workspace')
      await tick()
      streams[0]!.handlers.onConnected?.({ requestId: 'req-1', threadId: 'ordinary-thread-9' })
      await finishStream(streams[0]!, 'req-1')

      await sendFrom(controller, 'Andre melding', 'workspace')
      await tick()
      expect(streams[1]!.request.sourceScope).toBe('workspace')
      expect(buildChatWireBody(streams[1]!.request).features).not.toContain('conversation_only')
      const lastUserTurn = controller.state.turns.filter((turn) => turn.role === 'user').at(-1)
      expect(lastUserTurn?.sourceScope).toBe('workspace')
    })
  })
})
