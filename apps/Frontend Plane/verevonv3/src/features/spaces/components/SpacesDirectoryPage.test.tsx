import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'

const client = vi.hoisted(() => ({
  listDirectoryRooms: vi.fn(),
  joinSpaceRoom: vi.fn(),
  getSpaceMembership: vi.fn(),
}))
const router = vi.hoisted(() => ({ navigate: vi.fn() }))

vi.mock('@/shared/api/spaces-client', () => client)
vi.mock('@solidjs/router', () => ({ useNavigate: () => router.navigate }))

const { default: SpacesDirectoryPage } = await import('./SpacesDirectoryPage')

const room = (spaceRef: string, name: string, extra: Record<string, unknown> = {}) => ({
  spaceRef,
  name,
  topic: null,
  memberCount: 3,
  joinRole: 'viewer' as const,
  ...extra,
})
const page = (rooms: readonly unknown[], nextCursor: string | null = null, enabled = true) => ({
  enabled,
  rooms,
  nextCursor,
})
const renderPage = () => render(() => <SpacesDirectoryPage />)
const joinButton = (name: string) => screen.getByText(name).closest('li')?.querySelector('button') as HTMLButtonElement

beforeEach(() => {
  client.listDirectoryRooms.mockReset()
  client.listDirectoryRooms.mockResolvedValue(page([]))
  client.joinSpaceRoom.mockReset()
  client.getSpaceMembership.mockReset()
  router.navigate.mockReset()
})

afterEach(() => cleanup())

