// @vitest-environment jsdom

/**
 * A route a Space thread may not take (DEC-V-R8) is refused where it would be sent: a launch from another page, once the chat it
 * lands in is known, and a send the composer hands over that goes out now (R-spa-05). The composer withholds such a route before it
 * submits; the page checks it again, so the route never reaches a Space whatever the caller passes. The BYO gateway switch off, or
 * a chat that is not a Space, withholds nothing, and a plain launch is sent as before.
 *
 * Driven through the real controller, with the request `streamChat` was called with captured, as the other Space tests are.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { setActiveChatThreadId } from '@/features/chat/lib/chat-thread-history'
import { queueChatProjectStart } from '@/features/chat/lib/chat-projects'
import { writePendingChatLaunch } from '@/features/chat/lib/pending-chat-launch'

type CapturedStream = {
  request: {
    content: string
    model?: string
    provider?: string
    modelConnectionId?: string
    subscriptionConnectionId?: string
    spaceRef?: string
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

vi.mock('@/shared/api/spaces-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/spaces-client')>()
  return { ...actual, getSpaceThreadTranscript: vi.fn(async () => ({ turns: [] })) }
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
const SUBSCRIPTION_REFUSAL = /delt Space|shared Space/
const GATEWAY_REFUSAL = /egen gateway|own gateway/

type Controller = ReturnType<typeof useChatController>

/** Lets the work an active-thread event, a launch or a send starts run to its end. */
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

/** Writes a launch the way the dashboard composer does, then mounts the chat page, which takes it and sends it. */
async function launchOnMount(launch: Parameters<typeof writePendingChatLaunch>[0]) {
  await writePendingChatLaunch(launch)
  const mounted = mount()
  await settle()
  return mounted
}

/** Sends a message through the page's composer submit, with the route the composer chose. */
function submit(controller: Controller, route: { subscription?: boolean; gateway?: boolean } = {}) {
  return controller.handleComposerSubmit({
    actions: [],
    attachments: [],
    text: 'Legg til kolonne',
    tools: [],
    model: MODEL,
    ...(route.subscription ? { provider: SUBSCRIPTION_PROVIDER, subscriptionConnectionId: SUBSCRIPTION_CONNECTION } : {}),
    ...(route.gateway ? { modelConnectionId: GATEWAY_CONNECTION } : {}),
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

describe('a pending launch whose route the chat it lands in may not take, while the BYO gateway switch is on', () => {
  beforeEach(() => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
  })

  it('is refused in a Space thread: nothing is sent, and the words go back to the box with the notice', async () => {
    setActiveChatThreadId(SPACE_THREAD)
    const { controller, dispose } = await launchOnMount({
      text: 'Legg til kolonne',
      model: MODEL,
      provider: SUBSCRIPTION_PROVIDER,
      subscriptionConnectionId: SUBSCRIPTION_CONNECTION,
    })
    try {
      expect(controller.state.threadId).toBe(SPACE_THREAD)
      expect(streams).toHaveLength(0)
      expect(controller.input()).toBe('Legg til kolonne')
      expect(controller.feedbackNotice()).toMatch(SUBSCRIPTION_REFUSAL)
    } finally {
      dispose()
    }
  })

  it('is refused in a Space thread when it names the member\'s own gateway', async () => {
    setActiveChatThreadId(SPACE_THREAD)
    const { controller, dispose } = await launchOnMount({
      text: 'Legg til kolonne',
      model: MODEL,
      modelConnectionId: GATEWAY_CONNECTION,
    })
    try {
      expect(streams).toHaveLength(0)
      expect(controller.input()).toBe('Legg til kolonne')
      expect(controller.feedbackNotice()).toMatch(GATEWAY_REFUSAL)
    } finally {
      dispose()
    }
  })

  it('is refused in a new chat in a picked project, which is a Space from its first send', async () => {
    vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'true')
    queueChatProjectStart(SPACE_REF)
    const { controller, dispose } = await launchOnMount({
      text: 'Legg til kolonne',
      model: MODEL,
      provider: SUBSCRIPTION_PROVIDER,
      subscriptionConnectionId: SUBSCRIPTION_CONNECTION,
    })
    try {
      expect(controller.state.threadId).toBeNull()
      expect(controller.activeProjectRef()).toBe(SPACE_REF)
      expect(streams).toHaveLength(0)
      expect(controller.input()).toBe('Legg til kolonne')
      expect(controller.feedbackNotice()).toMatch(SUBSCRIPTION_REFUSAL)
    } finally {
      dispose()
    }
  })

  it('is sent as before into a chat that is not a Space, on the route it names', async () => {
    setActiveChatThreadId(PLAIN_THREAD)
    const { controller, dispose } = await launchOnMount({
      text: 'Legg til kolonne',
      model: MODEL,
      provider: SUBSCRIPTION_PROVIDER,
      subscriptionConnectionId: SUBSCRIPTION_CONNECTION,
    })
    try {
      expect(streams).toHaveLength(1)
      expect(streams[0]!.request).toMatchObject({ content: 'Legg til kolonne', subscriptionConnectionId: SUBSCRIPTION_CONNECTION })
      expect(controller.feedbackNotice()).toBeNull()
    } finally {
      dispose()
    }
  })

  it('a plain launch into a Space thread is sent to the Space, as before', async () => {
    setActiveChatThreadId(SPACE_THREAD)
    const { controller, dispose } = await launchOnMount({ text: 'Hei', model: MODEL })
    try {
      expect(streams).toHaveLength(1)
      expect(streams[0]!.request).toMatchObject({ content: 'Hei', spaceRef: SPACE_REF })
      expect(controller.feedbackNotice()).toBeNull()
      expect(controller.input()).toBe('')
    } finally {
      dispose()
    }
  })

  it('a handoff that starts a new chat does not carry a project picked on mount into it, so its route is not withheld', async () => {
    vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'true')
    queueChatProjectStart(SPACE_REF)
    const { controller, dispose } = await launchOnMount({
      text: 'Hei',
      model: MODEL,
      startNewThread: true,
      provider: SUBSCRIPTION_PROVIDER,
      subscriptionConnectionId: SUBSCRIPTION_CONNECTION,
    })
    try {
      expect(controller.activeProjectRef()).toBeNull()
      expect(streams).toHaveLength(1)
      expect(streams[0]!.request.spaceRef).toBeUndefined()
      expect(streams[0]!.request).toMatchObject({ subscriptionConnectionId: SUBSCRIPTION_CONNECTION })
    } finally {
      dispose()
    }
  })
})

