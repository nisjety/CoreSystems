// @vitest-environment jsdom

/**
 * A turn that is sent again (a launch from the composer, a rerun, an edit) whose file cannot be read whole is not sent.
 * The reason is the chat's error, as a failed send shows it. Before, the refusal was an unhandled rejection behind a
 * `void` call, so the person saw nothing happen. The default locale is Norwegian, so the copy is read from `no`.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'
import { ApiError } from '@/shared/api/api-error'
import { getRunProofBundle } from '@/shared/api/run-console-client'
import { consumePendingChatLaunchAsync } from '@/features/chat/lib/pending-chat-launch'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { LocalizedError, translateApiError } from '@/shared/i18n/errors'
import { toStreamAttachments } from './chat-normalizers'

type CapturedStream = {
  handlers: ChatStreamHandlers
  resolve: () => void
  signal: AbortSignal | undefined
}

type ProofBundle = Awaited<ReturnType<typeof getRunProofBundle>>

const { streams, mockStreamChat } = vi.hoisted(() => {
  const streams: CapturedStream[] = []
  const mockStreamChat = vi.fn((_request: unknown, handlers: ChatStreamHandlers, signal?: AbortSignal) => {
    return new Promise<void>((resolve) => {
      streams.push({ handlers, resolve, signal })
    })
  })
  return { streams, mockStreamChat }
})

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    streamChat: mockStreamChat,
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
    getChatThreadTranscript: vi.fn().mockResolvedValue(null),
    getThreadMessages: vi.fn().mockResolvedValue([]),
    cancelInvocation: vi.fn().mockResolvedValue(undefined),
  }
})

vi.mock('@/shared/api/run-console-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/run-console-client')>()
  return { ...actual, getRunProofBundle: vi.fn().mockResolvedValue(null) }
})

vi.mock('@/features/chat/lib/pending-chat-launch', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/features/chat/lib/pending-chat-launch')>()
  return { ...actual, consumePendingChatLaunchAsync: vi.fn().mockResolvedValue(null) }
})

// The read is the real one unless a test makes it refuse.
vi.mock('./chat-normalizers', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./chat-normalizers')>()
  return { ...actual, toStreamAttachments: vi.fn(actual.toStreamAttachments) }
})

import { useChatController } from './use-chat-controller'

const GATEWAY_MODEL = 'gateway-a'
const FALLBACK = { no: 'Kunne ikke sende. Prøv igjen.', en: 'Could not send. Please retry.' }
const image = { id: 'img-1', name: 'chart.png', size: 2, type: 'image/png', url: 'data:image/png;base64,aGk=' }
const effectfulBundle = { effectClass: 'effectful' } as unknown as ProofBundle
const noBundle = null as unknown as ProofBundle

/** The refusal the read gives for an image it cannot read whole, in the words the read uses. */
function unreadableImage() {
  return new LocalizedError({
    no: 'Kunne ikke lese bildet «chart.png». Prøv igjen eller fjern bildet.',
    en: 'Could not read the image “chart.png”. Try again or remove the image.',
  })
}

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

/** Connects the turn (under `runId` when given), writes one sentence and ends the answer. */
function finish(stream: CapturedStream, runId?: string) {
  stream.handlers.onConnected?.({ requestId: 'req-1', threadId: 't-1', ...(runId ? { runId } : {}) })
  stream.handlers.onMessage?.({ content: 'Svar', requestId: 'req-1' })
  stream.handlers.onDone?.({ requestId: 'req-1', modelUsed: GATEWAY_MODEL, outputTokens: 4, stopReason: 'stop' })
  stream.resolve()
}

beforeEach(() => {
  vi.mocked(getRunProofBundle).mockResolvedValue(noBundle)
  vi.mocked(consumePendingChatLaunchAsync).mockResolvedValue(null)
})

