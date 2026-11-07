// @vitest-environment jsdom

/**
 * WS17 S9 (F-t3-069): a rewind on a versioned thread takes the turns after the chosen answer off the transcript and
 * offers an undo under that answer, and the undo puts those turns back. The controller runs on the real transcript;
 * only the wire calls to the chat client are mocked.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { getThreadMessages, type ChatMessage } from '@/shared/api/chat-client'
import { ApiError } from '@/shared/api/http'
import { CHAT_ACTIVE_THREAD_CHANGED_EVENT } from '@/features/chat/lib/chat-thread-history'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'

const { mockRewind, mockUndo } = vi.hoisted(() => ({
  mockRewind: vi.fn(),
  mockUndo: vi.fn(),
}))

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    rewindChatThread: mockRewind,
    undoChatRewind: mockUndo,
    streamChat: vi.fn(() => new Promise(() => {})),
    resumeStream: vi.fn(),
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
    getChatThreadTranscript: vi.fn().mockResolvedValue(null),
    getThreadMessages: vi.fn().mockResolvedValue([]),
    cancelInvocation: vi.fn().mockResolvedValue(undefined),
  }
})

import { useChatController } from './use-chat-controller'

async function tick(times = 3) {
  for (let i = 0; i < times; i += 1) {
    await Promise.resolve()
    flush()
  }
}

/** Waits for a thread read to land: a load takes more hops than a fixed number of microtasks. */
async function until(done: () => boolean, attempts = 50) {
  for (let i = 0; i < attempts; i += 1) {
    flush()
    if (done()) return
    await new Promise((resolve) => setTimeout(resolve, 0))
  }
}

function withController<T>(
  fn: (controller: ReturnType<typeof useChatController>) => Promise<T>,
): Promise<T> {
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

/** The active path as the server holds it: a question, its answer, a follow-up and its answer. */
const transcript: ChatMessage[] = [
  { id: 'srv-q1', role: 'user', content: 'Hva er lakselus?', createdAt: '' },
  {
    id: 'srv-a1',
    role: 'assistant',
    content: 'Lakselus er en parasitt.',
    createdAt: '',
    version: { parentMessageId: 'srv-q1', siblingOrdinal: 1, siblingCount: 1, onActivePath: true, siblings: ['srv-a1'] },
  },
  { id: 'srv-q2', role: 'user', content: 'Og i sjøen?', createdAt: '' },
  {
    id: 'srv-a2',
    role: 'assistant',
    content: 'Den lever der.',
    createdAt: '',
    version: { parentMessageId: 'srv-q2', siblingOrdinal: 1, siblingCount: 1, onActivePath: true, siblings: ['srv-a2'] },
  },
]

describe('WS17 S9: rewind with an undo on a versioned thread', () => {
  /** The unpaged read the controller makes when paging is off (the tests run with it off). */
  const readMessages = vi.mocked(getThreadMessages as (threadId: string) => Promise<ChatMessage[]>)

  beforeEach(() => {
    readMessages.mockResolvedValue(transcript)
    mockRewind.mockReset()
    mockUndo.mockReset()
  })

  afterEach(() => {
    __resetRetentionForTests()
    readMessages.mockResolvedValue([])
    try {
      window.localStorage.clear()
    } catch {
      // ignore
    }
  })

  it('rewinds to an earlier answer, offers the hidden turns as an undo, and undoes the rewind', async () => {
    mockRewind.mockResolvedValue({ activeLeafMessageId: 'srv-a1', activePath: ['srv-q1', 'srv-a1'] })
    mockUndo.mockResolvedValue({
      activeLeafMessageId: 'srv-a2',
      activePath: ['srv-q1', 'srv-a1', 'srv-q2', 'srv-a2'],
    })

    await withController(async (controller) => {
      await tick()
      window.dispatchEvent(new CustomEvent(CHAT_ACTIVE_THREAD_CHANGED_EVENT, { detail: { threadId: 'thread-v' } }))
      await until(() => controller.state.turns.length === transcript.length)
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(['srv-q1', 'srv-a1', 'srv-q2', 'srv-a2'])

      // The preview names what the rewind hides, and the leaf is not rewindable.
      expect(controller.rewindPlanFor('srv-a1')?.hidden.map((turn) => turn.excerpt)).toEqual([
        'Og i sjøen?',
        'Den lever der.',
      ])
      expect(controller.rewindPlanFor('srv-a2')).toBeNull()

      await controller.rewindTo('srv-a1')
      await tick()
      expect(mockRewind).toHaveBeenCalledWith('thread-v', 'srv-a1', 'srv-a2')
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(['srv-q1', 'srv-a1'])
      expect(controller.rewindOfferFor('srv-a1')?.map((turn) => turn.id)).toEqual(['srv-q2', 'srv-a2'])

      await controller.undoRewind()
      await tick()
      // The undo names the turn the rewind left, and the leaf the rewind landed on.
      expect(mockUndo).toHaveBeenCalledWith('thread-v', 'srv-a2', 'srv-a1')
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(['srv-q1', 'srv-a1', 'srv-q2', 'srv-a2'])
      expect(controller.rewindOfferFor('srv-a1')).toBeNull()
    })
  })

  it('keeps the turns on screen and says so when the server refuses the rewind', async () => {
    mockRewind.mockRejectedValue(new ApiError('the thread moved on', 409, 'active_leaf_stale'))

    await withController(async (controller) => {
      await tick()
      window.dispatchEvent(new CustomEvent(CHAT_ACTIVE_THREAD_CHANGED_EVENT, { detail: { threadId: 'thread-v' } }))
      await until(() => controller.state.turns.length === transcript.length)

      await controller.rewindTo('srv-a1')
      await tick()
      expect(controller.state.turns.map((turn) => turn.id)).toEqual(['srv-q1', 'srv-a1', 'srv-q2', 'srv-a2'])
      expect(controller.rewindOfferFor('srv-a1')).toBeNull()
      expect(controller.feedbackNotice()).toBeTruthy()
    })
  })
})