describe('SpacesDirectoryPage (WS04-S8)', () => {
  it('says the rooms are loading until the first page is read', () => {
    client.listDirectoryRooms.mockReturnValue(new Promise(() => {}))
    renderPage()
    expect(screen.getByRole('status').textContent).toContain('Laster rom')
  })

  it('says the organization has not turned the directory on, instead of showing an empty list', async () => {
    client.listDirectoryRooms.mockResolvedValue(page([], null, false))
    renderPage()
    expect(await screen.findByText(/Organisasjonen din har ikke slått på romkatalogen/)).toBeTruthy()
    expect(screen.queryByRole('searchbox')).toBeNull()
    expect(screen.queryByText('Det finnes ingen åpne rom du ikke allerede er med i.')).toBeNull()
  })

  it('says the directory is not turned on yet when the gateway answers feature_disabled', async () => {
    client.listDirectoryRooms.mockRejectedValue(new ApiError('off', 404, 'feature_disabled'))
    renderPage()
    expect(await screen.findByText('Romkatalogen er ikke slått på ennå.')).toBeTruthy()
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('treats a failed read as an error with a retry, never as "no rooms"', async () => {
    client.listDirectoryRooms.mockRejectedValueOnce(new ApiError('x', 503, 'directory_unavailable'))
    renderPage()
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('Katalogen er utilgjengelig akkurat nå. Prøv igjen om litt.')
    expect(screen.queryByText('Det finnes ingen åpne rom du ikke allerede er med i.')).toBeNull()
    client.listDirectoryRooms.mockResolvedValue(page([room('room_a', 'Lager')]))
    fireEvent.click(screen.getByRole('button', { name: 'Prøv igjen' }))
    expect(await screen.findByText('Lager')).toBeTruthy()
    expect(client.listDirectoryRooms).toHaveBeenCalledTimes(2)
  })

  it('lists each open room with its topic, its member count and the role a join gives', async () => {
    client.listDirectoryRooms.mockResolvedValue(
      page([room('room_a', 'Lager', { topic: 'Innkjøp', memberCount: 4, joinRole: 'editor' })]),
    )
    renderPage()
    expect(await screen.findByText('Lager')).toBeTruthy()
    expect(screen.getByText('Innkjøp')).toBeTruthy()
    expect(screen.getByText('4 medlemmer. Du blir med som redaktør.')).toBeTruthy()
    expect(joinButton('Lager').textContent).toBe('Bli med')
  })

  it('says there is nothing left to join for the empty search, and no match for a search that finds none', async () => {
    renderPage()
    expect(await screen.findByText('Det finnes ingen åpne rom du ikke allerede er med i.')).toBeTruthy()
    client.listDirectoryRooms.mockResolvedValue(page([]))
    fireEvent.input(screen.getByRole('searchbox'), { target: { value: 'zzz' } })
    await waitFor(() => expect(client.listDirectoryRooms).toHaveBeenLastCalledWith('zzz', expect.any(Object)))
    expect(await screen.findByText('Ingen åpne rom passer søket.')).toBeTruthy()
  })

  it('keeps the same search box while a search runs, so the person keeps their place in it', async () => {
    client.listDirectoryRooms
      .mockResolvedValueOnce(page([room('room_a', 'Lager')]))
      .mockResolvedValue(page([]))
    renderPage()
    const box = (await screen.findByRole('searchbox')) as HTMLInputElement
    box.focus()
    fireEvent.input(box, { target: { value: 'la' } })
    expect(await screen.findByText('Ingen åpne rom passer søket.')).toBeTruthy()
    expect(box.isConnected).toBe(true)
    expect(document.activeElement).toBe(box)
    expect(box.value).toBe('la')
  })

  it('joins a room, waits for access to be prepared, then opens the room', async () => {
    client.listDirectoryRooms.mockResolvedValue(page([room('room_a', 'Lager')]))
    client.joinSpaceRoom.mockResolvedValue({ role: 'viewer', changed: true, alreadyMember: false, recipientReady: false })
    client.getSpaceMembership.mockResolvedValue({ recipient_ready: true })
    renderPage()
    fireEvent.click(await screen.findByRole('button', { name: 'Bli med' }))
    expect(await screen.findByText(/Tilgangen klargjøres/)).toBeTruthy()
    expect(joinButton('Lager').textContent).toBe('Klargjør …')
    expect(client.joinSpaceRoom).toHaveBeenCalledWith('room_a', expect.stringMatching(/^join-/))
    await waitFor(() => expect(router.navigate).toHaveBeenCalledWith('/spaces/room_a'), { timeout: 6000 })
  }, 10_000)

  it('a caller who is already a member goes straight to the room, without waiting for access', async () => {
    client.listDirectoryRooms.mockResolvedValue(page([room('room_a', 'Lager')]))
    client.joinSpaceRoom.mockResolvedValue({ role: 'viewer', changed: false, alreadyMember: true, recipientReady: false })
    renderPage()
    fireEvent.click(await screen.findByRole('button', { name: 'Bli med' }))
    await waitFor(() => expect(router.navigate).toHaveBeenCalledWith('/spaces/room_a'))
    expect(client.getSpaceMembership).not.toHaveBeenCalled()
  })

  it('says a room that was archived since the list loaded on its own row, and keeps the row', async () => {
    client.listDirectoryRooms.mockResolvedValue(page([room('room_a', 'Lager')]))
    client.joinSpaceRoom.mockRejectedValue(new ApiError('x', 409, 'room_archived'))
    renderPage()
    fireEvent.click(await screen.findByRole('button', { name: 'Bli med' }))
    expect((await screen.findByRole('alert')).textContent).toBe('Rommet er arkivert. Medlemmer kan ikke endres.')
    expect(screen.getByText('Lager')).toBeTruthy()
    expect(router.navigate).not.toHaveBeenCalled()
  })

  it('removes a room that closed since the list loaded, and says why', async () => {
    client.listDirectoryRooms.mockResolvedValue(page([room('room_a', 'Lager'), room('room_b', 'Arkiv')]))
    client.joinSpaceRoom.mockRejectedValue(new ApiError('x', 404, 'room_not_found'))
    renderPage()
    fireEvent.click((await screen.findAllByRole('button', { name: 'Bli med' }))[0] as HTMLElement)
    await waitFor(() => expect(screen.queryByText('Lager')).toBeNull())
    expect(screen.getByText('Arkiv')).toBeTruthy()
    expect(screen.getByRole('alert').textContent).toBe('Fant ikke rommet, eller du har ikke tilgang til det.')
  })

  it('a failed join says nothing was changed on that row, and the button can be pressed again', async () => {
    client.listDirectoryRooms.mockResolvedValue(page([room('room_a', 'Lager')]))
    client.joinSpaceRoom.mockRejectedValue(new ApiError('x', 500, null))
    renderPage()
    fireEvent.click(await screen.findByRole('button', { name: 'Bli med' }))
    expect((await screen.findByRole('alert')).textContent).toBe('Du kunne ikke bli med i rommet. Ingenting er endret.')
    expect(joinButton('Lager').disabled).toBe(false)
    expect(joinButton('Lager').textContent).toBe('Bli med')
    expect(router.navigate).not.toHaveBeenCalled()
  })

  it('show more adds the next page and keeps each room once', async () => {
    client.listDirectoryRooms
      .mockResolvedValueOnce(page([room('room_a', 'Alfa'), room('room_b', 'Beta')], 'cursor-2'))
      .mockResolvedValueOnce(page([room('room_b', 'Beta'), room('room_c', 'Gamma')], null))
    renderPage()
    expect(await screen.findByText('Beta')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Vis flere' }))
    expect(await screen.findByText('Gamma')).toBeTruthy()
    expect(screen.getAllByText('Beta')).toHaveLength(1)
    expect(screen.getByText('Alfa')).toBeTruthy()
    expect(client.listDirectoryRooms).toHaveBeenLastCalledWith('', expect.objectContaining({ cursor: 'cursor-2' }))
    expect(screen.queryByRole('button', { name: 'Vis flere' })).toBeNull()
  })
})
