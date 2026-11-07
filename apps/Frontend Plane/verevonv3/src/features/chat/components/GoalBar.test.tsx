// @vitest-environment jsdom

/**
 * The thread goal bar (EP-7 S1): its actions follow the goal's status, it never takes focus, and the
 * dialog's limits match the server's. EP-7 S4 adds Keep going, its countdown and Stop.
 */

import { cleanup, fireEvent, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ThreadGoal } from '@/shared/api/thread-goal-client'
import type { GoalAutoContinueView } from '../lib/goal-pump'
import {
  GoalBar,
  applyGoalProgress,
  compactTokens,
  goalAwayCron,
  goalAwayRuns,
  goalScheduleTemplate,
  goalUsageLine,
  type GoalAwayView,
} from './GoalBar'
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

describe('GoalBar usage line (EP-7 S2)', () => {
  it('shows tokens against the budget and the turn against the cap', () => {
    const { view } = mount(goal({ tokensUsed: 41_000, tokenBudget: 200_000, turnsUsed: 3, maxTurns: 10 }))
    expect(view.container.querySelector('.verevon-chat-goal-bar__usage')?.textContent).toMatch(
      /41k (av|of) 200k tokens - (runde|turn) 3 (av|of) 10/,
    )
  })

  it('shows only the turn cap when the goal sets no token budget (the subscription route)', () => {
    expect(goalUsageLine(goal({ maxTurns: 10, turnsUsed: 2 }))?.en).toBe('turn 2 of 10')
    expect(goalUsageLine(goal())).toBeNull()
  })

  it('names why a goal stopped on its own', () => {
    const { view, button } = mount(goal({ status: 'budget_reached', statusReason: 'token_budget' }))
    expect(view.container.querySelector('.verevon-chat-goal-bar__stop-reason')?.textContent).toMatch(
      /Tokenbudsjettet er brukt opp|Token budget spent/,
    )
    expect(button(/Fortsett|Continue/)).toBeUndefined()
  })

  it('compacts token counts', () => {
    expect(compactTokens(950)).toBe('950')
    expect(compactTokens(41_000)).toBe('41k')
    expect(compactTokens(1_500_000)).toBe('1.5M')
  })
})

describe('applyGoalProgress', () => {
  it('takes the frame counts and status for the same goal only', () => {
    const shown = goal({ maxTurns: 10 })
    const next = applyGoalProgress(shown, {
      goalId: 'goal_1',
      turnNo: 4,
      tokensUsed: 31_000,
      tokenBudget: 30_000,
      costUsedUsdMicros: 0,
      turnsUsed: 4,
      maxTurns: 10,
      status: 'budget_reached',
      statusReason: 'token_budget',
    })
    expect(next).toMatchObject({ status: 'budget_reached', statusReason: 'token_budget', tokensUsed: 31_000, turnsUsed: 4 })
    expect(next?.objective).toBe(shown.objective)
    const other = applyGoalProgress(shown, { ...progressOf('goal_2'), status: 'met' })
    expect(other).toBe(shown)
    expect(applyGoalProgress(shown, { ...progressOf('goal_1'), status: 'cleared' })).toBeNull()
  })
})

function progressOf(goalId: string) {
  return {
    goalId,
    turnNo: 1,
    tokensUsed: 0,
    costUsedUsdMicros: 0,
    turnsUsed: 1,
    maxTurns: 10,
    status: 'active' as const,
  }
}

