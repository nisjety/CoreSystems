import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'

const spacesClient = vi.hoisted(() => ({
  searchSpaceMessages: vi.fn(() => new Promise(() => {})),
  updateSpaceRoomDetails: vi.fn(),
  updateSpaceRoomSettings: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))

const { SpaceRoomSettings, canEditRoomDetails } = await import('./SpaceRoomSettings')

type Overrides = Partial<{
  role: string
  kind: string
  lifecycle: string
  enabled: boolean
  name: string
  topic: string | null | undefined
  onSaved: (details: unknown) => void
}>

// No I18nProvider wraps these renders, so the copy is the Norwegian default, as in the page tests.
function renderSettings(overrides: Overrides = {}) {
  return render(() => (
    <SpaceRoomSettings
      spaceRef="room_1"
      name={overrides.name ?? 'Lager'}
      topic={'topic' in overrides ? overrides.topic : 'Innkjøp'}
      role={overrides.role ?? 'owner'}
      kind={overrides.kind ?? 'room'}
      lifecycle={overrides.lifecycle ?? 'active'}
      enabled={overrides.enabled ?? true}
      onSaved={overrides.onSaved as never}
    />
  ))
}

const editButton = () => screen.queryByRole('button', { name: 'Rediger navn og emne' })
const nameInput = () => screen.getByLabelText('Navn') as HTMLInputElement
const topicInput = () => screen.getByLabelText('Emne') as HTMLInputElement

function type(input: HTMLInputElement, value: string) {
  fireEvent.input(input, { target: { value } })
  flush()
}

beforeEach(() => {
  spacesClient.updateSpaceRoomDetails.mockReset()
  spacesClient.updateSpaceRoomDetails.mockResolvedValue({ spaceRef: 'room_1', name: 'Lager', topic: 'Innkjøp' })
  spacesClient.updateSpaceRoomSettings.mockReset()
  spacesClient.updateSpaceRoomSettings.mockResolvedValue({ visibility: 'private', joinRole: 'viewer', historyMode: null, changed: true })
})

afterEach(() => cleanup())

describe('canEditRoomDetails', () => {
  it('is owner and manager only', () => {
    expect(canEditRoomDetails('owner')).toBe(true)
    expect(canEditRoomDetails('manager')).toBe(true)
    for (const role of ['editor', 'viewer', 'guest', '', 'OWNER']) expect(canEditRoomDetails(role), role).toBe(false)
  })
})

describe('SpaceRoomSettings visibility (WS04-S2)', () => {
  it('shows the edit control to an owner and to a manager of an active room with the flag on', () => {
    renderSettings({ role: 'owner' })
    expect(editButton()).toBeTruthy()
    cleanup()
    renderSettings({ role: 'manager' })
    expect(editButton()).toBeTruthy()
  })

  it('is absent for an editor and a viewer', () => {
    for (const role of ['editor', 'viewer']) {
      renderSettings({ role })
      expect(editButton(), role).toBeNull()
      expect(document.querySelector('.verevon-space-room-settings'), role).toBeNull()
      cleanup()
    }
  })

  it('is absent for a personal Space and any kind that is not a room', () => {
    for (const kind of ['personal', 'org_channel', '']) {
      renderSettings({ kind })
      expect(editButton(), kind).toBeNull()
      cleanup()
    }
  })

  it('is absent for a room that is not active', () => {
    for (const lifecycle of ['archived', 'deleted', 'pending_deletion', '']) {
      renderSettings({ lifecycle })
      expect(editButton(), lifecycle).toBeNull()
      cleanup()
    }
  })

  it('is absent with the flag off', () => {
    renderSettings({ enabled: false })
    expect(editButton()).toBeNull()
  })

  it('opens with the stored name and topic, and an empty topic when there is none', () => {
    renderSettings()
    fireEvent.click(editButton() as HTMLElement)
    flush()
    expect(nameInput().value).toBe('Lager')
    expect(topicInput().value).toBe('Innkjøp')
    cleanup()
    renderSettings({ topic: null })
    fireEvent.click(editButton() as HTMLElement)
    flush()
    expect(topicInput().value).toBe('')
  })

  it('stops typing past the name and topic limits', () => {
    renderSettings()
    fireEvent.click(editButton() as HTMLElement)
    flush()
    expect(nameInput().maxLength).toBe(120)
    expect(topicInput().maxLength).toBe(250)
    expect(nameInput().required).toBe(true)
  })

  it('Cancel closes the form and sends nothing', () => {
    renderSettings()
    fireEvent.click(editButton() as HTMLElement)
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Avbryt' }))
    flush()
    expect(editButton()).toBeTruthy()
    expect(spacesClient.updateSpaceRoomDetails).not.toHaveBeenCalled()
  })
})

describe('SpaceRoomSettings save (WS04-S2)', () => {
  const open = () => {
    fireEvent.click(editButton() as HTMLElement)
    flush()
  }
  const save = () => fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))

  it('sends only the name when only the name changed', async () => {
    renderSettings()
    open()
    type(nameInput(), 'Lager Oslo')
    save()
    await waitFor(() => expect(spacesClient.updateSpaceRoomDetails).toHaveBeenCalledTimes(1))
    expect(spacesClient.updateSpaceRoomDetails).toHaveBeenCalledWith('room_1', { name: 'Lager Oslo' })
  })

  it('sends only the topic when only the topic changed', async () => {
    renderSettings()
    open()
    type(topicInput(), 'Drift')
    save()
    await waitFor(() => expect(spacesClient.updateSpaceRoomDetails).toHaveBeenCalledTimes(1))
    expect(spacesClient.updateSpaceRoomDetails).toHaveBeenCalledWith('room_1', { topic: 'Drift' })
  })

  it('sends both when both changed', async () => {
    renderSettings()
    open()
    type(nameInput(), 'Ny')
    type(topicInput(), 'Emne')
    save()
    await waitFor(() => expect(spacesClient.updateSpaceRoomDetails).toHaveBeenCalledTimes(1))
    expect(spacesClient.updateSpaceRoomDetails).toHaveBeenCalledWith('room_1', { name: 'Ny', topic: 'Emne' })
  })

  it('clears the topic by sending an empty one', async () => {
    renderSettings()
    open()
    type(topicInput(), '')
    save()
    await waitFor(() => expect(spacesClient.updateSpaceRoomDetails).toHaveBeenCalledTimes(1))
    expect(spacesClient.updateSpaceRoomDetails).toHaveBeenCalledWith('room_1', { topic: '' })
  })

  it('sends nothing and closes when nothing changed', async () => {
    renderSettings()
    open()
    save()
    flush()
    await waitFor(() => expect(editButton()).toBeTruthy())
    expect(spacesClient.updateSpaceRoomDetails).not.toHaveBeenCalled()
  })

  it('hands the stored values to onSaved and closes the form', async () => {
    const onSaved = vi.fn()
    spacesClient.updateSpaceRoomDetails.mockResolvedValue({ spaceRef: 'room_1', name: 'Lager Oslo', topic: 'Innkjøp' })
    renderSettings({ onSaved })
    open()
    type(nameInput(), 'Lager Oslo')
    save()
    await waitFor(() => expect(onSaved).toHaveBeenCalledWith({ spaceRef: 'room_1', name: 'Lager Oslo', topic: 'Innkjøp' }))
    await waitFor(() => expect(editButton()).toBeTruthy())
  })

  it('disables the controls and says Saving while the request is out, and ignores a second submit', async () => {
    let finish: (value: unknown) => void = () => undefined
    spacesClient.updateSpaceRoomDetails.mockImplementation(() => new Promise((resolve) => { finish = resolve }))
    renderSettings()
    open()
    type(nameInput(), 'Ny')
    save()
    await waitFor(() => expect(screen.getByRole('button', { name: 'Lagrer...' })).toBeTruthy())
    expect((screen.getByRole('button', { name: 'Lagrer...' }) as HTMLButtonElement).disabled).toBe(true)
    expect(nameInput().disabled).toBe(true)
    fireEvent.submit(nameInput().closest('form') as HTMLFormElement)
    expect(spacesClient.updateSpaceRoomDetails).toHaveBeenCalledTimes(1)
    finish({ spaceRef: 'room_1', name: 'Ny', topic: 'Innkjøp' })
    await waitFor(() => expect(editButton()).toBeTruthy())
  })
})

