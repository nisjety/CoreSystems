import { cleanup, fireEvent, render, screen, within } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { SpaceEntryUnavailable } from './SpaceEntryUnavailable'

// No I18nProvider wraps these renders, so the copy is the default Norwegian (the same as the other
// Space components' tests). The English copy is checked where a test needs it.

afterEach(() => {
  cleanup()
})

describe('SpaceEntryUnavailable (WS13-S4b)', () => {
  it('says the message is not available to the reader, and offers no retry in the unavailable variant', () => {
    render(() => <SpaceEntryUnavailable variant="unavailable" onRetry={() => {}} spaceRef="space_room_1" />)
    const card = screen.getByRole('status')
    expect(within(card).getByText('Denne meldingen er ikke tilgjengelig for deg.')).toBeTruthy()
    expect(within(card).getByText(/Lenken kan være utløpt/)).toBeTruthy()
    // A refusal cannot pass by asking again, so the unavailable variant carries no retry control.
    expect(screen.queryByRole('button', { name: 'Prøv igjen' })).toBeNull()
  })

  it('offers a retry in the retry variant and calls onRetry when it is pressed', () => {
    const onRetry = vi.fn()
    render(() => <SpaceEntryUnavailable variant="retry" onRetry={onRetry} spaceRef="space_room_1" />)
    expect(screen.getByRole('status').textContent).toContain('Meldingen kunne ikke åpnes akkurat nå')
    fireEvent.click(screen.getByRole('button', { name: 'Prøv igjen' }))
    expect(onRetry).toHaveBeenCalledTimes(1)
  })

  it('draws no retry control in the retry variant when the page passes no onRetry', () => {
    render(() => <SpaceEntryUnavailable variant="retry" spaceRef="space_room_1" />)
    expect(screen.getByRole('status').textContent).toContain('Tilgangen kunne ikke kontrolleres')
    expect(screen.queryByRole('button', { name: 'Prøv igjen' })).toBeNull()
  })

  it('links back to the room the link named', () => {
    render(() => <SpaceEntryUnavailable variant="unavailable" spaceRef="space_room_1" />)
    const back = screen.getByRole('link', { name: 'Gå til rommet' })
    expect(back.getAttribute('href')).toBe('/spaces/space_room_1')
  })

  it('encodes the room reference in the way-back link, so it cannot add a path or query', () => {
    render(() => <SpaceEntryUnavailable variant="unavailable" spaceRef="space/with?query" />)
    expect(screen.getByRole('link', { name: 'Gå til rommet' }).getAttribute('href')).toBe(
      '/spaces/space%2Fwith%3Fquery',
    )
  })

  it('falls back to the room list when the link named no readable room', () => {
    render(() => <SpaceEntryUnavailable variant="unavailable" />)
    const back = screen.getByRole('link', { name: 'Gå til rommene' })
    expect(back.getAttribute('href')).toBe('/spaces')
  })

  it('never shows a title, a preview or the message id it was given', () => {
    render(() => <SpaceEntryUnavailable variant="unavailable" spaceRef="space_room_1" />)
    // The component takes no message id at all; the copy is fixed, so nothing from the link can be echoed.
    expect(document.body.textContent ?? '').not.toMatch(/msg_|thread_/)
  })
})
