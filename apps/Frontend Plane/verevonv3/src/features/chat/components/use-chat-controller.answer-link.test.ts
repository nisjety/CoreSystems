// @vitest-environment jsdom

/**
 * F-f07-01b (`VITE_QUESTION_ANSWER_MESSAGE_LINK`): the answer to a question names the message that carries it, and the
 * summary is sent as that message. A retry of the answer and the composer's give-back of a summary that did not send
 * reuse that id; once the summary is accepted, or the question is skipped, the link is gone. With the switch off, the
 * answer and the summary are sent exactly as they were.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatInvokeRequest, ChatStreamHandlers, ChatUserQuestionEvent } from '@/shared/api/chat-client'
import type { RunQuestionAnswerInput } from '@/shared/api/orchestration-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

type CapturedStream = {
  request: ChatInvokeRequest
  handlers: ChatStreamHandlers
  resolve: () => void
}

const { streams, mockStreamChat, mockAnswerQuestion } = vi.hoisted(() => {
  const streams: CapturedStream[] = []
  const mockStreamChat = vi.fn((request: ChatInvokeRequest, handlers: ChatStreamHandlers) => {
    return new Promise<void>((resolve) => {
      streams.push({ request, handlers, resolve })
    })
  })
  const mockAnswerQuestion = vi.fn(async (_questionId: string, _response: RunQuestionAnswerInput) => null)
  return { streams, mockStreamChat, mockAnswerQuestion }
})

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    streamChat: mockStreamChat,
    resumeStream: vi.fn(async () => undefined),
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: vi.fn().mockResolvedValue([]),
    getChatThread: vi.fn().mockResolvedValue(null),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
    getChatThreadTranscript: vi.fn().mockResolvedValue(null),
    getThreadMessages: vi.fn().mockResolvedValue([]),
    cancelInvocation: vi.fn().mockResolvedValue(undefined),
  }
})

vi.mock('@/shared/api/orchestration-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/orchestration-client')>()
  return {
    ...actual,
    answerQuestion: mockAnswerQuestion,
    listRunQuestions: vi.fn().mockResolvedValue([]),
  }
})

import { useChatController } from './use-chat-controller'

const question: ChatUserQuestionEvent = {
  questionId: 'q_1',
  delivery: 'next_turn',
  expiresAt: '2099-01-01T00:00:00Z',
  items: [
    {
      id: 'carrier',
      kind: 'single',
      prompt: 'Which carrier?',
      options: [
        { id: 'bring', label: 'Bring' },
        { id: 'postnord', label: 'PostNord' },
      ],
      allowOther: true,
    },
    {
      id: 'services',
      kind: 'multi',
      prompt: 'Which services?',
      options: [
        { id: 'cold', label: 'Cold' },
        { id: 'tracked', label: 'Tracked' },
      ],
      allowOther: false,
    },
  ],
}

const answer = {
  answers: [
    { itemId: 'carrier', optionIds: ['bring'], text: '' },
    { itemId: 'services', optionIds: ['cold'], text: '' },
  ],
}

const SUMMARY = 'Bring\nCold'

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

/** The first turn asks the question, and its run ends, so the answer is the person's next message. */
async function askQuestion(controller: ReturnType<typeof useChatController>) {
  await tick()
  await controller.handleComposerSubmit({ actions: [], attachments: [], text: 'Book the shipment', tools: [] })
  await tick()
  streams[0]!.handlers.onUserQuestion?.(question)
  flush()
  streams[0]!.resolve()
  await tick()
}

/** The message id an answer named on the wire, or undefined when it named none. */
function namedMessageId(call: number): string | undefined {
  const response = mockAnswerQuestion.mock.calls[call]![1] as { answeredMessageId?: string }
  return response.answeredMessageId
}

/** The summary an answer sent with the message it names (O-15), or undefined when it named none. */
function sentSummary(call: number): string | undefined {
  const response = mockAnswerQuestion.mock.calls[call]![1] as { answerSummary?: string }
  return response.answerSummary
}

function lastTurn(): ChatInvokeRequest {
  return streams[streams.length - 1]!.request
}

beforeEach(() => {
  setSessionUser({ id: 'user_1', email: 'member@example.com', name: 'Member', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role: 'member' })
  flush()
})

