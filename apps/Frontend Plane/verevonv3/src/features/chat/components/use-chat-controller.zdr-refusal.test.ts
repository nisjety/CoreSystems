// @vitest-environment jsdom

/**
 * A zero-retention refusal in a turn.
 *
 * The refusal is the turn's answer: the reader sees the catalogue's words, the
 * feature named by its key, and the action the server's code gives. A refusal is
 * not a lost connection, so the controller never answers it by reattaching to
 * the stream, which would be refused the same way.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatErrorEvent, ChatInvokeRequest, ChatStreamHandlers } from '@/shared/api/chat-client'
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
  const mockResumeStream = vi.fn()
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

/** Send one turn on the subscription connection, and have the stream end with `error`. */
async function sendAndFail(controller: ReturnType<typeof useChatController>, error: ChatErrorEvent) {
  await tick()
  await controller.handleComposerSubmit({
    actions: [],
    attachments: [],
    text: 'Oppsummer uka',
    tools: [],
    model: TERRA,
    provider: SUBSCRIPTION,
    subscriptionConnectionId: 'conn_1',
  })
  await tick()
  const first = streams[0]!
  first.handlers.onConnected?.({ ok: true, requestId: 'req-1' })
  first.handlers.onError?.(error)
  first.resolve()
  await tick(8)
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

describe('a zero-retention refusal in a turn', () => {
  it('shows the connection refusal as the turn, with its action, and does not reattach', async () => {
    await withController(async (controller) => {
      await sendAndFail(controller, {
        code: 'zdr_connection_not_acknowledged',
        message: 'Server operator text in English',
      })

      const turn = controller.state.turns.at(-1)
      expect(turn).toMatchObject({ role: 'assistant', status: 'error', failureAction: 'open_integrations' })
      expect(turn?.content).toMatch(/Zero [Dd]ata [Rr]etention/)
      expect(turn?.content).not.toContain('Server operator')
      expect(mockResumeStream).not.toHaveBeenCalled()
      expect(streams).toHaveLength(1)
    })
  })

  it('names the feature a feature refusal carries, and offers no retry for it', async () => {
    await withController(async (controller) => {
      await sendAndFail(controller, {
        code: 'zdr_feature_not_supported',
        message: 'This is not supported with zero data retention yet: web search and browsing.',
        feature: 'web_search',
      })

      const turn = controller.state.turns.at(-1)
      expect(turn).toMatchObject({ status: 'error', failureAction: 'none' })
      expect(turn?.content).toMatch(/web search and browsing|websøk og nettlesing/)
      expect(mockResumeStream).not.toHaveBeenCalled()
    })
  })

  it('reads a feature refusal whose feature the catalogue does not know as the unnamed fallback', async () => {
    await withController(async (controller) => {
      await sendAndFail(controller, {
        code: 'zdr_feature_not_supported',
        message: 'This is not supported with zero data retention yet: hologram export.',
        feature: 'hologram_export',
      })

      const turn = controller.state.turns.at(-1)
      expect(turn).toMatchObject({ status: 'error', failureAction: 'none' })
      expect(turn?.content).toMatch(/this feature|denne funksjonen/)
      expect(turn?.content).not.toMatch(/hologram/i)
      expect(turn?.content).not.toContain('This is not supported with zero data retention yet')
    })
  })

  it('reads a feature refusal frame without a feature as the unnamed fallback', async () => {
    await withController(async (controller) => {
      await sendAndFail(controller, {
        code: 'zdr_feature_not_supported',
        message: 'This is not supported with zero data retention yet: web search and browsing.',
      })

      const turn = controller.state.turns.at(-1)
      expect(turn).toMatchObject({ status: 'error', failureAction: 'none' })
      expect(turn?.content).toMatch(/this feature|denne funksjonen/)
      expect(turn?.content).not.toMatch(/web search and browsing|websøk og nettlesing/)
    })
  })
})
