import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { __resetRetentionForTests, applyServerRetention } from '@/features/chat/lib/chat-retention'
import { ApiError } from '@/shared/api/http'

const spacesClient = vi.hoisted(() => ({
  getOwnSpaceStatusView: vi.fn(),
  setOwnSpaceStatus: vi.fn(),
  clearOwnSpaceStatus: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))

const { SpaceStatusMenu } = await import('./SpaceStatusMenu')

const HOUR = 60 * 60 * 1000
const NOW = Date.parse('2026-11-18T10:00:00Z')

type Status = { state: 'available' | 'away' | 'dnd' | 'offline'; text: string | null; expiresAt: number | null }
/** The status read of an organization that may set text (`meta.text_allowed: true`). */
const open = (status: Status | null) => ({ status, textAllowed: true })

/** The load and the save settle on a later macrotask; Solid applies the signal writes at flush. */
async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 0))
  flush()
}

beforeEach(() => {
  // Only Date is faked, so "now + duration" is exact and waitFor keeps its real timers.
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(NOW)
  spacesClient.getOwnSpaceStatusView.mockReset()
  spacesClient.setOwnSpaceStatus.mockReset()
  spacesClient.clearOwnSpaceStatus.mockReset()
})

afterEach(() => {
  cleanup()
  vi.useRealTimers()
  __resetRetentionForTests()
})

