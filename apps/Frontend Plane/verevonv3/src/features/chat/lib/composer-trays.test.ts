import { describe, expect, it } from 'vitest'
import type { Approval } from '@/shared/api/orchestration-client'
import type { ThreadGoal } from '@/shared/api/thread-goal-client'
import type { ChatTurn, QueuedInput } from '../components/chat-types'
import { dockedNodesFor, selectDockedTrays } from './composer-trays'

const turn = (overrides: Partial<ChatTurn> = {}): ChatTurn => ({
  id: 'asst-1',
  role: 'assistant',
  content: 'Svar.',
  createdAt: '2026-10-22T08:00:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
  ...overrides,
})

const approval = (id: string): Approval => ({ id, kind: 'APPROVAL_KIND_TOOL_CALL', status: 'PENDING' })

const planTurn = (overrides: Partial<ChatTurn> = {}): ChatTurn =>
  turn({
    planMode: true,
    proposedPlan: { planId: 'plan_1', summary: 'Send the invoice', steps: [{ id: 's1', title: 'Draft it' }] },
    ...overrides,
  })

const queued = (overrides: Partial<QueuedInput> = {}): QueuedInput => ({
  id: 'q1',
  content: 'bruk EUR',
  state: 'pending',
  ...overrides,
})

const select = (turns: ChatTurn[], queuedInputs: QueuedInput[] = [], planApproval?: Parameters<typeof selectDockedTrays>[0]['planApproval']) =>
  selectDockedTrays({ turns, queuedInputs, planApproval })

describe('selectDockedTrays', () => {
  it('holds nothing when nothing is waiting', () => {
    expect(select([turn(), turn({ id: 'user-1', role: 'user' })])).toEqual([])
  })

  it('orders approval, then plan, then queue, whatever order the data arrived in', () => {
    const trays = select(
      [planTurn({ pendingApprovals: [approval('a1')] })],
      [queued()],
    )
    expect(trays.map((tray) => tray.kind)).toEqual(['approval', 'plan', 'queue'])
  })

  it('collects every undecided approval with the turn it is decided against', () => {
    const trays = select([
      turn({ id: 'asst-1', pendingApprovals: [approval('a1')] }),
      turn({ id: 'user-2', role: 'user' }),
      turn({ id: 'asst-2', pendingApprovals: [approval('a2'), approval('a3')] }),
    ])
    expect(trays).toHaveLength(1)
    const [tray] = trays
    expect(tray?.kind === 'approval' && tray.approvals.map((entry) => [entry.turnId, entry.approval.id])).toEqual([
      ['asst-1', 'a1'],
      ['asst-2', 'a2'],
      ['asst-2', 'a3'],
    ])
  })

  it('docks a plan only while it is undecided', () => {
    expect(select([planTurn()]).map((tray) => tray.kind)).toEqual(['plan'])
    expect(select([planTurn({ grantedRung: 'workspace_write' })])).toEqual([])
    expect(select([planTurn({ planRejected: true })])).toEqual([])
  })

  it('docks a plan-mode turn that carries no structured plan, so the run is never stranded', () => {
    const [tray] = select([turn({ planMode: true })])
    expect(tray?.kind).toBe('plan')
    expect(tray?.kind === 'plan' && tray.plan.planId).toBeUndefined()
  })

  it('waits for the plan-mode turn to finish before docking its plan', () => {
    expect(select([planTurn({ streaming: true })])).toEqual([])
  })

  it('docks the plan of the latest assistant turn only', () => {
    const trays = select([
      planTurn({ id: 'asst-old' }),
      turn({ id: 'user-2', role: 'user' }),
      turn({ id: 'asst-new' }),
    ])
    expect(trays).toEqual([])
  })

  it('carries the live approval status of the plan turn', () => {
    const [tray] = select([planTurn()], [], (turnId) =>
      turnId === 'asst-1' ? { pending: true, error: undefined } : undefined,
    )
    expect(tray?.kind === 'plan' && tray.plan.pending).toBe(true)
  })

  it('docks the thread goal after the plan and before the queue (EP-7 S1)', () => {
    const goal: ThreadGoal = {
      id: 'goal_1',
      threadId: 'thread-1',
      objective: 'Ship the report',
      successCriteria: [],
      status: 'active',
      tokensUsed: 0,
      turnsUsed: 0,
    }
    const trays = selectDockedTrays({
      turns: [planTurn({ pendingApprovals: [approval('a1')] })],
      queuedInputs: [queued()],
      goal,
    })
    expect(trays.map((tray) => tray.kind)).toEqual(['approval', 'plan', 'goal', 'queue'])
    expect(selectDockedTrays({ turns: [turn()], queuedInputs: [], goal: null })).toEqual([])
    // The goal holds no control of any turn, so the transcript keeps everything it had.
    expect(dockedNodesFor(trays, 'asst-1')).toEqual({ approvals: true, plan: true })
  })

  it('keeps the queue in every state the strip shows', () => {
    const [tray] = select([turn()], [queued(), queued({ id: 'q2', state: 'delivered' }), queued({ id: 'q3', state: 'refused', note: 'full' })])
    expect(tray?.kind === 'queue' && tray.entries.map((entry) => entry.state)).toEqual(['pending', 'delivered', 'refused'])
  })
})

describe('dockedNodesFor', () => {
  const trays = select(
    [
      turn({ id: 'asst-1', pendingApprovals: [approval('a1')] }),
      turn({ id: 'user-2', role: 'user' }),
      planTurn({ id: 'asst-2' }),
    ],
    [queued()],
  )

  it('says which of a turn\'s controls the dock holds', () => {
    expect(dockedNodesFor(trays, 'asst-1')).toEqual({ approvals: true, plan: false })
    expect(dockedNodesFor(trays, 'asst-2')).toEqual({ approvals: false, plan: true })
  })

  it('docks nothing of a turn the trays do not mention', () => {
    expect(dockedNodesFor(trays, 'user-2')).toEqual({ approvals: false, plan: false })
    expect(dockedNodesFor([], 'asst-1')).toEqual({ approvals: false, plan: false })
  })

  it('docks both when one turn has a pending approval and the latest plan', () => {
    const both = select([planTurn({ pendingApprovals: [approval('a1')] })])
    expect(dockedNodesFor(both, 'asst-1')).toEqual({ approvals: true, plan: true })
  })

  it('returns the same object for the same answer, so a row\'s context does not churn', () => {
    expect(dockedNodesFor(trays, 'user-2')).toBe(dockedNodesFor([], 'other'))
  })
})
