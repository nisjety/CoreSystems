// @vitest-environment jsdom

/**
 * The question card (WS14 S4): what the person can answer, when the answer may go out, what it sends, and
 * that an unanswered or expired question cannot be sent.
 */

import { cleanup, fireEvent, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ChatUserQuestionEvent } from '@/shared/api/chat-client'
import { QuestionCard } from './QuestionCard'

const NOW = Date.parse('2026-11-01T12:00:00Z')

const question = (overrides: Partial<ChatUserQuestionEvent> = {}): ChatUserQuestionEvent => ({
  questionId: 'q_1',
  delivery: 'next_turn',
  items: [
    {
      id: 'carrier',
      kind: 'single',
      prompt: 'Which carrier?',
      options: [
        { id: 'bring', label: 'Bring' },
        { id: 'postnord', label: 'PostNord' },
      ],
      allowOther: true,
    },
    {
      id: 'services',
      kind: 'multi',
      prompt: 'Which services?',
      options: [
        { id: 'cold', label: 'Cold' },
        { id: 'tracked', label: 'Tracked' },
      ],
      allowOther: false,
    },
  ],
  ...overrides,
})

function mount(options: { question?: Partial<ChatUserQuestionEvent>; pending?: boolean } = {}) {
  const onAnswer = vi.fn()
  const onSkip = vi.fn()
  const view = render(() => (
    <QuestionCard
      question={question(options.question)}
      pending={options.pending}
      now={() => NOW}
      onAnswer={onAnswer}
      onSkip={onSkip}
    />
  ))
  flush()
  const input = (selector: string) => view.container.querySelector<HTMLInputElement>(selector)!
  const buttons = () => Array.from(view.container.querySelectorAll<HTMLButtonElement>('button'))
  return {
    ...view,
    onAnswer,
    onSkip,
    radio: (value: string) => input(`input[type="radio"][value="${value}"]`),
    checkbox: (value: string) => input(`input[type="checkbox"][value="${value}"]`),
    otherText: () => input('input[type="text"]'),
    skip: () => buttons()[0]!,
    send: () => buttons()[1]!,
  }
}

afterEach(() => {
  cleanup()
  flush()
})

describe('QuestionCard', () => {
  it('shows each item with its choices, and says it asks for no credentials', () => {
    const view = mount()

    expect(view.container.textContent).toContain('Which carrier?')
    expect(view.container.textContent).toContain('PostNord')
    expect(view.container.textContent).toContain('Which services?')
    expect(view.container.textContent).toMatch(/passord|password/i)
  })

  it('keeps the send button shut until every item has an answer', () => {
    const view = mount()
    expect(view.send().disabled).toBe(true)

    fireEvent.click(view.radio('bring'))
    flush()
    expect(view.send().disabled).toBe(true)

    fireEvent.click(view.checkbox('cold'))
    flush()
    expect(view.send().disabled).toBe(false)
  })

  it('sends one answer per item, and the person\'s own words replace a picked choice', () => {
    const view = mount()
    fireEvent.click(view.radio('bring'))
    fireEvent.input(view.otherText(), { target: { value: 'Posten' } })
    fireEvent.click(view.checkbox('cold'))
    flush()

    fireEvent.click(view.send())

    expect(view.onAnswer).toHaveBeenCalledWith({
      answers: [
        { itemId: 'carrier', optionIds: [], text: 'Posten' },
        { itemId: 'services', optionIds: ['cold'], text: '' },
      ],
    })
  })

  it('a multiple-choice item takes several choices, and choosing one again clears it', () => {
    const view = mount()
    fireEvent.click(view.radio('postnord'))
    // The handler decides from the choices already held, so each change is one toggle of that choice.
    fireEvent.change(view.checkbox('cold'))
    fireEvent.change(view.checkbox('tracked'))
    fireEvent.change(view.checkbox('cold'))
    flush()

    fireEvent.click(view.send())

    expect(view.onAnswer).toHaveBeenCalledWith({
      answers: [
        { itemId: 'carrier', optionIds: ['postnord'], text: '' },
        { itemId: 'services', optionIds: ['tracked'], text: '' },
      ],
    })
  })

  it('skip sends the skip and never an answer', () => {
    const view = mount()
    fireEvent.click(view.skip())

    expect(view.onSkip).toHaveBeenCalledTimes(1)
    expect(view.onAnswer).not.toHaveBeenCalled()
  })

  it('shows the countdown only inside the final hour before the question expires', () => {
    const inside = mount({ question: { expiresAt: new Date(NOW + 30 * 60 * 1000).toISOString() } })
    expect(inside.container.querySelector('[role="timer"]')?.textContent).toMatch(/30:00/)
    inside.unmount()

    const outside = mount({ question: { expiresAt: new Date(NOW + 2 * 60 * 60 * 1000).toISOString() } })
    expect(outside.container.querySelector('[role="timer"]')).toBeNull()
  })

  it('an expired question is locked: nothing can be sent or skipped', () => {
    const view = mount({ question: { expiresAt: new Date(NOW - 60 * 1000).toISOString() } })
    fireEvent.click(view.radio('bring'))
    fireEvent.click(view.checkbox('cold'))
    flush()

    expect(view.send().disabled).toBe(true)
    expect(view.skip().disabled).toBe(true)
    expect(view.container.querySelector('[role="status"]')).not.toBeNull()
  })

  it('a pending answer is locked until the send settles', () => {
    const view = mount({ pending: true })
    fireEvent.click(view.radio('bring'))
    fireEvent.click(view.checkbox('cold'))
    flush()

    expect(view.send().disabled).toBe(true)
    expect(view.skip().disabled).toBe(true)
  })
})