describe('GoalBar Keep going (EP-7 S4)', () => {
  function pump(overrides: Partial<GoalAutoContinueView> = {}): GoalAutoContinueView {
    return {
      available: () => true,
      on: () => false,
      countdown: () => null,
      stopReason: () => null,
      setOn: vi.fn(),
      stop: vi.fn(),
      ...overrides,
    }
  }

  function mountAuto(current: ThreadGoal, autoContinue: GoalAutoContinueView) {
    const view = render(() => (
      <GoalBar
        goal={current}
        autoContinue={autoContinue}
        onContinue={vi.fn()}
        onEdit={vi.fn()}
        onPause={vi.fn()}
        onResume={vi.fn()}
        onClear={vi.fn()}
      />
    ))
    flush()
    const toggle = () => view.container.querySelector<HTMLButtonElement>('[role="switch"]')
    const stopButton = () => view.container.querySelector<HTMLButtonElement>('.verevon-chat-goal-bar__stop')
    return { view, toggle, stopButton }
  }

  it('hides the toggle unless the server reports it available', () => {
    expect(mountAuto(goal(), pump({ available: () => false })).toggle()).toBeNull()
    cleanup()
    // A paused or closed goal has nothing to keep going on.
    expect(mountAuto(goal({ status: 'paused' }), pump()).toggle()).toBeNull()
    cleanup()
    expect(mountAuto(goal({ status: 'met' }), pump()).toggle()).toBeNull()
  })

  it('turns Keep going on and off as a switch', () => {
    const off = pump()
    const { toggle } = mountAuto(goal(), off)
    expect(toggle()?.getAttribute('aria-checked')).toBe('false')
    fireEvent.click(toggle()!)
    expect(off.setOn).toHaveBeenCalledWith(true)
    cleanup()
    const on = pump({ on: () => true })
    const second = mountAuto(goal(), on)
    expect(second.toggle()?.getAttribute('aria-checked')).toBe('true')
    fireEvent.click(second.toggle()!)
    expect(on.setOn).toHaveBeenCalledWith(false)
  })

  it('shows the countdown with Stop, and Stop cancels it', () => {
    const counting = pump({ on: () => true, countdown: () => 5 })
    const { view, stopButton } = mountAuto(goal(), counting)
    const timer = view.container.querySelector('[role="timer"]')
    // Plain text in a timer role: not announced on every tick, and nothing moves.
    expect(timer?.textContent).toMatch(/Fortsetter om 5 s|Continuing in 5 s/)
    expect(timer?.getAttribute('aria-live')).toBeNull()
    fireEvent.click(stopButton()!)
    expect(counting.stop).toHaveBeenCalledTimes(1)
  })

  it('offers no Stop when nothing counts down, and names why the pump stopped', () => {
    const stopped = pump({ stopReason: () => 'pending_approval' })
    const { view, stopButton } = mountAuto(goal(), stopped)
    expect(stopButton()).toBeNull()
    const reason = view.container.querySelector('.verevon-chat-goal-bar__auto-stopped')
    expect(reason?.getAttribute('data-reason')).toBe('pending_approval')
    expect(reason?.textContent).toMatch(/godkjenning|approval/)
  })

  it('never takes focus when the countdown starts', () => {
    const field = document.createElement('textarea')
    document.body.append(field)
    field.focus()
    mountAuto(goal(), pump({ on: () => true, countdown: () => 5 }))
    expect(document.activeElement).toBe(field)
    field.remove()
  })
})

