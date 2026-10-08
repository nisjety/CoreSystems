// @vitest-environment jsdom

/**
 * plan-gate: the confirm gate across a RELOAD.
 *
 * `turn.proposedPlan` arrives on the `plan_proposed` SSE frame and is
 * session-only, but `plan.propose` IS the plan — the answer does not repeat it.
 * Before the controller read it back, reloading between "the agent proposed a
 * plan" and "the person decided" left the rung-only control on a message with
 * nothing to review, and approving it granted authority against steps the
 * person never saw. These tests load a thread the way a reload does and check
 * that the card gets its plan (or its rejection) back from the server's record.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Plan } from '@/shared/api/orchestration-client'
import type { RunDetail } from '@/shared/api/runs-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'

const mocks = vi.hoisted(() => ({
  listChatThreads: vi.fn(async () => [] as unknown[]),
  getThreadMessages: vi.fn(async () => [] as unknown[]),
  getChatThreadTranscript: vi.fn(async () => null as unknown),
  listRuns: vi.fn(async () => ({ runs: [] as RunDetail[], hasMore: false })),
  listPlans: vi.fn(async () => [] as Plan[]),
}))

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    streamChat: vi.fn(() => new Promise<void>(() => {})),
    resumeStream: vi.fn(async () => undefined),
    cancelInvocation: vi.fn().mockResolvedValue(undefined),
    getChatThreadTranscript: mocks.getChatThreadTranscript,
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: mocks.listChatThreads,
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
    getThreadMessages: mocks.getThreadMessages,
  }
})

vi.mock('@/shared/api/runs-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/runs-client')>()
  return { ...actual, getRun: vi.fn(async () => null), listRuns: mocks.listRuns }
})

vi.mock('@/shared/api/orchestration-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/orchestration-client')>()
  return {
    ...actual,
    listPlans: mocks.listPlans,
    listApprovals: vi.fn(async () => []),
    cancelRun: vi.fn(async (runId: string) => ({ runId, cancelled: true, status: 'cancelled' })),
  }
})

import { setActiveChatThreadId } from '@/features/chat/lib/chat-thread-history'
import { useChatController } from './use-chat-controller'

const THREAD = 'thread-plan-reload'
const RUN_ID = '01M2ZNAZ0W48WCREHFTGPKPNV2'
const QUESTION = { id: 'msg-q', role: 'user' as const, content: 'Send fakturaen.', createdAt: '' }
const ANSWER = { id: 'msg-a', role: 'assistant' as const, content: 'Planen er lagt frem.', createdAt: '' }

const planRun = (): RunDetail => ({
  runId: RUN_ID,
  status: 'completed',
  goal: QUESTION.content,
  mode: 'plan',
  threadId: THREAD,
  checkpointIndex: 0,
  stepsCompleted: 0,
  inputTokens: 0,
  outputTokens: 0,
})

const proposedPlan = (overrides: Partial<Plan> = {}): Plan => ({
  id: 'plan_1',
  runId: RUN_ID,
  state: 'PLAN_STATE_PROPOSED',
  summary: 'Send the overdue invoice',
  steps: [
    { id: 'step_1', title: 'Draft the email', state: 'PLAN_STEP_STATE_PENDING' },
    { id: 'step_2', title: 'Send it', operation: 'email_send', state: 'PLAN_STEP_STATE_PENDING' },
  ],
  ...overrides,
})

/** The run's own execution log — older than the proposal, never PROPOSED. */
const executionLog = (): Plan => ({
  id: `plan_${RUN_ID}`,
  runId: RUN_ID,
  state: 'PLAN_STATE_COMPLETED',
  summary: QUESTION.content,
  steps: [],
})

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

/** A reload: the stored thread comes back from the server as plain messages. */
function reload() {
  mocks.listChatThreads.mockResolvedValue([
    {
      threadId: THREAD,
      title: 'Faktura',
      preview: 'Faktura',
      updatedAt: '2026-09-28T10:00:00.000Z',
      pinned: false,
      latestRunId: RUN_ID,
      latestRunStatus: 'completed',
    },
  ])
  mocks.getThreadMessages.mockResolvedValue([QUESTION, ANSWER])
  mocks.listRuns.mockResolvedValue({ runs: [planRun()], hasMore: false })
  setActiveChatThreadId(THREAD)
}

afterEach(() => {
  mocks.listChatThreads.mockReset()
  mocks.listChatThreads.mockResolvedValue([])
  mocks.getThreadMessages.mockReset()
  mocks.getThreadMessages.mockResolvedValue([])
  mocks.getChatThreadTranscript.mockReset()
  mocks.getChatThreadTranscript.mockResolvedValue(null)
  mocks.listRuns.mockReset()
  mocks.listRuns.mockResolvedValue({ runs: [], hasMore: false })
  mocks.listPlans.mockReset()
  mocks.listPlans.mockResolvedValue([])
  __resetRetentionForTests()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('plan-gate: a reload gives the card its plan back', () => {
  it('restores the editable plan of a run that is still awaiting a decision', async () => {
    reload()
    mocks.listPlans.mockResolvedValue([executionLog(), proposedPlan()])
    await withController(async (controller) => {
      await tick(80)
      const turn = controller.state.turns.at(-1)
      expect(turn).toMatchObject({ role: 'assistant', runId: RUN_ID, planMode: true })
      expect(mocks.listPlans).toHaveBeenCalledWith(RUN_ID)
      expect(turn?.proposedPlan).toEqual({
        planId: 'plan_1',
        summary: 'Send the overdue invoice',
        steps: [
          { id: 'step_1', title: 'Draft the email' },
          { id: 'step_2', title: 'Send it', operation: 'email_send' },
        ],
      })
      expect(turn?.planRejected).toBeUndefined()
    })
  })

  it('says a rejected plan was rejected instead of offering to approve it again', async () => {
    reload()
    mocks.listPlans.mockResolvedValue([executionLog(), proposedPlan({ state: 'PLAN_STATE_REJECTED' })])
    await withController(async (controller) => {
      await tick(80)
      const turn = controller.state.turns.at(-1)
      expect(turn?.planRejected).toBe(true)
      expect(turn?.proposedPlan).toBeUndefined()
    })
  })

  it('restores nothing for a plan that was already approved, and survives an unreadable one', async () => {
    reload()
    mocks.listPlans.mockResolvedValue([executionLog(), proposedPlan({ state: 'PLAN_STATE_APPROVED' })])
    await withController(async (controller) => {
      await tick(80)
      const turn = controller.state.turns.at(-1)
      expect(turn).toMatchObject({ role: 'assistant', planMode: true })
      expect(turn?.proposedPlan).toBeUndefined()
      expect(turn?.planRejected).toBeUndefined()
    })

    // A plan the server cannot be asked for is not an error the person sees:
    // the turn keeps the rung-only control it always had.
    mocks.listPlans.mockRejectedValue(new Error('boom'))
    await withController(async (controller) => {
      await tick(80)
      const turn = controller.state.turns.at(-1)
      expect(turn).toMatchObject({ role: 'assistant', planMode: true })
      expect(turn?.proposedPlan).toBeUndefined()
      expect(controller.state.error).toBeNull()
    })
  })
})