describe('SpaceRoomSettings refusals (WS04-S2)', () => {
  const failWith = async (error: unknown, edit: (name: HTMLInputElement, topic: HTMLInputElement) => void) => {
    spacesClient.updateSpaceRoomDetails.mockRejectedValue(error)
    renderSettings()
    fireEvent.click(editButton() as HTMLElement)
    flush()
    edit(nameInput(), topicInput())
    fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))
    return (await screen.findByRole('alert')).textContent ?? ''
  }

  const invalid = (field: string, reason: string) =>
    new ApiError('bad', 400, 'room_details_invalid', { details: { field, reason } })

  it('maps room_details_invalid details to the field copy', async () => {
    const cases: Array<[string, string, RegExp]> = [
      ['name', 'name_required', /Rommet må ha et navn/],
      ['name', 'name_too_long', /Navnet kan ha opptil 120 tegn/],
      ['name', 'too_long', /Navnet kan ha opptil 120 tegn/],
      ['topic', 'topic_too_long', /Emnet kan ha opptil 250 tegn/],
      ['topic', 'too_long', /Emnet kan ha opptil 250 tegn/],
      ['name', 'control_character', /kan ikke inneholde kontrolltegn/],
      ['topic', 'bidi_override', /kan ikke inneholde kontrolltegn/],
    ]
    for (const [field, reason, expected] of cases) {
      const text = await failWith(invalid(field, reason), (name) => type(name, 'X'))
      expect(text, `${field}/${reason}`).toMatch(expected)
      cleanup()
    }
  })

  // A reason the form has no words for. With no `reason` key at all the error is not mistaken for a
  // typed Space denial, so the catalogue text for the code itself is shown.
  it('falls back to the catalogue text for room_details_invalid with details it cannot map', async () => {
    const text = await failWith(
      new ApiError('bad', 400, 'room_details_invalid', { details: { field: 'name' } }),
      (name) => type(name, 'X'),
    )
    expect(text).toBe('Navnet eller emnet er ikke gyldig. Ingenting er endret.')
    cleanup()
    const noDetails = await failWith(new ApiError('bad', 400, 'room_details_invalid'), (name) => type(name, 'X'))
    expect(noDetails).toBe('Navnet eller emnet er ikke gyldig. Ingenting er endret.')
  })

  it('renders every other code through translateApiError', async () => {
    const cases: Array<[number, string, string]> = [
      [403, 'room_role_forbidden', 'Rollen din i dette rommet tillater ikke dette.'],
      [409, 'room_not_active', 'Rommet er ikke aktivt, så navn og emne kan ikke endres.'],
      [409, 'space_kind_not_editable', 'Navn og emne kan bare endres på et vanlig rom, ikke på et personlig rom.'],
      [503, 'room_details_unavailable', 'Navnet og emnet kunne ikke lagres. Ingenting er endret. Prøv igjen om litt.'],
      [404, 'feature_disabled', 'Denne funksjonen er ikke slått på for deg ennå.'],
    ]
    for (const [status, code, expected] of cases) {
      const text = await failWith(new ApiError('x', status, code), (name) => type(name, 'X'))
      expect(text, code).toBe(expected)
      cleanup()
    }
  })

  it('uses its own "nothing was changed" sentence for a failure with no code', async () => {
    // A 5xx with no code: a lost connection (TypeError) is answered by the network copy before the caller's sentence.
    const text = await failWith(new ApiError('x', 500, null), (name) => type(name, 'X'))
    expect(text).toBe('Navnet og emnet kunne ikke lagres. Ingenting er endret.')
  })

  it('keeps the form open with the typed text after a refusal, and does not call onSaved', async () => {
    const onSaved = vi.fn()
    spacesClient.updateSpaceRoomDetails.mockRejectedValue(new ApiError('x', 403, 'room_role_forbidden'))
    renderSettings({ onSaved })
    fireEvent.click(editButton() as HTMLElement)
    flush()
    type(nameInput(), 'Beholdt')
    fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))
    await screen.findByRole('alert')
    expect(nameInput().value).toBe('Beholdt')
    expect(onSaved).not.toHaveBeenCalled()
  })

  it('clears an earlier error when the form is opened again', async () => {
    spacesClient.updateSpaceRoomDetails.mockRejectedValue(new ApiError('x', 403, 'room_role_forbidden'))
    renderSettings()
    fireEvent.click(editButton() as HTMLElement)
    flush()
    type(nameInput(), 'X')
    fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))
    await screen.findByRole('alert')
    fireEvent.click(screen.getByRole('button', { name: 'Avbryt' }))
    flush()
    fireEvent.click(editButton() as HTMLElement)
    flush()
    expect(screen.queryByRole('alert')).toBeNull()
  })
})

