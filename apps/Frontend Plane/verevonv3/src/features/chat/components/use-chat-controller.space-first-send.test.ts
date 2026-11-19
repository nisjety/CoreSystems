// @vitest-environment jsdom

/**
 * A new chat in a picked project is a Space from its first send (DEC-V-R8). Before that send the chat has no thread id, so the
 * composer's Space flag and the Space route gate must read the project the send is going to, not the open thread. A send held for
 * a run that ended is not sent on a route the open Space may not take: the words go back to the box and the queued strip says why.
 * With the BYO gateway switch off nothing is withheld, and with the projects switch off no project is ever picked.
 *
 * Driven through the real controller, with the request `streamChat` was called with captured, as the space-replay test is.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { setActiveChatThreadId } from '@/features/chat/lib/chat-thread-history'
import { queueChatProjectStart, rememberProjectThread } from '@/features/chat/lib/chat-projects'

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
const SPACE_REF = 'space-room'
const STAMP = '2026-11-18T09:00:00.000Z'
const MODEL = 'gateway-a'
const GATEWAY_CONNECTION = 'conn-gw-1'
const SUBSCRIPTION_PROVIDER = 'openai-codex-subscription'
const SUBSCRIPTION_CONNECTION = 'conn-sub-1'
const SUBSCRIPTION_REFUSAL = /delt Space|shared Space/
const GATEWAY_REFUSAL = /egen gateway|own gateway/

type Controller = ReturnType<typeof useChatController>

/** Lets the work an active-thread event or a send starts run to its end. */
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

/** Sends one message the way the composer does: the route it names is the one the composer chose. */
function send(controller: Controller, text: string, route: { subscription?: boolean; gateway?: boolean } = {}) {
  return controller.handleComposerSubmit({
    actions: [],
    attachments: [],
    text,
    tools: [],
    model: MODEL,
    ...(route.subscription ? { provider: SUBSCRIPTION_PROVIDER, subscriptionConnectionId: SUBSCRIPTION_CONNECTION } : {}),
    ...(route.gateway ? { modelConnectionId: GATEWAY_CONNECTION } : {}),
  })
}

/** Picks a project for a new chat as the sidebar does: the event reaches the mounted page. */
function pickProject(spaceRef = SPACE_REF) {
  queueChatProjectStart(spaceRef)
  flush()
}

/** Opens the project thread as the sidebar does; its routing hint names the room it belongs to. */
function openProjectThread() {
  setActiveChatThreadId(SPACE_THREAD)
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

describe('a new chat in a picked project, while the BYO gateway switch is on', () => {
  beforeEach(() => {
    vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'true')
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
  })

  it('is a Space before its first send, which is when the composer decides the routes it offers', () => {
    const { controller, dispose } = mount()
    try {
      expect(controller.spaceScoped()).toBe(false)
      pickProject()
      expect(controller.state.threadId).toBeNull()
      expect(controller.spaceScoped()).toBe(true)
      expect(controller.activeProjectRef()).toBe(SPACE_REF)
      expect(streams).toHaveLength(0)
    } finally {
      dispose()
    }
  })

  it('sends its first turn to the picked project, and stays a Space once the turn has a thread', async () => {
    const { controller, dispose } = mount()
    try {
      pickProject()
      await send(controller, 'Hei')
      await settle()
      expect(streams).toHaveLength(1)
      expect(streams[0]!.request).toMatchObject({ content: 'Hei', spaceRef: SPACE_REF })
      expect(controller.spaceScoped()).toBe(true)
      expect(controller.activeProjectRef()).toBe(SPACE_REF)
    } finally {
      dispose()
    }
  })

  it('is not a Space without a picked project', () => {
    const { controller, dispose } = mount()
    try {
      expect(controller.spaceScoped()).toBe(false)
      expect(controller.activeProjectRef()).toBeNull()
    } finally {
      dispose()
    }
  })
})

describe('a new chat while the projects switch is off', () => {
  it('never picks a project, so nothing is a Space and nothing is withheld', () => {
    vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'false')
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    const { controller, dispose } = mount()
    try {
      pickProject()
      expect(controller.spaceScoped()).toBe(false)
      expect(controller.activeProjectRef()).toBeNull()
    } finally {
      dispose()
    }
  })
})

describe('a send held for a run that ended, when the chat open at the flush is a Space', () => {
  beforeEach(() => {
    vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'true')
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    rememberProjectThread(SPACE_THREAD, SPACE_REF)
  })

  it('is not sent on a ChatGPT subscription route: the words go back to the box and the queue says why', async () => {
    const { controller, dispose } = mount()
    try {
      await send(controller, 'Først', { subscription: true })
      await settle()
      await send(controller, 'Vent litt', { subscription: true })
      await settle()
      expect(streams).toHaveLength(1)

      openProjectThread()
      await settle()

      expect(streams).toHaveLength(1)
      expect(controller.feedbackNotice()).toMatch(SUBSCRIPTION_REFUSAL)
      expect(controller.input()).toContain('Vent litt')
      const held = controller.state.queuedInputs.find((entry) => entry.content === 'Vent litt')
      expect(held?.state).toBe('refused')
      expect(held?.note).toMatch(SUBSCRIPTION_REFUSAL)
    } finally {
      dispose()
    }
  })

  it('is not sent on the member\'s own gateway either', async () => {
    const { controller, dispose } = mount()
    try {
      await send(controller, 'Først', { gateway: true })
      await settle()
      await send(controller, 'Vent litt', { gateway: true })
      await settle()

      openProjectThread()
      await settle()

      expect(streams).toHaveLength(1)
      expect(controller.feedbackNotice()).toMatch(GATEWAY_REFUSAL)
      expect(controller.input()).toContain('Vent litt')
    } finally {
      dispose()
    }
  })

  it('is sent to the open Space on the platform route, which the Space takes', async () => {
    const { controller, dispose } = mount()
    try {
      await send(controller, 'Først')
      await settle()
      await send(controller, 'Vent litt')
      await settle()

      openProjectThread()
      await settle()

      expect(streams).toHaveLength(2)
      expect(streams[1]!.request).toMatchObject({ content: 'Vent litt', spaceRef: SPACE_REF })
      expect(controller.feedbackNotice()).toBeNull()
    } finally {
      dispose()
    }
  })
})

describe('a send held for a run that ended, while the BYO gateway switch is off', () => {
  it('is sent as before, on the subscription route the person chose', async () => {
    vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'true')
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    rememberProjectThread(SPACE_THREAD, SPACE_REF)
    const { controller, dispose } = mount()
    try {
      await send(controller, 'Først', { subscription: true })
      await settle()
      await send(controller, 'Vent litt', { subscription: true })
      await settle()

      openProjectThread()
      await settle()

      expect(streams).toHaveLength(2)
      expect(streams[1]!.request).toMatchObject({
        content: 'Vent litt',
        subscriptionConnectionId: SUBSCRIPTION_CONNECTION,
        spaceRef: SPACE_REF,
      })
      expect(controller.feedbackNotice()).toBeNull()
    } finally {
      dispose()
    }
  })
})
