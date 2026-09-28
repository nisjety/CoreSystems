// @vitest-environment jsdom

/**
 * "Prøv igjen" after reconnecting ChatGPT.
 *
 * A turn stores the subscription connection it was first sent on. When
 * ChatGPT refuses a turn because the sign-in must be renewed, the reader
 * reconnects, and that creates a NEW connection: the old one stays in
 * `needs_refresh`, and the broker refuses it before any turn runs. So a
 * retry that resent the stored id was refused with the same "reconnect"
 * notice, however many times the reader reconnected.
 *
 * Every way of sending a subscription turn again (regenerate, which is what
 * "Prøv igjen" calls, continue, rerun, edit) now looks the connection up
 * again. It keeps the stored id while that connection is active, and it never
 * changes the provider or the model.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers, ChatInvokeRequest } from '@/shared/api/chat-client'
import type { ChatGptSubscriptionConnection } from '@/shared/api/chatgpt-subscription-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

type CapturedStream = {
  request: ChatInvokeRequest
  handlers: ChatStreamHandlers
  resolve: () => void
}

const { streams, mockStreamChat, mockListSubscriptions } = vi.hoisted(() => {
  const streams: CapturedStream[] = []
  const mockStreamChat = vi.fn((request: ChatInvokeRequest, handlers: ChatStreamHandlers) => {
    return new Promise<void>((resolve) => {
      streams.push({ request, handlers, resolve })
    })
  })
  const mockListSubscriptions = vi.fn<(orgId: string) => Promise<ChatGptSubscriptionConnection[]>>()
  return { streams, mockStreamChat, mockListSubscriptions }
})

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    streamChat: mockStreamChat,
    resumeStream: vi.fn(async (_id: string, handlers: ChatStreamHandlers) => handlers.onError?.({ code: 'connection_error', message: 'Buffer expired' })),
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
    getChatThreadTranscript: vi.fn().mockResolvedValue(null),
    getThreadMessages: vi.fn().mockResolvedValue([]),
    cancelInvocation: vi.fn().mockResolvedValue(undefined),
  }
})

vi.mock('@/shared/api/chatgpt-subscription-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chatgpt-subscription-client')>()
  return { ...actual, listChatGptSubscriptions: mockListSubscriptions }
})

import { useChatController } from './use-chat-controller'

const SUBSCRIPTION = 'openai-codex-subscription'
const TERRA = 'gpt-5.6-terra'

const connection = (id: string, status: string): ChatGptSubscriptionConnection => ({ id, providerKey: SUBSCRIPTION, status })

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

/** Send one Terra turn on `conn_old` and have ChatGPT refuse it for a sign-in that must be renewed. */
async function refusedForReauth(controller: ReturnType<typeof useChatController>) {
  await tick()
  await controller.handleComposerSubmit({
    actions: [],
    attachments: [],
    text: 'Oppsummer uka',
    tools: [],
    model: TERRA,
    provider: SUBSCRIPTION,
    subscriptionConnectionId: 'conn_old',
  })
  await tick()
  const first = streams[0]!
  expect(first.request).toMatchObject({ model: TERRA, provider: SUBSCRIPTION, subscriptionConnectionId: 'conn_old' })
  first.handlers.onConnected?.({ ok: true, requestId: 'req-1' })
  first.handlers.onError?.({ code: 'subscription_reauth_required', message: 'Reconnect your ChatGPT subscription.' })
  first.resolve()
  await tick(8)
  expect(controller.state.turns.at(-1)).toMatchObject({ status: 'error', failureAction: 'reconnect' })
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
  mockListSubscriptions.mockReset()
  __resetRetentionForTests()
  clearSession()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('retrying a ChatGPT-subscription turn after reconnecting', () => {
  it('sends the retry on the new connection, not the one that needed renewing', async () => {
    await withController(async (controller) => {
      await refusedForReauth(controller)

      // The reader reconnected ChatGPT: a new connection, the old one left needing renewal.
      mockListSubscriptions.mockResolvedValue([connection('conn_old', 'needs_refresh'), connection('conn_new', 'active')])
      await controller.regenerateLatest()
      await tick()

      expect(mockListSubscriptions).toHaveBeenCalledWith('org_1')
      expect(streams).toHaveLength(2)
      expect(streams[1]!.request).toMatchObject({ model: TERRA, provider: SUBSCRIPTION, subscriptionConnectionId: 'conn_new' })
      // The refused answer was replaced, not kept beside the retry.
      expect(controller.state.turns).toHaveLength(2)
      expect(controller.state.turns.at(-1)).toMatchObject({ role: 'assistant', status: 'waiting', provider: SUBSCRIPTION, subscriptionConnectionId: 'conn_new' })
      streams[1]!.handlers.onDone?.({})
      streams[1]!.resolve()
      await tick()
    })
  })

  it('keeps the stored connection while it is still active', async () => {
    await withController(async (controller) => {
      await refusedForReauth(controller)

      mockListSubscriptions.mockResolvedValue([connection('conn_other', 'active'), connection('conn_old', 'active')])
      await controller.regenerateLatest()
      await tick()

      expect(streams[1]!.request).toMatchObject({ provider: SUBSCRIPTION, subscriptionConnectionId: 'conn_old' })
      streams[1]!.resolve()
      await tick()
    })
  })

  it('never swaps route when no connection is active or the lookup fails', async () => {
    await withController(async (controller) => {
      await refusedForReauth(controller)

      // Nothing active: the stored id goes out again, so the broker says why.
      mockListSubscriptions.mockResolvedValueOnce([connection('conn_old', 'needs_refresh')])
      await controller.regenerateLatest()
      await tick()
      expect(streams[1]!.request).toMatchObject({ model: TERRA, provider: SUBSCRIPTION, subscriptionConnectionId: 'conn_old' })
      streams[1]!.handlers.onError?.({ code: 'subscription_reauth_required', message: 'x' })
      streams[1]!.resolve()
      await tick(8)

      // The lookup itself failed: the same, with no other model or provider.
      mockListSubscriptions.mockRejectedValueOnce(new Error('integrations unavailable'))
      await controller.regenerateLatest()
      await tick()
      expect(streams[2]!.request).toMatchObject({ model: TERRA, provider: SUBSCRIPTION, subscriptionConnectionId: 'conn_old' })
      streams[2]!.resolve()
      await tick()
    })
  })

  it('drops a retry whose exchange changed while the connection was looked up', async () => {
    await withController(async (controller) => {
      await refusedForReauth(controller)

      const answers: Array<(value: ChatGptSubscriptionConnection[]) => void> = []
      mockListSubscriptions.mockImplementation(() => new Promise((resolve) => { answers.push(resolve) }))
      // Two presses before the lookup answers: only one retry may go out.
      const firstPress = controller.regenerateLatest()
      const secondPress = controller.regenerateLatest()
      expect(answers).toHaveLength(2)
      for (const answer of answers) answer([connection('conn_new', 'active')])
      await firstPress
      await secondPress
      await tick()

      expect(streams).toHaveLength(2)
      expect(streams[1]!.request).toMatchObject({ subscriptionConnectionId: 'conn_new' })
      expect(controller.state.turns).toHaveLength(2)
      streams[1]!.resolve()
      await tick()
    })
  })

  it('looks nothing up for a turn on another route, and stays synchronous', async () => {
    await withController(async (controller) => {
      await tick()
      await controller.handleComposerSubmit({ actions: [], attachments: [], text: 'Hei', tools: [], model: 'verevon-balance' })
      await tick()
      streams[0]!.handlers.onError?.({ code: 'inference_stream_error', message: 'x' })
      streams[0]!.resolve()
      await tick(8)

      controller.regenerateLatest()
      // No lookup, so the retry has already truncated and been sent in this tick.
      expect(mockStreamChat).toHaveBeenCalledTimes(2)
      expect(mockListSubscriptions).not.toHaveBeenCalled()
      expect(streams[1]!.request.provider).toBeUndefined()
      streams[1]!.resolve()
      await tick()
    })
  })

  it('edits and resubmits a subscription turn on the new connection', async () => {
    await withController(async (controller) => {
      await refusedForReauth(controller)

      mockListSubscriptions.mockResolvedValue([connection('conn_new', 'active')])
      const userTurn = controller.state.turns[0]!
      // Settles only with its stream, so it is not awaited here.
      const edited = controller.editAndResubmit(userTurn.id, 'Oppsummer uka kort')
      await tick(8)

      expect(streams[1]!.request).toMatchObject({ model: TERRA, provider: SUBSCRIPTION, subscriptionConnectionId: 'conn_new' })
      streams[1]!.resolve()
      await edited
      await tick()
    })
  })
})
