// @vitest-environment jsdom

/**
 * plan-gate: the plan-approval control as it actually lives inside a message.
 *
 * `PlanApprovalControl` is unit-tested on its own in ChatPanels.test.tsx. What
 * those tests cannot see is the control's LIFETIME: `AssistantMessage` derives
 * the turn's nodes as a fresh list whenever anything they read changes, and
 * `<For>` keys that list by object reference, so a status change (Approve
 * pressed -> `pending`, approval refused -> `error`) tears the control down and
 * builds it again. These tests render the real message and change the status
 * the way the controller does.
 */

import { cleanup, fireEvent, render } from '@solidjs/testing-library'
import { createSignal, createStore, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { __resetPlanDraftsForTests, AssistantMessage } from './ChatMessages'
import type { ChatTurn } from './chat-types'

const planTurn = (overrides: Partial<ChatTurn> = {}): ChatTurn => ({
  id: 'asst-1',
  role: 'assistant',
  content: 'Her er planen.',
  createdAt: '2026-09-28T08:00:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
  planMode: true,
  runId: 'run_1',
  proposedPlan: {
    planId: 'plan_1',
    summary: 'Send the overdue invoice',
    steps: [
      { id: 'step_1', title: 'Draft the email' },
      { id: 'step_2', title: 'Send it', operation: 'email_send' },
    ],
  },
  ...overrides,
})

type Status = { pending: boolean; error?: string } | undefined

function mount(turn: ChatTurn, onApprovePlan = vi.fn(), onRejectPlan = vi.fn()) {
  const [message, setMessage] = createStore<ChatTurn>(turn)
  const [status, setStatus] = createSignal<Status>(undefined)
  const view = render(() => (
    <AssistantMessage
      copied={false}
      message={message}
      onBranch={() => {}}
      onCopy={() => {}}
      onFeedback={() => Promise.resolve(true)}
      onRegenerate={() => {}}
      onApprovalDecision={() => {}}
      onApprovePlan={onApprovePlan}
      onRejectPlan={onRejectPlan}
      planApproval={status()}
      onViewSteps={() => {}}
    />
  ))
  flush()
  const inputs = () =>
    view.container.querySelectorAll<HTMLInputElement>('.verevon-chat-plan__step-input')
  const reason = () => view.container.querySelector<HTMLTextAreaElement>('.verevon-chat-plan__reason')
  return { ...view, message, setMessage, setStatus, inputs, reason, onApprovePlan, onRejectPlan }
}

afterEach(() => {
  // Unmount first: tearing a control down is what stores its draft.
  cleanup()
  flush()
  __resetPlanDraftsForTests()
})

describe('PlanApprovalControl inside AssistantMessage', () => {
  it('keeps the person\'s step edits and justification when a refused approval rebuilds the control', () => {
    const view = mount(planTurn())
    expect(view.inputs()).toHaveLength(2)
    fireEvent.input(view.inputs()[0]!, { target: { value: 'Draft a shorter email' } })
    fireEvent.input(view.reason()!, { target: { value: 'planen skriver rapporten til arbeidsområdet' } })
    flush()

    // The controller's sequence for a refused approval: pending, then error.
    view.setStatus({ pending: true })
    flush()
    view.setStatus({ pending: false, error: 'a plan cannot be approved with every step removed' })
    flush()

    expect(view.container.textContent).toContain('cannot be approved')
    expect(view.inputs()[0]!.value).toBe('Draft a shorter email')
    expect(view.reason()!.value).toBe('planen skriver rapporten til arbeidsområdet')
  })

  it('keeps a reorder and a removal across the rebuild, and sends them on the retry', () => {
    const view = mount(planTurn())
    const rows = view.container.querySelectorAll('.verevon-chat-plan__step')
    fireEvent.click(rows[1]!.querySelector<HTMLButtonElement>('button[aria-label="Flytt opp"]')!)
    flush()
    fireEvent.click(view.container.querySelectorAll<HTMLButtonElement>('.verevon-chat-plan__step-remove')[1]!)
    flush()
    fireEvent.input(view.reason()!, { target: { value: 'planen skriver rapporten til arbeidsområdet' } })
    flush()

    view.setStatus({ pending: true })
    flush()
    view.setStatus({ pending: false, error: 'try again' })
    flush()

    expect(Array.from(view.inputs()).map((input) => input.value)).toEqual(['Send it'])
    fireEvent.click(view.container.querySelector<HTMLButtonElement>('.verevon-chat-plan__approve')!)
    flush()
    expect(view.onApprovePlan).toHaveBeenCalledTimes(1)
    expect(view.onApprovePlan.mock.calls[0]![2]).toEqual([
      { stepId: 'step_2', title: 'Send it', operation: 'email_send', removed: false },
      { stepId: 'step_1', title: 'Draft the email', operation: undefined, removed: true },
    ])
  })

  it('never hands one plan\'s edits to a different plan', () => {
    const first = mount(planTurn({ id: 'asst-a' }))
    fireEvent.input(first.inputs()[0]!, { target: { value: 'Edited in plan_1' } })
    flush()
    first.unmount()
    flush()

    const other = mount(
      planTurn({
        id: 'asst-b',
        proposedPlan: {
          planId: 'plan_2',
          summary: 'A different plan',
          steps: [{ id: 'step_9', title: 'Something else' }],
        },
      }),
    )
    expect(Array.from(other.inputs()).map((input) => input.value)).toEqual(['Something else'])
  })

  it('says a rejected plan was rejected instead of offering to approve it again', () => {
    const view = mount(planTurn({ planRejected: true }))
    expect(view.container.querySelector('.verevon-chat-plan')).toBeNull()
    expect(view.container.querySelector('.verevon-chat-plan__approve')).toBeNull()
    expect(view.container.textContent).toMatch(/Planen ble avvist|The plan was rejected/)
  })

  it('shows the control again when a plan-mode turn has no rejection', () => {
    const view = mount(planTurn())
    expect(view.container.querySelector('.verevon-chat-plan')).not.toBeNull()
    expect(view.container.textContent).not.toMatch(/Planen ble avvist/)
  })
})