describe('SpaceStatusMenu (WS11-S10)', () => {
  it('renders nothing when the gateway reports the status flag off (404 space_status_disabled)', async () => {
    spacesClient.getOwnSpaceStatusView.mockRejectedValue(new ApiError('Not enabled.', 404, 'space_status_disabled'))
    const view = render(() => <SpaceStatusMenu />)
    await settle()
    expect(spacesClient.getOwnSpaceStatusView).toHaveBeenCalledTimes(1)
    expect(view.container.innerHTML).toBe('')
    expect(screen.queryByRole('alert')).toBeNull()
    expect(spacesClient.setOwnSpaceStatus).not.toHaveBeenCalled()
  })

  it('saves the chosen state, the text and an expiry of now plus the chosen duration', async () => {
    spacesClient.getOwnSpaceStatusView.mockResolvedValue(open(null))
    spacesClient.setOwnSpaceStatus.mockResolvedValue({ state: 'dnd', text: 'På møte', expiresAt: NOW + 4 * HOUR })
    render(() => <SpaceStatusMenu />)
    await settle()
    fireEvent.click(screen.getByRole('radio', { name: 'Ikke forstyrr' }))
    fireEvent.input(screen.getByLabelText('Tekst (valgfri)'), { target: { value: 'På møte' } })
    fireEvent.change(screen.getByLabelText('Varighet'), { target: { value: '4h' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Lagre status' }))
    await waitFor(() => expect(spacesClient.setOwnSpaceStatus).toHaveBeenCalledTimes(1))
    expect(spacesClient.setOwnSpaceStatus).toHaveBeenCalledWith({
      state: 'dnd',
      text: 'På møte',
      expiresAt: NOW + 4 * HOUR,
    })
    expect(await screen.findByText('Statusen er lagret.')).toBeTruthy()
  })

  it('shows the zero-retention message on 412 and keeps nothing in local storage', async () => {
    spacesClient.getOwnSpaceStatusView.mockResolvedValue(open(null))
    spacesClient.setOwnSpaceStatus.mockRejectedValue(new ApiError('Refused.', 412, 'zdr_durable_mutation_forbidden'))
    render(() => <SpaceStatusMenu />)
    await settle()
    fireEvent.input(screen.getByLabelText('Tekst (valgfri)'), { target: { value: 'På møte' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Lagre status' }))
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toBe(
      'Organisasjonen lagrer ikke innhold, så statustekst er ikke tilgjengelig. Du kan fortsatt velge en tilstand.',
    )
    expect(screen.queryByText('Statusen er lagret.')).toBeNull()
    expect(spacesClient.setOwnSpaceStatus).toHaveBeenCalledTimes(1)
    expect(window.localStorage.length).toBe(0)
  })

  it('Clear removes the status through the clear route and saves nothing', async () => {
    spacesClient.getOwnSpaceStatusView.mockResolvedValue(open({ state: 'dnd', text: 'På møte', expiresAt: NOW + HOUR }))
    spacesClient.clearOwnSpaceStatus.mockResolvedValue(null)
    render(() => <SpaceStatusMenu />)
    await settle()
    // The saved text is in the box, so a Clear that echoed the box would send it.
    expect((screen.getByLabelText('Tekst (valgfri)') as HTMLInputElement).value).toBe('På møte')
    fireEvent.click(screen.getByRole('button', { name: 'Fjern status' }))
    flush()
    await waitFor(() => expect(spacesClient.clearOwnSpaceStatus).toHaveBeenCalledTimes(1))
    expect(spacesClient.setOwnSpaceStatus).not.toHaveBeenCalled()
    expect(await screen.findByText('Statusen er fjernet.')).toBeTruthy()
    flush()
    expect((screen.getByLabelText('Tekst (valgfri)') as HTMLInputElement).value).toBe('')
    expect(screen.queryByText(/Nå:/)).toBeNull()
  })

  it('draws no text field under zero data retention, and saves the state alone', async () => {
    applyServerRetention(true)
    spacesClient.getOwnSpaceStatusView.mockResolvedValue(open(null))
    spacesClient.setOwnSpaceStatus.mockResolvedValue({ state: 'away', text: null, expiresAt: null })
    render(() => <SpaceStatusMenu />)
    await settle()
    expect(screen.queryByLabelText('Tekst (valgfri)')).toBeNull()
    expect(
      screen.getByText('Statustekst er ikke tilgjengelig i denne organisasjonen. Du kan fortsatt velge en tilstand.'),
    ).toBeTruthy()
    fireEvent.click(screen.getByRole('radio', { name: 'Borte' }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Lagre status' }))
    await waitFor(() => expect(spacesClient.setOwnSpaceStatus).toHaveBeenCalledTimes(1))
    expect(spacesClient.setOwnSpaceStatus).toHaveBeenCalledWith({ state: 'away', text: '', expiresAt: undefined })
  })

  it('a zero-retention verdict that arrives while the menu is open takes the typed text away', async () => {
    spacesClient.getOwnSpaceStatusView.mockResolvedValue(open(null))
    spacesClient.setOwnSpaceStatus.mockResolvedValue(null)
    render(() => <SpaceStatusMenu />)
    await settle()
    fireEvent.input(screen.getByLabelText('Tekst (valgfri)'), { target: { value: 'På møte' } })
    flush()
    applyServerRetention(true)
    flush()
    expect(screen.queryByLabelText('Tekst (valgfri)')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Lagre status' }))
    await waitFor(() => expect(spacesClient.setOwnSpaceStatus).toHaveBeenCalledTimes(1))
    expect(spacesClient.setOwnSpaceStatus).toHaveBeenLastCalledWith(expect.objectContaining({ text: '' }))
  })

  it('draws no text field when the status read says text is not allowed, before any chat listing', async () => {
    // The chat listing has not stated a posture (retention still reads allowed): the status read decides.
    spacesClient.getOwnSpaceStatusView.mockResolvedValue({ status: null, textAllowed: false })
    spacesClient.setOwnSpaceStatus.mockResolvedValue({ state: 'dnd', text: null, expiresAt: null })
    render(() => <SpaceStatusMenu />)
    await settle()
    expect(screen.queryByLabelText('Tekst (valgfri)')).toBeNull()
    expect(
      screen.getByText('Statustekst er ikke tilgjengelig i denne organisasjonen. Du kan fortsatt velge en tilstand.'),
    ).toBeTruthy()
    fireEvent.click(screen.getByRole('radio', { name: 'Ikke forstyrr' }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Lagre status' }))
    await waitFor(() => expect(spacesClient.setOwnSpaceStatus).toHaveBeenCalledTimes(1))
    expect(spacesClient.setOwnSpaceStatus).toHaveBeenCalledWith({ state: 'dnd', text: '', expiresAt: undefined })
  })

  it('a remount asks the server again and keeps the field away for a zero-retention org', async () => {
    spacesClient.getOwnSpaceStatusView.mockResolvedValue(open(null))
    spacesClient.setOwnSpaceStatus.mockRejectedValue(new ApiError('Refused.', 412, 'zdr_durable_mutation_forbidden'))
    const first = render(() => <SpaceStatusMenu />)
    await settle()
    fireEvent.input(screen.getByLabelText('Tekst (valgfri)'), { target: { value: 'På møte' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Lagre status' }))
    expect(await screen.findByRole('alert')).toBeTruthy()
    first.unmount()
    // The 412 was this mount's; the next mount's read states the posture again.
    spacesClient.getOwnSpaceStatusView.mockResolvedValue({ status: null, textAllowed: false })
    render(() => <SpaceStatusMenu />)
    await settle()
    expect(spacesClient.getOwnSpaceStatusView).toHaveBeenCalledTimes(2)
    expect(screen.queryByLabelText('Tekst (valgfri)')).toBeNull()
  })

  it('a 412 on the text takes the field away while the menu stays mounted', async () => {
    spacesClient.getOwnSpaceStatusView.mockResolvedValue(open(null))
    spacesClient.setOwnSpaceStatus.mockRejectedValue(new ApiError('Refused.', 412, 'zdr_durable_mutation_forbidden'))
    render(() => <SpaceStatusMenu />)
    await settle()
    fireEvent.input(screen.getByLabelText('Tekst (valgfri)'), { target: { value: 'På møte' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Lagre status' }))
    expect(await screen.findByRole('alert')).toBeTruthy()
    flush()
    expect(screen.queryByLabelText('Tekst (valgfri)')).toBeNull()
    expect(
      screen.getByText('Statustekst er ikke tilgjengelig i denne organisasjonen. Du kan fortsatt velge en tilstand.'),
    ).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Lagre status' }))
    await waitFor(() => expect(spacesClient.setOwnSpaceStatus).toHaveBeenCalledTimes(2))
    expect(spacesClient.setOwnSpaceStatus).toHaveBeenLastCalledWith(expect.objectContaining({ text: '' }))
  })

  it('Clear keeps the status and says so when the clear fails', async () => {
    spacesClient.getOwnSpaceStatusView.mockResolvedValue(open({ state: 'away', text: null, expiresAt: null }))
    spacesClient.clearOwnSpaceStatus.mockRejectedValue(new ApiError('Unavailable.', 503, 'status_unavailable'))
    render(() => <SpaceStatusMenu />)
    await settle()
    fireEvent.click(screen.getByRole('button', { name: 'Fjern status' }))
    flush()
    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.queryByText('Statusen er fjernet.')).toBeNull()
    expect(screen.getByText(/Nå:/)).toBeTruthy()
  })
})
