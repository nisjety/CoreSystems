import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const spacesClient = vi.hoisted(() => ({
  searchSpaceMessages: vi.fn(() => new Promise(() => {})),
  archiveSpaceRoom: vi.fn(),
  unarchiveSpaceRoom: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))

const { SpaceArchivedBanner, SpaceArchivedNotice, archiveErrorCopy, canArchiveRoom } = await import('./SpaceRoomArchive')

// No I18nProvider wraps these renders, so the copy is the Norwegian default, as in the page tests.
const tr = (no: string) => no

beforeEach(() => {
  spacesClient.archiveSpaceRoom.mockReset()
  spacesClient.unarchiveSpaceRoom.mockReset()
  spacesClient.unarchiveSpaceRoom.mockResolvedValue({ state: 'active', changed: true, archivedAt: null })
})

afterEach(() => cleanup())

describe('canArchiveRoom', () => {
  const base = { role: 'owner', kind: 'room', isOrganizationRoom: false as boolean | undefined, featureOn: true }

  it('is an owner or manager of a named room with the flag on', () => {
    expect(canArchiveRoom(base)).toBe(true)
    expect(canArchiveRoom({ ...base, role: 'manager' })).toBe(true)
  })

  it.each([
    ['an editor', { role: 'editor' }],
    ['a viewer', { role: 'viewer' }],
    ['the flag off', { featureOn: false }],
    ['a personal Space', { kind: 'personal' }],
    ['the organization room', { isOrganizationRoom: true }],
    ['an unknown organization-room answer', { isOrganizationRoom: undefined }],
  ])('is false for %s', (_name, change) => {
    expect(canArchiveRoom({ ...base, ...change })).toBe(false)
  })
})

describe('archiveErrorCopy', () => {
  const err = (code: string, details?: Record<string, unknown>) => Object.assign(new Error(code), { status: 400, code, details })

  it('names the wait on a 429', () => {
    expect(archiveErrorCopy(err('rate_limited', { retry_after_seconds: 7.2 }), tr, 'archive')).toContain('8 sekunder')
    expect(archiveErrorCopy(err('rate_limited'), tr, 'archive')).toContain('Prøv igjen om litt')
  })

  it('says nothing was changed on room_admin_unavailable', () => {
    expect(archiveErrorCopy(err('room_admin_unavailable'), tr, 'archive')).toContain('Ingenting er endret')
  })

  it('has its own words for the organization room and a state conflict', () => {
    expect(archiveErrorCopy(err('room_not_archivable'), tr, 'archive')).toContain('Organisasjonens eget rom')
    expect(archiveErrorCopy(err('room_state_conflict'), tr, 'archive')).toContain('endret av noen andre')
  })

  it('falls to the shared translator for a role refusal, and to the action fallback for an unknown code', () => {
    expect(archiveErrorCopy(err('room_role_forbidden'), tr, 'archive')).toContain('Rollen din')
    expect(archiveErrorCopy(err('something_new'), tr, 'archive')).toBe('Rommet kunne ikke arkiveres. Ingenting er endret.')
    expect(archiveErrorCopy(err('something_new'), tr, 'unarchive')).toBe('Arkiveringen kunne ikke fjernes. Ingenting er endret.')
  })
})

describe('SpaceArchivedBanner', () => {
  it('says the room is archived and that reads still work', () => {
    render(() => <SpaceArchivedBanner spaceRef="room_1" canUnarchive={false} />)
    expect(screen.getByRole('status').textContent).toContain('Rommet er arkivert')
    expect(screen.getByRole('status').textContent).toContain('Du kan lese alt')
  })

  it('offers Unarchive only to someone who may, and reports the change', async () => {
    const onChanged = vi.fn()
    render(() => <SpaceArchivedBanner spaceRef="room_1" canUnarchive onChanged={onChanged} />)
    fireEvent.click(screen.getByRole('button', { name: 'Fjern arkivering' }))
    await waitFor(() => expect(spacesClient.unarchiveSpaceRoom).toHaveBeenCalledTimes(1))
    expect(spacesClient.unarchiveSpaceRoom.mock.calls[0]?.[0]).toBe('room_1')
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1))
  })

  it('has no button for a member who may not unarchive', () => {
    render(() => <SpaceArchivedBanner spaceRef="room_1" canUnarchive={false} />)
    expect(screen.queryByRole('button')).toBeNull()
  })

  it('shows the refusal and leaves the banner up', async () => {
    spacesClient.unarchiveSpaceRoom.mockRejectedValue(Object.assign(new Error('x'), { status: 403, code: 'room_role_forbidden' }))
    const onChanged = vi.fn()
    render(() => <SpaceArchivedBanner spaceRef="room_1" canUnarchive onChanged={onChanged} />)
    fireEvent.click(screen.getByRole('button', { name: 'Fjern arkivering' }))
    expect((await screen.findByRole('alert')).textContent).toContain('Rollen din')
    expect(onChanged).not.toHaveBeenCalled()
  })

  it('does not fire a second call while the first is in flight', async () => {
    let finish!: (value: unknown) => void
    spacesClient.unarchiveSpaceRoom.mockReturnValue(new Promise((resolve) => { finish = resolve }))
    render(() => <SpaceArchivedBanner spaceRef="room_1" canUnarchive />)
    const button = screen.getByRole('button', { name: 'Fjern arkivering' })
    fireEvent.click(button)
    fireEvent.click(button)
    expect(spacesClient.unarchiveSpaceRoom).toHaveBeenCalledTimes(1)
    finish({ state: 'active', changed: true, archivedAt: null })
  })
})

describe('SpaceArchivedNotice', () => {
  it('tells the reader messages cannot be sent', () => {
    render(() => <SpaceArchivedNotice />)
    expect(document.querySelector('.verevon-space-archived-notice')?.textContent).toContain('skrivebeskyttet')
  })
})
