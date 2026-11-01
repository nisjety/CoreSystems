import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'

const orgClient = vi.hoisted(() => ({ listOrganizationMembers: vi.fn() }))
const spacesClient = vi.hoisted(() => ({
  addSpaceMember: vi.fn(),
  removeSpaceMember: vi.fn(),
  changeSpaceMemberRole: vi.fn(),
  leaveSpaceRoom: vi.fn(),
  searchDirectoryPeople: vi.fn(),
}))

vi.mock('@/shared/api/organization-client', () => orgClient)
vi.mock('@/shared/api/spaces-client', () => spacesClient)

const { SpaceLeaveRoomButton, SpaceMemberControls, SpaceMemberRemoveButton, SpaceMemberRoleSelect } = await import(
  './SpaceMemberControls'
)

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
  spacesClient.changeSpaceMemberRole.mockReset().mockResolvedValue({ role: 'viewer', fromRole: 'editor', changed: true })
  spacesClient.leaveSpaceRoom.mockReset().mockResolvedValue({ changed: true, left: true, memberCount: 2 })
  spacesClient.searchDirectoryPeople.mockReset().mockResolvedValue({ people: [], nextCursor: null })
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

    // W1 (WS04-S6) passes the optional role as a third argument; undefined is the legacy add.
    await waitFor(() => expect(spacesClient.addSpaceMember).toHaveBeenCalledWith('space_1', 'user_3', undefined))
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

// WS04-S6: the Control member writer controls. Control re-decides every call; these only decide
// which controls are offered.
describe('SpaceMemberRoleSelect', () => {
  const member = (role: 'viewer' | 'editor' | 'manager' | 'owner', revision = 3) => ({
    subject_type: 'user' as const,
    subject_id: 'user_2',
    role,
    revision,
    display_name: 'Ola Hansen',
  })
  const optionValues = () =>
    Array.from(screen.getByRole('combobox').querySelectorAll('option')).map((option) => option.getAttribute('value'))

  it('lists only the roles the caller may assign', () => {
    render(() => (
      <SpaceMemberRoleSelect
        spaceRef="space_1"
        member={member('editor')}
        actorId="user_1"
        actorRole="manager"
        activeOwners={1}
      />
    ))
    // A manager cannot grant manager or owner.
    expect(optionValues()).toEqual(['viewer', 'editor'])
    cleanup()
    render(() => (
      <SpaceMemberRoleSelect
        spaceRef="space_1"
        member={member('editor')}
        actorId="user_1"
        actorRole="owner"
        activeOwners={1}
      />
    ))
    expect(optionValues()).toEqual(['viewer', 'editor', 'manager', 'owner'])
  })

  it('is disabled when the caller may not change this member, and still shows the current role', () => {
    render(() => (
      <SpaceMemberRoleSelect
        spaceRef="space_1"
        member={member('owner')}
        actorId="user_1"
        actorRole="manager"
        activeOwners={2}
      />
    ))
    expect(optionValues()).toEqual(['owner'])
    expect((screen.getByRole('combobox') as HTMLSelectElement).disabled).toBe(true)
  })

  it('sends the role with the revision the roster showed and tells the room to re-read', async () => {
    const onChanged = vi.fn()
    render(() => (
      <SpaceMemberRoleSelect
        spaceRef="space_1"
        member={member('editor', 7)}
        actorId="user_1"
        actorRole="owner"
        activeOwners={1}
        onChanged={onChanged}
      />
    ))
    const select = screen.getByRole('combobox') as HTMLSelectElement
    select.value = 'viewer'
    select.dispatchEvent(new Event('change', { bubbles: true }))
    await waitFor(() =>
      expect(spacesClient.changeSpaceMemberRole).toHaveBeenCalledWith('space_1', 'user_2', 'viewer', 7),
    )
    await waitFor(() => expect(onChanged).toHaveBeenCalled())
  })

  it('reports a refused change, and still re-reads because the roster on screen is stale', async () => {
    spacesClient.changeSpaceMemberRole.mockRejectedValue(new Error('room_member_changed'))
    const onChanged = vi.fn()
    render(() => (
      <SpaceMemberRoleSelect
        spaceRef="space_1"
        member={member('editor')}
        actorId="user_1"
        actorRole="owner"
        activeOwners={1}
        onChanged={onChanged}
      />
    ))
    const select = screen.getByRole('combobox') as HTMLSelectElement
    select.value = 'manager'
    select.dispatchEvent(new Event('change', { bubbles: true }))
    expect(await screen.findByText(/Ingenting er endret/)).toBeTruthy()
    expect(onChanged).toHaveBeenCalled()
  })
})

