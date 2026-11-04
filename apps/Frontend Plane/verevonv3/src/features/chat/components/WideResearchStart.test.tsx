// @vitest-environment jsdom

import { cleanup, fireEvent, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/shared/api/api-error'
import type { WideResearchEstimate } from '@/shared/api/chat-client'
import { requestedItemCount, WideResearchStart, wideResearchChoice } from './WideResearchStart'

// Without a provider the card renders in its default locale, Norwegian.

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

const estimate = (overrides: Partial<WideResearchEstimate> = {}): WideResearchEstimate => ({
  itemsRequested: 10,
  itemsAfterCap: 5,
  concurrency: 4,
  roundsPerItem: 2,
  minutesLow: 1,
  minutesHigh: 6,
  usdLow: 0.12,
  usdHigh: 0.4,
  costBasis: 'metered',
  cap: 5,
  capSource: 'org_policy',
  ...overrides,
})

function button(container: HTMLElement, selector: string): HTMLButtonElement {
  const found = container.querySelector<HTMLButtonElement>(selector)
  if (!found) throw new Error(`${selector} is not rendered`)
  return found
}

describe('wide research choice', () => {
  it('sends the pasted items when there are any, else the preset count', () => {
    expect(wideResearchChoice(10, '')).toEqual({ itemCap: 10 })
    expect(wideResearchChoice(10, ' a \n\n b\r\n')).toEqual({ items: ['a', 'b'] })
    expect(requestedItemCount({ items: ['a', 'b', 'c'] })).toBe(3)
    expect(requestedItemCount({ itemCap: 16 })).toBe(16)
  })

  it('bounds a pasted list to 64 items of at most 500 characters', () => {
    const many = Array.from({ length: 80 }, (_, n) => `q${n}`).join('\n')
    expect(wideResearchChoice(5, many).items).toHaveLength(64)
    expect(wideResearchChoice(5, 'x'.repeat(600)).items?.[0]).toHaveLength(500)
  })
})

describe('WideResearchStart', () => {
  it('asks for no estimate until told to, and offers no Start before one', () => {
    const load = vi.fn(async () => estimate())
    const { container } = render(() => <WideResearchStart onStart={vi.fn()} estimate={load} />)
    expect(load).not.toHaveBeenCalled()
    expect(button(container, '.verevon-wide-research__start').disabled).toBe(true)
  })

  it('shows the capped estimate and starts with the pasted items', async () => {
    const load = vi.fn(async () => estimate())
    const onStart = vi.fn()
    const { container, findByTestId } = render(() => (
      <WideResearchStart model="verevon-balance" onStart={onStart} estimate={load} />
    ))
    const list = Array.from({ length: 10 }, (_, n) => `question ${n}`).join('\n')
    fireEvent.input(container.querySelector('textarea') as HTMLTextAreaElement, { target: { value: list } })
    flush()
    fireEvent.click(button(container, '.verevon-wide-research__estimate'))
    expect(load).toHaveBeenCalledWith({ itemCount: 10, model: 'verevon-balance' })
    expect((await findByTestId('wide-research-items')).textContent).toContain('5 spørsmål')
    expect((await findByTestId('wide-research-capped')).textContent).toContain('grensen på 5')
    expect((await findByTestId('wide-research-cost')).textContent).toContain('$0.12')
    const start = button(container, '.verevon-wide-research__start')
    expect(start.disabled).toBe(false)
    fireEvent.click(start)
    expect(onStart).toHaveBeenCalledWith({ items: Array.from({ length: 10 }, (_, n) => `question ${n}`) })
  })

  it('states no cost on a subscription route and none for an unpriced model', async () => {
    const subscription = render(() => (
      <WideResearchStart
        onStart={vi.fn()}
        estimate={async () => estimate({ usdLow: null, usdHigh: null, costBasis: 'subscription' })}
      />
    ))
    fireEvent.click(button(subscription.container, '.verevon-wide-research__estimate'))
    expect((await subscription.findByTestId('wide-research-cost')).textContent).toContain('abonnementet')
    cleanup()

    const unpriced = render(() => (
      <WideResearchStart onStart={vi.fn()} estimate={async () => estimate({ usdLow: null, usdHigh: null })} />
    ))
    fireEvent.click(button(unpriced.container, '.verevon-wide-research__estimate'))
    expect((await unpriced.findByTestId('wide-research-cost')).textContent).toContain('kan ikke anslås')
  })

  it('says wide research is not available when the entry is off, and offers no Start', async () => {
    const off = new ApiError('not found', 404, 'not_found')
    const { container, findByText } = render(() => (
      <WideResearchStart onStart={vi.fn()} estimate={async () => Promise.reject(off)} />
    ))
    fireEvent.click(button(container, '.verevon-wide-research__estimate'))
    expect(await findByText('Bred forskning er ikke tilgjengelig her.')).toBeTruthy()
    expect(button(container, '.verevon-wide-research__start').disabled).toBe(true)
  })

  it('treats a zero-retention refusal like an entry that is off', async () => {
    const zdr = new ApiError('refused', 412, 'zdr_feature_not_supported', { feature: 'agentic' })
    const { findByText } = render(() => (
      <WideResearchStart onStart={vi.fn()} estimate={async () => Promise.reject(zdr)} />
    ))
    fireEvent.click(document.querySelector('.verevon-wide-research__estimate') as HTMLButtonElement)
    expect(await findByText('Bred forskning er ikke tilgjengelig her.')).toBeTruthy()
  })

  it('offers no Start when the organisation has wide research off', async () => {
    const { container, findByText } = render(() => (
      <WideResearchStart onStart={vi.fn()} estimate={async () => estimate({ itemsAfterCap: 0, cap: 0 })} />
    ))
    fireEvent.click(button(container, '.verevon-wide-research__estimate'))
    expect(await findByText('Organisasjonen din har slått av bred forskning.')).toBeTruthy()
    expect(button(container, '.verevon-wide-research__start').disabled).toBe(true)
  })

  it('makes an estimate stale when the choice changes', async () => {
    const { container, findByTestId } = render(() => (
      <WideResearchStart onStart={vi.fn()} estimate={async () => estimate()} />
    ))
    fireEvent.click(button(container, '.verevon-wide-research__estimate'))
    await findByTestId('wide-research-items')
    fireEvent.click(container.querySelectorAll<HTMLButtonElement>('.verevon-wide-research__preset')[2] as HTMLButtonElement)
    flush()
    expect(container.querySelector('[data-testid="wide-research-items"]')).toBeNull()
    expect(button(container, '.verevon-wide-research__start').disabled).toBe(true)
  })
})
