// @vitest-environment jsdom

/**
 * W3-EP-7-S3-c: the judge's verdict chip on the goal bar, its reasons and the "the judge can be wrong" note
 * on expand, and Mark done / Reopen by status. Solid 2: flush before every DOM assertion.
 */

import { cleanup, fireEvent, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ThreadGoal } from '@/shared/api/thread-goal-client'
import { GoalBar, goalVerdict } from './GoalBar'

function goal(overrides: Partial<ThreadGoal> = {}): ThreadGoal {
  return {
    id: 'goal_1',
    threadId: 'thread-1',
    objective: 'Ship the quarterly report',
    successCriteria: ['Numbers reconciled'],
    status: 'active',
    tokensUsed: 0,
    turnsUsed: 0,
    revision: 1,
    ...overrides,
  }
}

function mount(current: ThreadGoal) {
  const handlers = {
    onContinue: vi.fn(),
    onEdit: vi.fn(),
    onPause: vi.fn(),
    onResume: vi.fn(),
    onClear: vi.fn(),
    onMarkDone: vi.fn(),
    onReopen: vi.fn(),
  }
  const view = render(() => <GoalBar goal={current} {...handlers} />)
  flush()
  const button = (label: RegExp) =>
    [...view.container.querySelectorAll<HTMLButtonElement>('button')].find((candidate) =>
      label.test(candidate.textContent ?? ''),
    )
  const chip = () => view.container.querySelector<HTMLButtonElement>('.verevon-chat-goal-bar__verdict')
  return { view, handlers, button, chip }
}

afterEach(() => cleanup())

describe('goalVerdict', () => {
  it('names only the closed set', () => {
    expect(goalVerdict('met')).toBe('met')
    expect(goalVerdict('not_met')).toBe('not_met')
    expect(goalVerdict('blocked')).toBe('blocked')
    expect(goalVerdict('unsure')).toBe('unsure')
    expect(goalVerdict('MET')).toBeNull()
    expect(goalVerdict('passed')).toBeNull()
    expect(goalVerdict('')).toBeNull()
    expect(goalVerdict(undefined)).toBeNull()
  })
})

describe('GoalBar verdict chip', () => {
  it('shows no chip when the goal carries no verdict, or one outside the closed set', () => {
    expect(mount(goal()).chip()).toBeNull()
    cleanup()
    expect(mount(goal({ lastVerdict: 'passed' })).chip()).toBeNull()
  })

  it.each([
    ['met', /Mål nådd|Goal met/],
    ['not_met', /Ikke nådd|Not met/],
    ['blocked', /Blokkert|Blocked/],
    ['unsure', /Usikker|Unsure/],
  ])('labels the %s verdict', (verdict, label) => {
    const { chip } = mount(goal({ lastVerdict: verdict }))
    expect(chip()?.getAttribute('data-verdict')).toBe(verdict)
    expect(chip()?.textContent).toMatch(label)
  })

  it('opens the reasons and the note that the judge can be wrong, and closes them again', () => {
    const { view, chip } = mount(
      goal({ lastVerdict: 'not_met', lastVerdictReasons: ['The numbers are not reconciled', 'Nothing was sent'] }),
    )
    expect(chip()?.getAttribute('aria-expanded')).toBe('false')
    expect(view.container.querySelector('.verevon-chat-goal-bar__verdict-detail')).toBeNull()

    fireEvent.click(chip()!)
    flush()
    expect(chip()?.getAttribute('aria-expanded')).toBe('true')
    const reasons = [...view.container.querySelectorAll('.verevon-chat-goal-bar__verdict-reasons li')].map(
      (item) => item.textContent,
    )
    expect(reasons).toEqual(['The numbers are not reconciled', 'Nothing was sent'])
    expect(view.container.querySelector('.verevon-chat-goal-bar__verdict-note')?.textContent).toMatch(
      /modell og kan ta feil|model and can be wrong/,
    )

    fireEvent.click(chip()!)
    flush()
    expect(view.container.querySelector('.verevon-chat-goal-bar__verdict-detail')).toBeNull()
  })

  it('shows the note without a reasons list when the verdict came with no reasons', () => {
    const { view, chip } = mount(goal({ lastVerdict: 'unsure' }))
    fireEvent.click(chip()!)
    flush()
    expect(view.container.querySelector('.verevon-chat-goal-bar__verdict-reasons')).toBeNull()
    expect(view.container.querySelector('.verevon-chat-goal-bar__verdict-note')).not.toBeNull()
  })
})

describe('GoalBar Mark done and Reopen', () => {
  it.each(['active', 'paused'] as const)('offers Mark done and no Reopen on a %s goal', (status) => {
    const { button, handlers } = mount(goal({ status }))
    expect(button(/Gjenåpne|Reopen/)).toBeUndefined()
    fireEvent.click(button(/Marker som ferdig|Mark done/)!)
    expect(handlers.onMarkDone).toHaveBeenCalledTimes(1)
    expect(handlers.onReopen).not.toHaveBeenCalled()
  })

  it.each(['met', 'blocked'] as const)('offers Reopen and no Mark done on a %s goal', (status) => {
    const { button, handlers } = mount(goal({ status, lastVerdict: status === 'met' ? 'met' : 'blocked' }))
    expect(button(/Marker som ferdig|Mark done/)).toBeUndefined()
    fireEvent.click(button(/Gjenåpne|Reopen/)!)
    expect(handlers.onReopen).toHaveBeenCalledTimes(1)
    expect(handlers.onMarkDone).not.toHaveBeenCalled()
  })

  it.each(['budget_reached', 'turns_reached'] as const)('offers neither on a %s goal', (status) => {
    const { button } = mount(goal({ status }))
    expect(button(/Marker som ferdig|Mark done/)).toBeUndefined()
    expect(button(/Gjenåpne|Reopen/)).toBeUndefined()
  })
})
