import { cleanup, render } from '@solidjs/testing-library'
import { afterEach, describe, expect, it } from 'vitest'

// The chips are presentation only: no API call, so nothing is mocked here. The pure reader
// (spaceStatusFrom) decides what counts as a status, and the component renders what it returns.
const { SpaceAgentActivityChip, SpaceMemberStatusChip } = await import('./SpaceStatusChip')

afterEach(() => {
  cleanup()
})

const chipOf = () => document.querySelector<HTMLElement>('.verevon-space-status-chip')

describe('SpaceMemberStatusChip (WS11-S10)', () => {
  it('renders the state word and the text, with the text in the tooltip too', () => {
    render(() => <SpaceMemberStatusChip status={{ state: 'dnd', text: 'På møte', expires_at: null }} />)
    const chip = chipOf()
    expect(chip?.getAttribute('data-state')).toBe('dnd')
    expect(chip?.textContent).toContain('Ikke forstyrr')
    expect(chip?.textContent).toContain('· På møte')
    expect(chip?.getAttribute('title')).toBe('På møte')
  })

  it('renders the state word alone when no text was set', () => {
    render(() => <SpaceMemberStatusChip status={{ state: 'away', text: null, expires_at: null }} />)
    const chip = chipOf()
    expect(chip?.getAttribute('data-state')).toBe('away')
    expect(chip?.textContent).toBe('Borte')
    expect(chip?.querySelector('.verevon-space-status-chip__text')).toBeNull()
    expect(chip?.hasAttribute('title')).toBe(false)
  })

  it('shows the text as plain text, never as markup', () => {
    render(() => <SpaceMemberStatusChip status={{ state: 'away', text: '<img src="x">', expires_at: null }} />)
    expect(chipOf()?.querySelector('img')).toBeNull()
    expect(chipOf()?.textContent).toContain('<img src="x">')
  })

  it.each([
    ['null', null],
    ['absent', undefined],
    ['an unknown state', { state: 'lunch', text: null, expires_at: null }],
  ])('renders nothing for %s', (_label, status) => {
    const view = render(() => <SpaceMemberStatusChip status={status} />)
    expect(view.container.innerHTML).toBe('')
    expect(chipOf()).toBeNull()
  })
})

describe('SpaceAgentActivityChip (WS11-S10)', () => {
  it.each([
    ['working', 'Jobber'],
    ['waiting', 'Venter'],
    ['idle', 'Ledig'],
  ] as const)('renders %s as the word %s', (activity, word) => {
    render(() => <SpaceAgentActivityChip activity={activity} />)
    const chip = chipOf()
    expect(chip?.getAttribute('data-state')).toBe(activity)
    expect(chip?.textContent).toBe(word)
  })

  it('renders nothing when the gateway sent no activity', () => {
    const view = render(() => <SpaceAgentActivityChip activity={undefined} />)
    expect(view.container.innerHTML).toBe('')
    expect(chipOf()).toBeNull()
  })
})