describe('SpaceLeaveRoomButton', () => {
  it('is hidden for the sole owner and offered once there is a second owner', () => {
    const { unmount } = render(() => (
      <SpaceLeaveRoomButton spaceRef="space_1" actorRole="owner" activeOwners={1} onLeft={vi.fn()} />
    ))
    expect(screen.queryByRole('button', { name: 'Forlat rommet' })).toBeNull()
    unmount()
    render(() => <SpaceLeaveRoomButton spaceRef="space_1" actorRole="owner" activeOwners={2} onLeft={vi.fn()} />)
    expect(screen.getByRole('button', { name: 'Forlat rommet' })).toBeTruthy()
  })

  it('is offered to a viewer, who can always leave', () => {
    render(() => <SpaceLeaveRoomButton spaceRef="space_1" actorRole="viewer" activeOwners={1} onLeft={vi.fn()} />)
    expect(screen.getByRole('button', { name: 'Forlat rommet' })).toBeTruthy()
  })

  it('arms first, then leaves, and calls onLeft only after Control confirmed', async () => {
    const onLeft = vi.fn()
    render(() => <SpaceLeaveRoomButton spaceRef="space_1" actorRole="editor" activeOwners={1} onLeft={onLeft} />)
    screen.getByRole('button', { name: 'Forlat rommet' }).click()
    expect(spacesClient.leaveSpaceRoom).not.toHaveBeenCalled()
    ;(await screen.findByRole('button', { name: /Bekreft at du forlater rommet/ })).click()
    await waitFor(() => expect(spacesClient.leaveSpaceRoom).toHaveBeenCalledWith('space_1'))
    await waitFor(() => expect(onLeft).toHaveBeenCalled())
  })

  it('does not claim a leave Control did not confirm, nor one that failed', async () => {
    const onLeft = vi.fn()
    spacesClient.leaveSpaceRoom.mockResolvedValueOnce({ changed: false, left: false, memberCount: 3 })
    render(() => <SpaceLeaveRoomButton spaceRef="space_1" actorRole="editor" activeOwners={1} onLeft={onLeft} />)
    screen.getByRole('button', { name: 'Forlat rommet' }).click()
    ;(await screen.findByRole('button', { name: /Bekreft at du forlater rommet/ })).click()
    expect(await screen.findByText(/Du er fortsatt med i rommet/)).toBeTruthy()
    expect(onLeft).not.toHaveBeenCalled()

    cleanup()
    spacesClient.leaveSpaceRoom.mockRejectedValueOnce(new Error('room_sole_owner'))
    render(() => <SpaceLeaveRoomButton spaceRef="space_1" actorRole="editor" activeOwners={1} onLeft={onLeft} />)
    screen.getByRole('button', { name: 'Forlat rommet' }).click()
    ;(await screen.findByRole('button', { name: /Bekreft at du forlater rommet/ })).click()
    expect(await screen.findByText(/Du er fortsatt medlem/)).toBeTruthy()
    expect(onLeft).not.toHaveBeenCalled()
  })
})

