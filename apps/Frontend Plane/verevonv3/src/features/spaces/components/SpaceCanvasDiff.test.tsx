import { cleanup, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it } from 'vitest'

import { render } from '@/test/render'

import { SpaceCanvasDiff } from './SpaceCanvasDiff'

// WS19-S7: the line difference between two texts of the room canvas. The diff itself is the shared `diffText`; these tests
// pin what the reader sees: the sign on each changed line, the counts, and the two sentences that must never read as "no
// change" (a text too long to compare, and a comparison that was not made).

afterEach(() => cleanup())

describe('SpaceCanvasDiff (WS19-S7)', () => {
  it('shows No differences for two equal texts, and no diff block', () => {
    render(() => <SpaceCanvasDiff before={'# Mål\nLevér torsdag.'} after={'# Mål\nLevér torsdag.'} />)
    expect(screen.getByText('Ingen endringer.')).toBeTruthy()
    expect(document.querySelector('.verevon-space-canvas__diff')).toBeNull()
  })

  it('marks a removed line with a minus and an added line with a plus, each with its own class', () => {
    render(() => <SpaceCanvasDiff before={'a\nb\nc'} after={'a\nB\nc'} />)

    const removed = document.querySelector('.verevon-space-canvas__diff-line--removed')
    const added = document.querySelector('.verevon-space-canvas__diff-line--added')
    expect(removed?.textContent).toBe('-b')
    expect(added?.textContent).toBe('+B')
    // The unchanged line carries no change class and no sign.
    const context = [...document.querySelectorAll('.verevon-space-canvas__diff-line')].find(
      (line) => line.textContent === ' a',
    )
    expect(context).toBeTruthy()
    expect(context?.className).not.toMatch(/--(added|removed)/)
  })

  it('states how many lines were added and removed', () => {
    render(() => <SpaceCanvasDiff before={'a\nb\nc'} after={'a\nB\nC\nd'} />)
    expect(screen.getByText('Lagt til: 3 · Fjernet: 2')).toBeTruthy()
  })

  it('says a text too long to compare line by line, and not that nothing changed, even when both sides are equal', () => {
    const long = Array.from({ length: 2001 }, (_, index) => `linje ${index}`).join('\n')
    render(() => <SpaceCanvasDiff before={long} after={long} />)
    expect(screen.getByText('Teksten er for lang til å sammenligne linje for linje.')).toBeTruthy()
    expect(screen.queryByText('Ingen endringer.')).toBeNull()
  })
})
