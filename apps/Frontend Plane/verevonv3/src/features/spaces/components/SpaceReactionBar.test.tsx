// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { SpaceReactionChip } from '@/shared/api/spaces-client'

const { SpaceReactionBar } = await import('./SpaceReactionBar')

const THUMBS = '\u{1F44D}'
const PARTY = '\u{1F389}'

afterEach(() => {
  cleanup()
})

/** Solid 2 applies a signal write on a microtask: let it land before asserting on the DOM. */
async function flush() {
  await new Promise((resolve) => setTimeout(resolve, 0))
}

function renderBar(chips: readonly SpaceReactionChip[], extra: { canReact?: boolean; busy?: boolean } = {}) {
  const onToggle = vi.fn()
  const view = render(() => (
    <SpaceReactionBar chips={chips} canReact={extra.canReact ?? true} busy={extra.busy} onToggle={onToggle} />
  ))
  return { onToggle, view }
}

const chips: readonly SpaceReactionChip[] = [
  { emoji: THUMBS, count: 3, reacted: true },
  { emoji: PARTY, count: 1, reacted: false },
]

function chipButtons(): HTMLButtonElement[] {
  return screen
    .getAllByRole('button')
    .filter(
      (button) =>
        button.classList.contains('verevon-reaction-chip') && !button.classList.contains('verevon-reaction-chip--add'),
    ) as HTMLButtonElement[]
}

describe('SpaceReactionBar', () => {
  it('shows one chip per emoji with its count, pressed when the viewer used it', () => {
    renderBar(chips)
    const buttons = chipButtons()
    expect(buttons).toHaveLength(2)
    expect(buttons[0]?.textContent).toContain(THUMBS)
    expect(buttons[0]?.textContent).toContain('3')
    expect(buttons[0]?.getAttribute('aria-pressed')).toBe('true')
    expect(buttons[1]?.textContent).toContain('1')
    expect(buttons[1]?.getAttribute('aria-pressed')).toBe('false')
  })

  it('draws nothing when there are no chips and the viewer cannot react', () => {
    const { view } = renderBar([], { canReact: false })
    expect(view.container.innerHTML).toBe('')
  })

  it('draws only the add button when there are no chips and the viewer can react', () => {
    renderBar([])
    expect(chipButtons()).toHaveLength(0)
    expect(screen.getByRole('button', { name: 'Legg til reaksjon' })).toBeTruthy()
  })

  it('shows a viewer the chips as disabled marks with no add button', () => {
    renderBar(chips, { canReact: false })
    expect(chipButtons().every((button) => button.disabled)).toBe(true)
    expect(screen.queryByRole('button', { name: 'Legg til reaksjon' })).toBeNull()
  })

  it('disables the chips and the add button while a change is in flight', () => {
    renderBar(chips, { busy: true })
    expect(chipButtons().every((button) => button.disabled)).toBe(true)
    expect((screen.getByRole('button', { name: 'Legg til reaksjon' }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('toggles a chip off when it is mine and on when it is not', () => {
    const { onToggle } = renderBar(chips)
    const buttons = chipButtons()
    fireEvent.click(buttons[0]!)
    expect(onToggle).toHaveBeenLastCalledWith(THUMBS, false)
    fireEvent.click(buttons[1]!)
    expect(onToggle).toHaveBeenLastCalledWith(PARTY, true)
    expect(onToggle).toHaveBeenCalledTimes(2)
  })

  it('opens the picker with 24 emoji and closes it with Escape', async () => {
    renderBar(chips)
    const add = screen.getByRole('button', { name: 'Legg til reaksjon' })
    expect(add.getAttribute('aria-expanded')).toBe('false')
    expect(screen.queryByRole('group', { name: 'Velg emoji' })).toBeNull()

    fireEvent.click(add)
    await flush()
    const panel = screen.getByRole('group', { name: 'Velg emoji' })
    expect(panel.querySelectorAll('button')).toHaveLength(24)
    expect(add.getAttribute('aria-expanded')).toBe('true')

    fireEvent.keyDown(panel, { key: 'Escape' })
    await flush()
    expect(screen.queryByRole('group', { name: 'Velg emoji' })).toBeNull()
    expect(add.getAttribute('aria-expanded')).toBe('false')
  })

  it('closes the picker when the add button is pressed again', async () => {
    renderBar([])
    const add = screen.getByRole('button', { name: 'Legg til reaksjon' })
    fireEvent.click(add)
    await flush()
    expect(screen.getByRole('group', { name: 'Velg emoji' })).toBeTruthy()
    fireEvent.click(add)
    await flush()
    expect(screen.queryByRole('group', { name: 'Velg emoji' })).toBeNull()
  })

  it('adds the picked emoji and closes the picker', async () => {
    const { onToggle } = renderBar(chips)
    fireEvent.click(screen.getByRole('button', { name: 'Legg til reaksjon' }))
    await flush()
    fireEvent.click(screen.getByRole('button', { name: '\u{1F680}' }))
    await flush()
    expect(onToggle).toHaveBeenCalledTimes(1)
    expect(onToggle).toHaveBeenCalledWith('\u{1F680}', true)
    expect(screen.queryByRole('group', { name: 'Velg emoji' })).toBeNull()
  })

  it('does not remove a reaction when the viewer picks an emoji they already used', async () => {
    const { onToggle } = renderBar(chips)
    fireEvent.click(screen.getByRole('button', { name: 'Legg til reaksjon' }))
    await flush()
    const panel = screen.getByRole('group', { name: 'Velg emoji' })
    const mine = Array.from(panel.querySelectorAll('button')).find((button) => button.getAttribute('aria-label') === THUMBS)
    expect(mine).toBeTruthy()
    fireEvent.click(mine!)
    await flush()
    expect(onToggle).not.toHaveBeenCalled()
    expect(screen.queryByRole('group', { name: 'Velg emoji' })).toBeNull()
  })
})
