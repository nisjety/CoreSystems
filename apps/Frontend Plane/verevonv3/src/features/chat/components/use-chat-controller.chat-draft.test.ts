// @vitest-environment jsdom

/**
 * The composer's draft through the controller (chat-thread-draft). A send clears the composer, but its
 * text stays stored until the gateway accepts the turn: a send that is not accepted gives the text back,
 * an accepted one discards it, and text typed while it runs goes with the conversation when the server
 * names the thread.
 *
 * Mirrors the `streamChat` stub and the `withController`/`tick` helpers of
 * use-chat-controller.follow-up.test.ts. The draft's storage is keyed by the workspace and the user, so
 * the session is given an identity here.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers, QueuedInputResult } from '@/shared/api/chat-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { clearAssistantChatDraft, queueAssistantChatDraft } from '@/features/chat/lib/assistant-chat-draft'
import { resolveAssistantChatDraft } from '@/shared/actions/assistant-chat-draft'
import { chatDraftStorageKey, writeChatDraft } from '@/shared/session/home-draft-storage'

const identity = vi.hoisted(() => ({ user: { id: 'user-a' } as { id: string } | null, activeOrg: { id: 'org-a' } as { id: string } | null }))
vi.mock('@/shared/session/session-store', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/session/session-store')>()),
  getSession: () => identity,
}))

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

/** The text of every chat thread draft stored in this tab. */
function draftTexts(): string[] {
  const texts: string[] = []
  for (let index = 0; index < sessionStorage.length; index += 1) {
    const key = sessionStorage.key(index)
    if (!key?.startsWith('verevon.chat-draft.v1:')) continue
    const value: unknown = JSON.parse(sessionStorage.getItem(key)!)
    if (value && typeof value === 'object' && !Array.isArray(value) && 'text' in value) texts.push(String((value as { text: unknown }).text))
  }
  return texts
}

afterEach(() => {
  clearAssistantChatDraft()
  streams.length = 0
  mockStreamChat.mockClear()
  mockQueueInvocationInput.mockReset()
  __resetRetentionForTests()
  sessionStorage.clear()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('the assistant draft handoff', () => {
  it('prefills Chat after route mount without sending', async () => {
    const action = resolveAssistantChatDraft('kan du åpne chat og skrive at jeg er best')!
    expect(queueAssistantChatDraft(action)).toBe(true)
    await withController(async (controller) => {
      await tick(8)
      expect(controller.input()).toBe('jeg er best')
      expect(mockStreamChat).not.toHaveBeenCalled()
    })
  })

  it('appends to an existing draft when Chat is already mounted', async () => {
    await withController(async (controller) => {
      await tick(8)
      controller.setInput('Behold teksten min')
      expect(queueAssistantChatDraft(resolveAssistantChatDraft('Open Chat and type: A second line.')!)).toBe(true)
      await tick()
      expect(controller.input()).toBe('Behold teksten min\n\nA second line.')
      expect(mockStreamChat).not.toHaveBeenCalled()
    })
  })

  it('opens a new Chat draft without losing the previous thread draft', async () => {
    await withController(async (controller) => {
      await tick(8)
      await send(controller, 'Earlier conversation')
      await tick()
      const stream = streams[0]!
      stream.handlers.onConnected?.({ requestId: 'req-1', threadId: 'thread-server-1' })
      stream.handlers.onDone?.({ requestId: 'req-1', modelUsed: 'verevon-balance', outputTokens: 4, stopReason: 'stop' })
      stream.resolve()
      await tick()

      controller.setInput('Old thread draft')
      flush()
      writeChatDraft(chatDraftStorageKey('user-a', 'org-a', 'new'), 'Saved new draft')
      expect(queueAssistantChatDraft(resolveAssistantChatDraft('Open a new Chat and type: New text')!)).toBe(true)
      await tick()

      expect(controller.state.threadId).toBeNull()
      expect(controller.input()).toBe('Saved new draft\n\nNew text')
      expect(draftTexts()).toContain('Old thread draft')
      expect(mockStreamChat).toHaveBeenCalledOnce()
    })
  })

  it('discards a pending draft if the workspace changed before Chat mounted', async () => {
    expect(queueAssistantChatDraft(resolveAssistantChatDraft('Open Chat and type Private text')!)).toBe(true)
    const org = identity.activeOrg
    identity.activeOrg = { id: 'org-b' }
    try {
      await withController(async (controller) => {
        await tick(8)
        expect(controller.input()).toBe('')
        expect(mockStreamChat).not.toHaveBeenCalled()
      })
    } finally {
      identity.activeOrg = org
    }
  })
})

describe('the composer draft across a send', () => {
  it('gives the text back to the composer when the gateway does not accept the send', async () => {
    await withController(async (controller) => {
      await tick()
      controller.setInput('Viktig spørsmål')
      flush()
      await send(controller, 'Viktig spørsmål')
      await tick()
      expect(controller.input()).toBe('')
      expect(draftTexts()).toEqual(['Viktig spørsmål'])

      streams[0]!.handlers.onError?.({ code: 'upstream_failed', message: 'Upstream failed.', retryable: true })
      streams[0]!.resolve()
      await tick()
      expect(controller.input()).toBe('Viktig spørsmål')
      expect(draftTexts()).toEqual(['Viktig spørsmål'])
    })
  })

  it('discards the draft once the gateway has accepted the turn', async () => {
    await withController(async (controller) => {
      await tick()
      controller.setInput('Sendt spørsmål')
      flush()
      await send(controller, 'Sendt spørsmål')
      await tick()

      streams[0]!.handlers.onConnected?.({ requestId: 'req-1', threadId: 'thread-server-1' })
      flush()
      streams[0]!.handlers.onDone?.({ requestId: 'req-1', modelUsed: 'verevon-balance', outputTokens: 4, stopReason: 'stop' })
      streams[0]!.resolve()
      await tick()
      expect(controller.input()).toBe('')
      expect(draftTexts()).toEqual([])
    })
  })

  it('carries text typed during an accepted first send to the thread the server names', async () => {
    await withController(async (controller) => {
      await tick()
      controller.setInput('Første melding')
      flush()
      await send(controller, 'Første melding')
      await tick()

      controller.setInput('Neste, mens svaret kommer')
      flush()
      await tick()

      streams[0]!.handlers.onConnected?.({ requestId: 'req-1', threadId: 'thread-server-2' })
      flush()
      await tick()
      expect(controller.input()).toBe('Neste, mens svaret kommer')
      expect(draftTexts()).toEqual(['Neste, mens svaret kommer'])
    })
  })
})