afterEach(() => {
  streams.length = 0
  mockStreamChat.mockClear()
  vi.mocked(toStreamAttachments).mockClear()
  __resetRetentionForTests()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('a launch from the composer whose file cannot be read', () => {
  it('is not sent, and the reason is the chat error', async () => {
    const refusal = unreadableImage()
    vi.mocked(consumePendingChatLaunchAsync).mockResolvedValueOnce({ text: 'Hva ser du?', attachments: [image] })
    vi.mocked(toStreamAttachments).mockRejectedValueOnce(refusal)
    await withController(async (controller) => {
      await tick(10)
      expect(streams).toHaveLength(0)
      expect(controller.state.error).toBe(refusal.copy.no)
      expect(controller.state.status).toBe('error')
    })
  })

  it('shows a typed refusal as the words for its code, not the generic send failure', async () => {
    const refusal = new ApiError('Zero retention', 403, 'zdr_feature_not_supported')
    vi.mocked(consumePendingChatLaunchAsync).mockResolvedValueOnce({ text: 'Hva ser du?', attachments: [image] })
    vi.mocked(toStreamAttachments).mockRejectedValueOnce(refusal)
    await withController(async (controller) => {
      await tick(10)
      expect(streams).toHaveLength(0)
      expect(controller.state.error).toBe(translateApiError(refusal, (noText) => noText, FALLBACK))
      expect(controller.state.error).not.toBe(FALLBACK.no)
    })
  })
})

describe('a rerun whose file cannot be read', () => {
  it('is not sent, and the reason is the chat error', async () => {
    const refusal = unreadableImage()
    vi.mocked(getRunProofBundle).mockResolvedValue(effectfulBundle)
    await withController(async (controller) => {
      await tick()
      await controller.handleComposerSubmit({
        actions: [],
        attachments: [image],
        text: 'Lag rapporten',
        tools: ['research'],
        model: GATEWAY_MODEL,
      })
      await tick()
      finish(streams[0]!, 'run-1')
      await tick(8)
      const reply = controller.state.turns.at(-1)!
      expect(reply.effectClass).toBe('effectful')
      vi.mocked(toStreamAttachments).mockRejectedValueOnce(refusal)
      await expect(controller.rerunAsNewTurn(reply.id)).resolves.toBeUndefined()
      expect(streams).toHaveLength(1)
      expect(controller.state.error).toBe(refusal.copy.no)
      expect(controller.state.status).toBe('error')
    })
  })
})

describe('an edit whose file cannot be read', () => {
  it('keeps the exchange as it was and sends nothing', async () => {
    const refusal = unreadableImage()
    await withController(async (controller) => {
      await tick()
      await controller.handleComposerSubmit({
        actions: [],
        attachments: [image],
        text: 'Hva ser du?',
        tools: [],
        model: GATEWAY_MODEL,
      })
      await tick()
      finish(streams[0]!)
      await tick(6)
      const question = controller.state.turns[0]!
      vi.mocked(toStreamAttachments).mockRejectedValueOnce(refusal)
      await expect(controller.editAndResubmit(question.id, 'Hva ser du nå?')).resolves.toBeUndefined()
      expect(streams).toHaveLength(1)
      expect(controller.state.turns).toHaveLength(2)
      expect(controller.state.error).toBe(refusal.copy.no)
      expect(controller.state.status).toBe('error')
    })
  })

  it('does not stop an answer that is still running, and while it runs the reason is a notice', async () => {
    const refusal = unreadableImage()
    await withController(async (controller) => {
      await tick()
      await controller.handleComposerSubmit({
        actions: [],
        attachments: [],
        text: 'Skriv en lang rapport',
        tools: [],
        model: GATEWAY_MODEL,
      })
      await tick()
      const running = streams[0]!
      expect(controller.state.status).toBe('streaming')
      vi.mocked(toStreamAttachments).mockRejectedValueOnce(refusal)
      await expect(controller.editAndResubmit(controller.state.turns[0]!.id, 'Skriv en kort rapport')).resolves.toBeUndefined()
      expect(running.signal?.aborted).toBe(false)
      expect(controller.state.status).toBe('streaming')
      expect(controller.feedbackNotice()).toBe(refusal.copy.no)
      expect(streams).toHaveLength(1)
    })
  })
})
