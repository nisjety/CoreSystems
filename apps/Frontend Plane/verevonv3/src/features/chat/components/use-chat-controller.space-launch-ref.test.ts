// @vitest-environment jsdom

/**
 * A launch is judged only once the Space ref of the chat it lands in is known (P2-spa-03 rel1, the R-spa-01 rule for a launch). While
 * that ref loads, the chat's Space status is unknown, and it stays unknown when the load failed. So a launch that names the member's
 * ChatGPT subscription or own gateway is refused then, and so is a plain launch: it goes out without the chat's spaceRef, so it waits
 * for the ref as well. Nothing is sent, its words go back to the box, and the notice says why. With the BYO gateway switch off nothing
 * waits and nothing is refused. A zero-retention launch keeps its zero-retention choice with its words, which are not stored as a draft.
 *
 * Driven through the real controller, with the request `streamChat` was called with captured, as the other Space launch tests are.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { CHAT_DRAFT_DEBOUNCE_MS } from '@/features/chat/lib/chat-thread-draft'
import { setActiveChatThreadId } from '@/features/chat/lib/chat-thread-history'
import { writePendingChatLaunch } from '@/features/chat/lib/pending-chat-launch'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

type CapturedStream = {
  request: {
    content: string
    model?: string
    provider?: string
    modelConnectionId?: string
    subscriptionConnectionId?: string
    spaceRef?: string
    zdr?: boolean
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

const FAILED_THREAD = 'thread-lookup-failed'
const LOADING_THREAD = 'thread-lookup-loading'
const PLAIN_THREAD = 'thread-plain'
const MODEL = 'gateway-a'
const GATEWAY_CONNECTION = 'conn-gw-1'
const SUBSCRIPTION_PROVIDER = 'openai-codex-subscription'
const SUBSCRIPTION_CONNECTION = 'conn-sub-1'
const SUBSCRIPTION_ROUTE = { provider: SUBSCRIPTION_PROVIDER, subscriptionConnectionId: SUBSCRIPTION_CONNECTION }
const FAILED_NOTICE = /Kunne ikke sjekke|Could not check/
const LOADING_NOTICE = /lastes fortsatt|still loading/
const FILES_NOTICE = /Vedleggene ble ikke lagt tilbake|files were not put back/

// The lookup of the loading thread stays open until a test releases it, so that thread's Space ref is unknown while a launch is judged.
let releaseLoading: ((session: unknown) => void) | undefined

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

/** Lets the open loading thread's lookup answer, so its load can end, and waits for the work that follows. */
async function finishLoading() {
  releaseLoading?.(null)
  await settle()
}

/** The sessionStorage keys whose stored value holds `text`, so a chat draft of those words would show up here. */
function sessionKeysHolding(text: string): string[] {
  const keys: string[] = []
  for (let i = 0; i < window.sessionStorage.length; i += 1) {
    const key = window.sessionStorage.key(i)
    if (key !== null && window.sessionStorage.getItem(key)?.includes(text)) keys.push(key)
  }
  return keys
}

/** Sends a message through the page's composer submit, with the route the composer chose. */
function send(controller: Controller, route: { subscription?: boolean; gateway?: boolean } = {}) {
  return controller.handleComposerSubmit({
    actions: [],
    attachments: [],
    text: 'Legg til kolonne',
    tools: [],
    model: MODEL,
    ...(route.subscription ? SUBSCRIPTION_ROUTE : {}),
    ...(route.gateway ? { modelConnectionId: GATEWAY_CONNECTION } : {}),
  })
}

beforeEach(() => {
  __resetRetentionForTests()
  window.localStorage.clear()
  window.sessionStorage.clear()
  mockGetChatThread.mockImplementation(async (threadId: string) => {
    if (threadId === FAILED_THREAD) throw new Error('lookup failed')
    if (threadId === LOADING_THREAD) return new Promise((resolve) => { releaseLoading = resolve })
    return null
  })
})

