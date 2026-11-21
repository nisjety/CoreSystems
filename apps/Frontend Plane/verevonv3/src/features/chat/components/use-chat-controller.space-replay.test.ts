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
import { afterAll, afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { setActiveChatThreadId, upsertChatThreadTranscript } from '@/features/chat/lib/chat-thread-history'

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
const PLAIN_THREAD = 'thread-plain'
const SPACE_REF = 'space-room'
const STAMP = '2026-11-18T09:00:00.000Z'
const MODEL = 'gateway-a'
const GATEWAY_CONNECTION = 'conn-gw-1'
const SUBSCRIPTION_PROVIDER = 'openai-codex-subscription'
const SUBSCRIPTION_CONNECTION = 'conn-sub-1'

// `connectionKeptEnabled` is read once per build (chat-normalizers.ts): a saved turn keeps its customer-gateway connection only in a
// build with the BYO switch on. The seeded transcripts below are read as such a build reads them.
vi.hoisted(() => {
  process.env.VITE_BYO_GATEWAY = 'true'
})

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

/**
 * A Space thread whose saved exchange was written under a route, as its transcript holds it. A send on a route a Space may not take
 * is refused where it is made (R-spa-05), so the exchange a replay starts from is one that was saved before this thread was opened.
 */
function seedSpaceExchange(route: { subscription?: boolean; gateway?: boolean } = {}, answer: { stopped?: boolean } = {}) {
  const routed = {
    ...(route.subscription ? { provider: SUBSCRIPTION_PROVIDER, subscriptionConnectionId: SUBSCRIPTION_CONNECTION } : {}),
    ...(route.gateway ? { modelConnectionId: GATEWAY_CONNECTION } : {}),
  }
  upsertChatThreadTranscript({
    threadId: SPACE_THREAD,
    updatedAt: STAMP,
    turns: [
      { id: 'turn-question', role: 'user', content: 'Hei', createdAt: STAMP, model: MODEL, ...routed },
      {
        id: 'turn-answer',
        role: 'assistant',
        content: 'Svar',
        createdAt: STAMP,
        model: MODEL,
        ...routed,
        ...(answer.stopped ? { status: 'stopped' as const } : {}),
      },
    ],
  })
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

afterAll(() => {
  // The build-level switch set above belongs to this file only.
  delete process.env.VITE_BYO_GATEWAY
})

describe('replaying a turn on a Space thread while the BYO gateway switch is on', () => {
  beforeEach(() => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
  })

  it('refuses a regenerate of a subscription answer, keeps the answer, and sends nothing', async () => {
    seedSpaceExchange({ subscription: true })
    const { controller, dispose } = mount()
    try {
      await openSpaceThread()
      controller.regenerateLatest()
      await tick()
      expect(streams).toHaveLength(0)
      expect(controller.feedbackNotice()).toMatch(/delt Space|shared Space/)
      expect(controller.state.turns.map((turn) => turn.role)).toEqual(['user', 'assistant'])
    } finally {
      dispose()
    }
  })

  it('refuses a regenerate of an answer written under the member\'s own gateway, and sends nothing', async () => {
    seedSpaceExchange({ gateway: true })
    const { controller, dispose } = mount()
    try {
      await openSpaceThread()
      controller.regenerateLatest()
      await tick()
      expect(streams).toHaveLength(0)
      expect(controller.feedbackNotice()).toMatch(/egen gateway|own gateway/)
      expect(controller.state.turns.map((turn) => turn.role)).toEqual(['user', 'assistant'])
    } finally {
      dispose()
    }
  })

  it('refuses an edit of a question sent on a subscription, before the transcript is discarded', async () => {
    seedSpaceExchange({ subscription: true })
    const { controller, dispose } = mount()
    try {
      await openSpaceThread()
      const question = controller.state.turns[0]!
      void controller.editAndResubmit(question.id, 'Hei igjen')
      await tick(6)
      expect(streams).toHaveLength(0)
      expect(controller.state.turns.map((turn) => turn.role)).toEqual(['user', 'assistant'])
    } finally {
      dispose()
    }
  })

  it('refuses a continuation of a stopped subscription answer, and sends nothing', async () => {
    seedSpaceExchange({ subscription: true }, { stopped: true })
    const { controller, dispose } = mount()
    try {
      await openSpaceThread()
      await tick()
      const assistantId = controller.state.turns[1]!.id
      expect(controller.state.turns[1]?.status).toBe('stopped')

      controller.continueGeneration(assistantId)
      flush()
      await tick()
      expect(streams).toHaveLength(0)
      expect(controller.feedbackNotice()).toMatch(/delt Space|shared Space/)
    } finally {
      dispose()
    }
  })

  it('refuses an edit of a question sent under the member\'s own gateway, before the transcript is discarded', async () => {
    seedSpaceExchange({ gateway: true })
    const { controller, dispose } = mount()
    try {
      await openSpaceThread()
      const question = controller.state.turns[0]!
      void controller.editAndResubmit(question.id, 'Hei igjen')
      await tick(6)
      expect(streams).toHaveLength(0)
      expect(controller.feedbackNotice()).toMatch(/egen gateway|own gateway/)
      expect(controller.state.turns.map((turn) => turn.role)).toEqual(['user', 'assistant'])
    } finally {
      dispose()
    }
  })

  it('refuses a continuation of a stopped answer written under the member\'s own gateway, and sends nothing', async () => {
    seedSpaceExchange({ gateway: true }, { stopped: true })
    const { controller, dispose } = mount()
    try {
      await openSpaceThread()
      await tick()
      const assistantId = controller.state.turns[1]!.id
      expect(controller.state.turns[1]?.status).toBe('stopped')

      controller.continueGeneration(assistantId)
      flush()
      await tick()
      expect(streams).toHaveLength(0)
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

describe('a held send while the Space ref of the thread it would flush into has not loaded (R-spa-01)', () => {
  let holdRef = false
  let releaseRef: (() => void) | undefined

  beforeEach(() => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    holdRef = false
    releaseRef = undefined
    mockGetChatThread.mockImplementation((threadId: string) => {
      const session = threadId === SPACE_THREAD
        ? { threadId, title: 'Room', preview: 'Room', updatedAt: STAMP, pinned: false, spaceRef: SPACE_REF, origin: 'space' }
        : null
      if (!holdRef) return Promise.resolve(session)
      // The lookup answers when the test releases it, as a slow network does.
      return new Promise((resolve) => {
        releaseRef = () => resolve(session)
      })
    })
  })

  /** Holds a send behind the run that is streaming: that run has not named itself, so the next message waits for it to end. */
  async function holdBehindFirstRun(controller: Controller, route: { subscription?: boolean } = {}) {
    await controller.handleComposerSubmit({ actions: [], attachments: [], text: 'Først', tools: [], model: MODEL })
    await tick()
    expect(streams).toHaveLength(1)
    await controller.handleComposerSubmit({
      actions: [],
      attachments: [],
      text: 'Legg til kolonne',
      tools: [],
      model: MODEL,
      ...(route.subscription ? { provider: SUBSCRIPTION_PROVIDER, subscriptionConnectionId: SUBSCRIPTION_CONNECTION } : {}),
    })
    await tick()
    expect(controller.state.queuedInputs.find((entry) => entry.content === 'Legg til kolonne')?.state).toBe('pending')
  }

  /** Opens the thread the way the sidebar does, with its ref lookup held until the test releases it. */
  async function openWhileRefLoads(threadId: string) {
    holdRef = true
    setActiveChatThreadId(threadId)
    await settle()
    expect(releaseRef).toBeDefined()
  }

  it('is not sent before the Space ref of the thread has loaded, and goes out to the Space once it has', async () => {
    const { controller, dispose } = mount()
    try {
      await holdBehindFirstRun(controller)
      await openWhileRefLoads(SPACE_THREAD)
      expect(controller.state.threadId).toBe(SPACE_THREAD)
      expect(streams).toHaveLength(1)

      releaseRef?.()
      await settle()
      expect(streams).toHaveLength(2)
      expect(streams[1]!.request).toMatchObject({ content: 'Legg til kolonne', spaceRef: SPACE_REF })
    } finally {
      dispose()
    }
  })

  it('is refused, and its words go back to the box, when the resolved ref is a Space and the send held a subscription route', async () => {
    const { controller, dispose } = mount()
    try {
      await holdBehindFirstRun(controller, { subscription: true })
      await openWhileRefLoads(SPACE_THREAD)
      expect(streams).toHaveLength(1)

      releaseRef?.()
      await settle()
      expect(streams).toHaveLength(1)
      expect(controller.input()).toBe('Legg til kolonne')
      expect(controller.feedbackNotice()).toMatch(/delt Space|shared Space/)
      expect(controller.state.queuedInputs.find((entry) => entry.content === 'Legg til kolonne')?.state).toBe('refused')
    } finally {
      dispose()
    }
  })

  it('flushes a held subscription send into a thread that is not a Space once its ref has loaded', async () => {
    const { controller, dispose } = mount()
    try {
      await holdBehindFirstRun(controller, { subscription: true })
      await openWhileRefLoads(PLAIN_THREAD)
      expect(streams).toHaveLength(1)

      releaseRef?.()
      await settle()
      expect(streams).toHaveLength(2)
      expect(streams[1]!.request).toMatchObject({ content: 'Legg til kolonne', subscriptionConnectionId: SUBSCRIPTION_CONNECTION })
      expect(controller.feedbackNotice()).toBeNull()
    } finally {
      dispose()
    }
  })

  it('returns every held send to the box, with the Space copy, when the ref lookup fails', async () => {
    const { controller, dispose } = mount()
    try {
      await holdBehindFirstRun(controller)
      mockGetChatThread.mockRejectedValueOnce(new Error('offline'))
      setActiveChatThreadId(SPACE_THREAD)
      await settle()
      expect(controller.state.threadId).toBe(SPACE_THREAD)
      expect(streams).toHaveLength(1)
      expect(controller.input()).toBe('Legg til kolonne')
      expect(controller.feedbackNotice()).toMatch(/delt Space|shared Space/)
      expect(controller.state.queuedInputs.find((entry) => entry.content === 'Legg til kolonne')?.state).toBe('refused')
    } finally {
      dispose()
    }
  })
})
