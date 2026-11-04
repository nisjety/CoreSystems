// @vitest-environment jsdom

import { cleanup, fireEvent, render } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { buildChatWireBody, type ChatResearchPlanEvent } from '@/shared/api/chat-client'
import { ResearchPlanCard, researchPlanEdit, researchPlanTurnEnabled } from './ResearchPlanCard'

// Without a provider the card renders in its default locale, Norwegian.

afterEach(() => {
  cleanup()
  vi.unstubAllEnvs()
  vi.restoreAllMocks()
})

const plan = (overrides: Partial<ChatResearchPlanEvent> = {}): ChatResearchPlanEvent => ({
  subQueries: ['lakselus 2026', 'havbruk Vestland'],
  questions: ['Hvilken periode?'],
  maxSubQueries: 3,
  ...overrides,
})

function one<T extends Element>(container: HTMLElement, selector: string): T {
  const found = container.querySelector<T>(selector)
  if (!found) throw new Error(`${selector} is not rendered`)
  return found
}

function queryInputs(container: HTMLElement): HTMLInputElement[] {
  return [...container.querySelectorAll<HTMLInputElement>('.verevon-research-plan__query input')]
}

describe('research plan edit', () => {
  it('sends trimmed, non-blank sub-queries up to the cap and only the answered questions', () => {
    expect(
      researchPlanEdit([' a ', '', 'b', 'c', 'd'], ['Q1', 'Q2'], ['  svar ', '   '], 3),
    ).toEqual({ subQueries: ['a', 'b', 'c'], answers: [{ question: 'Q1', answer: 'svar' }] })
    expect(researchPlanEdit(['x'.repeat(250)], [], [], 9).subQueries[0]).toHaveLength(200)
  })

  it('asks for a plan only while VITE_DEEP_RESEARCH_PLAN_TURN is the literal true', () => {
    for (const value of ['', '1', 'TRUE', 'yes']) {
      vi.stubEnv('VITE_DEEP_RESEARCH_PLAN_TURN', value)
      expect(researchPlanTurnEnabled()).toBe(false)
    }
    vi.stubEnv('VITE_DEEP_RESEARCH_PLAN_TURN', 'true')
    expect(researchPlanTurnEnabled()).toBe(true)
  })
})

describe('the research phase on the wire', () => {
  const base = { content: 'Hva skjer med lakselus?', model: 'm', deepResearch: true }

  it('is absent unless the turn asks for a phase, so every other body is unchanged', () => {
    const body = buildChatWireBody(base)
    expect(body).not.toHaveProperty('research_phase')
    expect(body).not.toHaveProperty('research_plan')
  })

  it('names the plan turn, and the run turn with its edited plan', () => {
    expect(buildChatWireBody({ ...base, researchPhase: 'plan' })).toMatchObject({ research_phase: 'plan' })
    expect(buildChatWireBody({ ...base, researchPhase: 'plan' })).not.toHaveProperty('research_plan')
    expect(
      buildChatWireBody({
        ...base,
        researchPhase: 'run',
        researchPlan: { subQueries: [' a ', ''], answers: [{ question: 'Q', answer: ' svar ' }, { question: 'R', answer: '' }] },
      }),
    ).toMatchObject({
      research_phase: 'run',
      research_plan: { sub_queries: ['a'], answers: [{ question: 'Q', answer: 'svar' }] },
    })
  })

  it('is never sent on a zero-retention turn or a turn that is not deep research', () => {
    expect(buildChatWireBody({ ...base, researchPhase: 'plan', zdr: true })).not.toHaveProperty('research_phase')
    expect(buildChatWireBody({ ...base, deepResearch: false, researchPhase: 'plan' })).not.toHaveProperty(
      'research_phase',
    )
  })
})

describe('ResearchPlanCard', () => {
  it('starts with exactly the edited plan', () => {
    const onStart = vi.fn()
    const { container } = render(() => <ResearchPlanCard plan={plan()} onStart={onStart} onCancel={vi.fn()} />)
    const [first] = queryInputs(container)
    fireEvent.input(first as HTMLInputElement, { target: { value: 'lakselus Nordland' } })
    fireEvent.click(one(container, '.verevon-research-plan__remove'))
    // The first row went; the second is now the only one. Re-read after the removal.
    expect(queryInputs(container).map((input) => input.value)).toEqual(['havbruk Vestland'])
    fireEvent.click(one(container, '.verevon-research-plan__add'))
    const added = queryInputs(container)[1] as HTMLInputElement
    fireEvent.input(added, { target: { value: 'rensefisk' } })
    const answer = one<HTMLInputElement>(container, '.verevon-research-plan__question input')
    fireEvent.input(answer, { target: { value: '2025–2026' } })
    fireEvent.click(one(container, '.verevon-research-plan__start'))
    expect(onStart).toHaveBeenCalledWith({
      subQueries: ['havbruk Vestland', 'rensefisk'],
      answers: [{ question: 'Hvilken periode?', answer: '2025–2026' }],
    })
  })

  it('stops adding at the cap', () => {
    const { container } = render(() => (
      <ResearchPlanCard plan={plan({ maxSubQueries: 3 })} onStart={vi.fn()} onCancel={vi.fn()} />
    ))
    const add = one<HTMLButtonElement>(container, '.verevon-research-plan__add')
    fireEvent.click(add)
    expect(queryInputs(container)).toHaveLength(3)
    expect(add.disabled).toBe(true)
    fireEvent.click(add)
    expect(queryInputs(container)).toHaveLength(3)
  })

  it('seeds no more rows than the cap even when the frame carries more', () => {
    const { container } = render(() => (
      <ResearchPlanCard plan={plan({ subQueries: ['a', 'b', 'c', 'd'], maxSubQueries: 2 })} onStart={vi.fn()} onCancel={vi.fn()} />
    ))
    expect(queryInputs(container).map((input) => input.value)).toEqual(['a', 'b'])
  })

  it('offers no Start for an empty plan, and Cancel sends nothing', () => {
    const onStart = vi.fn()
    const onCancel = vi.fn()
    const { container } = render(() => (
      <ResearchPlanCard plan={plan({ subQueries: [], questions: [] })} onStart={onStart} onCancel={onCancel} />
    ))
    expect(one<HTMLButtonElement>(container, '.verevon-research-plan__start').disabled).toBe(true)
    expect(container.querySelector('.verevon-research-plan__questions')).toBeNull()
    fireEvent.click(one(container, '.verevon-research-plan__cancel'))
    expect(onCancel).toHaveBeenCalledTimes(1)
    expect(onStart).not.toHaveBeenCalled()
  })

  it('shows a plan whose run already went out read-only, with no actions', () => {
    const { container } = render(() => (
      <ResearchPlanCard plan={plan()} readOnly onStart={vi.fn()} onCancel={vi.fn()} />
    ))
    expect(queryInputs(container).every((input) => input.readOnly)).toBe(true)
    expect(container.querySelector('.verevon-research-plan__start')).toBeNull()
    expect(container.querySelector('.verevon-research-plan__add')).toBeNull()
    expect(container.querySelector('.verevon-research-plan__remove')).toBeNull()
  })
})