describe('SpaceMemberControls with the Control writer', () => {
  const open = async (props: { writerApi?: boolean; actorRole?: string }) => {
    render(() => (
      <SpaceMemberControls
        spaceRef="space_1"
        orgId="org_1"
        roster={() => roster}
        writerApi={props.writerApi}
        actorRole={props.actorRole}
      />
    ))
    screen.getByRole('button', { name: /Legg til personer/ }).click()
    return screen.findByRole('button', { name: 'Legg til' })
  }

  it('keeps the legacy add unchanged when the writer is off: no role picker, no role sent', async () => {
    const add = await open({ writerApi: false, actorRole: 'owner' })
    expect(screen.queryByText('Rolle for nye medlemmer')).toBeNull()
    add.click()
    await waitFor(() => expect(spacesClient.addSpaceMember).toHaveBeenCalledTimes(1))
    const call = spacesClient.addSpaceMember.mock.calls[0]!
    expect(call.slice(0, 2)).toEqual(['space_1', 'user_3'])
    expect(call[2]).toBeUndefined()
  })

  it('treats an unknown writer state as off', async () => {
    await open({ actorRole: 'owner' })
    expect(screen.queryByText('Rolle for nye medlemmer')).toBeNull()
  })

  it('offers only the roles a manager may grant and sends the chosen one', async () => {
    const add = await open({ writerApi: true, actorRole: 'manager' })
    const select = screen.getByLabelText('Rolle for nye medlemmer') as HTMLSelectElement
    expect(Array.from(select.options).map((option) => option.value)).toEqual(['viewer', 'editor'])
    select.value = 'viewer'
    select.dispatchEvent(new Event('change', { bubbles: true }))
    // Signals flush on the next tick: let the chosen role settle before the press reads it.
    await new Promise((resolve) => setTimeout(resolve, 0))
    add.click()
    await waitFor(() => expect(spacesClient.addSpaceMember).toHaveBeenCalledWith('space_1', 'user_3', 'viewer'))
  })

  it('sends the default editor role for an owner who did not pick one', async () => {
    const add = await open({ writerApi: true, actorRole: 'owner' })
    add.click()
    await waitFor(() => expect(spacesClient.addSpaceMember).toHaveBeenCalledWith('space_1', 'user_3', 'editor'))
  })

  it('says access is being prepared when the audience is pending', async () => {
    spacesClient.addSpaceMember.mockResolvedValue({ memberCount: 3, changed: true, audience: 'pending' })
    const add = await open({ writerApi: true, actorRole: 'owner' })
    add.click()
    expect(await screen.findByText(/Tilgangen forberedes/)).toBeTruthy()
    expect(screen.queryByText(/ikke bekreftet/)).toBeNull()
  })

  it('warns that the change is saved but access is not confirmed when the audience is unconfirmed', async () => {
    spacesClient.addSpaceMember.mockResolvedValue({ memberCount: 3, changed: true, audience: 'unconfirmed' })
    const add = await open({ writerApi: true, actorRole: 'owner' })
    add.click()
    expect(await screen.findByText(/Endringen er lagret, men tilgangen er ikke bekreftet/)).toBeTruthy()
    expect(screen.queryByText(/Tilgangen forberedes/)).toBeNull()
  })

  it('shows no notice when the audience is current', async () => {
    spacesClient.addSpaceMember.mockResolvedValue({ memberCount: 3, changed: true, audience: 'current' })
    const add = await open({ writerApi: true, actorRole: 'owner' })
    add.click()
    await waitFor(() => expect(spacesClient.addSpaceMember).toHaveBeenCalled())
    expect(screen.queryByText(/Tilgangen forberedes/)).toBeNull()
    expect(screen.queryByText(/ikke bekreftet/)).toBeNull()
  })
})