describe('a send the composer hands over that goes out now, while the BYO gateway switch is on (R-spa-05)', () => {
  beforeEach(() => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
  })

  it('refuses a ChatGPT subscription route in a Space thread, and throws so the composer keeps the draft', async () => {
    setActiveChatThreadId(SPACE_THREAD)
    const { controller, dispose } = mount()
    try {
      await settle()
      await expect(submit(controller, { subscription: true })).rejects.toThrow(SUBSCRIPTION_REFUSAL)
      expect(streams).toHaveLength(0)
      expect(controller.state.turns).toHaveLength(0)
    } finally {
      dispose()
    }
  })

  it('refuses the member\'s own gateway in a Space thread, and throws so the composer keeps the draft', async () => {
    setActiveChatThreadId(SPACE_THREAD)
    const { controller, dispose } = mount()
    try {
      await settle()
      await expect(submit(controller, { gateway: true })).rejects.toThrow(GATEWAY_REFUSAL)
      expect(streams).toHaveLength(0)
    } finally {
      dispose()
    }
  })

  it('refuses a route in a new chat in a picked project', async () => {
    vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'true')
    const { controller, dispose } = mount()
    try {
      queueChatProjectStart(SPACE_REF)
      flush()
      await settle()
      expect(controller.activeProjectRef()).toBe(SPACE_REF)
      await expect(submit(controller, { subscription: true })).rejects.toThrow(SUBSCRIPTION_REFUSAL)
      expect(streams).toHaveLength(0)
    } finally {
      dispose()
    }
  })

  it('sends a plain message in a Space thread to the Space, as before', async () => {
    setActiveChatThreadId(SPACE_THREAD)
    const { controller, dispose } = mount()
    try {
      await settle()
      await submit(controller)
      await settle()
      expect(streams).toHaveLength(1)
      expect(streams[0]!.request).toMatchObject({ content: 'Legg til kolonne', spaceRef: SPACE_REF })
    } finally {
      dispose()
    }
  })

  it('sends a route in a chat that is not a Space, as before', async () => {
    setActiveChatThreadId(PLAIN_THREAD)
    const { controller, dispose } = mount()
    try {
      await settle()
      await submit(controller, { subscription: true })
      await settle()
      expect(streams).toHaveLength(1)
      expect(streams[0]!.request).toMatchObject({ subscriptionConnectionId: SUBSCRIPTION_CONNECTION })
    } finally {
      dispose()
    }
  })
})

describe('the same launches and sends while the BYO gateway switch is off', () => {
  it('sends a launch on its route into a Space thread, as before the check', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    setActiveChatThreadId(SPACE_THREAD)
    const { controller, dispose } = await launchOnMount({
      text: 'Hei',
      model: MODEL,
      provider: SUBSCRIPTION_PROVIDER,
      subscriptionConnectionId: SUBSCRIPTION_CONNECTION,
    })
    try {
      expect(streams).toHaveLength(1)
      expect(streams[0]!.request).toMatchObject({ subscriptionConnectionId: SUBSCRIPTION_CONNECTION, spaceRef: SPACE_REF })
      expect(controller.feedbackNotice()).toBeNull()
    } finally {
      dispose()
    }
  })

  it('keeps no gateway id on a launch, so it is sent on the platform route as it was before', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    setActiveChatThreadId(PLAIN_THREAD)
    const { controller, dispose } = await launchOnMount({ text: 'Hei', model: MODEL, modelConnectionId: GATEWAY_CONNECTION })
    try {
      expect(streams).toHaveLength(1)
      expect(streams[0]!.request.modelConnectionId).toBeUndefined()
      expect(controller.feedbackNotice()).toBeNull()
    } finally {
      dispose()
    }
  })

  it('sends a direct route into a Space thread, as before', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    setActiveChatThreadId(SPACE_THREAD)
    const { controller, dispose } = mount()
    try {
      await settle()
      await submit(controller, { gateway: true })
      await settle()
      expect(streams).toHaveLength(1)
      expect(streams[0]!.request).toMatchObject({ modelConnectionId: GATEWAY_CONNECTION })
    } finally {
      dispose()
    }
  })
})