describe('readGoalDraft', () => {
  it('reads the caps and refuses one out of range', () => {
    expect(readGoalDraft('aim', '', { maxTurns: '20', tokenBudget: '30000' })).toEqual({
      draft: { objective: 'aim', successCriteria: [], maxTurns: 20, tokenBudget: 30_000 },
      valid: true,
    })
    expect(readGoalDraft('aim', '', { maxTurns: '', tokenBudget: '' }).draft).toEqual({
      objective: 'aim',
      successCriteria: [],
    })
    expect(readGoalDraft('aim', '', { maxTurns: '51', tokenBudget: '' }).valid).toBe(false)
    expect(readGoalDraft('aim', '', { maxTurns: '0', tokenBudget: '' }).valid).toBe(false)
    expect(readGoalDraft('aim', '', { maxTurns: '', tokenBudget: '5000001' }).valid).toBe(false)
    expect(readGoalDraft('aim', '', { maxTurns: '2.5', tokenBudget: '' }).valid).toBe(false)
  })

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

describe('Keep working while away (EP-7 S6)', () => {
  const tool = { tool: 'web_search', capability: 'cap.tool.web_search' }
  const now = new Date('2026-11-04T10:00:00.000Z')

  function awayView(overrides: Partial<GoalAwayView> = {}): GoalAwayView {
    return {
      available: () => true,
      tools: () => [tool],
      scheduled: () => false,
      runs: () => [],
      threadHref: (threadId) => `/chat/${threadId}`,
      onStart: vi.fn(),
      ...overrides,
    }
  }

  function mountAway(current: ThreadGoal, away: GoalAwayView) {
    const view = render(() => (
      <GoalBar
        goal={current}
        away={away}
        onContinue={vi.fn()}
        onEdit={vi.fn()}
        onPause={vi.fn()}
        onResume={vi.fn()}
        onClear={vi.fn()}
      />
    ))
    flush()
    const toggle = () => view.container.querySelector<HTMLButtonElement>('.verevon-chat-goal-bar__away-toggle')
    const start = () => view.container.querySelector<HTMLButtonElement>('.verevon-chat-goal-bar__away-start')
    return { view, toggle, start }
  }

  it('builds a read-only goal-bound template within the limits, or none', () => {
    const form = { tools: [tool], maxRuns: 3, everyMinutes: 60, days: 7 }
    const template = goalScheduleTemplate(goal(), form, now)
    expect(template).toMatchObject({
      kind: 'cron',
      description: 'Ship the quarterly report',
      purpose: 'goal',
      goal_id: 'goal_1',
      goal_max_runs: 3,
    })
    const contract = JSON.parse(template!.approval_contract)
    expect(contract.tools).toEqual([{ ...tool, class: 'read', approved_at: '2026-11-04T10:00:00Z' }])
    expect(contract.expires_at).toBe('2026-11-11T10:00:00Z')
    expect(goalScheduleTemplate(goal(), { ...form, tools: [] }, now)).toBeNull()
    expect(goalScheduleTemplate(goal(), { ...form, maxRuns: 21 }, now)).toBeNull()
    expect(goalScheduleTemplate(goal(), { ...form, maxRuns: 0 }, now)).toBeNull()
    expect(goalScheduleTemplate(goal(), { ...form, everyMinutes: 5 }, now)).toBeNull()
    expect(goalScheduleTemplate(goal(), { ...form, days: 91 }, now)).toBeNull()
    expect(goalScheduleTemplate(goal({ status: 'paused' }), form, now)).toBeNull()
    expect(goalAwayCron(30)).toBe('*/30 * * * *')
    expect(goalAwayCron(15)).toBeNull()
  })

  it('reads only finished fires that reached a scheduled thread', () => {
    expect(
      goalAwayRuns([
        {
          id: 'f1',
          fired_at: '2026-11-04T08:00:00Z',
          completed_at: '2026-11-04T08:01:00Z',
          status: 'completed',
          outcome: 'completed',
          run_id: 'run-1',
          thread_id: 'sched-1',
          withheld_effects: 2,
        },
        { id: 'f2', fired_at: '2026-11-04T09:00:00Z', status: 'running', withheld_effects: 0 },
      ]),
    ).toEqual([{ runId: 'run-1', threadId: 'sched-1', outcome: 'completed', withheldEffects: 2 }])
  })

  it('stays hidden unless available, and offers no new schedule while one is set', () => {
    expect(mountAway(goal(), awayView({ available: () => false })).toggle()).toBeNull()
    cleanup()
    expect(mountAway(goal(), awayView({ scheduled: () => true })).toggle()).toBeNull()
    cleanup()
    expect(mountAway(goal({ status: 'paused' }), awayView()).toggle()).toBeNull()
  })

  it('starts only once a read tool is picked, and says it runs read only', () => {
    const away = awayView()
    const { view, toggle, start } = mountAway(goal(), away)
    fireEvent.click(toggle()!)
    expect(view.container.textContent).toMatch(/Kun lesing|Read only/)
    expect(start()!.disabled).toBe(true)
    fireEvent.change(view.container.querySelector<HTMLInputElement>('input[type="checkbox"]')!)
    expect(start()!.disabled).toBe(false)
    fireEvent.click(start()!)
    expect(away.onStart).toHaveBeenCalledWith({ tools: [tool], maxRuns: 3, everyMinutes: 60, days: 7 })
  })

  it('cannot start without a delegable tool', () => {
    const { view, toggle, start } = mountAway(goal(), awayView({ tools: () => [] }))
    fireEvent.click(toggle()!)
    expect(view.container.querySelector('.verevon-chat-goal-bar__away-none')).not.toBeNull()
    expect(start()!.disabled).toBe(true)
  })

  it('lists the runs made while away with links to their scheduled threads', () => {
    const { view } = mountAway(
      goal({ status: 'met' }),
      awayView({ runs: () => [{ runId: 'run-1', threadId: 'sched-1', outcome: 'completed', withheldEffects: 1 }] }),
    )
    expect(view.container.textContent).toMatch(/1 (kjøring mens du var borte|run while you were away)/)
    expect(view.container.querySelector('a')?.getAttribute('href')).toBe('/chat/sched-1')
    expect(view.container.querySelector('.verevon-chat-goal-bar__away-drafts')).not.toBeNull()
  })
})
