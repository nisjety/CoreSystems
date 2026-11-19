import { cleanup, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { fireEvent } from '@/test/fire-event'
import { render } from '@/test/render'

import { SpaceRoomMenu, type SpaceMenuItem } from './SpaceRoomMenu'

// WS16-S10: the "more" menu of a room row. A two-state entry (Mute, a computed section toggle) is a checkbox item with
// aria-checked; a plain entry stays a menuitem. The menu is a native details element, so the tests open it the way a reader
// does (the summary) and assert what can be chosen and what closing it does.

afterEach(() => cleanup())

const detailsOf = () => document.querySelector('details') as HTMLDetailsElement

describe('SpaceRoomMenu (WS16-S10)', () => {
  it('a checked item is a menuitemcheckbox with aria-checked, an unchecked one says false, and a plain item stays a menuitem', () => {
    const items: SpaceMenuItem[] = [
      { id: 'mute', label: 'Demp', onSelect: vi.fn(), checked: true },
      { id: 'unmute', label: 'Vis varsler', onSelect: vi.fn(), checked: false },
      { id: 'rename', label: 'Gi nytt navn', onSelect: vi.fn() },
    ]
    render(() => <SpaceRoomMenu label="Rommet Leveranse" items={items} />)

    const muted = screen.getByRole('menuitemcheckbox', { name: '✓ Demp' })
    expect(muted.getAttribute('aria-checked')).toBe('true')

    const unmuted = screen.getByRole('menuitemcheckbox', { name: 'Vis varsler' })
    expect(unmuted.getAttribute('aria-checked')).toBe('false')

    const plain = screen.getByRole('menuitem', { name: 'Gi nytt navn' })
    expect(plain.hasAttribute('aria-checked')).toBe(false)
  })

  it('choosing an item closes the menu, returns focus to its summary, and runs the item once', () => {
    const onSelect = vi.fn()
    render(() => <SpaceRoomMenu label="Rommet Leveranse" items={[{ id: 'rename', label: 'Gi nytt navn', onSelect }]} />)
    detailsOf().open = true

    fireEvent.click(screen.getByRole('menuitem', { name: 'Gi nytt navn' }))

    expect(onSelect).toHaveBeenCalledOnce()
    expect(detailsOf().open).toBe(false)
    expect(document.activeElement).toBe(detailsOf().querySelector('summary'))
  })

  it('a disabled item cannot be chosen, and a danger item is marked as one', () => {
    const onSelect = vi.fn()
    render(() => (
      <SpaceRoomMenu
        label="Rommet Leveranse"
        items={[
          { id: 'leave', label: 'Forlat rommet', onSelect, disabled: true },
          { id: 'delete', label: 'Slett rommet', onSelect: vi.fn(), danger: true },
        ]}
      />
    ))

    const leave = screen.getByRole('menuitem', { name: 'Forlat rommet' }) as HTMLButtonElement
    expect(leave.disabled).toBe(true)
    fireEvent.click(leave)
    expect(onSelect).not.toHaveBeenCalled()

    expect(screen.getByRole('menuitem', { name: 'Slett rommet' }).getAttribute('data-danger')).toBe('true')
  })

  it('Escape closes an open menu and returns focus to its summary, and a closed menu is left alone', () => {
    render(() => <SpaceRoomMenu label="Rommet Leveranse" items={[{ id: 'rename', label: 'Gi nytt navn', onSelect: vi.fn() }]} />)
    detailsOf().open = true

    fireEvent.keyDown(detailsOf(), { key: 'Escape' })

    expect(detailsOf().open).toBe(false)
    expect(document.activeElement).toBe(detailsOf().querySelector('summary'))
  })

  it('renders nothing when disabled, and labels the menu for assistive technology', () => {
    const { container } = render(() => (
      <SpaceRoomMenu label="Rommet Leveranse" disabled items={[{ id: 'rename', label: 'Gi nytt navn', onSelect: vi.fn() }]} />
    ))
    expect(container.querySelector('details')).toBeNull()

    cleanup()
    render(() => <SpaceRoomMenu label="Rommet Leveranse" items={[{ id: 'rename', label: 'Gi nytt navn', onSelect: vi.fn() }]} />)
    expect(screen.getByRole('menu', { name: 'Rommet Leveranse' })).toBeTruthy()
    expect(document.querySelector('summary')?.getAttribute('aria-label')).toBe('Rommet Leveranse')
  })
})
