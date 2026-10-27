import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { IdentityCardModel } from '../lib/agent-activity'
import { SpaceIdentityHoverCard } from './SpaceIdentityHoverCard'

// No I18nProvider wraps these renders, so useI18n() falls back to Norwegian (the default locale).

const agentCard = (overrides: Partial<Extract<IdentityCardModel, { kind: 'agent' }>> = {}): IdentityCardModel => ({
  kind: 'agent',
  name: 'Lagerhjelpen',
  lead: { kind: 'none' },
  ...overrides,
})

const memberCard = (overrides: Partial<Extract<IdentityCardModel, { kind: 'member' }>> = {}): IdentityCardModel => ({
  kind: 'member',
  name: 'Kari Nordmann',
  ...overrides,
})

function renderCard(model: IdentityCardModel) {
  render(() => (
    <SpaceIdentityHoverCard model={model}>
      <span>{model.name}</span>
    </SpaceIdentityHoverCard>
  ))
  return screen.getByText(model.name).closest('.verevon-identity-trigger') as HTMLElement
}

const card = () => screen.queryByRole('tooltip')

async function settle(ms: number) {
  await vi.advanceTimersByTimeAsync(ms)
  flush()
}

beforeEach(() => {
  vi.useFakeTimers()
})

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('SpaceIdentityHoverCard open and close (WS07-S8)', () => {
  it('the wrapped name stays plain text in a focusable trigger and the card starts closed', () => {
    const trigger = renderCard(agentCard())
    expect(trigger.textContent).toBe('Lagerhjelpen')
    expect(trigger.getAttribute('tabindex')).toBe('0')
    expect(trigger.hasAttribute('aria-describedby')).toBe(false)
    expect(card()).toBeNull()
  })

  it('opens only after the hover delay, not on the first instant', async () => {
    const trigger = renderCard(agentCard())
    fireEvent.mouseEnter(trigger)
    await settle(300)
    expect(card()).toBeNull()
    await settle(100)
    expect(card()).toBeTruthy()
    expect(trigger.getAttribute('aria-describedby')).toBe(card()?.id)
  })

  it('a pointer that leaves before the delay never opens it', async () => {
    const trigger = renderCard(agentCard())
    fireEvent.mouseEnter(trigger)
    await settle(200)
    fireEvent.mouseLeave(trigger)
    await settle(1000)
    expect(card()).toBeNull()
  })

  it('opens immediately on keyboard focus', () => {
    const trigger = renderCard(agentCard())
    fireEvent.focus(trigger)
    flush()
    expect(card()).toBeTruthy()
  })

  it('Escape closes it', () => {
    const trigger = renderCard(agentCard())
    fireEvent.focus(trigger)
    flush()
    expect(card()).toBeTruthy()
    fireEvent.keyDown(document, { key: 'Escape' })
    flush()
    expect(card()).toBeNull()
  })

  it('another key does not close it', () => {
    const trigger = renderCard(agentCard())
    fireEvent.focus(trigger)
    flush()
    fireEvent.keyDown(document, { key: 'a' })
    flush()
    expect(card()).toBeTruthy()
  })

  it('blur closes it', () => {
    const trigger = renderCard(agentCard())
    fireEvent.focus(trigger)
    flush()
    fireEvent.blur(trigger)
    flush()
    expect(card()).toBeNull()
  })

  it('leaving the trigger closes it after a short grace, and entering the card itself keeps it open', async () => {
    const trigger = renderCard(agentCard())
    fireEvent.focus(trigger)
    flush()
    fireEvent.mouseLeave(trigger)
    await settle(60)
    // The pointer moved onto the card: the pending close is cancelled.
    fireEvent.mouseEnter(card() as HTMLElement)
    await settle(1000)
    expect(card()).toBeTruthy()
    fireEvent.mouseLeave(card() as HTMLElement)
    await settle(200)
    expect(card()).toBeNull()
  })

  it('is read-only: the card is a tooltip with no buttons, links or inputs', () => {
    const trigger = renderCard(agentCard({ lead: { kind: 'blocking', ownerSubjectId: 'user-9' }, ownerName: 'Kari' }))
    fireEvent.focus(trigger)
    flush()
    const shown = card() as HTMLElement
    expect(shown.getAttribute('role')).toBe('tooltip')
    expect(shown.querySelectorAll('button, a, input, select, textarea, [role="button"]')).toHaveLength(0)
  })
})

