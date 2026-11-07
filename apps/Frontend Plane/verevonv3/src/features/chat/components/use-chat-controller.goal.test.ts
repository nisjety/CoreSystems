// @vitest-environment jsdom

/**
 * The thread goal as the chat controller holds it (EP-7 S1): read for the open thread, and a Continue
 * that sends one ordinary turn naming the goal, in the thread's own mode, without touching the
 * composer's Do toggle.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatInvokeRequest, ChatStreamHandlers } from '@/shared/api/chat-client'
import type { ThreadGoal } from '@/shared/api/thread-goal-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

type CapturedStream = {
  request: ChatInvokeRequest
  handlers: ChatStreamHandlers
  resolve: () => void
}

const { streams, mockStreamChat, mockGetThreadGoal } = vi.hoisted(() => {
  const streams: CapturedStream[] = []
  const mockStreamChat = vi.fn((request: ChatInvokeRequest, handlers: ChatStreamHandlers) => {
    return new Promise<void>((resolve) => {
      streams.push({ request, handlers, resolve })
    })
  })
  const mockGetThreadGoal = vi.fn()
  return { streams, mockStreamChat, mockGetThreadGoal }
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

vi.mock('@/shared/api/thread-goal-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/thread-goal-client')>()
  return { ...actual, getThreadGoal: mockGetThreadGoal }
})

import { goalContinuationMode, useChatController } from './use-chat-controller'

function goal(threadId: string, overrides: Partial<ThreadGoal> = {}): ThreadGoal {
  return {
    id: 'goal_1',
    threadId,
    objective: 'Ship the quarterly report',
    successCriteria: ['Numbers reconciled'],
    status: 'active',
    tokensUsed: 0,
    turnsUsed: 0,
    revision: 1,
    ...overrides,
  }
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
  mockGetThreadGoal.mockImplementation(async (threadId: string) => goal(threadId))
})

afterEach(() => {
  streams.length = 0
  mockStreamChat.mockClear()
  mockGetThreadGoal.mockReset()
  __resetRetentionForTests()
  clearSession()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('goalContinuationMode', () => {
  it('continue_uses_execute_only_with_live_grant', () => {
    const executed = [
      { role: 'user' as const },
      { role: 'assistant' as const, executeGranted: true },
    ]
    expect(goalContinuationMode({ liveGrant: true, turns: executed })).toBe('execute')
    // No live grant: a plan turn, whatever the last turn was.
    expect(goalContinuationMode({ liveGrant: false, turns: executed })).toBe('plan')
    // A grant, but the last Do turn planned: the next turn plans too.
    expect(
      goalContinuationMode({
        liveGrant: true,
        turns: [...executed, { role: 'user' as const }, { role: 'assistant' as const, planMode: true }],
      }),
    ).toBe('plan')
    expect(goalContinuationMode({ liveGrant: true, turns: [] })).toBe('plan')
  })
})

describe('the goal the chat holds', () => {
  it('continue_sends_one_turn_with_goal_id', async () => {
    await withController(async (controller) => {
      await tick()
      await controller.handleComposerSubmit({ actions: [], attachments: [], text: 'Start on the report', tools: [] })
      await tick()
      streams[0]!.resolve()
      await tick(6)

      const threadId = controller.state.threadId as string
      expect(mockGetThreadGoal).toHaveBeenCalledWith(threadId)
      expect(controller.threadGoal()?.id).toBe('goal_1')
      expect(controller.goalEntryAvailable()).toBe(true)

      // The send awaits the run's stream, which this test leaves open: the turn is checked as sent.
      void controller.continueGoal()
      await tick()

      expect(streams).toHaveLength(2)
      const turn = streams[1]!.request
      expect(turn.goalId).toBe('goal_1')
      expect(turn.planMode).toBe(true)
      expect(turn.executeGranted).toBeFalsy()
      // The composer's Do toggle is not the Continue's to change.
      expect(controller.planMode()).toBe(false)

      // A second Continue while the first runs sends nothing.
      void controller.continueGoal()
      await tick()
      expect(streams).toHaveLength(2)
    })
  })

  it('sends nothing for a paused goal', async () => {
    mockGetThreadGoal.mockImplementation(async (threadId: string) => goal(threadId, { status: 'paused' }))
    await withController(async (controller) => {
      await tick()
      await controller.handleComposerSubmit({ actions: [], attachments: [], text: 'Start on the report', tools: [] })
      await tick()
      streams[0]!.resolve()
      await tick(6)

      expect(controller.threadGoal()?.status).toBe('paused')
      await controller.continueGoal()
      expect(streams).toHaveLength(1)
    })
  })
})
