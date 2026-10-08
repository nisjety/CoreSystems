import { describe, expect, it } from 'vitest'
import type { Plan } from '@/shared/api/orchestration-client'
import { hydratablePlan } from './chat-plan-hydration'

const plan = (overrides: Partial<Plan> = {}): Plan => ({
  id: 'plan_1',
  runId: 'run_1',
  state: 'PLAN_STATE_PROPOSED',
  summary: 'Send the overdue invoice',
  steps: [
    { id: 'step_1', title: 'Draft the email', state: 'PLAN_STEP_STATE_PENDING' },
    { id: 'step_2', title: 'Send it', operation: 'email_send', state: 'PLAN_STEP_STATE_PENDING' },
  ],
  ...overrides,
})

/** The run's execution log: goes draft -> executing -> terminal, never proposed. */
const executionLog = (state = 'PLAN_STATE_COMPLETED'): Plan =>
  plan({ id: 'plan_run_1', state, summary: 'Send fakturaen', steps: [] })

describe('hydratablePlan', () => {
  it('restores a plan that is still awaiting a decision, with its steps', () => {
    expect(hydratablePlan([executionLog(), plan()])).toEqual({
      kind: 'proposed',
      plan: {
        planId: 'plan_1',
        summary: 'Send the overdue invoice',
        steps: [
          { id: 'step_1', title: 'Draft the email' },
          { id: 'step_2', title: 'Send it', operation: 'email_send' },
        ],
      },
    })
  })

  it('reads the state however the server spells it', () => {
    expect(hydratablePlan([plan({ state: 'proposed' })])?.kind).toBe('proposed')
    expect(hydratablePlan([plan({ state: 'Proposed' })])?.kind).toBe('proposed')
    expect(hydratablePlan([plan({ state: 'PROPOSED' })])?.kind).toBe('proposed')
  })

  it('restores a rejection so the card does not offer to approve a rejected plan again', () => {
    expect(hydratablePlan([executionLog(), plan({ state: 'PLAN_STATE_REJECTED' })])).toEqual({
      kind: 'rejected',
    })
  })

  it('passes over a superseded plan and shows the one that replaced it', () => {
    const first = plan({ id: 'plan_1', state: 'PLAN_STATE_SUPERSEDED' })
    const second = plan({
      id: 'plan_2',
      summary: 'Revised',
      steps: [{ id: 'step_9', title: 'New step' }],
    })
    const restored = hydratablePlan([executionLog(), first, second])
    expect(restored?.kind === 'proposed' ? restored.plan.planId : undefined).toBe('plan_2')
  })

  it('restores nothing for a plan that was already approved or is running', () => {
    // The plan row does not record which rung was granted, so there is nothing
    // truthful to put back — and the older proposal must not resurface either.
    for (const state of ['PLAN_STATE_APPROVED', 'PLAN_STATE_EXECUTING', 'PLAN_STATE_COMPLETED']) {
      expect(hydratablePlan([executionLog(), plan({ state })])).toBeNull()
    }
    expect(
      hydratablePlan([
        plan({ id: 'plan_old', state: 'PLAN_STATE_PROPOSED' }),
        plan({ id: 'plan_new', state: 'PLAN_STATE_APPROVED' }),
      ]),
    ).toBeNull()
  })

  it('never mistakes the run\'s own execution log for a proposal', () => {
    expect(hydratablePlan([executionLog('PLAN_STATE_EXECUTING')])).toBeNull()
    expect(hydratablePlan([executionLog('PLAN_STATE_DRAFT')])).toBeNull()
    expect(hydratablePlan([])).toBeNull()
  })

  it('offers no step the person already struck out, and no plan with nothing left to edit', () => {
    const struck = plan({
      steps: [
        { id: 'step_1', title: 'Draft the email', state: 'PLAN_STEP_STATE_SKIPPED' },
        { id: 'step_2', title: 'Send it', state: 'PLAN_STEP_STATE_PENDING' },
      ],
    })
    const restored = hydratablePlan([struck])
    expect(restored?.kind === 'proposed' ? restored.plan.steps.map((s) => s.id) : []).toEqual([
      'step_2',
    ])
    expect(hydratablePlan([plan({ steps: [] })])).toBeNull()
    expect(
      hydratablePlan([
        plan({ steps: [{ id: 'step_1', title: 'x', state: 'PLAN_STEP_STATE_SKIPPED' }] }),
      ]),
    ).toBeNull()
  })
})
