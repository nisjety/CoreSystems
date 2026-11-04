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
import type { SettledFold } from '../lib/settled-fold'
import type { DockedNodes } from '@/shared/chat-nodes/types'

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

function mount(
  turn: ChatTurn,
  onApprovePlan = vi.fn(),
  onRejectPlan = vi.fn(),
  initialDocked?: DockedNodes,
  settledFold?: SettledFold,
) {
  const [message, setMessage] = createStore<ChatTurn>(turn)
  const [status, setStatus] = createSignal<Status>(undefined)
  const [docked, setDocked] = createSignal<DockedNodes | undefined>(initialDocked)
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
      docked={docked()}
      settledFold={settledFold}
      onViewSteps={() => {}}
    />
  ))
  flush()
  const inputs = () =>
    view.container.querySelectorAll<HTMLInputElement>('.verevon-chat-plan__step-input')
  const reason = () => view.container.querySelector<HTMLTextAreaElement>('.verevon-chat-plan__reason')
  return { ...view, message, setMessage, setStatus, setDocked, inputs, reason, onApprovePlan, onRejectPlan }
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

describe('a plan the composer dock holds', () => {
  const DOCKED: DockedNodes = { approvals: false, plan: true }

  it('leaves a one-line pointer in the transcript instead of a second control', () => {
    const view = mount(planTurn(), vi.fn(), vi.fn(), DOCKED)
    expect(view.container.querySelector('.verevon-chat-plan')).toBeNull()
    expect(view.container.querySelector('.verevon-chat-plan__approve')).toBeNull()
    expect(view.container.querySelector('.verevon-chat-docked-pointer')?.textContent).toMatch(
      /Venter på svar, se over skrivefeltet|Waiting for an answer, see above the composer/,
    )
  })

  it('gives the full card back once the dock lets go of it', () => {
    const view = mount(planTurn(), vi.fn(), vi.fn(), DOCKED)
    view.setDocked({ approvals: false, plan: false })
    flush()
    expect(view.container.querySelector('.verevon-chat-docked-pointer')).toBeNull()
    expect(view.container.querySelector('.verevon-chat-plan')).not.toBeNull()
  })

  it('says a decided plan was decided, not that it is still waiting', () => {
    const view = mount(planTurn({ planRejected: true }))
    expect(view.container.querySelector('.verevon-chat-docked-pointer')).toBeNull()
    expect(view.container.textContent).toMatch(/Planen ble avvist|The plan was rejected/)
  })

  it('does not point away from an approval the dock does not hold', () => {
    const view = mount(planTurn({ pendingApprovals: [{ id: 'a1', kind: 'APPROVAL_KIND_TOOL_CALL' }] }), vi.fn(), vi.fn(), DOCKED)
    // The plan is docked, the approval is not: the approval keeps its card.
    expect(view.container.querySelector('.verevon-chat-approvals')).not.toBeNull()
    expect(view.container.querySelectorAll('.verevon-chat-docked-pointer')).toHaveLength(1)
  })
})

describe('a settled plan turn inside the fold', () => {
  const FOLD: SettledFold = { defaultFolded: true, steps: 1, files: 0, durationMs: 252_000 }
  const RAN_A_TOOL = { toolCalls: [{ id: 'call_1', name: 'web_search', args: { query: 'faktura' } }] }
  const foldBody = (container: HTMLElement) => container.querySelector('.verevon-chat-settled-fold__body')

  it('keeps the plan card outside the folded body, with its approve control', () => {
    const view = mount(planTurn(RAN_A_TOOL), vi.fn(), vi.fn(), undefined, FOLD)
    expect(foldBody(view.container)?.hasAttribute('hidden')).toBe(true)
    expect(view.container.querySelector('.verevon-chat-plan')?.closest('[hidden]')).toBeNull()
    expect(view.container.querySelector('.verevon-chat-plan__approve')?.closest('[hidden]')).toBeNull()
  })

  it('folds the work behind the row, and keeps the answer in view', () => {
    const view = mount(planTurn(RAN_A_TOOL), vi.fn(), vi.fn(), undefined, FOLD)
    expect(foldBody(view.container)?.querySelector('.verevon-chat-steps-pill')).not.toBeNull()
    const answer = Array.from(view.container.querySelectorAll('p')).find(
      (paragraph) => paragraph.textContent === 'Her er planen.',
    )
    expect(answer).toBeDefined()
    expect(answer?.closest('[hidden]')).toBeNull()
  })

  it('keeps a pending approval outside the folded body', () => {
    const view = mount(
      planTurn({ ...RAN_A_TOOL, pendingApprovals: [{ id: 'a1', kind: 'APPROVAL_KIND_TOOL_CALL' }] }),
      vi.fn(),
      vi.fn(),
      undefined,
      FOLD,
    )
    expect(view.container.querySelector('.verevon-chat-approvals')).not.toBeNull()
    expect(view.container.querySelector('.verevon-chat-approvals')?.closest('[hidden]')).toBeNull()
  })

  it('renders no fold when the page passes none, as with the switch off', () => {
    const view = mount(planTurn(RAN_A_TOOL))
    expect(view.container.querySelector('.verevon-chat-settled-fold')).toBeNull()
    expect(view.container.querySelector('.verevon-chat-plan')).not.toBeNull()
  })
})
