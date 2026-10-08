// @vitest-environment jsdom

/**
 * plan-gate (chat-wave2-specs, 2026-09-28): the editable-plan review gate.
 *
 * Covers the three controller-level behaviors the spec's own §3 test list
 * calls out: `onPlanProposed` populating `turn.proposedPlan`, `approveTurnPlan`
 * forwarding the turn's edited steps and plan id, and `setPlanMode(false)`
 * firing on BOTH the approve and the reject path.
 *
 * Mirrors the `streamChat` stub and `withController`/`tick` helpers of
 * use-chat-controller.follow-up.test.ts.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'

type CapturedStream = {
  request: { content: string; planMode?: boolean }
  handlers: ChatStreamHandlers
  resolve: () => void
}

const { streams, mockStreamChat, mockApprovePlan, mockRejectPlan } = vi.hoisted(() => {
  const streams: CapturedStream[] = []
  const mockStreamChat = vi.fn((request: CapturedStream['request'], handlers: ChatStreamHandlers) => {
    return new Promise<void>((resolve) => {
      streams.push({ request, handlers, resolve })
    })
  })
  const mockApprovePlan = vi.fn()
  const mockRejectPlan = vi.fn()
  return { streams, mockStreamChat, mockApprovePlan, mockRejectPlan }
})

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    streamChat: mockStreamChat,
    approvePlan: mockApprovePlan,
    rejectPlan: mockRejectPlan,
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

afterEach(() => {
  streams.length = 0
  mockStreamChat.mockClear()
  mockApprovePlan.mockReset()
  mockRejectPlan.mockReset()
  __resetRetentionForTests()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('plan-gate: proposed-plan wiring', () => {
  it('onPlanProposed populates the streaming turn.proposedPlan', async () => {
    await withController(async (controller) => {
      await tick()
      controller.setPlanMode(true)
      flush()
      await send(controller, 'Send fakturaen')
      await tick()

      streams[0]!.handlers.onConnected?.({ requestId: 'req-1', threadId: 't-1', runId: 'run-1' })
      flush()
      streams[0]!.handlers.onPlanProposed?.({
        planId: 'plan_1',
        summary: 'Send the overdue invoice',
        steps: [
          { id: 'step_1', title: 'Draft the email' },
          { id: 'step_2', title: 'Send it', operation: 'email_send' },
        ],
        requestId: 'req-1',
      })
      flush()

      const turn = controller.state.turns.at(-1)
      expect(turn?.proposedPlan).toEqual({
        planId: 'plan_1',
        summary: 'Send the overdue invoice',
        steps: [
          { id: 'step_1', title: 'Draft the email' },
          { id: 'step_2', title: 'Send it', operation: 'email_send' },
        ],
      })
    })
  })

  it('a second plan_proposed call in the same turn REPLACES the first (last-proposed-wins)', async () => {
    await withController(async (controller) => {
      await tick()
      controller.setPlanMode(true)
      flush()
      await send(controller, 'Send fakturaen')
      await tick()
      streams[0]!.handlers.onConnected?.({ requestId: 'req-1', threadId: 't-1', runId: 'run-1' })
      flush()

      streams[0]!.handlers.onPlanProposed?.({
        planId: 'plan_1',
        summary: 'First draft',
        steps: [{ id: 'step_1', title: 'Old step' }],
      })
      flush()
      streams[0]!.handlers.onPlanProposed?.({
        planId: 'plan_2',
        summary: 'Revised plan',
        steps: [{ id: 'step_9', title: 'New step' }],
      })
      flush()

      expect(controller.state.turns.at(-1)?.proposedPlan?.planId).toBe('plan_2')
      expect(controller.state.turns.at(-1)?.proposedPlan?.summary).toBe('Revised plan')
    })
  })

  it('approveTurnPlan forwards the proposed plan id and the caller-supplied edited steps', async () => {
    mockApprovePlan.mockResolvedValue({
      grantedRung: 'workspace_write',
      wasInPlanMode: true,
      persisted: true,
      plan: { id: 'plan_1', state: 'PLAN_STATE_APPROVED' },
    })
    await withController(async (controller) => {
      await tick()
      controller.setPlanMode(true)
      flush()
      await send(controller, 'Send fakturaen')
      await tick()
      streams[0]!.handlers.onConnected?.({ requestId: 'req-1', threadId: 't-1', runId: 'run-1' })
      flush()
      streams[0]!.handlers.onPlanProposed?.({
        planId: 'plan_1',
        summary: 'Send the overdue invoice',
        steps: [{ id: 'step_1', title: 'Draft the email' }],
      })
      flush()

      const turnId = controller.state.turns.at(-1)!.id
      const edits = [{ stepId: 'step_1', title: 'Draft a shorter email', removed: false }]
      await controller.approveTurnPlan(turnId, 'workspace_write', 'writes its report', edits)

      expect(mockApprovePlan).toHaveBeenCalledWith(
        'run-1',
        'workspace_write',
        'writes its report',
        'plan_1',
        edits,
      )
      // setPlanMode(false) fires on approve.
      expect(controller.planMode()).toBe(false)
    })
  })

  it('rejectTurnPlan calls rejectPlan with the proposed plan id and turns plan mode off', async () => {
    mockRejectPlan.mockResolvedValue({ plan: { id: 'plan_1', state: 'PLAN_STATE_REJECTED' } })
    await withController(async (controller) => {
      await tick()
      controller.setPlanMode(true)
      flush()
      await send(controller, 'Send fakturaen')
      await tick()
      streams[0]!.handlers.onConnected?.({ requestId: 'req-1', threadId: 't-1', runId: 'run-1' })
      flush()
      streams[0]!.handlers.onPlanProposed?.({
        planId: 'plan_1',
        summary: 'Send the overdue invoice',
        steps: [{ id: 'step_1', title: 'Draft the email' }],
      })
      flush()

      const turnId = controller.state.turns.at(-1)!.id
      await controller.rejectTurnPlan(turnId, 'Ikke nødvendig lenger')

      expect(mockRejectPlan).toHaveBeenCalledWith('run-1', 'plan_1', 'Ikke nødvendig lenger')
      // setPlanMode(false) fires on reject too — the user must ask again in a
      // fresh message rather than silently retrying (owner ruling, no
      // revise-after-reject in this wave).
      expect(controller.planMode()).toBe(false)
      // And the turn records the decision, so the card can say the plan was
      // rejected instead of sitting there with an Approve button the server
      // would now refuse.
      expect(controller.state.turns.at(-1)?.planRejected).toBe(true)
    })
  })

  it('a refused rejection leaves the plan undecided and says why', async () => {
    mockRejectPlan.mockRejectedValue(new Error('the plan was decided by another request first'))
    await withController(async (controller) => {
      await tick()
      controller.setPlanMode(true)
      flush()
      await send(controller, 'Send fakturaen')
      await tick()
      streams[0]!.handlers.onConnected?.({ requestId: 'req-1', threadId: 't-1', runId: 'run-1' })
      flush()
      streams[0]!.handlers.onPlanProposed?.({
        planId: 'plan_1',
        summary: 'Send the overdue invoice',
        steps: [{ id: 'step_1', title: 'Draft the email' }],
      })
      flush()

      const turnId = controller.state.turns.at(-1)!.id
      await controller.rejectTurnPlan(turnId, 'Ikke nødvendig lenger')

      expect(controller.state.turns.at(-1)?.planRejected).toBeUndefined()
      // Plan mode stays on: nothing was decided, so nothing was left behind.
      expect(controller.planMode()).toBe(true)
    })
  })

  it('a refused approval grants nothing and leaves plan mode on for a retry', async () => {
    mockApprovePlan.mockRejectedValue(new Error('a plan cannot be approved with every step removed'))
    await withController(async (controller) => {
      await tick()
      controller.setPlanMode(true)
      flush()
      await send(controller, 'Send fakturaen')
      await tick()
      streams[0]!.handlers.onConnected?.({ requestId: 'req-1', threadId: 't-1', runId: 'run-1' })
      flush()
      streams[0]!.handlers.onPlanProposed?.({
        planId: 'plan_1',
        summary: 'Send the overdue invoice',
        steps: [{ id: 'step_1', title: 'Draft the email' }],
      })
      flush()

      const turnId = controller.state.turns.at(-1)!.id
      await controller.approveTurnPlan(turnId, 'workspace_write', 'writes its report', [])

      expect(controller.state.turns.at(-1)?.grantedRung).toBeUndefined()
      expect(controller.planMode()).toBe(true)
    })
  })
})
