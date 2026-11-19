import { cleanup, render } from '@solidjs/testing-library'
import { afterEach, describe, expect, it } from 'vitest'

import { turnEntryCard, turnEntryFromWire, turnEntryReplacesBody } from '../../lib/entry-registry'
import type { AgentCardEntry as AgentCardEntryType } from '../../lib/entry-registry'
import { AgentCardEntry } from './AgentCardEntry'

// WS18-S7: the plain-text card a room agent posts. Every part of it is text: no anchor, no image, no markup,
// and its body is the message text, which replaces the markdown body of its turn. No I18nProvider wraps these
// renders, so the copy is the Norwegian default, as in the other component tests.

afterEach(() => cleanup())

function cardOf(payload: Record<string, unknown>, version = 1): AgentCardEntryType {
  const entry = turnEntryFromWire({ kind: 'agent_card', version, agent_card: payload })
  if (entry?.kind !== 'agent_card') throw new Error('expected an agent card entry')
  return entry
}

describe('AgentCardEntry (WS18-S7)', () => {
  it('draws the title, the tone, the fields and the body as text, with the authorship note', () => {
    const entry = cardOf({ title: 'Importen er klar', tone: 'done', fields: [{ label: 'Rader', value: '1 204' }] })
    const { container } = render(() => <AgentCardEntry entry={entry} body="Alt er lastet inn." />)
    const root = container.querySelector('[data-entry-kind="agent_card"]') as HTMLElement
    expect(root.getAttribute('role')).toBe('group')
    expect(root.getAttribute('data-tone')).toBe('done')
    expect(root.querySelector('.verevon-room-entry--card__title')?.textContent).toBe('Importen er klar')
    expect(root.querySelector('.verevon-room-entry--card__tone')?.textContent).toBe('Ferdig')
    expect(root.querySelector('.verevon-room-entry--card__field dt')?.textContent).toBe('Rader')
    expect(root.querySelector('.verevon-room-entry--card__field dd')?.textContent).toBe('1 204')
    expect(root.querySelector('.verevon-room-entry--card__body')?.textContent).toBe('Alt er lastet inn.')
    expect(root.querySelector('.verevon-room-entry--card__note')?.textContent).toBe('Skrevet av en agent i dette rommet')
  })

  it.each([
    ['info', 'Info'],
    ['progress', 'Pågår'],
    ['done', 'Ferdig'],
    ['blocked', 'Blokkert'],
  ] as const)('says the %s tone in words, and keeps its icon from assistive tech', (tone, word) => {
    const entry = cardOf({ title: 'Status', tone })
    const { container } = render(() => <AgentCardEntry entry={entry} body="" />)
    const line = container.querySelector('.verevon-room-entry--card__tone') as HTMLElement
    expect(line.textContent).toBe(word)
    expect(line.querySelector('.verevon-room-entry--card__icon')?.getAttribute('aria-hidden')).toBe('true')
  })

  it('leaves a link the agent wrote as inert text: no anchor and no image', () => {
    const entry = cardOf({ title: 'Lenke', tone: 'info' })
    const { container } = render(() => (
      <AgentCardEntry entry={entry} body="Se https://example.test/rapport og <img src=x onerror=alert(1)>" />
    ))
    expect(container.querySelector('a')).toBeNull()
    expect(container.querySelector('img')).toBeNull()
    expect(container.querySelector('.verevon-room-entry--card__body')?.textContent).toBe(
      'Se https://example.test/rapport og <img src=x onerror=alert(1)>',
    )
  })

  it('draws a card with a version it does not know as its body alone, with no tone and no title', () => {
    const entry = cardOf({ title: 'Nytt format', tone: 'done', fields: [{ label: 'Steg', value: '2' }] }, 2)
    const { container } = render(() => <AgentCardEntry entry={entry} body="Bare teksten." />)
    expect(container.querySelector('[data-tone]')?.getAttribute('data-tone')).toBe('none')
    expect(container.querySelector('.verevon-room-entry--card__title')).toBeNull()
    expect(container.querySelector('.verevon-room-entry--card__tone')).toBeNull()
    expect(container.querySelector('.verevon-room-entry--card__fields')).toBeNull()
    expect(container.querySelector('.verevon-room-entry--card__body')?.textContent).toBe('Bare teksten.')
  })

  it('drops a tone it does not know and keeps the title and the fields', () => {
    const entry = cardOf({ title: 'Ukjent tone', tone: 'alarm', fields: [{ label: 'Steg', value: '3' }] })
    const { container } = render(() => <AgentCardEntry entry={entry} body="" />)
    expect(container.querySelector('[data-tone]')?.getAttribute('data-tone')).toBe('none')
    expect(container.querySelector('.verevon-room-entry--card__tone')).toBeNull()
    expect(container.querySelector('.verevon-room-entry--card__title')?.textContent).toBe('Ukjent tone')
    expect(container.querySelector('.verevon-room-entry--card__field dd')?.textContent).toBe('3')
    expect(container.querySelector('.verevon-room-entry--card__body')).toBeNull()
  })

  it('is the card for its kind, and its kind replaces the message body', () => {
    const entry = cardOf({ title: 'Registrert' })
    expect(turnEntryCard(entry)).toBe(AgentCardEntry)
    expect(turnEntryReplacesBody(entry)).toBe(true)
  })
})
