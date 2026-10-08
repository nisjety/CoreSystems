// @vitest-environment jsdom

/**
 * Do on the ChatGPT subscription (A5), as the chat controller sees it.
 *
 * The composer offers Do on a subscription model only when the server
 * advertises the `agentic` capability. A request that still arrives while the
 * server's AGENTIC_SUBSCRIPTION_ENABLED switch is off (a stale catalogue, a
 * switch flipped after the page loaded) is refused before any model runs, with
 * the stable code `agentic_subscription_disabled` in an SSE `error` frame. The
 * turn must say so in words, offer no retry of the same refused turn, and never
 * reattach, resend or move to another model.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { buildChatWireBody } from '@/shared/api/chat-client'
import type { ChatStreamHandlers, ChatInvokeRequest } from '@/shared/api/chat-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

type CapturedStream = {
  request: ChatInvokeRequest
  handlers: ChatStreamHandlers
  resolve: () => void
}

const { streams, mockStreamChat, mockResumeStream } = vi.hoisted(() => {
  const streams: CapturedStream[] = []
  const mockStreamChat = vi.fn((request: ChatInvokeRequest, handlers: ChatStreamHandlers) => {
    return new Promise<void>((resolve) => {
      streams.push({ request, handlers, resolve })
    })
  })
  const mockResumeStream = vi.fn(async () => undefined)
  return { streams, mockStreamChat, mockResumeStream }
})

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    streamChat: mockStreamChat,
    resumeStream: mockResumeStream,
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
    getChatThreadTranscript: vi.fn().mockResolvedValue(null),
    getThreadMessages: vi.fn().mockResolvedValue([]),
    cancelInvocation: vi.fn().mockResolvedValue(undefined),
  }
})

import { useChatController } from './use-chat-controller'

const SUBSCRIPTION = 'openai-codex-subscription'
const TERRA = 'gpt-5.6-terra'
// What model-gateway sends with the switch off: operator English, never shown.
const OPERATOR_ENGLISH =
  'openai-codex-subscription supports text-only, non-ZDR chat without image generation, browsing, deep research, plan mode, or agentic tool specs'

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

async function sendTerraTurn(controller: ReturnType<typeof useChatController>, text: string) {
  await tick()
  await controller.handleComposerSubmit({
    actions: [],
    attachments: [],
    text,
    tools: [],
    model: TERRA,
    provider: SUBSCRIPTION,
    subscriptionConnectionId: 'conn_1',
  })
  await tick()
}

beforeEach(() => {
  setSessionUser({ id: 'user_1', email: 'member@example.com', name: 'Member', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role: 'member' })
  flush()
})

afterEach(() => {
  streams.length = 0
  mockStreamChat.mockClear()
  mockResumeStream.mockClear()
  __resetRetentionForTests()
  clearSession()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('Do on a ChatGPT-subscription turn', () => {
  it('goes out as an agentic plan-mode turn on the same model and connection, with nothing else the route refuses', async () => {
    await withController(async (controller) => {
      // Past the synchronous part of createRoot: a write inside it is an owned write.
      await tick()
      controller.setPlanMode(true)
      flush()
      await sendTerraTurn(controller, 'Undersøk dette')

      expect(streams).toHaveLength(1)
      const { request } = streams[0]!
      expect(request).toMatchObject({
        model: TERRA,
        provider: SUBSCRIPTION,
        subscriptionConnectionId: 'conn_1',
        planMode: true,
      })
      // The body the gateway receives for that request.
      const body = buildChatWireBody(request)
      expect(body).toMatchObject({
        provider: SUBSCRIPTION,
        subscription_connection_id: 'conn_1',
        plan_mode: true,
        browse_web: false,
        generate_image: false,
        deep_research: false,
        attachments: [],
        tools: [],
      })
      expect(body.features).toEqual(expect.arrayContaining(['agentic', 'tools']))

      streams[0]!.handlers.onDone?.({})
      streams[0]!.resolve()
      await tick()
    })
  })

  it('says Do is not switched on, in words, when the server has it off, and neither resends nor reattaches', async () => {
    await withController(async (controller) => {
      // Past the synchronous part of createRoot: a write inside it is an owned write.
      await tick()
      controller.setPlanMode(true)
      flush()
      await sendTerraTurn(controller, 'Undersøk dette')
      const first = streams[0]!

      first.handlers.onError?.({ code: 'agentic_subscription_disabled', message: OPERATOR_ENGLISH })
      first.resolve()
      await tick(8)

      const turn = controller.state.turns.at(-1)!
      expect(turn).toMatchObject({ role: 'assistant', status: 'error', failureAction: 'none', streaming: false })
      expect(turn.content).toMatch(
        /Utfør er ikke slått på for ChatGPT-abonnementet|Do is not switched on for the ChatGPT subscription/,
      )
      // The operator text is never what the reader sees.
      expect(turn.content).not.toContain('text-only')
      expect(controller.state.error).toBe(turn.content)

      // Refused before any model ran: nothing is retried, resumed or sent elsewhere.
      expect(streams).toHaveLength(1)
      expect(mockResumeStream).not.toHaveBeenCalled()
      expect(turn).toMatchObject({ provider: SUBSCRIPTION, model: TERRA })
    })
  })
})
