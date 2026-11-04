// @vitest-environment jsdom

/**
 * The thread goal bar (EP-7 S1): its actions follow the goal's status, it never takes focus, and the
 * dialog's limits match the server's.
 */

import { cleanup, fireEvent, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ThreadGoal } from '@/shared/api/thread-goal-client'
import { GoalBar } from './GoalBar'
import { readGoalDraft } from './GoalDialog'

function goal(overrides: Partial<ThreadGoal> = {}): ThreadGoal {
  return {
    id: 'goal_1',
    threadId: 'thread-1',
    objective: 'Ship the quarterly report',
    successCriteria: ['Numbers reconciled', 'Sent to finance'],
    status: 'active',
    tokensUsed: 0,
    turnsUsed: 0,
    revision: 1,
    ...overrides,
  }
}

function mount(current: ThreadGoal, extra: { busy?: boolean; pending?: boolean } = {}) {
  const handlers = {
    onContinue: vi.fn(),
    onEdit: vi.fn(),
    onPause: vi.fn(),
    onResume: vi.fn(),
    onClear: vi.fn(),
  }
  const view = render(() => <GoalBar goal={current} {...extra} {...handlers} />)
  flush()
  const button = (label: RegExp) =>
    [...view.container.querySelectorAll<HTMLButtonElement>('button')].find((candidate) =>
      label.test(candidate.textContent ?? ''),
    )
  return { view, handlers, button }
}

afterEach(() => cleanup())

describe('GoalBar', () => {
  it('offers Continue, Edit, Pause and Clear on an active goal, and each calls its handler', () => {
    const { handlers, button, view } = mount(goal())
    fireEvent.click(button(/Fortsett|Continue/)!)
    fireEvent.click(button(/Rediger|Edit/)!)
    fireEvent.click(button(/^Pause$/)!)
    fireEvent.click(button(/Fjern|Clear/)!)
    expect(handlers.onContinue).toHaveBeenCalledTimes(1)
    expect(handlers.onEdit).toHaveBeenCalledTimes(1)
    expect(handlers.onPause).toHaveBeenCalledTimes(1)
    expect(handlers.onClear).toHaveBeenCalledTimes(1)
    expect(view.container.querySelector('.verevon-chat-goal-bar__status')?.getAttribute('data-status')).toBe('active')
    expect(view.container.textContent).toMatch(/2 (kriterier|criteria)/)
  })

  it('offers Resume and no Continue on a paused goal', () => {
    const { handlers, button } = mount(goal({ status: 'paused' }))
    expect(button(/Fortsett|Continue/)).toBeUndefined()
    fireEvent.click(button(/Gjenoppta|Resume/)!)
    expect(handlers.onResume).toHaveBeenCalledTimes(1)
  })

  it('offers only Clear on a goal that is met or stopped', () => {
    const { button } = mount(goal({ status: 'met' }))
    expect(button(/Fortsett|Continue/)).toBeUndefined()
    expect(button(/Rediger|Edit/)).toBeUndefined()
    expect(button(/Fjern|Clear/)).toBeDefined()
  })

  it('holds Continue while a turn runs', () => {
    const { button } = mount(goal(), { busy: true })
    expect(button(/Fortsett|Continue/)!.disabled).toBe(true)
  })

  it('never takes focus when it appears', () => {
    const field = document.createElement('textarea')
    document.body.append(field)
    field.focus()
    mount(goal())
    expect(document.activeElement).toBe(field)
    field.remove()
  })
})

describe('readGoalDraft', () => {
  it('drops blank criteria and holds the server limits', () => {
    expect(readGoalDraft('  Ship it ', 'one\n\n two \n')).toEqual({
      draft: { objective: 'Ship it', successCriteria: ['one', 'two'] },
      valid: true,
    })
    expect(readGoalDraft('', 'one').valid).toBe(false)
    expect(readGoalDraft('x'.repeat(2001), '').valid).toBe(false)
    expect(readGoalDraft('aim', Array.from({ length: 9 }, (_, i) => `c${i}`).join('\n')).valid).toBe(false)
    expect(readGoalDraft('aim', 'x'.repeat(501)).valid).toBe(false)
  })
})