describe('SpaceIdentityHoverCard agent lead line (WS07-S8)', () => {
  const open = (model: IdentityCardModel) => {
    fireEvent.focus(renderCard(model))
    flush()
    return card() as HTMLElement
  }

  it('shows the agent name, the Agent chip and the title', () => {
    const shown = open(agentCard({ title: 'Lager', color: '#2563eb' }))
    expect(shown.querySelector('.verevon-identity-card__name')?.textContent).toBe('Lagerhjelpen')
    expect(shown.querySelector('.verevon-identity-card__chip')?.textContent).toBe('Agent')
    expect(shown.querySelector('.verevon-identity-card__sub')?.textContent).toContain('Lager')
    expect(shown.querySelector('.verevon-agent-avatar')?.getAttribute('data-kind')).toBe('agent')
  })

  it('blocking leads: "Waiting for <owner>" with no approval detail and no controls', () => {
    const shown = open(agentCard({ lead: { kind: 'blocking', ownerSubjectId: 'user-9' }, ownerName: 'Kari Nordmann' }))
    const lead = shown.querySelector('.verevon-identity-card__lead')
    expect(lead?.textContent).toBe('Venter på Kari Nordmann')
    expect(lead?.className).toContain('verevon-identity-card__lead--blocking')
    expect(shown.querySelectorAll('.verevon-identity-card__lead')).toHaveLength(1)
    expect(shown.querySelectorAll('button, a, input')).toHaveLength(0)
    // Nothing about what the run wants to do.
    expect(shown.textContent).not.toMatch(/godkjenn|approve|tool|verktøy|kommando/i)
  })

  it('blocking without a resolvable owner says so without inventing a name', () => {
    const shown = open(agentCard({ lead: { kind: 'blocking' } }))
    expect(shown.querySelector('.verevon-identity-card__lead')?.textContent).toBe('Venter på eieren av samtalen')
  })

  it('working is shown when nothing blocks, with the thread title when known', () => {
    const shown = open(agentCard({ lead: { kind: 'working', threadTitle: 'Innkjøp' } }))
    expect(shown.querySelector('.verevon-identity-card__lead')?.textContent).toBe('Jobber i Innkjøp')
    expect(shown.querySelector('.verevon-identity-card__lead--blocking')).toBeNull()
  })

  it('working without a title reads "Jobber nå"', () => {
    const shown = open(agentCard({ lead: { kind: 'working' } }))
    expect(shown.querySelector('.verevon-identity-card__lead')?.textContent).toBe('Jobber nå')
  })

  it('last shows the latest entry text with its time', () => {
    const shown = open(agentCard({ lead: { kind: 'last', text: 'Alt er klart.', at: '2026-10-02T10:00:00Z' } }))
    const lead = shown.querySelector('.verevon-identity-card__lead')
    expect(lead?.textContent).toContain('Siste svar')
    expect(lead?.textContent).toContain('Alt er klart.')
    expect(lead?.querySelector('.verevon-identity-card__when')).toBeTruthy()
  })

  it('last without a time has no time span', () => {
    const shown = open(agentCard({ lead: { kind: 'last', text: 'Alt er klart.' } }))
    expect(shown.querySelector('.verevon-identity-card__when')).toBeNull()
  })

  it('no lead line at all when there is nothing to say', () => {
    const shown = open(agentCard({ lead: { kind: 'none' } }))
    expect(shown.querySelector('.verevon-identity-card__lead')).toBeNull()
  })

  it('exactly one lead line is rendered per lead kind (order blocking > working > last is decided upstream)', () => {
    for (const lead of [
      { kind: 'blocking' } as const,
      { kind: 'working' } as const,
      { kind: 'last', text: 'x' } as const,
    ]) {
      const shown = open(agentCard({ lead }))
      expect(shown.querySelectorAll('.verevon-identity-card__lead')).toHaveLength(1)
      cleanup()
    }
  })
})

describe('SpaceIdentityHoverCard member card (WS07-S8)', () => {
  const open = (model: IdentityCardModel) => {
    fireEvent.focus(renderCard(model))
    flush()
    return card() as HTMLElement
  }

  it('shows the translated role and the last post time', () => {
    const shown = open(memberCard({ role: 'editor', lastPostAt: '2026-10-02T10:00:00Z' }))
    expect(shown.querySelector('.verevon-identity-card__name')?.textContent).toBe('Kari Nordmann')
    expect(shown.querySelector('.verevon-identity-card__sub')?.textContent).toBe('Redaktør')
    expect(shown.querySelector('.verevon-identity-card__lead')?.textContent).toMatch(/^Siste innlegg /)
    expect(shown.querySelector('.verevon-agent-avatar')?.getAttribute('data-kind')).toBe('member')
    expect(shown.querySelector('.verevon-identity-card__chip')).toBeNull()
  })

  it('shows an unknown role as given rather than hiding it', () => {
    const shown = open(memberCard({ role: 'auditor' }))
    expect(shown.querySelector('.verevon-identity-card__sub')?.textContent).toBe('auditor')
  })

  it('has no role line and no last-post line when neither is known', () => {
    const shown = open(memberCard())
    expect(shown.querySelector('.verevon-identity-card__sub')).toBeNull()
    expect(shown.querySelector('.verevon-identity-card__lead')).toBeNull()
  })
})
