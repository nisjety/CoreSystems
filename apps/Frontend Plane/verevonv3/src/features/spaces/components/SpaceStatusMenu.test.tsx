import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'

const spacesClient = vi.hoisted(() => ({
  getOwnSpaceStatus: vi.fn(),
  setOwnSpaceStatus: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))

const { SpaceStatusMenu } = await import('./SpaceStatusMenu')

const HOUR = 60 * 60 * 1000
const NOW = Date.parse('2026-11-18T10:00:00Z')

/** The load and the save settle on a later macrotask; Solid applies the signal writes at flush. */
async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 0))
  flush()
}

beforeEach(() => {
  // Only Date is faked, so "now + duration" is exact and waitFor keeps its real timers.
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(NOW)
  spacesClient.getOwnSpaceStatus.mockReset()
  spacesClient.setOwnSpaceStatus.mockReset()
})

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('SpaceStatusMenu (WS11-S10)', () => {
  it('renders nothing when the gateway reports the status flag off (404 space_status_disabled)', async () => {
    spacesClient.getOwnSpaceStatus.mockRejectedValue(new ApiError('Not enabled.', 404, 'space_status_disabled'))
    const view = render(() => <SpaceStatusMenu />)
    await settle()
    expect(spacesClient.getOwnSpaceStatus).toHaveBeenCalledTimes(1)
    expect(view.container.innerHTML).toBe('')
    expect(screen.queryByRole('alert')).toBeNull()
    expect(spacesClient.setOwnSpaceStatus).not.toHaveBeenCalled()
  })

  it('saves the chosen state, the text and an expiry of now plus the chosen duration', async () => {
    spacesClient.getOwnSpaceStatus.mockResolvedValue(null)
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
    spacesClient.getOwnSpaceStatus.mockResolvedValue(null)
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

  it('Clear sends the available state with no text and no expiry', async () => {
    spacesClient.getOwnSpaceStatus.mockResolvedValue({ state: 'dnd', text: 'På møte', expiresAt: NOW + HOUR })
    spacesClient.setOwnSpaceStatus.mockResolvedValue(null)
    render(() => <SpaceStatusMenu />)
    await settle()
    // The saved text is in the box, so a Clear that echoed the box would send it.
    expect((screen.getByLabelText('Tekst (valgfri)') as HTMLInputElement).value).toBe('På møte')
    fireEvent.click(screen.getByRole('button', { name: 'Fjern status' }))
    flush()
    await waitFor(() => expect(spacesClient.setOwnSpaceStatus).toHaveBeenCalledTimes(1))
    expect(spacesClient.setOwnSpaceStatus).toHaveBeenCalledWith({ state: 'available', text: '', expiresAt: undefined })
  })
})
