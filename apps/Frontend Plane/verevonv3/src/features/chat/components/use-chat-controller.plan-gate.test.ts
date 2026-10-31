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
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers } from '@/shared/api/chat-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'

type CapturedStream = {
  request: { content: string; planMode?: boolean; executeGranted?: boolean; threadId?: string; zdr?: boolean }
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
    queueInvocationInput: vi.fn().mockResolvedValue({ outcome: 'queued', pending: 1, persisted: true }),
  }
})

// A completed Work turn reads its proof bundle; nothing here talks to a server.
vi.mock('@/shared/api/run-console-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/run-console-client')>()
  return { ...actual, getRunProofBundle: vi.fn().mockResolvedValue(null) }
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

// The S2a execute turn is offered only where the build says the gateway can run
// it (VITE_PLAN_EXECUTE_ENABLED, flipped together with the gateway's switch).
// These tests are about the offered turn, so it is on; the describe near the
// end pins what happens when it is off.
beforeEach(() => {
  vi.stubEnv('VITE_PLAN_EXECUTE_ENABLED', 'true')
})

afterEach(() => {
  vi.unstubAllEnvs()
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

// ---------------------------------------------------------------------------
// WS14 S2a, "Run the approved plan". Approving a plan arms ONE execute turn: the
// next send goes out agentic with plan_mode false and `executeGranted`, and the
// gateway runs it under the grant it holds for the thread. The client holds only
// a session-only pointer to that grant.
// ---------------------------------------------------------------------------

const APPROVED_TEXT = /Kjør den godkjente planen|Run the approved plan/
const GRANT_GONE_TEXT = /Fullmakten er ikke lenger aktiv|The authority is no longer active/
const DISABLED_TEXT = /Kjøring av godkjent plan er slått av|Running an approved plan is switched off/
const OPERATOR_ENGLISH = 'No live plan grant is held for this thread (plan_grant_unavailable).'

type Controller = ReturnType<typeof useChatController>
type SubmitPayload = Parameters<Controller['handleComposerSubmit']>[0]

const sendWith = (controller: Controller, text: string, extra: Partial<SubmitPayload> = {}) =>
  controller.handleComposerSubmit({ actions: [], attachments: [], text, tools: [], ...extra })

/**
 * Plan a turn in Do mode, optionally let it finish, and approve its plan.
 * Returns the plan turn's id (the card's id).
 */
async function planAndApprove(
  controller: Controller,
  options: {
    finish?: boolean
    expiresAtMs?: number
    rung?: 'workspace_write' | 'danger_full_access'
    draft?: string
  } = {},
): Promise<string> {
  const { finish = true, expiresAtMs, rung = 'workspace_write', draft } = options
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
  if (finish) {
    streams[0]!.handlers.onDone?.({ requestId: 'req-1' })
    streams[0]!.resolve()
    await tick(8)
  }
  mockApprovePlan.mockResolvedValue({
    grantedRung: rung,
    wasInPlanMode: true,
    persisted: true,
    ...(expiresAtMs !== undefined ? { grantExpiresAtMs: expiresAtMs } : {}),
  })
  const planTurnId = controller.state.turns.find((turn) => turn.role === 'assistant')!.id
  if (draft !== undefined) {
    controller.setInput(draft)
    flush()
  }
  await controller.approveTurnPlan(planTurnId, rung, 'writes its report', [])
  flush()
  return planTurnId
}

const finishStream = async (index: number) => {
  streams[index]!.handlers.onDone?.({ requestId: `req-${index + 1}` })
  streams[index]!.resolve()
  await tick(8)
}

describe('WS14 S2a: approving a plan arms the next turn', () => {
  it('turns plan mode off, reports the rung with the pill selected, and prefills an empty draft', async () => {
    await withController(async (controller) => {
      await planAndApprove(controller, { expiresAtMs: Date.now() + 3_600_000 })

      expect(controller.planMode()).toBe(false)
      expect(controller.approvedPlanView()).toEqual({ rung: 'workspace_write', selected: true })
      expect(controller.input()).toMatch(APPROVED_TEXT)
      // It does not send anything by itself: auto-send stays in S2.
      expect(streams).toHaveLength(1)
    })
  })

  it('never overwrites a draft the person already typed', async () => {
    await withController(async (controller) => {
      await planAndApprove(controller, { draft: 'Husk å si fra til Kari' })

      expect(controller.input()).toBe('Husk å si fra til Kari')
      expect(controller.approvedPlanView()?.selected).toBe(true)
    })
  })

  it('arms nothing when the approval is refused', async () => {
    await withController(async (controller) => {
      await tick()
      controller.setPlanMode(true)
      flush()
      await send(controller, 'Send fakturaen')
      await tick()
      streams[0]!.handlers.onConnected?.({ requestId: 'req-1', threadId: 't-1', runId: 'run-1' })
      flush()
      mockApprovePlan.mockRejectedValue(new Error('a plan cannot be approved with every step removed'))
      await controller.approveTurnPlan(controller.state.turns.at(-1)!.id, 'workspace_write', 'writes its report', [])

      expect(controller.approvedPlanView()).toBeNull()
      expect(controller.input()).toBe('')
    })
  })

  it('a rejection arms nothing and leaves an armed grant for another plan untouched', async () => {
    mockRejectPlan.mockResolvedValue({ plan: { id: 'plan_2', state: 'PLAN_STATE_REJECTED' } })
    await withController(async (controller) => {
      await planAndApprove(controller)
      expect(controller.approvedPlanView()?.selected).toBe(true)

      // A second plan on the same thread is proposed and then rejected.
      controller.setPlanMode(true)
      flush()
      await send(controller, 'Og en ny plan')
      await tick()
      streams[1]!.handlers.onConnected?.({ requestId: 'req-2', threadId: 't-1', runId: 'run-2' })
      streams[1]!.handlers.onPlanProposed?.({ planId: 'plan_2', summary: 'Another plan', steps: [] })
      flush()
      await finishStream(1)
      controller.selectExecute()
      flush()
      await controller.rejectTurnPlan(controller.state.turns.at(-1)!.id, 'Ikke nødvendig lenger')

      expect(controller.planMode()).toBe(false)
      // The reject neither dropped the grant nor changed whether it is armed.
      expect(controller.approvedPlanView()).toEqual({ rung: 'workspace_write', selected: true })
    })
  })
})

describe('WS14 S2a: the next send is an execute turn', () => {
  it('carries executeGranted with plan mode off, and spends the grant on connect', async () => {
    await withController(async (controller) => {
      const planTurnId = await planAndApprove(controller)

      await send(controller, controller.input())
      await tick()

      expect(streams).toHaveLength(2)
      expect(streams[1]!.request).toMatchObject({ planMode: false, executeGranted: true, threadId: 't-1' })
      // Not spent before the gateway has accepted the turn: a network failure may still be retried.
      expect(controller.approvedPlanView()?.selected).toBe(true)

      streams[1]!.handlers.onConnected?.({ requestId: 'req-2', threadId: 't-1', runId: 'run-2' })
      flush()

      expect(controller.approvedPlanView()).toBeNull()
      // The turn is a run: the Work surface follows it even though it is not a plan turn.
      expect(controller.liveRunId()).toBe('run-2')
      const executeTurn = controller.state.turns.at(-1)!
      expect(executeTurn).toMatchObject({ executeGranted: true, planMode: false, runId: 'run-2' })
      // The plan card's own turn is untouched.
      expect(controller.state.turns.find((turn) => turn.id === planTurnId)?.executeGranted).toBeUndefined()
    })
  })

  it('makes the send after that an ordinary Ask turn', async () => {
    await withController(async (controller) => {
      await planAndApprove(controller)
      await send(controller, controller.input())
      await tick()
      streams[1]!.handlers.onConnected?.({ requestId: 'req-2', threadId: 't-1', runId: 'run-2' })
      flush()
      await finishStream(1)

      await send(controller, 'Takk, og hva nå?')
      await tick()

      expect(streams).toHaveLength(3)
      expect(streams[2]!.request.executeGranted).toBeUndefined()
      expect(streams[2]!.request.planMode).toBe(false)
    })
  })

  it('keeps the grant and the armed state across a failure before the gateway connects, so a retry works', async () => {
    await withController(async (controller) => {
      await planAndApprove(controller)
      await send(controller, controller.input())
      await tick()

      streams[1]!.handlers.onError?.({ code: 'inference_stream_error', message: 'Stream failed' })
      streams[1]!.resolve()
      await tick(8)

      expect(controller.approvedPlanView()).toEqual({ rung: 'workspace_write', selected: true })
      await send(controller, 'Kjør den godkjente planen')
      await tick()
      expect(streams[2]!.request.executeGranted).toBe(true)
    })
  })
})

describe('WS14 S2a: what disarms the execute turn', () => {
  it('Ask, Do and a plan-mode write each disarm it, while the pill stays selectable', async () => {
    await withController(async (controller) => {
      await planAndApprove(controller)
      expect(controller.approvedPlanView()?.selected).toBe(true)

      controller.setPlanMode(false)
      flush()
      expect(controller.approvedPlanView()).toEqual({ rung: 'workspace_write', selected: false })

      controller.selectExecute()
      flush()
      expect(controller.approvedPlanView()?.selected).toBe(true)
      expect(controller.planMode()).toBe(false)

      controller.setPlanMode(true)
      flush()
      expect(controller.planMode()).toBe(true)
      expect(controller.approvedPlanView()?.selected).toBe(false)

      // An updater function is accepted, as the existing callers use it.
      controller.selectExecute()
      controller.setPlanMode((previous) => !previous)
      flush()
      expect(controller.approvedPlanView()?.selected).toBe(false)
    })
  })

  it('selecting execute again forces plan mode off', async () => {
    await withController(async (controller) => {
      await planAndApprove(controller)
      controller.setPlanMode(true)
      flush()

      controller.selectExecute()
      flush()

      expect(controller.planMode()).toBe(false)
      expect(controller.approvedPlanView()?.selected).toBe(true)
    })
  })

  it('a disarmed send is an ordinary turn, and a plan turn is a plan turn', async () => {
    await withController(async (controller) => {
      await planAndApprove(controller)
      controller.setPlanMode(true)
      flush()

      await send(controller, 'Planlegg på nytt')
      await tick()

      expect(streams[1]!.request).toMatchObject({ planMode: true })
      expect(streams[1]!.request.executeGranted).toBeUndefined()
    })
  })
})

describe('WS14 S2a: a send that cannot execute never carries the grant', () => {
  const blocked: Array<[string, Partial<SubmitPayload>]> = [
    ['zero data retention', { zdr: true }],
    ['the ChatGPT subscription route', { provider: 'openai-codex-subscription' }],
    ['a conversation-only scope', { sourceScope: 'conversation' }],
    ['a filtered retrieval scope', { retrievalScope: 'web_only' }],
  ]

  for (const [name, extra] of blocked) {
    it(`goes out as an ordinary turn under ${name}, disarms, and keeps the grant for later`, async () => {
      await withController(async (controller) => {
        await planAndApprove(controller)

        await sendWith(controller, 'Kjør den godkjente planen', extra)
        await tick()

        expect(streams[1]!.request.executeGranted).toBeUndefined()
        expect(controller.approvedPlanView()).toEqual({ rung: 'workspace_write', selected: false })
        // The grant is still there: a later, valid send can pick the pill again.
        controller.selectExecute()
        flush()
        expect(controller.approvedPlanView()?.selected).toBe(true)
      })
    })
  }

  it('does not run on a different thread, and never on a new one', async () => {
    await withController(async (controller) => {
      await planAndApprove(controller)

      controller.startNewChat()
      flush()
      expect(controller.approvedPlanView()).toBeNull()
      await send(controller, 'Kjør den godkjente planen')
      await tick()

      expect(streams[1]!.request.executeGranted).toBeUndefined()
      expect(streams[1]!.request.threadId).toBeUndefined()
    })
  })

  it('a message typed while the run is still working is queued input, never an execute turn', async () => {
    await withController(async (controller) => {
      // The plan run is still streaming when the person approves.
      await planAndApprove(controller, { finish: false })
      expect(controller.state.status).toBe('streaming')

      await send(controller, 'Kjør den godkjente planen')
      await tick(8)

      // No new stream was opened; the message went to the running agent.
      expect(streams).toHaveLength(1)
      expect(controller.approvedPlanView()?.selected).toBe(true)

      // And it does not wait to be sent as an execute turn later either.
      streams[0]!.handlers.onDone?.({ requestId: 'req-1' })
      streams[0]!.resolve()
      await tick(12)
      for (const stream of streams.slice(1)) expect(stream.request.executeGranted).toBeUndefined()
    })
  })
})

describe('WS14 S2a: a message deferred while the execute turn is opening', () => {
  it('is sent later as an ordinary turn, even when the first execute turn failed before it connected and the pill is still armed', async () => {
    await withController(async (controller) => {
      await planAndApprove(controller)
      await send(controller, controller.input())
      await tick()
      expect(streams[1]!.request.executeGranted).toBe(true)
      // No `connected` yet, so the run has no request id: a message typed now is
      // deferred until the machine is free, and it was typed with the pill armed.
      await send(controller, 'Og husk å varsle Kari')
      await tick()
      expect(streams).toHaveLength(2)
      expect(controller.approvedPlanView()?.selected).toBe(true)

      streams[1]!.handlers.onError?.({ code: 'inference_stream_error', message: 'Stream failed' })
      streams[1]!.resolve()
      await tick(12)

      const deferred = streams[2]
      expect(deferred?.request.content).toBe('Og husk å varsle Kari')
      expect(deferred?.request.executeGranted).toBeUndefined()
    })
  })
})

describe('WS14 S2a: an expired grant', () => {
  it('aborts the send, keeps the draft, drops the grant and says so on the plan card', async () => {
    const realNow = Date.now.bind(Date)
    await withController(async (controller) => {
      const planTurnId = await planAndApprove(controller, { expiresAtMs: realNow() + 60_000 })
      expect(controller.approvedPlanView()?.selected).toBe(true)

      const spy = vi.spyOn(Date, 'now').mockReturnValue(realNow() + 120_000)
      try {
        const draft = controller.input()
        await send(controller, draft)
        await tick()

        expect(streams).toHaveLength(1)
        expect(controller.input()).toBe(draft)
        expect(controller.approvedPlanView()).toBeNull()
        expect(controller.planApprovalError()[planTurnId]).toMatch(/utløpt|expired/)
      } finally {
        spy.mockRestore()
      }
    })
  })
})

describe('WS14 S2a: the gateway refuses the execute turn, typed', () => {
  for (const [code, text] of [
    ['plan_grant_unavailable', GRANT_GONE_TEXT],
    ['plan_execute_disabled', DISABLED_TEXT],
  ] as const) {
    it(`${code} after connect: shows the typed words, offers no retry and drops the grant`, async () => {
      await withController(async (controller) => {
        const planTurnId = await planAndApprove(controller)
        await send(controller, controller.input())
        await tick()
        streams[1]!.handlers.onConnected?.({ requestId: 'req-2', threadId: 't-1', runId: 'run-2' })
        flush()

        streams[1]!.handlers.onError?.({ code, message: OPERATOR_ENGLISH, retryable: false })
        streams[1]!.resolve()
        await tick(8)

        const turn = controller.state.turns.at(-1)!
        expect(turn).toMatchObject({ status: 'error', failureAction: 'none', streaming: false })
        expect(turn.content).toMatch(text)
        expect(turn.content).not.toContain('plan grant')
        expect(controller.state.error).toBe(turn.content)
        expect(controller.approvedPlanView()).toBeNull()
        // The plan card says the same, so the person sees it where they approved.
        expect(controller.planApprovalError()[planTurnId]).toMatch(text)
        // Nothing is resent or resumed.
        expect(streams).toHaveLength(2)
      })
    })

    it(`${code} before connect: drops the grant too, so the pill cannot offer a turn that always refuses`, async () => {
      await withController(async (controller) => {
        const planTurnId = await planAndApprove(controller)
        await send(controller, controller.input())
        await tick()
        expect(controller.approvedPlanView()?.selected).toBe(true)

        streams[1]!.handlers.onError?.({ code, message: OPERATOR_ENGLISH, retryable: false })
        streams[1]!.resolve()
        await tick(8)

        expect(controller.approvedPlanView()).toBeNull()
        expect(controller.planApprovalError()[planTurnId]).toMatch(text)
        expect(streams).toHaveLength(2)
      })
    })
  }
})

describe('WS14 S2a: no replay path carries the grant', () => {
  it('regenerate and edit-resubmit send ordinary turns even while the pill is armed', async () => {
    await withController(async (controller) => {
      await planAndApprove(controller)
      // Run the plan and let it finish; then approve once more so the pill is armed again.
      await send(controller, controller.input())
      await tick()
      streams[1]!.handlers.onConnected?.({ requestId: 'req-2', threadId: 't-1', runId: 'run-2' })
      flush()
      await finishStream(1)
      mockApprovePlan.mockResolvedValue({ grantedRung: 'workspace_write', wasInPlanMode: true, persisted: true })
      await controller.approveTurnPlan(
        controller.state.turns.find((turn) => turn.proposedPlan)!.id,
        'workspace_write',
        'writes its report',
        [],
      )
      flush()
      expect(controller.approvedPlanView()?.selected).toBe(true)

      controller.regenerateLatest()
      await tick(12)
      const executeUser = controller.state.turns.find(
        (turn) => turn.role === 'user' && APPROVED_TEXT.test(turn.content),
      )
      expect(executeUser).toBeDefined()
      // Not awaited: it resolves when its stream ends, and the mock streams stay open.
      void controller.editAndResubmit(executeUser!.id, 'Kjør planen, men si fra først')
      await tick(12)

      expect(streams.length).toBeGreaterThan(2)
      for (const stream of streams.slice(2)) expect(stream.request.executeGranted).toBeUndefined()
    })
  })
})

describe('WS14 S2a: a thread switch while the approval is in flight', () => {
  it('leaves the pointer, the armed state and the draft off the thread that is open now', async () => {
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
      streams[0]!.handlers.onDone?.({ requestId: 'req-1' })
      streams[0]!.resolve()
      await tick(8)

      let release!: (value: unknown) => void
      mockApprovePlan.mockReturnValue(new Promise((resolve) => { release = resolve }))
      const pending = controller.approveTurnPlan(
        controller.state.turns.find((turn) => turn.role === 'assistant')!.id,
        'workspace_write',
        'writes its report',
        [],
      )
      // The person leaves the thread whose plan card they just approved.
      // and is on another thread, t-2, with a thread id of its own, when it lands.
      controller.startNewChat()
      flush()
      await send(controller, 'Hei')
      await tick()
      streams[1]!.handlers.onConnected?.({ requestId: 'req-2', threadId: 't-2', runId: 'run-2' })
      flush()
      expect(controller.state.threadId).toBe('t-2')
      release({ grantedRung: 'workspace_write', wasInPlanMode: true, persisted: true })
      await pending
      flush()

      // The grant belongs to thread t-1 on the gateway; nothing of it lands on t-2.
      expect(controller.approvedPlanView()).toBeNull()
      expect(controller.input()).toBe('')
      streams[1]!.handlers.onDone?.({ requestId: 'req-2' })
      streams[1]!.resolve()
      await tick(8)
      await send(controller, 'Og mer')
      await tick()
      expect(streams[2]!.request.executeGranted).toBeUndefined()
    })
  })
})

