// @vitest-environment jsdom

/**
 * The question a run put to its owner, as the chat controller holds it (WS14 S4): it waits until it is
 * answered or skipped. The answer is recorded first, and the turn that follows is the person's next message.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatInvokeRequest, ChatStreamHandlers, ChatUserQuestionEvent } from '@/shared/api/chat-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { setActiveChatThreadId } from '@/features/chat/lib/chat-thread-history'
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
  const mockAnswerQuestion = vi.fn(async () => null)
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

import { questionAnswerSummary, useChatController } from './use-chat-controller'

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

beforeEach(() => {
  setSessionUser({ id: 'user_1', email: 'member@example.com', name: 'Member', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role: 'member' })
  flush()
})

afterEach(() => {
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

describe('questionAnswerSummary', () => {
  it('reads the answer as the labels chosen and the words written, one line per item', () => {
    expect(
      questionAnswerSummary(question, {
        answers: [
          { itemId: 'carrier', optionIds: ['bring'], text: '' },
          { itemId: 'services', optionIds: ['cold', 'tracked'], text: '' },
        ],
      }),
    ).toBe('Bring\nCold, Tracked')

    expect(
      questionAnswerSummary(question, {
        answers: [
          { itemId: 'carrier', optionIds: [], text: ' Posten ' },
          { itemId: 'services', optionIds: ['cold'], text: '' },
        ],
      }),
    ).toBe('Posten\nCold')
  })
})

describe('the question the chat holds', () => {
  it('waits until it is answered, which is recorded before the next turn carries its summary', async () => {
    await withController(async (controller) => {
      await tick()
      await controller.handleComposerSubmit({ actions: [], attachments: [], text: 'Book the shipment', tools: [] })
      await tick()

      streams[0]!.handlers.onUserQuestion?.(question)
      flush()
      expect(controller.pendingQuestion()).toEqual(question)

      // The run that asked has finished, so the next turn is a turn of its own, not queued input.
      streams[0]!.resolve()
      await tick()

      const answer = {
        answers: [
          { itemId: 'carrier', optionIds: ['bring'], text: '' },
          { itemId: 'services', optionIds: ['cold'], text: '' },
        ],
      }
      // The send awaits the run's stream, which this test leaves open: the turn is checked as sent, not as finished.
      void controller.answerPendingQuestion(answer)
      await tick()

      expect(mockAnswerQuestion).toHaveBeenCalledWith('q_1', { answers: answer.answers })
      expect(controller.pendingQuestion()).toBeNull()
      const turn = streams[streams.length - 1]!.request
      expect(turn.content).toBe('Bring\nCold')
      expect(turn).not.toHaveProperty('answersQuestionId')
    })
  })

  it('leaves no card when the person opens another thread, nor a refusal left on the old one', async () => {
    await withController(async (controller) => {
      await tick()
      await controller.handleComposerSubmit({ actions: [], attachments: [], text: 'Book the shipment', tools: [] })
      await tick()

      streams[0]!.handlers.onUserQuestion?.(question)
      flush()
      mockAnswerQuestion.mockRejectedValueOnce(new Error('network down'))
      await controller.answerPendingQuestion({
        answers: [
          { itemId: 'carrier', optionIds: ['bring'], text: '' },
          { itemId: 'services', optionIds: ['cold'], text: '' },
        ],
      })
      expect(controller.pendingQuestion()).toEqual(question)
      expect(controller.questionError()).not.toBeNull()

      setActiveChatThreadId('thread_other')
      await tick(10)

      expect(controller.pendingQuestion()).toBeNull()
      expect(controller.questionError()).toBeNull()
    })
  })

  it('leaves no card when the person starts a new chat', async () => {
    await withController(async (controller) => {
      await tick()
      await controller.handleComposerSubmit({ actions: [], attachments: [], text: 'Book the shipment', tools: [] })
      await tick()

      streams[0]!.handlers.onUserQuestion?.(question)
      flush()
      expect(controller.pendingQuestion()).toEqual(question)

      controller.startNewChat()
      await tick()

      expect(controller.pendingQuestion()).toBeNull()
    })
  })
})

describe('an answer or a skip that lands after the person opened another thread', () => {
  const answer = {
    answers: [
      { itemId: 'carrier', optionIds: ['bring'], text: '' },
      { itemId: 'services', optionIds: ['cold'], text: '' },
    ],
  }
  const otherQuestion: ChatUserQuestionEvent = { ...question, questionId: 'q_2' }

  /** The first chat asks `question`, and its send has ended, so the card waits. */
  async function askFirstChat(controller: ReturnType<typeof useChatController>): Promise<void> {
    await tick()
    await controller.handleComposerSubmit({ actions: [], attachments: [], text: 'Book the shipment', tools: [] })
    await tick()
    streams[0]!.handlers.onUserQuestion?.(question)
    flush()
    streams[0]!.resolve()
    await tick()
  }

  /** The person opens another thread, which asks its own question on a send of its own. */
  async function openOtherThread(controller: ReturnType<typeof useChatController>): Promise<void> {
    setActiveChatThreadId('thread_other')
    await tick(10)
    await controller.handleComposerSubmit({ actions: [], attachments: [], text: 'Another chat', tools: [] })
    await tick()
    streams[1]!.handlers.onUserQuestion?.(otherQuestion)
    flush()
  }

  /** Holds the next recording of the question open; the returned function settles it, so the thread can change first. */
  function holdAnswer(outcome: 'record' | 'refuse'): () => void {
    let settle!: () => void
    mockAnswerQuestion.mockImplementationOnce(
      () =>
        new Promise<null>((resolve, reject) => {
          settle = outcome === 'record' ? () => resolve(null) : () => reject(new Error('network down'))
        }),
    )
    return () => settle()
  }

  it('sends nothing, and leaves the card of the other thread alone, when the answer is recorded after the switch', async () => {
    await withController(async (controller) => {
      await askFirstChat(controller)
      const recorded = holdAnswer('record')
      const answering = controller.answerPendingQuestion(answer)
      await tick()
      expect(mockAnswerQuestion).toHaveBeenCalledWith('q_1', { answers: answer.answers })

      await openOtherThread(controller)
      recorded()
      await answering
      await tick()

      expect(streams).toHaveLength(2)
      expect(streams[1]!.request.content).toBe('Another chat')
      expect(controller.pendingQuestion()).toEqual(otherQuestion)
      expect(controller.questionError()).toBeNull()
    })
  })

  it('leaves the card of the other thread alone when the skip is recorded after the switch', async () => {
    await withController(async (controller) => {
      await askFirstChat(controller)
      const recorded = holdAnswer('record')
      const skipping = controller.skipPendingQuestion()
      await tick()
      expect(mockAnswerQuestion).toHaveBeenCalledWith('q_1', { skip: true })

      await openOtherThread(controller)
      recorded()
      await skipping
      await tick()

      expect(streams).toHaveLength(2)
      expect(controller.pendingQuestion()).toEqual(otherQuestion)
      expect(controller.questionError()).toBeNull()
    })
  })

  it('keeps a refusal of the answer off the thread the person has left', async () => {
    await withController(async (controller) => {
      await askFirstChat(controller)
      const refused = holdAnswer('refuse')
      const answering = controller.answerPendingQuestion(answer)
      await tick()

      await openOtherThread(controller)
      refused()
      await answering
      await tick()

      expect(controller.pendingQuestion()).toEqual(otherQuestion)
      expect(controller.questionError()).toBeNull()
    })
  })

  it('gives an answer whose send is not accepted back to the composer', async () => {
    await withController(async (controller) => {
      await askFirstChat(controller)
      void controller.answerPendingQuestion(answer)
      await tick()
      expect(streams[1]!.request.content).toBe('Bring\nCold')
      expect(controller.input()).toBe('')

      streams[1]!.handlers.onError?.({ code: 'upstream_failed', message: 'Upstream failed.', retryable: true })
      streams[1]!.resolve()
      await tick()
      expect(controller.input()).toBe('Bring\nCold')
    })
  })

  it('keeps the text the person typed while the answer is sent, and adds the answer after it when the send is not accepted', async () => {
    await withController(async (controller) => {
      await askFirstChat(controller)
      controller.setInput('Halvferdig')
      flush()
      void controller.answerPendingQuestion(answer)
      await tick()
      expect(controller.input()).toBe('Halvferdig')

      streams[1]!.handlers.onError?.({ code: 'upstream_failed', message: 'Upstream failed.', retryable: true })
      streams[1]!.resolve()
      await tick()
      expect(controller.input()).toBe('Halvferdig\n\nBring\nCold')
    })
  })

  it('keeps the text the person typed when the answer is accepted', async () => {
    await withController(async (controller) => {
      await askFirstChat(controller)
      controller.setInput('Halvferdig')
      flush()
      void controller.answerPendingQuestion(answer)
      await tick()

      streams[1]!.handlers.onConnected?.({ requestId: 'req-2', threadId: 'thread_a' })
      flush()
      await tick()
      expect(controller.input()).toBe('Halvferdig')
    })
  })
})
