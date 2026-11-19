import { cleanup, screen } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, describe, expect, it } from 'vitest'

import type { SpaceOwnHistoryWindow } from '@/shared/api/spaces-client'
import { expectNoNorwegian, renderInLocale } from '@/test/locale-leak'
import { render } from '@/test/render'

import { SpaceHistoryWindowNote } from './SpaceHistoryWindowNote'

// D18: the quiet marker for a member whose earlier messages are hidden. It is drawn only on Control's own `hidden: true`
// and says no more than Control did. With no copy of this component in the default render, the words are Norwegian.

afterEach(() => cleanup())

const noon = new Date(2026, 10, 12, 12, 0, 0).toISOString()
const own = (over: Partial<SpaceOwnHistoryWindow> = {}): SpaceOwnHistoryWindow => ({
  source: 'member',
  mode: 'since_join',
  sinceAt: null,
  hidden: true,
  ...over,
})
const note = () => document.querySelector('.verevon-space-history-window')

describe('SpaceHistoryWindowNote (D18)', () => {
  it('draws one note with the title when Control says earlier messages are hidden', () => {
    render(() => <SpaceHistoryWindowNote window={() => own({ mode: 'full' })} />)
    const found = screen.getByRole('note')
    expect(found.textContent).toContain('Tidligere meldinger er skjult')
    expect(screen.getAllByRole('note')).toHaveLength(1)
    // A note, not a live region: it must not be announced again on every poll.
    expect(found.getAttribute('aria-live')).toBeNull()
    expect(screen.queryByRole('status')).toBeNull()
  })

  it('says from when the member reads for a date, and for a join, and nothing more for any other mode', () => {
    const [window, setWindow] = createSignal<SpaceOwnHistoryWindow | undefined>(own({ mode: 'since_date', sinceAt: noon }))
    render(() => <SpaceHistoryWindowNote window={window} />)
    expect(screen.getByRole('note').textContent).toMatch(/Tidligere meldinger er skjult.*Du ser meldinger fra 12\.? nov\.? 2026\./i)
    setWindow(own({ mode: 'since_join' }))
    flush()
    expect(screen.getByRole('note').textContent).toContain('Du ser meldinger fra du ble med.')
    setWindow(own({ mode: 'full' }))
    flush()
    expect(screen.getByRole('note').textContent).toBe('Tidligere meldinger er skjult')
    // A date window that names no instant says no date.
    setWindow(own({ mode: 'since_date', sinceAt: null }))
    flush()
    expect(screen.getByRole('note').textContent).toBe('Tidligere meldinger er skjult')
  })

  it('draws nothing when the window is not hidden, absent, or goes away', () => {
    const [window, setWindow] = createSignal<SpaceOwnHistoryWindow | undefined>(own({ hidden: false }))
    render(() => <SpaceHistoryWindowNote window={window} />)
    expect(note()).toBeNull()
    setWindow(undefined)
    flush()
    expect(note()).toBeNull()
    setWindow(own())
    flush()
    expect(note()).not.toBeNull()
    // The floor moved back: the marker goes with it, in the same page.
    setWindow(own({ hidden: false }))
    flush()
    expect(note()).toBeNull()
  })

  it('offers no way to the hidden messages: no button, no link, no input', () => {
    render(() => <SpaceHistoryWindowNote window={() => own({ mode: 'since_date', sinceAt: noon })} />)
    const found = screen.getByRole('note')
    expect(found.querySelector('button, a, input, select, textarea')).toBeNull()
  })

  it('says it in English, with no Norwegian left in the note', () => {
    renderInLocale('en', () => <SpaceHistoryWindowNote window={() => own({ mode: 'since_date', sinceAt: noon })} />)
    const found = screen.getByRole('note')
    expect(found.textContent).toBe('Earlier messages are hiddenYou see messages from Nov 12, 2026.')
    expectNoNorwegian(found)
  })

  it('says a join in English', () => {
    renderInLocale('en', () => <SpaceHistoryWindowNote window={() => own({ mode: 'since_join' })} />)
    expect(screen.getByRole('note').textContent).toBe('Earlier messages are hiddenYou see messages from when you joined.')
  })
})