describe('WS14 S2a: the execute turn is offered only where the build says it can run', () => {
  for (const value of ['false', '', 'yes', '1']) {
    it(`VITE_PLAN_EXECUTE_ENABLED=${JSON.stringify(value)}: an approval arms nothing, prefills nothing and shows no pill`, async () => {
      vi.stubEnv('VITE_PLAN_EXECUTE_ENABLED', value)
      await withController(async (controller) => {
        const planTurnId = await planAndApprove(controller)

        // The approval itself is unchanged: the rung is recorded and plan mode is off.
        expect(controller.state.turns.find((turn) => turn.id === planTurnId)?.grantedRung).toBe('workspace_write')
        expect(controller.planMode()).toBe(false)
        // Byte-identical to before the feature: no pill, no draft, no execute turn.
        expect(controller.approvedPlanView()).toBeNull()
        expect(controller.input()).toBe('')
        controller.selectExecute()
        flush()
        expect(controller.approvedPlanView()).toBeNull()

        await send(controller, 'Kjør den godkjente planen')
        await tick()
        expect(streams[1]!.request.executeGranted).toBeUndefined()
      })
    })
  }

  it('plan_execute_disabled switches the offer off for the rest of the session', async () => {
    await withController(async (controller) => {
      const planTurnId = await planAndApprove(controller)
      await send(controller, controller.input())
      await tick()
      streams[1]!.handlers.onConnected?.({ requestId: 'req-2', threadId: 't-1', runId: 'run-2' })
      flush()
      streams[1]!.handlers.onError?.({ code: 'plan_execute_disabled', message: OPERATOR_ENGLISH, retryable: false })
      streams[1]!.resolve()
      await tick(8)
      expect(controller.approvedPlanView()).toBeNull()

      // The next approval is honoured as an approval, and offers nothing more.
      mockApprovePlan.mockResolvedValue({ grantedRung: 'workspace_write', wasInPlanMode: true, persisted: true })
      await controller.approveTurnPlan(planTurnId, 'workspace_write', 'writes its report', [])
      flush()
      expect(controller.approvedPlanView()).toBeNull()
      expect(controller.input()).toBe('')
    })
  })

  it('plan_grant_unavailable does not: a lapsed grant says nothing about the switch', async () => {
    await withController(async (controller) => {
      const planTurnId = await planAndApprove(controller)
      await send(controller, controller.input())
      await tick()
      streams[1]!.handlers.onConnected?.({ requestId: 'req-2', threadId: 't-1', runId: 'run-2' })
      flush()
      streams[1]!.handlers.onError?.({ code: 'plan_grant_unavailable', message: OPERATOR_ENGLISH, retryable: false })
      streams[1]!.resolve()
      await tick(8)
      expect(controller.approvedPlanView()).toBeNull()

      mockApprovePlan.mockResolvedValue({ grantedRung: 'workspace_write', wasInPlanMode: true, persisted: true })
      await controller.approveTurnPlan(planTurnId, 'workspace_write', 'writes its report', [])
      flush()
      expect(controller.approvedPlanView()).toEqual({ rung: 'workspace_write', selected: true })
    })
  })
})
