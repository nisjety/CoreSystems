// @vitest-environment jsdom

/**
 * The trays docked above the composer.
 *
 * They hold the controls that used to sit in the transcript, so what matters is
 * that each one still works from here, that only the first tray is shown, and
 * that the dock never pulls focus out of whatever the person is typing in.
 */

import { cleanup, fireEvent, render } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Approval } from '@/shared/api/orchestration-client'
import { __resetPlanDraftsForTests } from './ChatMessages'
import { ComposerTrays } from './ComposerTrays'
import type { DockedTray } from '../lib/composer-trays'

const approval = (id: string): Approval => ({
  id,
  kind: 'APPROVAL_KIND_TOOL_CALL',
  cause: { code: 'provider_write', tool: 'execute_provider_action', scopeOptions: ['once'], stepUpRequired: false },
})

const approvalTray = (): DockedTray => ({
  kind: 'approval',
  approvals: [
    { turnId: 'asst-1', approval: approval('a1') },
    { turnId: 'asst-2', approval: approval('a2') },
  ],
})

const planTray = (overrides: Partial<Extract<DockedTray, { kind: 'plan' }>['plan']> = {}): DockedTray => ({
  kind: 'plan',
  turnId: 'asst-2',
  plan: {
    kind: 'plan-approval',
    planId: 'plan_1',
    summary: 'Send the overdue invoice',
    steps: [{ id: 'step_1', title: 'Draft the email' }],
    pending: false,
    ...overrides,
  },
})

const queueTray = (): DockedTray => ({
  kind: 'queue',
  entries: [{ id: 'q1', content: 'bruk EUR i stedet', state: 'pending' }],
})

function mount(trays: DockedTray[]) {
  const [current, setTrays] = createSignal(trays)
  const handlers = {
    onApprovalDecision: vi.fn(),
    onApprovePlan: vi.fn(),
    onRejectPlan: vi.fn(),
  }
  const view = render(() => <ComposerTrays trays={current()} {...handlers} />)
  flush()
  const region = () => view.container.querySelector<HTMLElement>('.verevon-chat-composer-trays')!
  return { ...view, setTrays, region, ...handlers }
}

afterEach(() => {
  cleanup()
  flush()
  __resetPlanDraftsForTests()
})