afterEach(() => {
  vi.unstubAllEnvs()
  streams.length = 0
  mockStreamChat.mockClear()
  mockGetChatThread.mockReset()
  releaseLoading = undefined
})

describe('a launch judged while the BYO gateway switch is on, whose route a Space may not take or whose chat ref is unsettled (P2-spa-03 rel1)', () => {
  beforeEach(() => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
  })

  it('is refused while the chat it lands in is still loading its Space ref: nothing is sent and the words go back to the box', async () => {
    await writePendingChatLaunch({ text: 'Legg til kolonne', model: MODEL, ...SUBSCRIPTION_ROUTE })
    const { controller, dispose } = mount()
    try {
      // The user picks a thread while the page is starting, so its ref load is still in flight when the launch is judged.
      setActiveChatThreadId(LOADING_THREAD)
      await settle()
      expect(controller.state.threadId).toBe(LOADING_THREAD)
      expect(streams).toHaveLength(0)
      expect(controller.input()).toBe('Legg til kolonne')
      expect(controller.feedbackNotice()).toMatch(LOADING_NOTICE)
    } finally {
      await finishLoading()
      dispose()
    }
  })

  it('a plain launch judged while the chat it lands in is still loading is refused too, and its words go back to the box', async () => {
    await writePendingChatLaunch({ text: 'Hei', model: MODEL })
    const { controller, dispose } = mount()
    try {
      setActiveChatThreadId(LOADING_THREAD)
      await settle()
      expect(controller.state.threadId).toBe(LOADING_THREAD)
      expect(streams).toHaveLength(0)
      expect(controller.input()).toBe('Hei')
      expect(controller.feedbackNotice()).toMatch(LOADING_NOTICE)
    } finally {
      await finishLoading()
      dispose()
    }
  })

  it('is refused when the lookup of the chat it lands in failed, so its Space status is unknown: nothing is sent', async () => {
    setActiveChatThreadId(FAILED_THREAD)
    const { controller, dispose } = await launchOnMount({ text: 'Legg til kolonne', model: MODEL, ...SUBSCRIPTION_ROUTE })
    try {
      expect(controller.state.threadId).toBe(FAILED_THREAD)
      expect(streams).toHaveLength(0)
      expect(controller.input()).toBe('Legg til kolonne')
      expect(controller.feedbackNotice()).toMatch(FAILED_NOTICE)
    } finally {
      dispose()
    }
  })

  it('is refused when it names the member\'s own gateway and the lookup of the chat it lands in failed', async () => {
    setActiveChatThreadId(FAILED_THREAD)
    const { controller, dispose } = await launchOnMount({ text: 'Legg til kolonne', model: MODEL, modelConnectionId: GATEWAY_CONNECTION })
    try {
      expect(streams).toHaveLength(0)
      expect(controller.input()).toBe('Legg til kolonne')
      expect(controller.feedbackNotice()).toMatch(FAILED_NOTICE)
    } finally {
      dispose()
    }
  })

  it('a plain launch into a chat whose lookup failed is refused as well: it goes out without the spaceRef, so it waits for the ref', async () => {
    setActiveChatThreadId(FAILED_THREAD)
    const { controller, dispose } = await launchOnMount({ text: 'Hei', model: MODEL })
    try {
      expect(controller.state.threadId).toBe(FAILED_THREAD)
      expect(streams).toHaveLength(0)
      expect(controller.input()).toBe('Hei')
      expect(controller.feedbackNotice()).toMatch(FAILED_NOTICE)
    } finally {
      dispose()
    }
  })
})

