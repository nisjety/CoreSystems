// @vitest-environment jsdom

/**
 * A regenerate, continue, edit or held send that would replay a turn's route on a Space thread (DEC-V-R8, findings 42 and 44).
 * With the BYO gateway switch on, a Space thread never takes the member's ChatGPT subscription or their own gateway, and a turn
 * written under one of them is not sent on it again: the replay is refused before anything is truncated, aborted or sent, and
 * the answer on screen stays. Off, or outside a Space, the replay goes out on its connection as it always did.
 *
 * Driven through the real controller, with the request `streamChat` was called with captured, as the connection-replay test is.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { setActiveChatThreadId } from '@/features/chat/lib/chat-thread-history'

type CapturedStream = {
  request: {
    content: string
    model?: string
    provider?: string
    modelConnectionId?: string
    subscriptionConnectionId?: string
    regenerated?: boolean
    editResubmit?: boolean
  }
  handlers: ChatStreamHandlers
  resolve: () => void
}

const { streams, mockStreamChat, mockGetChatThread } = vi.hoisted(() => {
  const streams: CapturedStream[] = []
  const mockStreamChat = vi.fn((request: CapturedStream['request'], handlers: ChatStreamHandlers) => {
    return new Promise<void>((resolve) => {
      streams.push({ request, handlers, resolve })
    })
  })
  const mockGetChatThread = vi.fn(async (_threadId: string): Promise<unknown> => null)
  return { streams, mockStreamChat, mockGetChatThread }
})

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    streamChat: mockStreamChat,
    getChatThread: mockGetChatThread,
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
    getChatThreadTranscript: vi.fn().mockResolvedValue(null),
    getThreadMessages: vi.fn().mockResolvedValue([]),
    cancelInvocation: vi.fn().mockResolvedValue(undefined),
  }
})

import { useChatController } from './use-chat-controller'

const SPACE_THREAD = 'thread-room'
const SPACE_REF = 'space-room'
const STAMP = '2026-11-18T09:00:00.000Z'
const MODEL = 'gateway-a'
const GATEWAY_CONNECTION = 'conn-gw-1'
const SUBSCRIPTION_PROVIDER = 'openai-codex-subscription'
const SUBSCRIPTION_CONNECTION = 'conn-sub-1'

type Controller = ReturnType<typeof useChatController>

/** Flush pending microtasks (SSE-handler continuations, chained awaits) and Solid's batched store writes. */
async function tick(times = 3) {
  for (let i = 0; i < times; i += 1) {
    await Promise.resolve()
    flush()
  }
}

/** Lets the load an active-thread event starts run to its end. */
async function settle() {
  for (let i = 0; i < 4; i += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0))
    flush()
  }
}

/** Mounts the controller the way a rendered page does: its mount effects run outside the owned scope. */
function mount(): { controller: Controller; dispose: () => void } {
  let controller: Controller | undefined
  let dispose: () => void = () => undefined
  createRoot((disposeRoot) => {
    dispose = disposeRoot
    controller = useChatController()
  })
  flush()
  if (!controller) throw new Error('the controller did not mount')
  return { controller, dispose }
}

/** Sends one question on the route given and lets its answer finish. */
async function answered(
  controller: Controller,
  route: { subscription?: boolean; gateway?: boolean } = {},
) {
  await controller.handleComposerSubmit({
    actions: [],
    attachments: [],
    text: 'Hei',
    tools: [],
    model: MODEL,
    ...(route.subscription ? { provider: SUBSCRIPTION_PROVIDER, subscriptionConnectionId: SUBSCRIPTION_CONNECTION } : {}),
    ...(route.gateway ? { modelConnectionId: GATEWAY_CONNECTION } : {}),
  })
  await tick()
  const stream = streams.at(-1)!
  stream.handlers.onMessage?.({ content: 'Svar', requestId: 'req-1' })
  stream.handlers.onDone?.({ requestId: 'req-1', modelUsed: MODEL, outputTokens: 4, stopReason: 'stop' })
  stream.resolve()
  await tick()
}

/** Opens the Space thread the way the sidebar does, so its routing hint marks it as a Space thread. */
async function openSpaceThread() {
  setActiveChatThreadId(SPACE_THREAD)
  await settle()
}

beforeEach(() => {
  __resetRetentionForTests()
  window.localStorage.clear()
  window.sessionStorage.clear()
  mockGetChatThread.mockImplementation(async (threadId: string) => (
    threadId === SPACE_THREAD
      ? { threadId, title: 'Room', preview: 'Room', updatedAt: STAMP, pinned: false, spaceRef: SPACE_REF, origin: 'space' }
      : null
  ))
})

afterEach(() => {
  vi.unstubAllEnvs()
  streams.length = 0
  mockStreamChat.mockClear()
  mockGetChatThread.mockReset()
})

