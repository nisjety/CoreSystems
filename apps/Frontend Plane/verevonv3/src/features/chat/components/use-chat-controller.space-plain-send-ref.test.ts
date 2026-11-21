// @vitest-environment jsdom

/**
 * A plain send the composer makes, with no route, while the open chat's Space ref has not settled (P2-spa-03 rel1). Such a send goes out
 * without the chat's spaceRef, so while the ref loads, or after its lookup failed, the send cannot tell whether the chat is a Space. With
 * the BYO gateway switch on it is refused where it is made: it throws, so the composer keeps the draft and says why, and nothing is sent.
 * Once the ref has loaded it goes out as before, to the Space when the chat is one. With the switch off nothing waits, as before.
 *
 * Driven through the real controller, with the request `streamChat` was called with captured, as the other Space tests are.
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
const FAILED_THREAD = 'thread-lookup-failed'
const LOADING_THREAD = 'thread-lookup-loading'
const SPACE_REF = 'space-room'
const STAMP = '2026-11-18T09:00:00.000Z'
const MODEL = 'gateway-a'
const TEXT = 'Legg til kolonne'
const FAILED_NOTICE = /Kunne ikke sjekke|Could not check/
const LOADING_NOTICE = /lastes fortsatt|still loading/

// The lookup of the loading thread stays open until a test releases it, so that thread's Space ref is unknown while a send is judged.
let releaseLoading: ((session: unknown) => void) | undefined

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

/** Lets the open loading thread's lookup answer, so its load can end, and waits for the work that follows. */
async function finishLoading() {
  releaseLoading?.(null)
  await settle()
}

/** Sends a message through the page's composer submit with no route, the way the composer sends a plain message. */
function sendPlain(controller: Controller, text = TEXT) {
  return controller.handleComposerSubmit({
    actions: [],
    attachments: [],
    text,
    tools: [],
    model: MODEL,
  })
}

beforeEach(() => {
  __resetRetentionForTests()
  window.localStorage.clear()
  window.sessionStorage.clear()
  mockGetChatThread.mockImplementation(async (threadId: string) => {
    if (threadId === SPACE_THREAD) {
      return { threadId, title: 'Room', preview: 'Room', updatedAt: STAMP, pinned: false, spaceRef: SPACE_REF, origin: 'space' }
    }
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

describe('a plain send the composer makes while the open chat\'s Space ref has not settled, with the BYO gateway switch on (P2-spa-03 rel1)', () => {
  beforeEach(() => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
  })

  it('is refused while the Space ref of the open chat is still loading: it throws so the composer keeps the draft, and nothing is sent', async () => {
    const { controller, dispose } = mount()
    try {
      setActiveChatThreadId(LOADING_THREAD)
      await settle()
      expect(controller.state.threadId).toBe(LOADING_THREAD)
      await expect(sendPlain(controller)).rejects.toThrow(LOADING_NOTICE)
      await settle()
      expect(streams).toHaveLength(0)
      expect(controller.state.turns).toHaveLength(0)
    } finally {
      await finishLoading()
      dispose()
    }
  })

  it('is refused after the lookup of the open chat failed, so whether the chat is a Space is unknown: nothing is sent', async () => {
    const { controller, dispose } = mount()
    try {
      setActiveChatThreadId(FAILED_THREAD)
      await settle()
      expect(controller.state.threadId).toBe(FAILED_THREAD)
      await expect(sendPlain(controller)).rejects.toThrow(FAILED_NOTICE)
      await settle()
      expect(streams).toHaveLength(0)
      expect(controller.state.turns).toHaveLength(0)
    } finally {
      dispose()
    }
  })

  it('is sent once the ref has loaded and the chat is not a Space, without a spaceRef', async () => {
    const { controller, dispose } = mount()
    try {
      setActiveChatThreadId(LOADING_THREAD)
      await settle()
      await finishLoading()
      expect(controller.state.threadId).toBe(LOADING_THREAD)
      await sendPlain(controller)
      await settle()
      expect(streams).toHaveLength(1)
      expect(streams[0]!.request).toMatchObject({ content: TEXT })
      expect(streams[0]!.request.spaceRef).toBeUndefined()
      expect(controller.feedbackNotice()).toBeNull()
    } finally {
      dispose()
    }
  })

  it('is sent to the Space the chat belongs to once its ref has loaded, as before', async () => {
    const { controller, dispose } = mount()
    try {
      setActiveChatThreadId(SPACE_THREAD)
      await settle()
      expect(controller.state.threadId).toBe(SPACE_THREAD)
      await sendPlain(controller)
      await settle()
      expect(streams).toHaveLength(1)
      expect(streams[0]!.request).toMatchObject({ content: TEXT, spaceRef: SPACE_REF })
      expect(controller.feedbackNotice()).toBeNull()
    } finally {
      dispose()
    }
  })
})

describe('the same plain send while the open chat\'s Space ref has not settled, with the BYO gateway switch off', () => {
  it('is sent as before while the ref is still loading', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    const { controller, dispose } = mount()
    try {
      setActiveChatThreadId(LOADING_THREAD)
      await settle()
      await sendPlain(controller)
      await settle()
      expect(streams).toHaveLength(1)
      expect(streams[0]!.request).toMatchObject({ content: TEXT })
      expect(controller.feedbackNotice()).toBeNull()
    } finally {
      await finishLoading()
      dispose()
    }
  })

  it('is sent as before after the lookup of the open chat failed', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    const { controller, dispose } = mount()
    try {
      setActiveChatThreadId(FAILED_THREAD)
      await settle()
      await sendPlain(controller)
      await settle()
      expect(streams).toHaveLength(1)
      expect(streams[0]!.request).toMatchObject({ content: TEXT })
      expect(controller.feedbackNotice()).toBeNull()
    } finally {
      dispose()
    }
  })
})