describe('the same launches while the BYO gateway switch is off', () => {
  it('a plain launch into a chat whose lookup failed is sent as before', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    setActiveChatThreadId(FAILED_THREAD)
    const { controller, dispose } = await launchOnMount({ text: 'Hei', model: MODEL })
    try {
      expect(streams).toHaveLength(1)
      expect(streams[0]!.request).toMatchObject({ content: 'Hei' })
      expect(controller.feedbackNotice()).toBeNull()
    } finally {
      dispose()
    }
  })

  it('a zero-retention launch into a chat whose lookup failed is sent with zdr, as before', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    setActiveChatThreadId(FAILED_THREAD)
    const { controller, dispose } = await launchOnMount({ text: 'Hei', model: MODEL, zdr: true })
    try {
      expect(streams).toHaveLength(1)
      expect(streams[0]!.request).toMatchObject({ content: 'Hei', zdr: true })
      expect(controller.feedbackNotice()).toBeNull()
    } finally {
      dispose()
    }
  })

  it('a route into a chat whose lookup failed is sent on that route, as before', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    setActiveChatThreadId(FAILED_THREAD)
    const { controller, dispose } = await launchOnMount({ text: 'Legg til kolonne', model: MODEL, ...SUBSCRIPTION_ROUTE })
    try {
      expect(streams).toHaveLength(1)
      expect(streams[0]!.request).toMatchObject({ content: 'Legg til kolonne', subscriptionConnectionId: SUBSCRIPTION_CONNECTION })
      expect(controller.feedbackNotice()).toBeNull()
    } finally {
      dispose()
    }
  })

  it('a route judged while the chat it lands in is still loading is sent on that route, as before', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    await writePendingChatLaunch({ text: 'Legg til kolonne', model: MODEL, ...SUBSCRIPTION_ROUTE })
    const { controller, dispose } = mount()
    try {
      setActiveChatThreadId(LOADING_THREAD)
      await settle()
      expect(streams).toHaveLength(1)
      expect(streams[0]!.request).toMatchObject({ content: 'Legg til kolonne', subscriptionConnectionId: SUBSCRIPTION_CONNECTION })
      expect(controller.feedbackNotice()).toBeNull()
    } finally {
      await finishLoading()
      dispose()
    }
  })
})

describe('a send the composer hands over while the chat it goes into has not settled its Space ref, with the BYO gateway switch on (P2-spa-03 rel1, R-spa-05)', () => {
  beforeEach(() => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
  })

  it('is refused while the Space ref of the chat is still loading: it throws so the composer keeps the draft, and nothing is sent', async () => {
    const { controller, dispose } = mount()
    try {
      setActiveChatThreadId(LOADING_THREAD)
      await settle()
      expect(controller.state.threadId).toBe(LOADING_THREAD)
      await expect(send(controller, { subscription: true })).rejects.toThrow(LOADING_NOTICE)
      await settle()
      expect(streams).toHaveLength(0)
      expect(controller.state.turns).toHaveLength(0)
    } finally {
      await finishLoading()
      dispose()
    }
  })

  it('refuses the member\'s own gateway the same way while the Space ref of the chat is still loading', async () => {
    const { controller, dispose } = mount()
    try {
      setActiveChatThreadId(LOADING_THREAD)
      await settle()
      await expect(send(controller, { gateway: true })).rejects.toThrow(LOADING_NOTICE)
      await settle()
      expect(streams).toHaveLength(0)
    } finally {
      await finishLoading()
      dispose()
    }
  })

  it('refuses a subscription route when the lookup of the chat failed, so its Space status is unknown', async () => {
    const { controller, dispose } = mount()
    try {
      setActiveChatThreadId(FAILED_THREAD)
      await settle()
      expect(controller.state.threadId).toBe(FAILED_THREAD)
      await expect(send(controller, { subscription: true })).rejects.toThrow(FAILED_NOTICE)
      await settle()
      expect(streams).toHaveLength(0)
    } finally {
      dispose()
    }
  })

  it('sends the same route once the ref has loaded and the chat is not a Space', async () => {
    const { controller, dispose } = mount()
    try {
      setActiveChatThreadId(LOADING_THREAD)
      await settle()
      await finishLoading()
      expect(controller.state.threadId).toBe(LOADING_THREAD)
      await send(controller, { subscription: true })
      await settle()
      expect(streams).toHaveLength(1)
      expect(streams[0]!.request).toMatchObject({ content: 'Legg til kolonne', subscriptionConnectionId: SUBSCRIPTION_CONNECTION })
      expect(controller.feedbackNotice()).toBeNull()
    } finally {
      dispose()
    }
  })
})