// WS04-S8: who can find the room. Owner and manager only, on an active shared room with the directory flag on.
// Anywhere else the block is absent from the page, not merely disabled.
describe('SpaceRoomSettings who-can-find-the-room block (WS04-S8)', () => {
  type VisibilityOverrides = Partial<{
    role: string
    kind: string
    lifecycle: string
    archived: boolean
    directoryEnabled: boolean | undefined
    isOrganizationRoom: boolean
    visibility: string | null
    joinRole: string | null
  }>

  function renderVisibility(overrides: VisibilityOverrides = {}) {
    return render(() => (
      <SpaceRoomSettings
        spaceRef="room_1"
        name="Lager"
        topic="Innkjøp"
        role={overrides.role ?? 'owner'}
        kind={overrides.kind ?? 'room'}
        lifecycle={overrides.lifecycle ?? 'active'}
        archived={overrides.archived}
        enabled
        directoryEnabled={'directoryEnabled' in overrides ? overrides.directoryEnabled : true}
        isOrganizationRoom={overrides.isOrganizationRoom ?? false}
        visibility={'visibility' in overrides ? overrides.visibility : 'private'}
        joinRole={'joinRole' in overrides ? overrides.joinRole : 'viewer'}
      />
    ))
  }

  const block = () => document.querySelector('.verevon-space-room-visibility')
  const radios = () => document.querySelectorAll('input[type="radio"]')
  const joinRoleSelect = () => document.querySelector('.verevon-space-room-visibility select') as HTMLSelectElement | null
  const choose = (name: RegExp) => {
    fireEvent.click(screen.getByRole('radio', { name }))
    flush()
  }
  const save = () => fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))
  const settle = () => new Promise((resolve) => setTimeout(resolve, 0))
  const sentBody = () => spacesClient.updateSpaceRoomSettings.mock.calls[0]?.[1]

  it('is shown to an owner and to a manager of an active shared room with the directory on', () => {
    renderVisibility({ role: 'owner' })
    expect(block()).toBeTruthy()
    expect(screen.getByText('Hvem kan finne rommet')).toBeTruthy()
    expect(radios()).toHaveLength(2)
    cleanup()
    renderVisibility({ role: 'manager' })
    expect(block()).toBeTruthy()
  })

  it('is absent from the page, not merely disabled, for a viewer and an editor', () => {
    for (const role of ['viewer', 'editor']) {
      renderVisibility({ role })
      expect(block(), role).toBeNull()
      expect(screen.queryByText('Hvem kan finne rommet'), role).toBeNull()
      expect(radios(), role).toHaveLength(0)
      cleanup()
    }
  })

  it.each([
    ['an archived room', { archived: true }],
    ['an organization room', { isOrganizationRoom: true }],
    ['a personal Space', { kind: 'personal' }],
    ['a room with the directory flag off', { directoryEnabled: false }],
    ['a room whose gateway sends no directory flag', { directoryEnabled: undefined }],
  ])('is absent from the page, not merely disabled, for %s', (_label, overrides) => {
    renderVisibility(overrides)
    expect(block()).toBeNull()
    expect(screen.queryByText('Hvem kan finne rommet')).toBeNull()
    expect(radios()).toHaveLength(0)
  })

  it('sends only the visibility when only the visibility changed', async () => {
    renderVisibility({ visibility: 'private', joinRole: 'viewer' })
    choose(/Åpent for organisasjonen/)
    save()
    await waitFor(() => expect(spacesClient.updateSpaceRoomSettings).toHaveBeenCalledTimes(1))
    expect(spacesClient.updateSpaceRoomSettings.mock.calls[0]?.[0]).toBe('room_1')
    expect(sentBody()).toStrictEqual({ visibility: 'org_public' })
  })

  it('sends only the join role when only the join role changed', async () => {
    renderVisibility({ visibility: 'org_public', joinRole: 'viewer' })
    fireEvent.change(joinRoleSelect() as HTMLSelectElement, { target: { value: 'editor' } })
    flush()
    save()
    await waitFor(() => expect(spacesClient.updateSpaceRoomSettings).toHaveBeenCalledTimes(1))
    expect(sentBody()).toStrictEqual({ joinRole: 'editor' })
  })

  it('sends both when both changed', async () => {
    renderVisibility({ visibility: 'private', joinRole: 'viewer' })
    choose(/Åpent for organisasjonen/)
    fireEvent.change(joinRoleSelect() as HTMLSelectElement, { target: { value: 'editor' } })
    flush()
    save()
    await waitFor(() => expect(spacesClient.updateSpaceRoomSettings).toHaveBeenCalledTimes(1))
    expect(sentBody()).toStrictEqual({ visibility: 'org_public', joinRole: 'editor' })
  })

  it('sends nothing when nothing changed', async () => {
    renderVisibility({ visibility: 'org_public', joinRole: 'editor' })
    save()
    await settle()
    expect(spacesClient.updateSpaceRoomSettings).not.toHaveBeenCalled()
  })

  it('offers the join role only while the room is open to the organization', () => {
    renderVisibility({ visibility: 'private' })
    expect(joinRoleSelect()).toBeNull()
    choose(/Åpent for organisasjonen/)
    expect(joinRoleSelect()).not.toBeNull()
    expect(Array.from(joinRoleSelect()?.options ?? []).map((option) => option.textContent)).toEqual(['Leser', 'Redaktør'])
    choose(/Privat/)
    expect(joinRoleSelect()).toBeNull()
    cleanup()
    renderVisibility({ visibility: 'org_public' })
    expect(joinRoleSelect()).not.toBeNull()
    expect(joinRoleSelect()?.value).toBe('viewer')
  })

  it('says the directory policy refused the change, in the directory copy', async () => {
    spacesClient.updateSpaceRoomSettings.mockRejectedValue(new ApiError('x', 403, 'directory_disabled'))
    renderVisibility({ visibility: 'private' })
    choose(/Åpent for organisasjonen/)
    save()
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toBe(
      'Organisasjonen din har ikke slått på romkatalogen, så rom kan ikke åpnes for alle ennå. Spør en administrator.',
    )
  })

  it('says the members follow the organization when the room roster is derived', async () => {
    spacesClient.updateSpaceRoomSettings.mockRejectedValue(new ApiError('x', 409, 'room_roster_derived'))
    renderVisibility({ visibility: 'private' })
    choose(/Åpent for organisasjonen/)
    save()
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toBe('Medlemmene i dette rommet følger organisasjonen og redigeres ikke her.')
    expect(alert.textContent).not.toContain('Innstillingene kunne ikke lagres')
  })
})