// WS04-S3: with `features.people_search` the picker is a debounced search over the directory. The
// server decides who the caller may see (organisation from the session, the room's own members left
// out); the component only debounces, pages by cursor and drops stale answers.
describe('SpaceMemberControls people search (WS04-S3)', () => {
  const person = (id: string, name: string) => ({ subjectId: id, displayName: name, email: `${id}@example.test`, isSelf: false })
  const ola = person('user_2', 'Ola Hansen')
  const nina = person('user_3', 'Nina Berg')
  const kari = person('user_4', 'Kari Solheim')

  // `'absent'` leaves the prop off, which a default parameter value of `undefined` could not express.
  const renderSearch = (peopleSearch: boolean | 'absent' = true) =>
    render(() => (
      <SpaceMemberControls spaceRef="space_1" orgId="org_1" roster={() => roster} peopleSearch={peopleSearch === 'absent' ? undefined : peopleSearch} />
    ))
  const openPicker = () => screen.getByRole('button', { name: /Legg til personer/ }).click()
  const box = () => screen.getByRole('searchbox') as HTMLInputElement
  const type = (value: string) => fireEvent.input(box(), { target: { value } })
  const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))
  const searchCalls = () => spacesClient.searchDirectoryPeople.mock.calls as Array<[string, Record<string, unknown>]>

  it('shows a search box and never reads the organisation list', async () => {
    spacesClient.searchDirectoryPeople.mockResolvedValue({ people: [nina], nextCursor: null })
    renderSearch()
    openPicker()
    expect(await screen.findByText('Nina Berg')).toBeTruthy()
    expect(box().placeholder).toBe('Navn eller e-post')
    expect(box().maxLength).toBe(100)
    expect(orgClient.listOrganizationMembers).not.toHaveBeenCalled()
  })

  it('opens with a first page for the empty query, 20 at a time, leaving out the room’s own members', async () => {
    renderSearch()
    openPicker()
    await waitFor(() => expect(spacesClient.searchDirectoryPeople).toHaveBeenCalledTimes(1))
    expect(searchCalls()[0]?.[0]).toBe('')
    expect(searchCalls()[0]?.[1]).toEqual({ limit: 20, cursor: null, excludeMemberOf: 'space_1' })
  })

  it('does not search before the picker is opened', async () => {
    renderSearch()
    await sleep(50)
    expect(spacesClient.searchDirectoryPeople).not.toHaveBeenCalled()
  })

  it('debounces typing to one request 250 ms after the last keystroke', async () => {
    renderSearch()
    openPicker()
    await waitFor(() => expect(spacesClient.searchDirectoryPeople).toHaveBeenCalledTimes(1))
    type('k')
    await sleep(100)
    type('ka')
    await sleep(100)
    type('kar')
    // 200 ms since the first keystroke, 0 since the last: nothing yet.
    expect(spacesClient.searchDirectoryPeople).toHaveBeenCalledTimes(1)
    await waitFor(() => expect(spacesClient.searchDirectoryPeople).toHaveBeenCalledTimes(2))
    await sleep(400)
    expect(spacesClient.searchDirectoryPeople).toHaveBeenCalledTimes(2)
    expect(searchCalls()[1]?.[0]).toBe('kar')
    expect(searchCalls()[1]?.[1]).toMatchObject({ cursor: null, excludeMemberOf: 'space_1' })
  })

  it('sends the trimmed text', async () => {
    renderSearch()
    openPicker()
    await waitFor(() => expect(spacesClient.searchDirectoryPeople).toHaveBeenCalledTimes(1))
    type('  nina  ')
    await waitFor(() => expect(spacesClient.searchDirectoryPeople).toHaveBeenCalledTimes(2))
    expect(searchCalls()[1]?.[0]).toBe('nina')
  })

  it('drops an answer that arrives after a newer search was made', async () => {
    let answerFirst: (page: unknown) => void = () => undefined
    spacesClient.searchDirectoryPeople
      .mockImplementationOnce(() => new Promise((resolve) => { answerFirst = resolve }))
      .mockResolvedValueOnce({ people: [kari], nextCursor: null })
    renderSearch()
    openPicker()
    await waitFor(() => expect(spacesClient.searchDirectoryPeople).toHaveBeenCalledTimes(1))
    type('kari')
    expect(await screen.findByText('Kari Solheim')).toBeTruthy()

    // The slow first page finally lands: it must not replace the newer list.
    answerFirst({ people: [nina], nextCursor: 'old-cursor' })
    await sleep(50)
    expect(screen.queryByText('Nina Berg')).toBeNull()
    expect(screen.getByText('Kari Solheim')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Vis flere' })).toBeNull()
  })

  it('Show more appends the next page using the server’s cursor, and goes away at the end', async () => {
    spacesClient.searchDirectoryPeople
      .mockResolvedValueOnce({ people: [nina], nextCursor: 'cursor-2' })
      .mockResolvedValueOnce({ people: [kari], nextCursor: null })
    renderSearch()
    openPicker()
    expect(await screen.findByText('Nina Berg')).toBeTruthy()
    screen.getByRole('button', { name: 'Vis flere' }).click()
    expect(await screen.findByText('Kari Solheim')).toBeTruthy()
    expect(screen.getByText('Nina Berg')).toBeTruthy()
    expect(searchCalls()[1]?.[1]).toMatchObject({ cursor: 'cursor-2', limit: 20, excludeMemberOf: 'space_1' })
    expect(screen.queryByRole('button', { name: 'Vis flere' })).toBeNull()
  })

  it('a new search starts from the first page again and replaces the list', async () => {
    spacesClient.searchDirectoryPeople
      .mockResolvedValueOnce({ people: [nina], nextCursor: 'cursor-2' })
      .mockResolvedValueOnce({ people: [kari], nextCursor: null })
    renderSearch()
    openPicker()
    await screen.findByText('Nina Berg')
    type('kari')
    expect(await screen.findByText('Kari Solheim')).toBeTruthy()
    expect(screen.queryByText('Nina Berg')).toBeNull()
    expect(searchCalls()[1]?.[1]).toMatchObject({ cursor: null })
  })

  it('hides people who are already on the room’s roster, even if the server sends them', async () => {
    spacesClient.searchDirectoryPeople.mockResolvedValue({ people: [ola, nina], nextCursor: null })
    renderSearch()
    openPicker()
    expect(await screen.findByText('Nina Berg')).toBeTruthy()
    expect(screen.queryByText('Ola Hansen')).toBeNull()
    expect(screen.getAllByRole('button', { name: 'Legg til' })).toHaveLength(1)
  })

  it('adds a found person by their subject id', async () => {
    spacesClient.searchDirectoryPeople.mockResolvedValue({ people: [nina], nextCursor: null })
    renderSearch()
    openPicker()
    ;(await screen.findByRole('button', { name: 'Legg til' })).click()
    await waitFor(() => expect(spacesClient.addSpaceMember).toHaveBeenCalledWith('space_1', 'user_3', undefined))
  })

  it('says nothing matches a search that found nobody, and nothing left to add for the empty query', async () => {
    spacesClient.searchDirectoryPeople.mockResolvedValue({ people: [], nextCursor: null })
    renderSearch()
    openPicker()
    expect(await screen.findByText('Ingen flere personer å legge til i dette rommet.')).toBeTruthy()
    type('zzz')
    expect(await screen.findByText('Ingen personer passer søket.')).toBeTruthy()
  })

  it('shows a typed error, not an empty list, for a disabled, unavailable or rate-limited search', async () => {
    const cases: Array<[number, string, string]> = [
      [404, 'feature_disabled', 'Denne funksjonen er ikke slått på for deg ennå.'],
      [503, 'directory_unavailable', 'Personsøket er utilgjengelig akkurat nå. Prøv igjen om litt.'],
      // `rate_limited` is one app-wide sentence (retryable); a 429 with no retry hint ends with "Prøv igjen om litt."
      [429, 'rate_limited', 'Det kommer for mange forespørsler akkurat nå. Prøv igjen om litt.'],
    ]
    for (const [status, code, expected] of cases) {
      spacesClient.searchDirectoryPeople.mockReset().mockRejectedValue(new ApiError('x', status, code))
      renderSearch()
      openPicker()
      const alert = await screen.findByRole('alert')
      expect(alert.textContent, code).toBe(expected)
      expect(screen.queryByText('Ingen personer passer søket.'), code).toBeNull()
      expect(screen.queryByText('Ingen flere personer å legge til i dette rommet.'), code).toBeNull()
      expect(screen.queryByRole('button', { name: 'Legg til' }), code).toBeNull()
      cleanup()
    }
  })

  it('uses its own sentence for a failure with no code', async () => {
    // A 5xx with no code: a lost connection (TypeError) is answered by the network copy before the caller's sentence.
    spacesClient.searchDirectoryPeople.mockRejectedValue(new ApiError('x', 500, null))
    renderSearch()
    openPicker()
    expect((await screen.findByRole('alert')).textContent).toBe('Personsøket mislyktes. Prøv igjen om litt.')
  })

  it('a failed Show more keeps the people already found and says why', async () => {
    spacesClient.searchDirectoryPeople
      .mockResolvedValueOnce({ people: [nina], nextCursor: 'cursor-2' })
      .mockRejectedValueOnce(new ApiError('x', 503, 'directory_unavailable'))
    renderSearch()
    openPicker()
    await screen.findByText('Nina Berg')
    screen.getByRole('button', { name: 'Vis flere' }).click()
    expect((await screen.findByRole('alert')).textContent).toContain('Personsøket er utilgjengelig')
    expect(screen.getByText('Nina Berg')).toBeTruthy()
  })

  it('a recovered search clears the earlier error', async () => {
    spacesClient.searchDirectoryPeople
      .mockRejectedValueOnce(new ApiError('x', 503, 'directory_unavailable'))
      .mockResolvedValueOnce({ people: [nina], nextCursor: null })
    renderSearch()
    openPicker()
    await screen.findByRole('alert')
    type('nina')
    expect(await screen.findByText('Nina Berg')).toBeTruthy()
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('with peopleSearch off the picker lists the organisation as before, with no search box', async () => {
    renderSearch(false)
    openPicker()
    expect(await screen.findByText('Nina Berg')).toBeTruthy()
    expect(screen.queryByRole('searchbox')).toBeNull()
    expect(orgClient.listOrganizationMembers).toHaveBeenCalledWith('org_1')
    expect(spacesClient.searchDirectoryPeople).not.toHaveBeenCalled()
    expect(screen.queryByRole('button', { name: 'Vis flere' })).toBeNull()
  })

  it('with peopleSearch undefined (the server did not say) it is not treated as on', async () => {
    renderSearch('absent')
    openPicker()
    expect(await screen.findByText('Nina Berg')).toBeTruthy()
    expect(screen.queryByRole('searchbox')).toBeNull()
    expect(orgClient.listOrganizationMembers).toHaveBeenCalledTimes(1)
    expect(spacesClient.searchDirectoryPeople).not.toHaveBeenCalled()
  })

  it('stops searching once the component is removed', async () => {
    const view = renderSearch()
    openPicker()
    await waitFor(() => expect(spacesClient.searchDirectoryPeople).toHaveBeenCalledTimes(1))
    type('kari')
    view.unmount()
    await sleep(400)
    expect(spacesClient.searchDirectoryPeople).toHaveBeenCalledTimes(1)
  })
})
