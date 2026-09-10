import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const orgClient = vi.hoisted(() => ({ listOrganizationMembers: vi.fn() }))
const spacesClient = vi.hoisted(() => ({ addSpaceMember: vi.fn(), removeSpaceMember: vi.fn() }))

vi.mock('@/shared/api/organization-client', () => orgClient)
vi.mock('@/shared/api/spaces-client', () => spacesClient)

const { SpaceMemberControls, SpaceMemberRemoveButton } = await import('./SpaceMemberControls')

const roster = [
  { subject_type: 'user' as const, subject_id: 'user_1', role: 'owner' as const, revision: 1, display_name: 'Kari Nordmann' },
  { subject_type: 'user' as const, subject_id: 'user_2', role: 'editor' as const, revision: 1, display_name: 'Ola Hansen' },
  { subject_type: 'service' as const, subject_id: 'agent-1', role: 'editor' as const, revision: 1, display_name: 'Driftsassistent' },
]

beforeEach(() => {
  orgClient.listOrganizationMembers.mockReset()
  orgClient.listOrganizationMembers.mockResolvedValue([
    { userId: 'user_1', name: 'Kari Nordmann', email: 'kari@example.com', role: 'admin', status: 'active' },
    { userId: 'user_2', name: 'Ola Hansen', email: 'ola@example.com', role: 'member', status: 'active' },
    { userId: 'user_3', name: 'Nina Berg', email: 'nina@example.com', role: 'member', status: 'active' },
  ])
  spacesClient.addSpaceMember.mockReset().mockResolvedValue({ memberCount: 3, changed: true })
  spacesClient.removeSpaceMember.mockReset().mockResolvedValue({ memberCount: 1, changed: true })
})

afterEach(() => cleanup())

describe('SpaceMemberControls', () => {
  it('offers only people who are not already in the room', async () => {
    const onChanged = vi.fn()
    render(() => (
      <SpaceMemberControls spaceRef="space_1" orgId="org_1" roster={() => roster} onChanged={onChanged} />
    ))

    screen.getByRole('button', { name: /Legg til personer/ }).click()
    expect(await screen.findByText('Nina Berg')).toBeTruthy()
    // Both are already on Control's roster, so neither may be offered again.
    expect(screen.queryByRole('button', { name: 'Legg til' })).toBeTruthy()
    const addButtons = screen.getAllByRole('button', { name: 'Legg til' })
    expect(addButtons).toHaveLength(1)
  })

  it('adds a person and tells the room to re-read Control', async () => {
    const onChanged = vi.fn()
    render(() => (
      <SpaceMemberControls spaceRef="space_1" orgId="org_1" roster={() => roster} onChanged={onChanged} />
    ))
    screen.getByRole('button', { name: /Legg til personer/ }).click()
    ;(await screen.findByRole('button', { name: 'Legg til' })).click()

    await waitFor(() => expect(spacesClient.addSpaceMember).toHaveBeenCalledWith('space_1', 'user_3'))
    await waitFor(() => expect(onChanged).toHaveBeenCalled())
  })

  // The removal control lives on the roster row the Members tab already draws,
  // so this component renders no second copy of the roster.
  it('does not draw its own roster beside the one the tab already shows', () => {
    render(() => <SpaceMemberControls spaceRef="space_1" orgId="org_1" roster={() => roster} />)
    expect(screen.queryByText('Kari Nordmann')).toBeNull()
    expect(screen.queryByRole('button', { name: 'Fjern' })).toBeNull()
  })

  // Two presses, with the consequence on screen between them. The first press
  // must not remove anyone — that is the whole point of arming.
  it('arms before removing, and states the consequence while armed', async () => {
    const member = roster[1]!
    render(() => <SpaceMemberRemoveButton spaceRef="space_1" member={member} />)

    screen.getByRole('button', { name: 'Fjern' }).click()
    expect(spacesClient.removeSpaceMember).not.toHaveBeenCalled()
    expect(await screen.findByText(/mister tilgang/)).toBeTruthy()

    // The armed button names what it will do, for anyone not reading the page.
    const armed = screen.getByRole('button', { name: /Bekreft at Ola Hansen fjernes/ })
    armed.click()
    await waitFor(() => expect(spacesClient.removeSpaceMember).toHaveBeenCalledWith('space_1', 'user_2'))
  })

  // An armed button that stays armed is a trap: a reader who thinks better of
  // it and comes back later must not find a one-click removal.
  it('disarms when it loses focus', async () => {
    const member = roster[1]!
    render(() => <SpaceMemberRemoveButton spaceRef="space_1" member={member} />)

    const button = screen.getByRole('button', { name: 'Fjern' })
    button.click()
    expect(await screen.findByText(/mister tilgang/)).toBeTruthy()
    button.dispatchEvent(new FocusEvent('blur'))
    await waitFor(() => expect(screen.queryByText(/mister tilgang/)).toBeNull())
    expect(screen.getByRole('button', { name: 'Fjern' })).toBeTruthy()
    expect(spacesClient.removeSpaceMember).not.toHaveBeenCalled()
  })

  // An unreadable organization roster is not an empty organization.
  it('separates "could not load people" from "everyone is already here"', async () => {
    orgClient.listOrganizationMembers.mockRejectedValue(new Error('down'))
    render(() => <SpaceMemberControls spaceRef="space_1" orgId="org_1" roster={() => roster} />)

    screen.getByRole('button', { name: /Legg til personer/ }).click()
    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.queryByText(/Alle i organisasjonen er allerede med/)).toBeNull()
  })

  it('reports a failed add without claiming the person joined', async () => {
    spacesClient.addSpaceMember.mockRejectedValue(new Error('nope'))
    const onChanged = vi.fn()
    render(() => (
      <SpaceMemberControls spaceRef="space_1" orgId="org_1" roster={() => roster} onChanged={onChanged} />
    ))
    screen.getByRole('button', { name: /Legg til personer/ }).click()
    ;(await screen.findByRole('button', { name: 'Legg til' })).click()

    expect(await screen.findByText(/Ingenting er endret/)).toBeTruthy()
    expect(onChanged).not.toHaveBeenCalled()
  })
})
