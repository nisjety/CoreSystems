import { cleanup, fireEvent, render, screen, within } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { MentionCandidate } from '../lib/mention-model'
import type { MentionPickerRow } from './MentionPicker'

const { MentionPicker } = await import('./MentionPicker')

// No I18nProvider wraps these renders, so the copy is the default Norwegian.
const LIST_ID = 'room-picker-test'

const person = (id: string, label: string): MentionCandidate => ({
  id,
  kind: 'person',
  subjectId: id,
  name: label,
  label,
  role: 'editor',
})

const agent = (id: string, label: string): MentionCandidate => ({
  id,
  kind: 'agent',
  subjectId: id,
  name: label,
  label,
  role: 'agent',
  color: '#2563eb',
})

const rows: readonly MentionPickerRow[] = [
  { kind: 'candidate', candidate: person('p-kari', 'Kari') },
  { kind: 'candidate', candidate: person('p-ola', 'Ola') },
  { kind: 'candidate', candidate: agent('a-lager', 'Lagerhjelpen') },
  { kind: 'add', agentRef: 'agent-innkjop', name: 'Innkjøpsagenten' },
]

afterEach(() => {
  cleanup()
})

// The composer owns the active index and the keys; the picker only shows the index and reports hovers.
function renderPicker() {
  const [active, setActive] = createSignal(0)
  const onHover = vi.fn((index: number) => setActive(index))
  const onPick = vi.fn()
  render(() => (
    <MentionPicker id={LIST_ID} rows={() => rows} activeIndex={active} onHover={onHover} onPick={onPick} />
  ))
  const listbox = screen.getByRole('listbox', { name: 'Nevn noen' })
  return { active, setActive, onHover, onPick, listbox }
}

describe('MentionPicker (WS08-S6)', () => {
  it('is a named listbox of options under People, Agents and Add to room headings, in that order', () => {
    const { listbox } = renderPicker()
    expect(within(listbox).getAllByRole('option')).toHaveLength(4)
    for (const name of ['Kari', 'Ola', 'Lagerhjelpen', 'Legg til Innkjøpsagenten']) {
      expect(within(listbox).getByRole('option', { name })).toBeTruthy()
    }
    // A heading is a presentational row, never an option a screen reader would count.
    expect(within(listbox).getByText('Personer').getAttribute('role')).toBe('presentation')
    expect(within(listbox).getByText('Agenter').getAttribute('role')).toBe('presentation')
    expect(within(listbox).getByText('Legg til i rommet').getAttribute('role')).toBe('presentation')

    const text = listbox.textContent ?? ''
    const at = (needle: string) => text.indexOf(needle)
    expect(at('Personer')).toBeGreaterThanOrEqual(0)
    expect(at('Personer')).toBeLessThan(at('Kari'))
    expect(at('Kari')).toBeLessThan(at('Agenter'))
    expect(at('Agenter')).toBeLessThan(at('Lagerhjelpen'))
    expect(at('Lagerhjelpen')).toBeLessThan(at('Legg til i rommet'))
    expect(at('Legg til i rommet')).toBeLessThan(at('Legg til Innkjøpsagenten'))
  })

  it('gives each option the list id and its row index, and marks only the active row aria-selected', () => {
    const { active, setActive, listbox } = renderPicker()
    const options = within(listbox).getAllByRole('option')
    expect(options.map((option) => option.id)).toEqual([`${LIST_ID}-0`, `${LIST_ID}-1`, `${LIST_ID}-2`, `${LIST_ID}-3`])
    expect(options.map((option) => option.getAttribute('aria-selected'))).toEqual(['true', 'false', 'false', 'false'])

    setActive(2)
    flush()
    expect(options.map((option) => option.getAttribute('aria-selected'))).toEqual(['false', 'false', 'true', 'false'])
    // The composer points aria-activedescendant at `${id}-${activeIndex}`: that id must be the highlighted row.
    expect(document.getElementById(`${LIST_ID}-${active()}`)).toBe(options[2])
  })

  it('hovering an option makes it the active row, and a click picks the row that option was drawn for', () => {
    const { active, onHover, onPick, listbox } = renderPicker()
    fireEvent.mouseEnter(within(listbox).getByRole('option', { name: 'Ola' }))
    expect(onHover).toHaveBeenLastCalledWith(1)
    flush()
    expect(active()).toBe(1)
    expect(within(listbox).getByRole('option', { name: 'Ola' }).getAttribute('aria-selected')).toBe('true')

    fireEvent.click(within(listbox).getByRole('option', { name: 'Lagerhjelpen' }))
    expect(onPick).toHaveBeenCalledTimes(1)
    expect(onPick).toHaveBeenCalledWith(rows[2])
  })
})