afterEach(() => {
  vi.unstubAllEnvs()
  streams.length = 0
  mockStreamChat.mockClear()
  mockAnswerQuestion.mockClear()
  __resetRetentionForTests()
  clearSession()
  sessionStorage.clear()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('the answer link with the switch on (F-f07-01b)', () => {
  beforeEach(() => {
    vi.stubEnv('VITE_QUESTION_ANSWER_MESSAGE_LINK', 'true')
  })

  it('the answer names one message, and the summary is sent as that message', async () => {
    await withController(async (controller) => {
      await askQuestion(controller)
      void controller.answerPendingQuestion(answer)
      await tick()

      const messageId = namedMessageId(0)
      expect(messageId).toMatch(/^msg-/)
      expect(sentSummary(0), 'the answer carries the text its message is stored as (O-15)').toBe(SUMMARY)
      expect(lastTurn().content).toBe(SUMMARY)
      expect(lastTurn().clientMessageId, 'the stored message is the one the answer names').toBe(messageId)
    })
  })

  it('a retry of the answer names the same message, and the summary is sent under that id', async () => {
    await withController(async (controller) => {
      await askQuestion(controller)
      mockAnswerQuestion.mockRejectedValueOnce(new Error('network down'))
      await controller.answerPendingQuestion(answer)
      expect(controller.pendingQuestion()).toEqual(question)

      void controller.answerPendingQuestion(answer)
      await tick()

      expect(mockAnswerQuestion).toHaveBeenCalledTimes(2)
      expect(namedMessageId(1)).toMatch(/^msg-/)
      expect(namedMessageId(1)).toBe(namedMessageId(0))
      expect(lastTurn().clientMessageId).toBe(namedMessageId(1))
    })
  })

  it('a summary that did not send goes back to the composer, and its resend carries the same id', async () => {
    await withController(async (controller) => {
      await askQuestion(controller)
      mockStreamChat.mockImplementationOnce(() => Promise.reject(new Error('offline')))
      await controller.answerPendingQuestion(answer)
      await tick()

      const messageId = namedMessageId(0)
      expect(controller.input()).toBe(SUMMARY)

      await controller.handleComposerSubmit({ actions: [], attachments: [], text: SUMMARY, tools: [] })
      await tick()

      expect(lastTurn().content).toBe(SUMMARY)
      expect(lastTurn().clientMessageId, 'the give-back is the message the answer named').toBe(messageId)
    })
  })

  it('once the summary is accepted, the same words sent again are a new message', async () => {
    await withController(async (controller) => {
      await askQuestion(controller)
      void controller.answerPendingQuestion(answer)
      await tick()
      const messageId = namedMessageId(0)
      expect(lastTurn().clientMessageId).toBe(messageId)

      streams[streams.length - 1]!.handlers.onConnected?.({ requestId: 'req-2', threadId: 'thread_a' })
      streams[streams.length - 1]!.handlers.onDone?.({
        requestId: 'req-2',
        modelUsed: 'verevon-balance',
        outputTokens: 4,
        stopReason: 'stop',
      })
      streams[streams.length - 1]!.resolve()
      await tick(10)

      await controller.handleComposerSubmit({ actions: [], attachments: [], text: SUMMARY, tools: [] })
      await tick()

      expect(lastTurn().content).toBe(SUMMARY)
      expect(lastTurn().clientMessageId).toMatch(/^msg-/)
      expect(lastTurn().clientMessageId, 'a new message, not a repeat of the accepted one').not.toBe(messageId)
    })
  })

  it('a skip names no message and drops the link an earlier answer minted', async () => {
    await withController(async (controller) => {
      await askQuestion(controller)
      mockAnswerQuestion.mockRejectedValueOnce(new Error('network down'))
      await controller.answerPendingQuestion(answer)
      expect(namedMessageId(0)).toMatch(/^msg-/)

      await controller.skipPendingQuestion()
      await tick()

      expect(mockAnswerQuestion).toHaveBeenLastCalledWith('q_1', { skip: true })
      expect(mockAnswerQuestion.mock.calls[1]).toHaveLength(2)
    })
  })
})

describe('the answer with the switch off (F-f07-01b)', () => {
  beforeEach(() => {
    vi.stubEnv('VITE_QUESTION_ANSWER_MESSAGE_LINK', 'false')
  })

  it('the answer is sent as it always was, and the summary carries a message id of its own', async () => {
    await withController(async (controller) => {
      await askQuestion(controller)
      void controller.answerPendingQuestion(answer)
      await tick()

      expect(mockAnswerQuestion).toHaveBeenCalledWith('q_1', { answers: answer.answers })
      expect(mockAnswerQuestion.mock.calls[0]).toHaveLength(2)
      expect(lastTurn().content).toBe(SUMMARY)
      expect(lastTurn().clientMessageId).toMatch(/^msg-/)
    })
  })
})