describe('replaying a turn on a Space thread while the BYO gateway switch is on', () => {
  beforeEach(() => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
  })

  it('refuses a regenerate of a subscription answer, keeps the answer, and sends nothing', async () => {
    const { controller, dispose } = mount()
    try {
      await openSpaceThread()
      await answered(controller, { subscription: true })
      controller.regenerateLatest()
      await tick()
      expect(streams).toHaveLength(1)
      expect(controller.feedbackNotice()).toMatch(/delt Space|shared Space/)
      expect(controller.state.turns.map((turn) => turn.role)).toEqual(['user', 'assistant'])
    } finally {
      dispose()
    }
  })

  it('refuses a regenerate of an answer written under the member\'s own gateway, and sends nothing', async () => {
    const { controller, dispose } = mount()
    try {
      await openSpaceThread()
      await answered(controller, { gateway: true })
      controller.regenerateLatest()
      await tick()
      expect(streams).toHaveLength(1)
      expect(controller.feedbackNotice()).toMatch(/egen gateway|own gateway/)
      expect(controller.state.turns.map((turn) => turn.role)).toEqual(['user', 'assistant'])
    } finally {
      dispose()
    }
  })

  it('refuses an edit of a question sent on a subscription, before the transcript is discarded', async () => {
    const { controller, dispose } = mount()
    try {
      await openSpaceThread()
      await answered(controller, { subscription: true })
      const question = controller.state.turns[0]!
      void controller.editAndResubmit(question.id, 'Hei igjen')
      await tick(6)
      expect(streams).toHaveLength(1)
      expect(controller.state.turns.map((turn) => turn.role)).toEqual(['user', 'assistant'])
    } finally {
      dispose()
    }
  })

  it('refuses a continuation of a stopped subscription answer, and sends nothing', async () => {
    const { controller, dispose } = mount()
    try {
      await openSpaceThread()
      await tick()
      await controller.handleComposerSubmit({
        actions: [],
        attachments: [],
        text: 'Skriv litt om fiskeoppdrett',
        tools: [],
        model: MODEL,
        provider: SUBSCRIPTION_PROVIDER,
        subscriptionConnectionId: SUBSCRIPTION_CONNECTION,
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
      expect(streams).toHaveLength(1)
      expect(controller.feedbackNotice()).toMatch(/delt Space|shared Space/)
    } finally {
      dispose()
    }
  })

  it('refuses an edit of a question sent under the member\'s own gateway, before the transcript is discarded', async () => {
    const { controller, dispose } = mount()
    try {
      await openSpaceThread()
      await answered(controller, { gateway: true })
      const question = controller.state.turns[0]!
      void controller.editAndResubmit(question.id, 'Hei igjen')
      await tick(6)
      expect(streams).toHaveLength(1)
      expect(controller.feedbackNotice()).toMatch(/egen gateway|own gateway/)
      expect(controller.state.turns.map((turn) => turn.role)).toEqual(['user', 'assistant'])
    } finally {
      dispose()
    }
  })

  it('refuses a continuation of a stopped answer written under the member\'s own gateway, and sends nothing', async () => {
    const { controller, dispose } = mount()
    try {
      await openSpaceThread()
      await tick()
      await controller.handleComposerSubmit({
        actions: [],
        attachments: [],
        text: 'Skriv litt om fiskeoppdrett',
        tools: [],
        model: MODEL,
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
      expect(streams).toHaveLength(1)
      expect(controller.feedbackNotice()).toMatch(/egen gateway|own gateway/)
    } finally {
      dispose()
    }
  })

  it('refuses a message held behind a run that has not named itself when it goes out on the subscription route, and keeps its words', async () => {
    const { controller, dispose } = mount()
    try {
      await openSpaceThread()
      await tick()
      await controller.handleComposerSubmit({ actions: [], attachments: [], text: 'Hei', tools: [], model: MODEL })
      await tick()
      expect(streams).toHaveLength(1)
      // The first run has not named itself yet, so this message is held until the run ends. The held send keeps the
      // subscription route it was typed with, and the gate applies when it would go out.
      await controller.handleComposerSubmit({
        actions: [],
        attachments: [],
        text: 'Legg til kolonne',
        tools: [],
        model: MODEL,
        provider: SUBSCRIPTION_PROVIDER,
        subscriptionConnectionId: SUBSCRIPTION_CONNECTION,
      })
      await tick()
      const first = streams[0]!
      first.handlers.onDone?.({ requestId: 'req-1', modelUsed: MODEL, outputTokens: 4, stopReason: 'stop' })
      first.resolve()
      await tick(6)
      expect(streams).toHaveLength(1)
      expect(controller.input()).toBe('Legg til kolonne')
      expect(controller.feedbackNotice()).toMatch(/delt Space|shared Space/)
      expect(controller.state.queuedInputs.find((entry) => entry.content === 'Legg til kolonne')?.state).toBe('refused')
    } finally {
      dispose()
    }
  })
})

describe('replaying a turn while the route is not withheld', () => {
  it('replays a customer-gateway answer on its connection in a chat that is not a Space thread', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    const { controller, dispose } = mount()
    try {
      await tick()
      await answered(controller, { gateway: true })
      controller.regenerateLatest()
      await tick()
      expect(streams).toHaveLength(2)
      expect(streams[1]!.request).toMatchObject({ modelConnectionId: GATEWAY_CONNECTION, regenerated: true })
    } finally {
      dispose()
    }
  })

  it('replays a customer-gateway answer on its connection in a Space thread while the switch is off, as before', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    const { controller, dispose } = mount()
    try {
      await openSpaceThread()
      await answered(controller, { gateway: true })
      controller.regenerateLatest()
      await tick()
      expect(streams).toHaveLength(2)
      expect(streams[1]!.request).toMatchObject({ modelConnectionId: GATEWAY_CONNECTION, regenerated: true })
    } finally {
      dispose()
    }
  })
})
