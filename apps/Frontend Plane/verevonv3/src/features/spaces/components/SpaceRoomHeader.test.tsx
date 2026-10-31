import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { SpaceRosterMember } from '@/shared/api/spaces-client'

const spacesClient = vi.hoisted(() => ({
  updateSpaceRoomDetails: vi.fn(),
  archiveSpaceRoom: vi.fn(),
  unarchiveSpaceRoom: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', () => spacesClient)

const { SpaceRoomHeader, headerPeople, memberTint, HEADER_AVATAR_LIMIT } = await import('./SpaceRoomHeader')

const member = (id: string, name: string, type: 'user' | 'service' = 'user'): SpaceRosterMember => ({
  subject_type: type,
  subject_id: id,
  role: 'editor',
  revision: 1,
  display_name: name,
})

const ROSTER: SpaceRosterMember[] = [
  member('u1', 'Sara Nilsen'),
  member('u2', 'Ola Hansen'),
  member('u3', 'Kari Berg'),
  member('u4', 'Per Olsen'),
  member('u5', 'Eva Moen'),
  member('u6', 'Jon Dahl'),
  member('agent_1', 'Verevon', 'service'),
]

type Overrides = Partial<{
  role: string
  kind: string
  archived: boolean
  isOrganizationRoom: boolean | undefined
  roomDetailsEnabled: boolean
  roomArchiveEnabled: boolean
  roster: readonly SpaceRosterMember[] | undefined
  topic: string | null
  onOpenMembers: () => void
  onArchiveChanged: () => void
}>

// No I18nProvider wraps these renders, so the copy is the Norwegian default, as in the page tests.
function renderHeader(overrides: Overrides = {}) {
  return render(() => (
    <SpaceRoomHeader
      spaceRef="room_1"
      name="Innkjøp"
      topic={'topic' in overrides ? overrides.topic : 'Ukentlig oppfølging av innkjøp'}
      kind={overrides.kind ?? 'room'}
      lifecycle="active"
      role={overrides.role ?? 'owner'}
      isOrganizationRoom={'isOrganizationRoom' in overrides ? overrides.isOrganizationRoom : false}
      archived={overrides.archived ?? false}
      roomDetailsEnabled={overrides.roomDetailsEnabled ?? true}
      roomArchiveEnabled={overrides.roomArchiveEnabled ?? true}
      roster={'roster' in overrides ? overrides.roster : ROSTER}
      facts={[
        { label: 'Type', value: 'Rom' },
        { label: 'Din rolle', value: 'Eier' },
      ]}
      onOpenMembers={overrides.onOpenMembers ?? (() => undefined)}
      onArchiveChanged={overrides.onArchiveChanged}
    />
  ))
}

const detailsButton = () => screen.getByRole('button', { name: 'Romdetaljer' })
const openDetails = () => {
  fireEvent.click(detailsButton())
  flush()
}

beforeEach(() => {
  spacesClient.updateSpaceRoomDetails.mockReset()
  spacesClient.archiveSpaceRoom.mockReset()
  spacesClient.unarchiveSpaceRoom.mockReset()
  spacesClient.archiveSpaceRoom.mockResolvedValue({ state: 'archived', changed: true, archivedAt: '2026-10-31T09:30:00Z' })
  spacesClient.unarchiveSpaceRoom.mockResolvedValue({ state: 'active', changed: true, archivedAt: null })
})

afterEach(() => cleanup())

describe('headerPeople', () => {
  it('draws at most four people, counts all of them, and leaves agents out', () => {
    const { shown, total } = headerPeople(ROSTER)
    expect(HEADER_AVATAR_LIMIT).toBe(4)
    expect(shown.map((m) => m.subject_id)).toEqual(['u1', 'u2', 'u3', 'u4'])
    expect(total).toBe(6)
  })

  it('draws everyone when there are fewer than the limit', () => {
    expect(headerPeople(ROSTER.slice(0, 2))).toMatchObject({ total: 2 })
    expect(headerPeople(ROSTER.slice(0, 2)).shown).toHaveLength(2)
    expect(headerPeople([])).toEqual({ shown: [], total: 0 })
  })
})

describe('memberTint', () => {
  it('is stable for the same member and always one of the palette colours', () => {
    expect(memberTint('u1')).toBe(memberTint('u1'))
    expect(memberTint('u1')).toMatch(/^#[0-9a-f]{6}$/)
    expect(memberTint('')).toMatch(/^#[0-9a-f]{6}$/)
  })
})

describe('SpaceRoomHeader (U4)', () => {
  it('shows the name, then the topic, and no rename button in the header row', () => {
    renderHeader()
    expect(screen.getByRole('heading', { level: 1, name: 'Innkjøp' })).toBeTruthy()
    expect(document.querySelector('.verevon-space-header__topic')?.textContent).toBe('Ukentlig oppfølging av innkjøp')
    expect(screen.queryByRole('button', { name: 'Rediger navn og emne' })).toBeNull()
  })

  it('draws the avatar stack with initials and the count of people', () => {
    renderHeader()
    const avatars = [...document.querySelectorAll('.verevon-space-header__avatar')].map((el) => el.textContent)
    expect(avatars).toEqual(['SN', 'OH', 'KB', 'PO'])
    expect(document.querySelector('.verevon-space-header__count')?.textContent).toBe('6')
    expect(screen.getByRole('button', { name: 'Vis medlemmer (6)' })).toBeTruthy()
  })

  it('the stack opens the members tab', () => {
    const onOpenMembers = vi.fn()
    renderHeader({ onOpenMembers })
    fireEvent.click(screen.getByRole('button', { name: 'Vis medlemmer (6)' }))
    expect(onOpenMembers).toHaveBeenCalledTimes(1)
  })

  it('draws no stack while the roster is unknown, or when nobody is in it', () => {
    renderHeader({ roster: undefined })
    expect(document.querySelector('.verevon-space-header__people')).toBeNull()
    cleanup()
    renderHeader({ roster: [] })
    expect(document.querySelector('.verevon-space-header__people')).toBeNull()
  })

  it('the details button opens a dialog with the topic, the facts and the member count', () => {
    renderHeader()
    expect(detailsButton().getAttribute('aria-expanded')).toBe('false')
    expect(screen.queryByRole('dialog')).toBeNull()
    openDetails()
    const dialog = screen.getByRole('dialog', { name: 'Romdetaljer' })
    expect(detailsButton().getAttribute('aria-expanded')).toBe('true')
    expect(within(dialog).getByText('Ukentlig oppfølging av innkjøp')).toBeTruthy()
    expect(within(dialog).getByText('Din rolle')).toBeTruthy()
    expect(within(dialog).getByText('Eier')).toBeTruthy()
    const members = within(dialog).getByText('Medlemmer').closest('div')
    expect(members?.textContent).toContain('6')
  })

  it('moves the rename control into the details, for an owner', () => {
    renderHeader()
    openDetails()
    const dialog = screen.getByRole('dialog')
    expect(within(dialog).getByRole('button', { name: 'Rediger navn og emne' })).toBeTruthy()
  })

  it('offers no rename to an editor, but still shows the details', () => {
    renderHeader({ role: 'editor' })
    openDetails()
    const dialog = screen.getByRole('dialog')
    expect(within(dialog).queryByRole('button', { name: 'Rediger navn og emne' })).toBeNull()
    expect(within(dialog).queryByRole('button', { name: 'Arkiver rommet' })).toBeNull()
  })

  it('closes on Escape and hands focus back to the details button', async () => {
    renderHeader()
    openDetails()
    fireEvent.keyDown(document, { key: 'Escape' })
    flush()
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(document.activeElement).toBe(detailsButton())
  })

  it('closes on a press outside, and stays open on a press inside', async () => {
    renderHeader()
    openDetails()
    fireEvent.mouseDown(screen.getByRole('dialog'))
    flush()
    expect(screen.queryByRole('dialog')).toBeTruthy()
    fireEvent.mouseDown(document.body)
    flush()
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  })

  it('the close button closes it', async () => {
    renderHeader()
    openDetails()
    fireEvent.click(screen.getByRole('button', { name: 'Lukk romdetaljer' }))
    flush()
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  })

  it('"Se medlemmer" closes the panel and opens the members tab', async () => {
    const onOpenMembers = vi.fn()
    renderHeader({ onOpenMembers })
    openDetails()
    fireEvent.click(screen.getByRole('button', { name: 'Se medlemmer' }))
    flush()
    expect(onOpenMembers).toHaveBeenCalledTimes(1)
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  })

  it('an empty topic draws no topic paragraph in the header or the panel', () => {
    renderHeader({ topic: '   ' })
    expect(document.querySelector('.verevon-space-header__topic')).toBeNull()
    openDetails()
    expect(document.querySelector('.verevon-space-details__topic')).toBeNull()
  })
})

describe('SpaceRoomHeader archive (U4)', () => {
  it('offers Archive to an owner and a manager when the flag is on, and archives after a confirm', async () => {
    const onArchiveChanged = vi.fn()
    renderHeader({ onArchiveChanged })
    openDetails()
    fireEvent.click(screen.getByRole('button', { name: 'Arkiver rommet' }))
    flush()
    expect(spacesClient.archiveSpaceRoom).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Bekreft arkivering av rommet' }))
    await waitFor(() => expect(spacesClient.archiveSpaceRoom).toHaveBeenCalledTimes(1))
    expect(spacesClient.archiveSpaceRoom.mock.calls[0]?.[0]).toBe('room_1')
    expect(spacesClient.archiveSpaceRoom.mock.calls[0]?.[1]).toMatchObject({ idempotencyKey: expect.stringMatching(/^space-archive:/) })
    await waitFor(() => expect(onArchiveChanged).toHaveBeenCalledTimes(1))
  })

  it('offers Archive to a manager too', () => {
    renderHeader({ role: 'manager' })
    openDetails()
    expect(screen.getByRole('button', { name: 'Arkiver rommet' })).toBeTruthy()
  })

  it('has no archive control with the flag off, in the organization room, or when the gateway did not say', () => {
    for (const overrides of [
      { roomArchiveEnabled: false },
      { isOrganizationRoom: true },
      { isOrganizationRoom: undefined },
      { kind: 'personal' },
    ] as Overrides[]) {
      renderHeader(overrides)
      openDetails()
      expect(screen.queryByRole('button', { name: 'Arkiver rommet' }), JSON.stringify(overrides)).toBeNull()
      expect(screen.queryByRole('button', { name: 'Fjern arkivering' }), JSON.stringify(overrides)).toBeNull()
      cleanup()
    }
  })

  it('shows Unarchive instead of Archive for an archived room, and hides the rename', async () => {
    const onArchiveChanged = vi.fn()
    renderHeader({ archived: true, onArchiveChanged })
    openDetails()
    const dialog = screen.getByRole('dialog')
    expect(within(dialog).queryByRole('button', { name: 'Arkiver rommet' })).toBeNull()
    expect(within(dialog).queryByRole('button', { name: 'Rediger navn og emne' })).toBeNull()
    fireEvent.click(within(dialog).getByRole('button', { name: 'Fjern arkivering' }))
    await waitFor(() => expect(spacesClient.unarchiveSpaceRoom).toHaveBeenCalledTimes(1))
    expect(spacesClient.unarchiveSpaceRoom.mock.calls[0]?.[1]).toMatchObject({ idempotencyKey: expect.stringMatching(/^space-unarchive:/) })
    await waitFor(() => expect(onArchiveChanged).toHaveBeenCalledTimes(1))
  })

  it('says why when the server refuses, and does not report a change', async () => {
    const onArchiveChanged = vi.fn()
    spacesClient.archiveSpaceRoom.mockRejectedValue(
      Object.assign(new Error('rate'), { status: 429, code: 'rate_limited', details: { retry_after_seconds: 12 } }),
    )
    renderHeader({ onArchiveChanged })
    openDetails()
    fireEvent.click(screen.getByRole('button', { name: 'Arkiver rommet' }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Bekreft arkivering av rommet' }))
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('12 sekunder')
    expect(onArchiveChanged).not.toHaveBeenCalled()
  })
})