describe('a zero-retention launch refused while the BYO gateway switch is on and its chat ref is unsettled (P2-spa-03 rel1, ZDR floor)', () => {
  beforeEach(() => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    // A signed-in member, so the chat's draft has a storage key: the refused words must not be written under it.
    setSessionUser({ id: 'user_1', email: 'member@example.com', name: 'Member', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role: 'member' })
    flush()
  })

  afterEach(() => {
    clearSession()
  })

  it('keeps the zero-retention choice with the words: nothing is sent, ZDR stays on, and no draft of the words is stored', async () => {
    setActiveChatThreadId(FAILED_THREAD)
    const { controller, dispose } = await launchOnMount({ text: 'Legg til kolonne', model: MODEL, zdr: true })
    try {
      expect(controller.state.threadId).toBe(FAILED_THREAD)
      expect(streams).toHaveLength(0)
      expect(controller.input()).toBe('Legg til kolonne')
      expect(controller.feedbackNotice()).toMatch(FAILED_NOTICE)
      expect(controller.temporaryChat()).toBe(true)
      // A draft is written once typing has paused: the composer's effects run first, then the check waits past that pause.
      flush()
      await new Promise((resolve) => setTimeout(resolve, CHAT_DRAFT_DEBOUNCE_MS + 50))
      flush()
      expect(sessionKeysHolding('Legg til kolonne')).toEqual([])
    } finally {
      dispose()
    }
  })

  it('sends the words from the box with zdr once they are sent from a chat that can take them', async () => {
    setActiveChatThreadId(FAILED_THREAD)
    const { controller, dispose } = await launchOnMount({ text: 'Legg til kolonne', model: MODEL, zdr: true })
    try {
      expect(streams).toHaveLength(0)
      setActiveChatThreadId(PLAIN_THREAD)
      await settle()
      expect(controller.state.threadId).toBe(PLAIN_THREAD)
      expect(controller.temporaryChat()).toBe(true)
      await controller.handleComposerSubmit({
        actions: [],
        attachments: [],
        text: 'Legg til kolonne',
        tools: [],
        model: MODEL,
        zdr: controller.temporaryChat(),
      })
      await settle()
      expect(streams).toHaveLength(1)
      expect(streams[0]!.request).toMatchObject({ content: 'Legg til kolonne', zdr: true })
    } finally {
      dispose()
    }
  })

  it('says that the files of a refused launch did not come back with its words', async () => {
    setActiveChatThreadId(FAILED_THREAD)
    const { controller, dispose } = await launchOnMount({
      text: 'Legg til kolonne',
      model: MODEL,
      zdr: true,
      attachments: [{ id: 'att-1', name: 'notes.txt', size: 5, type: 'text/plain', url: 'data:text/plain;base64,aGVsbG8=', extractedText: 'hello' }],
    })
    try {
      expect(streams).toHaveLength(0)
      expect(controller.input()).toBe('Legg til kolonne')
      expect(controller.feedbackNotice()).toMatch(FAILED_NOTICE)
      expect(controller.feedbackNotice()).toMatch(FILES_NOTICE)
    } finally {
      dispose()
    }
  })
})

describe('the same send while the chat it goes into is still loading, with the BYO gateway switch off (P2-spa-03 rel1)', () => {
  it('goes out on the route it names, as before the check', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    const { controller, dispose } = mount()
    try {
      setActiveChatThreadId(LOADING_THREAD)
      await settle()
      await send(controller, { subscription: true })
      await settle()
      expect(streams).toHaveLength(1)
      expect(streams[0]!.request).toMatchObject({ content: 'Legg til kolonne', subscriptionConnectionId: SUBSCRIPTION_CONNECTION })
      expect(controller.feedbackNotice()).toBeNull()
    } finally {
      await finishLoading()
      dispose()
    }
  })
})