describe('ComposerTrays', () => {
  it('is a polite live region that stays mounted, and marks itself empty when nothing waits', () => {
    const view = mount([])
    expect(view.region().getAttribute('role')).toBe('region')
    expect(view.region().getAttribute('aria-live')).toBe('polite')
    expect(view.region().hasAttribute('data-empty')).toBe(true)

    view.setTrays([queueTray()])
    flush()
    expect(view.region().hasAttribute('data-empty')).toBe(false)
    expect(view.region().textContent).toContain('bruk EUR i stedet')
  })

  it('records an approval decision against the turn that asked', () => {
    const view = mount([approvalTray()])
    const approveButtons = view.container.querySelectorAll<HTMLButtonElement>('.verevon-chat-approval__approve')
    expect(approveButtons).toHaveLength(2)
    fireEvent.click(approveButtons[1]!)
    // The trailing scope and promote are the tray's own: an approve with no standing scope chosen passes none.
    expect(view.onApprovalDecision).toHaveBeenCalledWith('asst-2', 'a2', 'approve', undefined, undefined)

    fireEvent.click(view.container.querySelectorAll<HTMLButtonElement>('.verevon-chat-approval__reject')[0]!)
    expect(view.onApprovalDecision).toHaveBeenLastCalledWith('asst-1', 'a1', 'reject', undefined, undefined)
  })

  it('shows only the first tray and names the others under it', () => {
    const view = mount([approvalTray(), planTray(), queueTray()])
    expect(view.container.querySelector('.verevon-chat-approvals')).not.toBeNull()
    expect(view.container.querySelector('.verevon-chat-plan')).toBeNull()
    expect(view.container.querySelector('.verevon-chat-queued')).toBeNull()
    expect(view.container.querySelector('.verevon-chat-composer-trays__more')?.textContent).toMatch(
      /Venter også:\s*plan, meldinger|Also waiting:\s*plan, messages/,
    )
  })

  it('shows the next tray once the first one is decided', () => {
    const view = mount([approvalTray(), planTray()])
    view.setTrays([planTray()])
    flush()
    expect(view.container.querySelector('.verevon-chat-approvals')).toBeNull()
    expect(view.container.querySelector('.verevon-chat-plan')).not.toBeNull()
    expect(view.container.querySelector('.verevon-chat-composer-trays__more')).toBeNull()
  })

  it('approves and rejects a plan against its own turn', () => {
    const view = mount([planTray()])
    const reason = view.container.querySelector<HTMLTextAreaElement>('.verevon-chat-plan__reason')!
    fireEvent.input(reason, { target: { value: 'planen skriver rapporten til arbeidsområdet' } })
    flush()
    fireEvent.click(view.container.querySelector<HTMLButtonElement>('.verevon-chat-plan__approve')!)
    expect(view.onApprovePlan).toHaveBeenCalledTimes(1)
    expect(view.onApprovePlan.mock.calls[0]![0]).toBe('asst-2')
    expect(view.onApprovePlan.mock.calls[0]![1]).toBe('workspace_write')
    expect(view.onApprovePlan.mock.calls[0]![2]).toBe('planen skriver rapporten til arbeidsområdet')

    fireEvent.click(view.container.querySelector<HTMLButtonElement>('.verevon-chat-plan__reject')!)
    expect(view.onRejectPlan).toHaveBeenCalledWith('asst-2', 'planen skriver rapporten til arbeidsområdet')
  })

  it('keeps the plan edits across a status change and starts over for a different plan', () => {
    const view = mount([planTray()])
    const input = () => view.container.querySelector<HTMLInputElement>('.verevon-chat-plan__step-input')!
    fireEvent.input(input(), { target: { value: 'Draft a shorter email' } })
    flush()

    view.setTrays([planTray({ pending: true })])
    flush()
    view.setTrays([planTray({ pending: false, error: 'try again' })])
    flush()
    expect(input().value).toBe('Draft a shorter email')
    expect(view.container.textContent).toContain('try again')

    view.setTrays([
      planTray({ planId: 'plan_2', steps: [{ id: 'step_9', title: 'Something else' }] }),
    ])
    flush()
    expect(input().value).toBe('Something else')
  })

  it('never takes focus from the field the person is typing in', () => {
    const field = document.createElement('textarea')
    document.body.appendChild(field)
    field.focus()
    const view = mount([])
    view.setTrays([approvalTray(), planTray(), queueTray()])
    flush()
    expect(document.activeElement).toBe(field)
    field.remove()
  })

  /** WS14 S4: a question takes the tray behind an approval and ahead of a plan. */
  it('shows a waiting question ahead of a plan, and behind an approval', () => {
    const question = {
      questionId: 'q_1',
      delivery: 'next_turn',
      items: [
        {
          id: 'carrier',
          kind: 'single' as const,
          prompt: 'Which carrier?',
          options: [
            { id: 'bring', label: 'Bring' },
            { id: 'postnord', label: 'PostNord' },
          ],
          allowOther: false,
        },
      ],
    }
    const handlers = { onApprovalDecision: vi.fn(), onApprovePlan: vi.fn(), onRejectPlan: vi.fn() }

    const ahead = render(() => <ComposerTrays trays={[planTray()]} question={question} {...handlers} />)
    flush()
    expect(ahead.container.querySelector('.verevon-question-card')).not.toBeNull()
    cleanup()

    const behind = render(() => <ComposerTrays trays={[approvalTray()]} question={question} {...handlers} />)
    flush()
    expect(behind.container.querySelector('.verevon-question-card')).toBeNull()
    cleanup()
  })
})
